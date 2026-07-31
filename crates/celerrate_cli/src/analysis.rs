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
use celerrate_source::{FileId, TextSize};
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
    /// The `[severity]` remap, applied by the per-file composers below,
    /// never inside a query: `persistable_diagnostics` and `typed_portion`
    /// remap right where they filter suppressions, so the exit-code count,
    /// the printed report, and the persisted verdict carry the same
    /// severities by construction.
    pub severity_remap: Arc<std::collections::BTreeMap<String, celerrate_diagnostics::Severity>>,
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

/// Filters `diagnostics` down to what no suppression directive
/// admits, and answers the sorted indexes (into
/// `suppression_directives(db, file)`) of every directive that
/// admitted at least one diagnostic - any-match attribution: a
/// diagnostic admitted by several co-located directives marks them
/// all used. Shared by `persistable_diagnostics`
/// and `typed_portion` so the two composers apply the exact same
/// filter.
fn retain_unsuppressed(
    database: &dyn salsa::Database,
    file: SourceFile,
    directives: &[celerrate_semantics::ResolvedDirective],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<u32> {
    let text_end = celerrate_db::source_text(database, file)
        .as_ref()
        .map(|text| TextSize::of(text.text()))
        .unwrap_or_default();
    let mut matched = std::collections::BTreeSet::new();
    diagnostics.retain(|diagnostic| {
        let Some((_, range)) = diagnostic.span() else {
            return true;
        };
        let mut suppressed = false;
        for (index, directive) in directives.iter().enumerate() {
            if directive.admits(diagnostic.id, range.start(), text_end) {
                suppressed = true;
                // A file cannot carry more than `u32::MAX` directives, so
                // this conversion cannot fail in practice. Were it ever to
                // fail, losing the attribution is the safe direction:
                // suppression itself (`suppressed = true` above) is
                // unaffected, only the reporting of which directive did it.
                if let Ok(index) = u32::try_from(index) {
                    matched.insert(index);
                }
            }
        }
        !suppressed
    });
    matched.into_iter().collect()
}

/// Applies the `[severity]` remap in place. Only remappable
/// identifiers are in the map (`configuration::severity_remap`), so
/// resilience diagnostics cannot be touched here by construction.
fn apply_severity_remap(
    remap: &std::collections::BTreeMap<String, celerrate_diagnostics::Severity>,
    diagnostics: &mut [Diagnostic],
) {
    if remap.is_empty() {
        return;
    }
    for diagnostic in diagnostics {
        if let Some(&severity) = remap.get(diagnostic.id.as_str()) {
            diagnostic.severity = severity;
        }
    }
}

/// One filtered half of a file's diagnostics: what survived the
/// directive filter, and the sorted indexes (into
/// `suppression_directives(db, file)`) of every directive that
/// admitted at least one diagnostic of this half. Halves keep their
/// own matched sets because they are served independently on a
/// partial cache hit; the reporting phase consumes the union.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilteredPortion {
    pub diagnostics: Vec<Diagnostic>,
    pub matched: Vec<u32>,
}

/// The cache-servable portion: syntax, decode, and semantic families,
/// suppression applied. Exactly what `StoredVerdict` persists, the
/// typed families stay out of the packs until they get their own
/// revalidation records as a separate artifact class.
///
/// This is the previous `composed_diagnostics` body, moved rather than
/// paraphrased: `persist`'s `composed_verdict` re-composes through it,
/// and the equivalence harness recomputes through the union that wraps
/// it, so the composers cannot drift (the first hand-maintained mirror
/// of this duplication). Filtering here, below the verdict, is sound
/// because directives are strictly file-local: the verdict's
/// content-hash key covers every directive edit, and it keeps the
/// exit-code count, the printed report, and the persisted verdict the
/// same post-filter set by construction (the vendor-filter rationale
/// above, applied again).
pub fn persistable_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> FilteredPortion {
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
    let directives = celerrate_semantics::suppression_directives(database, file);
    let matched = if directives.is_empty() {
        Vec::new()
    } else {
        retain_unsuppressed(database, file, directives, &mut diagnostics)
    };
    apply_severity_remap(&inputs.severity_remap, &mut diagnostics);
    FilteredPortion {
        diagnostics,
        matched,
    }
}

