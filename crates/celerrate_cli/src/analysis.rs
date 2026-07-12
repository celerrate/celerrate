//! The analysis loop: rayon over the file set, each file behind a panic
//! guard, the two diagnostic families composed, the result sorted.
//!
//! Parallelism lives here and only here. Salsa's contract is that queries
//! are pure functions of their inputs, so the fan-out happens at the
//! declared boundary, over database snapshots, never inside a query.

use std::sync::Arc;

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
    /// Every file the analysis may *read*: the project's own and its
    /// installed dependencies'. This is the salsa input the semantic
    /// queries resolve names against, so it is the whole set.
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
    /// Every file the analysis *reports on*: the project's own, and only
    /// those. A dependency's symbols are what make `use Vendor\Package
    /// \Thing;` resolve, so its files stay in `files` above; what they
    /// must not do is speak. A third-party finding is not the user's to
    /// fix, it drowns the report on any real Composer project, and it
    /// fails the build on code the user does not own.
    ///
    /// The distinction is drawn here, at the fan-out's input, rather than
    /// in the renderer, and that is the whole point. The exit code comes
    /// from the length of `AnalysisOutcome::diagnostics`, so filtering at
    /// the render would leave a vendor finding exiting 1 over a report
    /// that printed nothing: worse than either half of the bug. Filtering
    /// the input makes the count the run reports and the count it prints
    /// the same set by construction. It also means a dependency's
    /// diagnostics are never computed at all, which is the bulk of the
    /// files on a real project.
    ///
    /// An `Arc` because `AnalysisInputs` is cloned once per file.
    pub reported: Arc<[SourceFile]>,
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

/// Analyzes every reported file, in parallel, resolving names against the
/// whole set.
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
        .reported
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
    let database = &inputs.database;
    let file_id = file.file_id(database);
    guarded(file_id, || {
        let mut diagnostics = celerrate_db::file_diagnostics(database, file).clone();
        diagnostics.extend(
            celerrate_semantics::semantic_diagnostics(
                database,
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
        //
        // Real salsa throws with `resume_unwind`, not `panic!`, precisely
        // to skip the panic hook and keep test and terminal output clean.
        // Reproduce that here rather than `panic_any`, which would print
        // panic hook noise even on a passing run.
        let escaped = std::panic::catch_unwind(|| {
            let _: Result<(), FileId> = guarded(FileId::new(0), || {
                std::panic::resume_unwind(Box::new(salsa::Cancelled::PendingWrite))
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
