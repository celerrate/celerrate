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
use celerrate_source::{FileId, TextRange, TextSize};
use celerrate_stubs::StubIndexInput;
use rayon::prelude::*;

use crate::cache::snapshot::CacheSnapshot;
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
    /// The cache snapshot the pass may serve verdicts from. Attached
    /// range-gated by `Session::inputs`; an empty default when the
    /// range moved since the snapshot was loaded.
    pub cache: Arc<CacheSnapshot>,
    /// The session's cache counters. Written by the pass, never read
    /// by it: statistics do not feed analysis.
    pub statistics: Arc<crate::cache::statistics::CacheStatistics>,
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

/// The analysis loop itself panicked, outside every file's guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panicked;

/// Runs one pass with the loop itself behind a guard.
///
/// `analyze` deliberately re-raises anything that is not one file's panic,
/// because `guarded` already caught that one. Under `--watch` the pass runs
/// on its own thread and `worker.join()` hands that escape back, which
/// becomes `InternalError::AnalysisPanicked` and exit 2. A single `check`
/// has no thread to join: without this guard the panic escaped `run` and
/// `main`, and the user got a raw Rust panic and exit 101 rather than the
/// internal-error report. The variant existed with no path that could
/// produce it.
///
/// Catching here is panic *handling*, not panic *raising*. The zero-panic
/// rule forbids the second; the first is what it is for.
///
/// A `salsa::Cancelled` never arrives here as a panic: `analyze` turns it
/// into `Err(Cancelled)` before it can escape, and `guarded` re-raises it
/// so that it can. That is what keeps a cancellation from ever being read
/// as a bug, and this guard does not weaken it.
pub fn isolated<T>(pass: impl FnOnce() -> T) -> Result<T, Panicked> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(pass)).map_err(|_| Panicked)
}

/// Filters `diagnostics` down to what no suppression directive covers.
/// Shared by `persistable_diagnostics` and `typed_portion` so the two
/// composers apply the exact same filter rather than each maintaining
/// its own copy: suppression is family-agnostic (design section 5), and
/// that must hold for the typed families exactly as it does for the two
/// that predate them.
fn retain_unsuppressed(
    database: &dyn salsa::Database,
    file: SourceFile,
    suppressed: &[TextRange],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let text_end = celerrate_db::source_text(database, file)
        .as_ref()
        .map(|text| TextSize::of(text.text()))
        .unwrap_or_default();
    diagnostics.retain(|diagnostic| {
        let Some((_, range)) = diagnostic.span() else {
            return true;
        };
        !celerrate_semantics::is_suppressed(suppressed, range.start(), text_end)
    });
}

/// The cache-servable portion: syntax, decode, and semantic families,
/// suppression applied. Exactly what `StoredVerdict` persists — the
/// typed families stay out of the packs until plan 9a designs their own
/// revalidation records as a separate artifact class.
///
/// This is the previous `composed_diagnostics` body, moved rather than
/// paraphrased: `persist`'s `composed_verdict` re-composes through it,
/// and the equivalence harness recomputes through the union that wraps
/// it, so the composers cannot drift (audit finding I2's first
/// hand-maintained mirror). Filtering here, below the verdict, is sound
/// because directives are strictly file-local: the verdict's
/// content-hash key covers every directive edit, and it keeps the
/// exit-code count, the printed report, and the persisted verdict the
/// same post-filter set by construction (the vendor-filter rationale
/// above, applied again).
pub fn persistable_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let mut diagnostics = celerrate_db::file_diagnostics(database, file).clone();
    diagnostics.extend(
        celerrate_rules::syntax_phase_diagnostics(database, file, inputs.configuration)
            .iter()
            .cloned(),
    );
    diagnostics.extend(
        celerrate_rules::semantic_phase_diagnostics(
            database,
            file,
            inputs.files,
            inputs.stubs,
            inputs.configuration,
        )
        .iter()
        .cloned(),
    );
    let suppressed = celerrate_semantics::suppressed_ranges(database, file);
    if !suppressed.is_empty() {
        retain_unsuppressed(database, file, suppressed, &mut diagnostics);
    }
    diagnostics
}