/// The typed families as the rule framework's typed-body phase
/// (`celerrate_rules::typed_body_phase_diagnostics`) renders them,
/// suppression applied, computed fresh from the live project: the
/// recompute-path building block, never called on a typed-serve hit.
/// The typed families have their own persistent
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
pub fn typed_portion(inputs: &AnalysisInputs, file: SourceFile) -> FilteredPortion {
    let database = &inputs.database;
    let mut diagnostics = celerrate_rules::typed_body_phase_diagnostics(
        database,
        file,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
    )
    .clone();
    let directives = celerrate_semantics::suppression_directives(database, file);
    let matched = if directives.is_empty() {
        Vec::new()
    } else {
        retain_unsuppressed(database, file, directives, &mut diagnostics)
    };
    apply_severity_remap(&inputs.severity_remap, &mut diagnostics);
    FilteredPortion {
        diagnostics,
        matched,
    }
}

/// Builds the reporting phase's input: converted directive records
/// (identity plus untyped matched flag) unioned with the typed half's
/// admitting indexes. Both cache paths and the recompute path funnel
/// through this one constructor so the union cannot drift. On a
/// partial hit the stored records and the fresh typed indexes align
/// because a verdict hit implies identical content and the pack is
/// keyed on binary identity, the one load-time property taken on
/// faith from the content hash (a sharp edge).
///
/// `matched_typed` is binary-searched here, so every caller must have
/// established that it is strictly increasing and in range first. A
/// stored list earns that through `StoredVerdict::directives_convert`,
/// which validates it and answers the records this function's first
/// argument carries: converting the records is therefore the caller's
/// proof that the indexes are trustworthy, and a `None` there discards
/// the whole verdict rather than reaching this function.
pub fn directive_outcomes(
    directives: &[(celerrate_semantics::ResolvedDirective, bool)],
    matched_typed: &[u32],
) -> Vec<celerrate_rules::DirectiveOutcome> {
    directives
        .iter()
        .enumerate()
        .map(|(index, (directive, matched_untyped))| {
            let matched = *matched_untyped
                || u32::try_from(index)
                    .map(|index| matched_typed.binary_search(&index).is_ok())
                    .unwrap_or(false);
            celerrate_rules::DirectiveOutcome {
                directive: directive.clone(),
                matched,
            }
        })
        .collect()
}

/// The reporting portion of one file: the directive rules' output,
/// computed from final match outcomes on both the warm and the cold
/// path. Shared by `analyze_one`,
/// `composed_diagnostics`, and the equivalence harness.
pub fn reporting_portion(
    inputs: &AnalysisInputs,
    file: SourceFile,
    outcomes: &[celerrate_rules::DirectiveOutcome],
) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let file_id = file.file_id(database);
    // Standing note, deliberate. `source_text` decodes here for every
    // file this function is called on, including the overwhelming
    // majority that carry no directive at all. The cost is a decode,
    // not a parse, and the corpus gate has never moved for it. The
    // remedy is available whenever that cost starts to matter: an
    // `outcomes.is_empty()` early return is behavior-preserving,
    // because the reporting runner below iterates `outcomes` in every
    // loop and yields nothing for an empty list. It is not taken
    // because the unconditional decode also keeps this function's salsa
    // dependency footprint uniform across files, and a uniform
    // footprint is worth more than the decode it saves on a file that
    // carries no directive.
    let text_end = celerrate_db::source_text(database, file)
        .as_ref()
        .map(|text| TextSize::of(text.text()))
        .unwrap_or_default();
    let mut diagnostics =
        celerrate_rules::reporting_phase_diagnostics(database, file_id, text_end, outcomes);
    // The reporting phase is the rule framework's fourth phase (CEL0041,
    // CEL0042): every consumer of this function (`composed_diagnostics`,
    // `analyze_one`, and the equivalence harness) reaches its diagnostics
    // only through here, so remapping in this one place is what makes
    // `[severity]` apply to the reporting family exactly as it already
    // applies to the syntax/semantic/typed families above. This portion
    // is never persisted — both paths recompute it from the match
    // records — so nothing moves cache-side.
    apply_severity_remap(&inputs.severity_remap, &mut diagnostics);
    diagnostics
}

