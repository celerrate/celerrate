//! The analysis loop: rayon over the file set, each file behind a panic
//! guard, the two diagnostic families composed, the result sorted.
//!
//! Parallelism lives here and only here. Salsa's contract is that queries
//! are pure functions of their inputs, so the fan-out happens at the
//! declared boundary, over database snapshots, never inside a query.

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::Diagnostic;
use celerrate_project::ProjectConfiguration;
use celerrate_source::FileId;
use celerrate_stubs::StubIndexInput;
use rayon::prelude::*;

use crate::database::AnalysisDatabase;

/// Everything a fan-out needs, and nothing it must not touch. Owned and
/// `'static`, so `--watch` can hand it to a thread while the main thread
/// keeps `&mut Session`.
#[derive(Clone)]
pub struct AnalysisInputs {
    pub database: AnalysisDatabase,
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
}

/// An input was mutated while the fan-out ran. Not an error: a restart
/// signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// One complete pass over the file set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalysisOutcome {
    /// Every diagnostic, in the total order the shared model defines.
    pub diagnostics: Vec<Diagnostic>,
    /// The files whose analysis panicked. They yielded nothing.
    pub panicked: Vec<FileId>,
}

/// Analyzes every file in the set, in parallel.
///
/// The salsa storage is `Send` but not `Sync`: cloning it hands a thread
/// its own independent local state over shared underlying revisions, but
/// it cannot be reached through a shared reference from several threads
/// at once. So every file's clone is made up front, on this thread, and
/// handed to rayon as an owned task; the closure that runs on the pool
/// captures nothing and therefore imposes no `Sync` requirement of its
/// own.
pub fn analyze(inputs: &AnalysisInputs) -> Result<AnalysisOutcome, Cancelled> {
    let tasks: Vec<(SourceFile, AnalysisInputs)> = inputs
        .files
        .files(&inputs.database)
        .iter()
        .map(|&file| (file, inputs.clone()))
        .collect();
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tasks
            .into_par_iter()
            .map(|(file, inputs)| analyze_one(&inputs, file))
            .collect::<Vec<_>>()
    }));
    match attempt {
        Ok(results) => Ok(assemble(results)),
        Err(payload) => {
            if payload.downcast_ref::<salsa::Cancelled>().is_some() {
                return Err(Cancelled);
            }
            // A per-file panic never reaches here: `guarded` caught it.
            // Anything that does is a panic in this loop itself.
            std::panic::resume_unwind(payload)
        }
    }
}

/// One file's total: decode and syntax, then references and gating.
/// Nothing composes those two families below this line.
fn analyze_one(inputs: &AnalysisInputs, file: SourceFile) -> Result<Vec<Diagnostic>, FileId> {
    let database = inputs.database.clone();
    let file_id = file.file_id(&database);
    guarded(file_id, move || {
        let mut diagnostics = celerrate_db::file_diagnostics(&database, file).clone();
        diagnostics.extend(
            celerrate_semantics::semantic_diagnostics(
                &database,
                file,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
            )
            .iter()
            .cloned(),
        );
        diagnostics
    })
}

/// The panic guard: transparent to `salsa::Cancelled`, which is always
/// re-raised, and naming the file for anything else. Its product is never
/// memoized: a panicked query leaves no cached value behind, and neither
/// does this.
pub fn guarded<T>(file: FileId, analyze: impl FnOnce() -> T) -> Result<T, FileId> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(analyze)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            if payload.downcast_ref::<salsa::Cancelled>().is_some() {
                std::panic::resume_unwind(payload);
            }
            Err(file)
        }
    }
}

/// Folds the per-file results into one outcome, sorted. Rayon preserves
/// the file order, and the sort makes the result independent of it
/// anyway: parallel collection is deterministic before rendering.
pub fn assemble(results: Vec<Result<Vec<Diagnostic>, FileId>>) -> AnalysisOutcome {
    let mut diagnostics = Vec::new();
    let mut panicked = Vec::new();
    for result in results {
        match result {
            Ok(file_diagnostics) => diagnostics.extend(file_diagnostics),
            Err(file) => panicked.push(file),
        }
    }
    diagnostics.sort();
    panicked.sort();
    AnalysisOutcome {
        diagnostics,
        panicked,
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::expect_used
    )]

    use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
    use celerrate_source::{FileId, TextRange, TextSize};

    use super::{assemble, guarded};

    fn diagnostic(file: u32, offset: u32) -> Diagnostic {
        Diagnostic {
            id: DiagnosticId::new("CEL0018"),
            severity: Severity::Error,
            file: FileId::new(file),
            range: TextRange::empty(TextSize::from(offset)),
            message: "unknown class".to_owned(),
        }
    }

    #[test]
    fn the_guard_returns_what_the_analysis_produced() {
        let result = guarded(FileId::new(0), || vec![diagnostic(0, 1)]);
        assert_eq!(result, Ok(vec![diagnostic(0, 1)]));
    }

    #[test]
    fn a_panic_names_the_file_and_is_not_reraised() {
        let result: Result<Vec<Diagnostic>, FileId> =
            guarded(FileId::new(7), || panic!("a bug in a rule"));
        assert_eq!(result, Err(FileId::new(7)));
    }

    #[test]
    fn the_guard_is_transparent_to_cancellation() {
        // Cancellation is not a bug: it is `--watch` telling the analysis
        // its inputs moved. It must pass straight through the guard.
        let escaped = std::panic::catch_unwind(|| {
            let _: Result<(), FileId> = guarded(FileId::new(0), || {
                std::panic::panic_any(salsa::Cancelled::PendingWrite)
            });
        });
        let payload = escaped.expect_err("cancellation is re-raised");
        assert!(
            payload.downcast_ref::<salsa::Cancelled>().is_some(),
            "the guard must re-raise the cancellation, not swallow it",
        );
    }

    #[test]
    fn a_panicking_file_yields_nothing_and_every_other_file_still_reports() {
        let outcome = assemble(vec![
            Ok(vec![diagnostic(0, 5)]),
            Err(FileId::new(1)),
            Ok(vec![diagnostic(2, 3)]),
        ]);
        assert_eq!(
            outcome.diagnostics,
            vec![diagnostic(0, 5), diagnostic(2, 3)],
        );
        assert_eq!(outcome.panicked, vec![FileId::new(1)]);
    }

    #[test]
    fn the_collected_diagnostics_are_sorted_before_rendering() {
        // Parallel collection is only deterministic because of this sort.
        let outcome = assemble(vec![Ok(vec![diagnostic(2, 0), diagnostic(0, 9)])]);
        assert_eq!(
            outcome.diagnostics,
            vec![diagnostic(0, 9), diagnostic(2, 0)]
        );
    }

    #[test]
    fn a_file_reports_both_its_syntax_and_its_semantic_findings() {
        use crate::session::Session;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("Kernel.php"),
            "<?php class Kernel extends Missing { public function f() { $x = } }",
        )
        .unwrap();

        let session = Session::start(root.path());
        let outcome = super::analyze(&session.inputs()).expect("no cancellation");

        let identifiers: Vec<&str> = outcome
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.id.as_str())
            .collect();
        assert!(
            identifiers.contains(&"CEL0007"),
            "the syntax family reports: {identifiers:?}",
        );
        assert!(
            identifiers.contains(&"CEL0018"),
            "the semantic family reports: {identifiers:?}",
        );
        assert!(outcome.panicked.is_empty());
    }
}