/// The typed families as the rule framework's typed-body phase
/// (`celerrate_rules::typed_body_phase_diagnostics`) renders them,
/// suppression applied, computed fresh from the live project: the
/// recompute-path building block, never called on a typed-serve hit.
/// Plan 9a (task 9) gave the typed families their own persistent
/// artifact class (`crate::cache::stored::StoredTypedVerdict`,
/// validated by `crate::cache::verdict::TypedOutcome`); this function is
/// what every recompute path calls once its outcome is `Recompute`:
/// `served_typed_diagnostics`'s fallback (the orchestration layer's own
/// fork, `analyze_one`'s hit path and the equivalence harness alike),
/// `crate::cache::composed_typed_verdict` (the persist path, computing
/// what gets stored), and `composed_diagnostics` below (an untyped miss,
/// where nothing typed could have been served either). A warm hit whose
/// typed half validates never reaches this at all — that is the whole
/// point of the artifact class this function now feeds rather than
/// stands in for.
pub fn typed_portion(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let mut diagnostics = celerrate_rules::typed_body_phase_diagnostics(
        database,
        file,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
    )
    .clone();
    let suppressed = celerrate_semantics::suppressed_ranges(database, file);
    if !suppressed.is_empty() {
        retain_unsuppressed(database, file, suppressed, &mut diagnostics);
    }
    diagnostics
}

/// One file's diagnostics, computed: decode and syntax, then references
/// and gating, then the typed families, then the directive filter's
/// effect on each half. The single composition point — `analyze_one`
/// serves it on a cache miss, `persist`'s `composed_verdict` re-composes
/// through its `persistable_diagnostics` half, and the equivalence
/// harness recomputes through it — so the composers cannot drift (audit
/// finding I2's first hand-maintained mirror).
pub fn composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let mut diagnostics = persistable_diagnostics(inputs, file);
    diagnostics.extend(typed_portion(inputs, file));
    diagnostics.sort();
    diagnostics
}

/// The typed half of one file's diagnostics on a cache hit (plan 9a,
/// task 9's fork): served from `typed_source` when it is present and
/// every one of its diagnostics still re-interns (no body walked, no
/// inference ran — the substance
/// `a_warm_run_serves_typed_verdicts_without_inference` pins), a fresh
/// `typed_portion` otherwise. `typed_source` is `Some` exactly when
/// `crate::cache::verdict::TypedOutcome::Served` and the caller already
/// holds the verdict's own `typed` field; `None` covers every other
/// case (`TypedOutcome::Recompute`, a discarded or absent untyped half,
/// or a whole-verdict diagnostic-conversion failure) uniformly, since
/// all of them mean the same thing here: nothing typed survived to
/// serve.
///
/// Public and called from both `analyze_one`'s hit path and the
/// equivalence harness (`tests/cache_equivalence.rs`), so the two
/// compositions cannot independently drift (audit finding I2's own
/// concern, extended to task 9's typed half — "the equivalence harness
/// keeps ONE truth"). The counters this increments
/// (`typed_served`/`typed_recomputed`) are the orchestration layer's own
/// (plan 5's decision 13: never inside a query), so calling this from a
/// test also nudges them; harmless, since no test asserts an exact
/// count without first driving every file through this same fork.
pub fn served_typed_diagnostics(
    inputs: &AnalysisInputs,
    file: SourceFile,
    typed_source: Option<&crate::cache::stored::StoredTypedVerdict>,
) -> Vec<Diagnostic> {
    use std::sync::atomic::Ordering;

    let database = &inputs.database;
    let file_id = file.file_id(database);
    let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
    let statistics = &inputs.statistics;
    if let Some(typed) = typed_source
        && let Some(diagnostics) = typed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length))
            .collect::<Option<Vec<_>>>()
    {
        statistics.typed_served.fetch_add(1, Ordering::Relaxed);
        return diagnostics;
    }
    statistics.typed_recomputed.fetch_add(1, Ordering::Relaxed);
    let result = celerrate_types::typed_file_verdicts(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        file,
    );
    statistics.record_typed(result);
    typed_portion(inputs, file)
}