/// The fresh equivalent of the stored directive records: the query's
/// directives paired with the untyped half's match outcomes. Shared by
/// `composed_diagnostics` and `analyze_one`'s recompute arms so the
/// two derivations cannot drift.
fn fresh_directive_records(
    inputs: &AnalysisInputs,
    file: SourceFile,
    matched_untyped: &[u32],
) -> Vec<(celerrate_semantics::ResolvedDirective, bool)> {
    celerrate_semantics::suppression_directives(&inputs.database, file)
        .iter()
        .enumerate()
        .map(|(index, directive)| {
            let matched = u32::try_from(index)
                .map(|index| matched_untyped.binary_search(&index).is_ok())
                .unwrap_or(false);
            (directive.clone(), matched)
        })
        .collect()
}

/// One file's diagnostics, computed: decode and syntax, then references
/// and gating, then the typed families, then the directive filter's
/// effect on each half, and finally the reporting phase over the union
/// of the two halves' match outcomes. The single composition point -
/// `analyze_one` serves it on a cache miss, `persist`'s
/// `composed_verdict` re-composes through its `persistable_diagnostics`
/// half, and the equivalence harness recomputes through it, so the
/// composers cannot drift (the first hand-maintained mirror of this
/// duplication).
///
/// The reporting portion is never persisted: both paths recompute it
/// from the match records, which is what makes a warm run and a cold
/// run report the same directive diagnostics byte for byte.
pub fn composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let untyped = persistable_diagnostics(inputs, file);
    let typed = typed_portion(inputs, file);
    let fresh = fresh_directive_records(inputs, file, &untyped.matched);
    let outcomes = directive_outcomes(&fresh, &typed.matched);
    let mut diagnostics = untyped.diagnostics;
    diagnostics.extend(typed.diagnostics);
    diagnostics.extend(reporting_portion(inputs, file, &outcomes));
    diagnostics.sort();
    diagnostics
}