/// One file's total: decode and syntax, then references and gating,
/// then the typed families. On a cache hit the untyped half is served
/// from the pack and the typed half is layered on top independently
/// (plan 9a, task 9): served from the cache when its own records
/// validate, recomputed fresh otherwise — a partial hit (untyped served,
/// typed recomputed) is a first-class outcome, not a fallback. On a miss
/// `composed_diagnostics` already produces the full union. Either way
/// the result is the exact same composed set, sorted once at the end.
fn analyze_one(inputs: &AnalysisInputs, file: SourceFile) -> Result<Vec<Diagnostic>, FileId> {
    use std::sync::atomic::Ordering;

    use crate::cache::verdict::{TypedOutcome, VerdictLookup};

    let database = &inputs.database;
    let file_id = file.file_id(database);
    let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
    guarded(file_id, || {
        let statistics = &inputs.statistics;
        let (mut diagnostics, typed_source) =
            match crate::cache::verdict::lookup_verdict(inputs, file) {
                VerdictLookup::Hit { verdict, typed } => match verdict
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length))
                    .collect::<Option<Vec<_>>>()
                {
                    Some(diagnostics) => {
                        statistics.verdicts_served.fetch_add(1, Ordering::Relaxed);
                        let typed_source = match typed {
                            TypedOutcome::Served => verdict.typed.as_ref(),
                            TypedOutcome::Recompute => None,
                        };
                        (diagnostics, typed_source)
                    }
                    None => {
                        // Revalidated, but a stored diagnostic failed
                        // conversion: the same refusal as a moved answer,
                        // and it takes the typed half down with it — a
                        // discarded untyped half has nothing left to layer
                        // a typed serve over.
                        statistics
                            .verdicts_discarded
                            .fetch_add(1, Ordering::Relaxed);
                        (persistable_diagnostics(inputs, file), None)
                    }
                },
                VerdictLookup::Discarded => {
                    statistics
                        .verdicts_discarded
                        .fetch_add(1, Ordering::Relaxed);
                    (persistable_diagnostics(inputs, file), None)
                }
                VerdictLookup::Absent => {
                    statistics.verdicts_absent.fetch_add(1, Ordering::Relaxed);
                    (persistable_diagnostics(inputs, file), None)
                }
            };
        diagnostics.extend(served_typed_diagnostics(inputs, file, typed_source));
        diagnostics.sort();
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

    use super::{Panicked, assemble, guarded, isolated};

    fn diagnostic(file: u32, offset: u32) -> Diagnostic {
        Diagnostic::spanned(
            DiagnosticId::new("CEL0018"),
            Severity::Error,
            FileId::new(file),
            TextRange::empty(TextSize::from(offset)),
            "unknown class".to_owned(),
        )
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

    /// The panic `guarded` does not catch: one in the loop itself, outside
    /// every file. Under `--watch` the thread's join catches it; the single
    /// pass has no thread, so it needs this, or the user gets a raw Rust
    /// panic and exit 101 instead of the report and exit 2.
    #[test]
    fn a_panic_in_the_loop_itself_is_caught_rather_than_escaping_the_process() {
        let result: Result<(), Panicked> = isolated(|| panic!("a bug in the loop"));
        assert_eq!(result, Err(Panicked));
    }

    #[test]
    fn a_pass_that_does_not_panic_passes_straight_through() {
        assert_eq!(isolated(|| 7), Ok(7));
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