/// The typed half of one file's diagnostics on a cache hit: served
/// from `typed_source` when it is present and
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
/// compositions cannot independently drift ("the equivalence harness
/// keeps ONE truth"). The counters this increments
/// (`typed_served`/`typed_recomputed`) are the orchestration layer's own
/// (never inside a query), so calling this from a
/// test also nudges them; harmless, since no test asserts an exact
/// count without first driving every file through this same fork.
pub fn served_typed_diagnostics(
    inputs: &AnalysisInputs,
    file: SourceFile,
    typed_source: Option<&crate::cache::stored::StoredTypedVerdict>,
) -> FilteredPortion {
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
        // `typed.matched_directives` is unvalidated here: the only
        // validator is `StoredVerdict::directives_convert`, which lives
        // on the untyped verdict, not on `StoredTypedVerdict`. Every
        // consumer of `matched` on this `FilteredPortion` must call
        // `StoredVerdict::directives_convert` on the same stored verdict
        // and discard the whole result on `None` BEFORE trusting these
        // indexes, since `directive_outcomes` binary-searches them and a
        // hostile pack could otherwise make that search lie. Both of
        // today's consumers do: `analyze_one` only reaches a `Some`
        // `typed_source` through an arm that already converted the
        // records (a `None` there joins the `verdicts_discarded`
        // fallback, which recomputes both halves), and the equivalence
        // harness converts them before composing the outcomes.
        return FilteredPortion {
            diagnostics,
            matched: typed.matched_directives.clone(),
        };
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
/// from the pack and the typed half is layered on top independently:
/// served from the cache when its own records validate, recomputed
/// fresh otherwise, a partial hit (untyped served,
/// typed recomputed) is a first-class outcome, not a fallback. On a miss
/// `composed_diagnostics` already produces the full union. Either way
/// the result is the exact same composed set, sorted once at the end.
///
/// The reporting portion is layered on top from the union of both
/// halves' match records, taken from whichever source each half came
/// from on this run: the stored records on a served untyped half, a
/// fresh `fresh_directive_records` otherwise, unioned with the typed
/// half's own admitting indexes. It is never served from the pack -
/// reporting diagnostics are not persisted, both paths recompute them
/// from the records, which is what keeps a warm run and a cold run
/// reporting the same directive diagnostics byte for byte.
fn analyze_one(inputs: &AnalysisInputs, file: SourceFile) -> Result<Vec<Diagnostic>, FileId> {
    use std::sync::atomic::Ordering;

    use crate::cache::verdict::{TypedOutcome, VerdictLookup};

    let database = &inputs.database;
    let file_id = file.file_id(database);
    let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
    guarded(file_id, || {
        let statistics = &inputs.statistics;
        // Every arm that cannot serve the untyped half: recompute it,
        // and derive the directive records from that recomputation. The
        // typed half is `None` here, so it recomputes too and answers
        // its own fresh indexes into the same fresh directive list.
        let recomputed = || {
            let portion = persistable_diagnostics(inputs, file);
            let records = fresh_directive_records(inputs, file, &portion.matched);
            (portion.diagnostics, None, records)
        };
        let (mut diagnostics, typed_source, records) =
            match crate::cache::verdict::lookup_verdict(inputs, file) {
                VerdictLookup::Hit { verdict, typed } => {
                    let served = verdict
                        .diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length))
                        .collect::<Option<Vec<_>>>();
                    // The directive records are converted here, before
                    // anything reads `verdict.typed`'s own
                    // `matched_directives`: `directives_convert` is that
                    // list's ONLY validator (in range, strictly
                    // increasing), and `directive_outcomes` downstream
                    // binary-searches it. A `None` therefore has to
                    // discard the whole verdict, exactly like a failed
                    // diagnostic conversion, rather than let a
                    // checksum-valid but dishonest pack reach the
                    // reporting phase.
                    match (served, verdict.directives_convert(content_length)) {
                        (Some(diagnostics), Some(records)) => {
                            statistics.verdicts_served.fetch_add(1, Ordering::Relaxed);
                            let typed_source = match typed {
                                TypedOutcome::Served => verdict.typed.as_ref(),
                                TypedOutcome::Recompute => None,
                            };
                            (diagnostics, typed_source, records)
                        }
                        _ => {
                            // Revalidated, but a stored diagnostic or a
                            // stored directive record failed conversion:
                            // the same refusal as a moved answer, and it
                            // takes the typed half down with it - a
                            // discarded untyped half has nothing left to
                            // layer a typed serve over.
                            statistics
                                .verdicts_discarded
                                .fetch_add(1, Ordering::Relaxed);
                            recomputed()
                        }
                    }
                }
                VerdictLookup::Discarded => {
                    statistics
                        .verdicts_discarded
                        .fetch_add(1, Ordering::Relaxed);
                    recomputed()
                }
                VerdictLookup::Absent => {
                    statistics.verdicts_absent.fetch_add(1, Ordering::Relaxed);
                    recomputed()
                }
            };
        let typed = served_typed_diagnostics(inputs, file, typed_source);
        let outcomes = directive_outcomes(&records, &typed.matched);
        diagnostics.extend(typed.diagnostics);
        diagnostics.extend(reporting_portion(inputs, file, &outcomes));
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
    fn retain_unsuppressed_attributes_every_directive_that_admits() {
        use celerrate_semantics::{DirectiveOrigin, ResolvedDirective, SuppressionFilter};

        let db = super::AnalysisDatabase::default();
        let file = celerrate_db::SourceFile::new(&db, FileId::new(0), b"<?php\n$x = 1;\n".to_vec());

        // Two overlapping directives, both covering offset 5.
        let first = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(0), TextSize::from(5)),
            scope: TextRange::new(TextSize::from(0), TextSize::from(10)),
            filter: SuppressionFilter::All,
            identifiers: Vec::new(),
            widened_by: Vec::new(),
            origin: DirectiveOrigin::Foreign,
        };
        let second = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(3), TextSize::from(8)),
            scope: TextRange::new(TextSize::from(2), TextSize::from(12)),
            filter: SuppressionFilter::All,
            identifiers: Vec::new(),
            widened_by: Vec::new(),
            origin: DirectiveOrigin::Foreign,
        };
        let directives = vec![first, second];

        let mut both_admit = vec![diagnostic(0, 5)];
        let attributed = super::retain_unsuppressed(&db, file, &directives, &mut both_admit);
        assert_eq!(
            attributed,
            vec![0, 1],
            "both admitting directives are attributed, not just the first",
        );
        assert!(both_admit.is_empty(), "the admitted diagnostic is dropped");
    }

    #[test]
    fn retain_unsuppressed_attributes_only_the_admitting_directive() {
        use celerrate_semantics::{DirectiveOrigin, ResolvedDirective, SuppressionFilter};

        let db = super::AnalysisDatabase::default();
        let file = celerrate_db::SourceFile::new(&db, FileId::new(0), b"<?php\n$x = 1;\n".to_vec());

        // Only the second directive's scope covers offset 5.
        let first = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(0), TextSize::from(4)),
            scope: TextRange::new(TextSize::from(0), TextSize::from(4)),
            filter: SuppressionFilter::All,
            identifiers: Vec::new(),
            widened_by: Vec::new(),
            origin: DirectiveOrigin::Foreign,
        };
        let second = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(3), TextSize::from(8)),
            scope: TextRange::new(TextSize::from(2), TextSize::from(12)),
            filter: SuppressionFilter::All,
            identifiers: Vec::new(),
            widened_by: Vec::new(),
            origin: DirectiveOrigin::Foreign,
        };
        let directives = vec![first, second];

        let mut only_second_admits = vec![diagnostic(0, 5)];
        let attributed =
            super::retain_unsuppressed(&db, file, &directives, &mut only_second_admits);
        assert_eq!(attributed, vec![1]);
        assert!(only_second_admits.is_empty());
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
    fn a_portion_names_every_directive_that_admitted_a_diagnostic() {
        use crate::session::Session;

        // Line 2's one comment carries two directives (the foreign tag
        // first: the native identifier list runs to the end of the line,
        // so it must come last): the native one admits CEL0018, the
        // foreign blanket admits everything - any-match marks both. The
        // directive on line 4 admits nothing.
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("a.php"),
            "<?php\nnew MissingOne(); // @phpstan-ignore-line @celerrate-ignore CEL0018\n$x = 1;\n// @celerrate-ignore CEL0019\n$y = 2;\n",
        )
        .unwrap();

        let session = Session::start(root.path());
        let inputs = session.inputs();
        let &file = session.sources.values().next().unwrap();
        let portion = super::persistable_diagnostics(&inputs, file);
        assert!(portion.diagnostics.is_empty(), "{:?}", portion.diagnostics);

        let directives = celerrate_semantics::suppression_directives(&inputs.database, file);
        assert_eq!(directives.len(), 3, "{directives:?}");
        assert_eq!(portion.matched, vec![0, 1], "{directives:?}");
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
