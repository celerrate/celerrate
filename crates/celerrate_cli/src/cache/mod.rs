//! The persistent artifact cache: a content-addressed derived-artifact
//! cache above salsa, persisted to `.celerrate/cache/` and used to
//! re-seed a fresh database at startup. Nothing here is ever fatal:
//! every failure mode of a cache file answers by recomputation.
//!
//! The typed families (CEL0030-CEL0038) are their own artifact
//! class: `StoredVerdict.typed` carries their post-suppression
//! diagnostics alongside the class, function, and inferred-edge records
//! `crate::cache::verdict`'s layered validation replays against the live
//! project, the file-level counterpart of the fourth pack's per-body
//! `StoredInferredSignature`. `PERSIST_TYPED_ARTIFACTS` gates
//! both: off, `StoredVerdict.typed` stays `None` and the typed portion
//! is recomputed fresh on every path, cold or warm, exactly as before
//! these families existed.

pub mod identity;
pub mod pack;
pub mod snapshot;
pub mod statistics;
pub mod stored;
pub mod verdict;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use celerrate_db::{ContentHash, SourceFile};
use celerrate_semantics::{
    BodyQuery, ClassQuery, DeclarationKind, MemberKind, SymbolSpace, folded_member_key,
    folded_symbol_key, fully_qualified_name,
};
use celerrate_source::FileId;
use celerrate_types::{
    FunctionQuery, InferredBody, StoredClassDependency, StoredFunctionDependency,
    StoredInferredEdge, StoredInferredSignature, StoredSignatureKey, StoredType,
    class_surface_digest, function_signature_digest, inferred_body_types,
};
use serde::Serialize;

use crate::analysis::{AnalysisInputs, AnalysisOutcome};
use crate::database::AnalysisDatabase;
use crate::session::Session;

use pack::{Pack, PackHeader};
use snapshot::{
    CacheSnapshot, DIAGNOSTICS_PACK, INFERRED_SIGNATURES_PACK, ITEM_TREES_PACK, MEMBER_TREES_PACK,
};
use stored::{
    StoredDiagnostic, StoredDirective, StoredItemTree, StoredMemberTree, StoredRecord,
    StoredTypedVerdict, StoredVerdict,
};

/// One pack's entries in memory: sorted, deduplicated, one entry per key.
type TreeEntries = Vec<(ContentHash, StoredItemTree)>;
type MemberTreeEntries = Vec<(ContentHash, StoredMemberTree)>;
type VerdictEntries = Vec<(ContentHash, StoredVerdict)>;
type SignatureEntries = Vec<(StoredSignatureKey, StoredInferredSignature)>;

/// The persist lever for the typed-artifact families: `true` persists
/// the inferred-signature pack ([`collect_signature_entries`]) AND
/// populates `StoredVerdict.typed` ([`composed_verdict`]'s typed
/// half); `false` drops both. Fixed at `true` today (the lever has
/// never been pulled); no runtime toggle exists yet; until one does,
/// this is the named, reviewable hook a future flip lands on, and the
/// const is threaded into [`composed_verdict_with_lever`] so its two
/// branches stay unit-testable without one.
///
/// What pulling it to `false` would mean: every typed warm serve falls
/// back to fresh interprocedural inference, so the warm number for the
/// typed families converges toward cold-with-inference rather than
/// staying near the untyped warm floor, a tracked trade-off.
/// Escalating that flip past this const into a real release decision
/// (a CLI flag, a project setting) is a separate call, informed by
/// measured numbers.
pub(crate) const PERSIST_TYPED_ARTIFACTS: bool = true;

/// How one pack write ended.
enum PackWrite {
    /// Already on disk, byte-identical, under the current header.
    Unchanged,
    /// Encoded and atomically written.
    Written,
    /// Encoding or the atomic write failed; whatever was on disk before
    /// (if anything) is untouched.
    Failed,
}

/// Persists the packs after one completed pass, best-effort: an I/O
/// failure skips the write and nothing else. The session's snapshot is
/// replaced by what was actually WRITTEN, and only when every pack
/// confirms — whole or nothing, so the next cycle's equality check never
/// compares against a snapshot the disk does not hold. On failure the
/// old snapshot stays, the next pass recomputes the same entries and
/// retries the write; an occasional redundant rewrite of the healthy
/// packs alongside a retried failing one is harmless and best-effort.
///
/// Collecting the entries runs the very queries a panicked file left
/// unmemoized (`item_tree`, `member_tree`, `resolution_records`), so it
/// happens behind
/// `analysis::isolated`: a file `outcome.panicked` names is skipped
/// before either query runs for it, and anything that panics here
/// anyway — the guard exists for the unexpected, not the expected —
/// drops this persist silently rather than escaping `run`/`watch` as a
/// raw abort. The old snapshot stays and the next pass retries, exactly
/// as it already does for an I/O failure below.
pub fn persist(session: &mut Session, outcome: &AnalysisOutcome) {
    // Wall-clock read, legal here: this is the persist orchestration
    // layer, never a salsa query, and the reading feeds only
    // `CacheStatistics` — telemetry for the stats line, never analysis
    // or the rendered diagnostics. The general rule (orchestration code
    // only, never inside a query) is what makes this legal, not
    // exclusivity to this call site: `persist_timed` below reads the
    // clock three more times for `session.phases`, and `session.rs` and
    // `lib.rs` do the same for their own phases.
    let started = std::time::Instant::now();
    persist_timed(session, outcome);
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    session
        .statistics
        .persist_milliseconds
        .fetch_add(elapsed, std::sync::atomic::Ordering::Relaxed);
}

/// `persist`'s body, timed by its caller. Split out so every early
/// `return` below (an isolated collection failing, the cache directory
/// being unwritable) still has its elapsed time recorded — the timer
/// wraps the call, not each exit path individually.
fn persist_timed(session: &mut Session, outcome: &AnalysisOutcome) {
    let inputs = session.inputs();
    let database = &inputs.database;
    let current_range = session.configuration.php_version_range(database);
    let header = PackHeader::current(
        current_range,
        session.plugin_set_digest,
        session.configuration_digest,
    );
    let panicked: BTreeSet<FileId> = outcome.panicked.iter().copied().collect();

    let started = std::time::Instant::now();
    let collected =
        crate::analysis::isolated(|| collect_entries(&session.sources, &inputs, &panicked));
    session.phases.record(
        crate::phases::Phase::PersistCollectEntries,
        started.elapsed(),
    );
    let Ok((trees, member_trees, verdicts)) = collected else {
        return;
    };
    let started = std::time::Instant::now();
    let collected_signatures =
        crate::analysis::isolated(|| collect_signature_entries(&inputs, &panicked));
    session.phases.record(
        crate::phases::Phase::PersistCollectSignatures,
        started.elapsed(),
    );
    let Ok(signatures) = collected_signatures else {
        return;
    };

    if prepare_directory(&session.cache_directory).is_err() {
        session
            .statistics
            .persist_failed
            .fetch_add(4, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    // The header the on-disk packs were last confirmed to hold, derived
    // from the range the snapshot was loaded or last written under: under
    // `--watch` a manifest edit can move the range at runtime, and a
    // cycle whose entries happen to be byte-equal (item trees are
    // range-independent) must not skip the write in that case, or the
    // disk keeps a stale header that no later cycle ever revisits. The
    // schema, binary, and stub hash cannot move mid-process, so comparing
    // the range and the configuration digest is exactly comparing the
    // header.
    let header_moved = current_range != session.cache_loaded_range
        || session.configuration_digest != session.cache_loaded_configuration_digest;
    let started = std::time::Instant::now();
    let trees_written = write_when_changed(
        &session.cache_directory.join(ITEM_TREES_PACK),
        &header,
        &trees,
        &session.cache.item_trees,
        header_moved,
    );
    let member_trees_written = write_when_changed(
        &session.cache_directory.join(MEMBER_TREES_PACK),
        &header,
        &member_trees,
        &session.cache.member_trees,
        header_moved,
    );
    let verdicts_written = write_when_changed(
        &session.cache_directory.join(DIAGNOSTICS_PACK),
        &header,
        &verdicts,
        &session.cache.verdicts,
        header_moved,
    );
    // `PERSIST_TYPED_ARTIFACTS` gates the write attempt itself, not just
    // the collected entries: `collect_signature_entries` already answers
    // an empty `Vec` when the lever is off, but writing that empty `Vec`
    // through `write_when_changed` would still create an (empty) pack
    // file the first time — the lever's contract is that the pack is
    // never written at all, not written empty.
    let signatures_written = if PERSIST_TYPED_ARTIFACTS {
        write_when_changed(
            &session.cache_directory.join(INFERRED_SIGNATURES_PACK),
            &header,
            &signatures,
            &session.cache.signatures,
            header_moved,
        )
    } else {
        PackWrite::Unchanged
    };
    session
        .phases
        .record(crate::phases::Phase::PersistPackWrites, started.elapsed());
    for write in [
        &trees_written,
        &member_trees_written,
        &verdicts_written,
        &signatures_written,
    ] {
        let counter = match write {
            PackWrite::Unchanged => &session.statistics.persist_skipped,
            PackWrite::Written => &session.statistics.persist_written,
            PackWrite::Failed => &session.statistics.persist_failed,
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if !matches!(trees_written, PackWrite::Failed)
        && !matches!(member_trees_written, PackWrite::Failed)
        && !matches!(verdicts_written, PackWrite::Failed)
        && !matches!(signatures_written, PackWrite::Failed)
    {
        session.cache = Arc::new(CacheSnapshot {
            item_trees: trees.into_iter().collect(),
            member_trees: member_trees.into_iter().collect(),
            verdicts: verdicts.into_iter().collect(),
            signatures: signatures.into_iter().collect(),
        });
        session.cache_loaded_range = current_range;
        session.cache_loaded_configuration_digest = session.configuration_digest;
    }
}

/// The trees, member trees, and verdicts one pass may persist: every
/// analyzed file's item tree and member tree, and every reported file's
/// composed verdict, save for whatever `panicked` names. A panicked
/// file's per-query memoization is empty (`guarded` never lets a
/// panicked query's result reach salsa's cache), so recomputing
/// `item_tree`, `member_tree`, or `resolution_records` for it here would
/// deterministically reproduce the same panic; skipping it in every
/// loop, before any query runs, is what keeps this call free of that
/// panic rather than merely surviving it.
fn collect_entries(
    sources: &BTreeMap<FileId, SourceFile>,
    inputs: &AnalysisInputs,
    panicked: &BTreeSet<FileId>,
) -> (TreeEntries, MemberTreeEntries, VerdictEntries) {
    use rayon::prelude::*;

    // The salsa storage is `Send` but not `Sync` (see
    // `analysis::analyze`): every task's handle clone is made up front
    // on this thread and handed to rayon as owned data. The queries
    // underneath are memoized from the pass that just ran, so the
    // parallel work is mostly the `Stored*::of` conversions.
    let tree_tasks: Vec<(SourceFile, AnalysisInputs)> = sources
        .iter()
        .filter(|(file_id, _)| !panicked.contains(file_id))
        .map(|(_, &file)| (file, inputs.clone()))
        .collect();
    let (mut trees, mut member_trees): (TreeEntries, MemberTreeEntries) = tree_tasks
        .into_par_iter()
        .map(|(file, inputs)| {
            let database = &inputs.database;
            let hash = celerrate_db::content_hash(database, file);
            (
                (
                    hash,
                    StoredItemTree::of(celerrate_semantics::item_tree(database, file)),
                ),
                (
                    hash,
                    StoredMemberTree::of(celerrate_semantics::member_tree(database, file)),
                ),
            )
        })
        .unzip();
    sort_entries(&mut trees);
    sort_entries(&mut member_trees);

    let verdict_tasks: Vec<(SourceFile, AnalysisInputs)> = inputs
        .reported
        .iter()
        .filter(|file| !panicked.contains(&file.file_id(&inputs.database)))
        .map(|&file| (file, inputs.clone()))
        .collect();
    let mut verdicts: VerdictEntries = verdict_tasks
        .into_par_iter()
        .map(|(file, inputs)| {
            let database = &inputs.database;
            let file_id = file.file_id(database);
            let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
            // Mirrors `analyze_one`: a validated hit's whole entry is only
            // reused byte-for-byte when every stored diagnostic still
            // re-interns AND the typed half itself validated
            // (`TypedOutcome::Served`) — a `Recompute` typed outcome means
            // this file's typed portion is already stale against the live
            // project, and reusing the old entry verbatim would persist that
            // staleness forward untouched (never re-checked until the whole
            // entry moves for some unrelated reason). `composed_verdict`
            // recomputes both halves in that case; every query underneath it
            // is memoized from the pass that already ran, so this costs
            // nothing beyond a salsa cache read.
            let stored = match verdict::lookup_verdict(&inputs, file) {
                verdict::VerdictLookup::Hit {
                    verdict: stored,
                    typed: verdict::TypedOutcome::Served,
                } if stored.diagnostics.iter().all(|diagnostic| {
                    diagnostic.to_diagnostic(file_id, content_length).is_some()
                }) && stored.directives_convert(content_length).is_some() =>
                {
                    stored.clone()
                }
                _ => composed_verdict(&inputs, file),
            };
            (celerrate_db::content_hash(database, file), stored)
        })
        .collect();
    sort_entries(&mut verdicts);

    (trees, member_trees, verdicts)
}

/// The inferred-signature pack's entries: one per
/// eligible body — every REPORTED file's free functions and
/// `MemberKind::Method` members — save for whatever `panicked` names,
/// mirroring `collect_entries`'s own panic guard exactly (this reads
/// `member_tree` and `inferred_body_types`, the same panic-reproducing
/// concern). Answers an empty `Vec` outright when
/// [`PERSIST_TYPED_ARTIFACTS`] is off.
///
/// `inputs.reported` (project files only), never the broader `sources`/
/// `inputs.files` set the item-tree and member-tree packs iterate: this
/// function calls `inferred_body_types`, and `analysis::analyze` fans
/// interprocedural inference over `inputs.reported` alone (`analyze`'s
/// own rustdoc — dependency files exist to resolve names against, never
/// to be inferred themselves). Persist may only READ a result the
/// analysis pass already computed (this module's own read-only
/// invariant); widening this loop to `sources` would force a FRESH,
/// unbounded interprocedural inference of every vendor body the pass
/// never touched — thousands of them on a real Composer project — and
/// let a panic in code the pass never risked abort every pack this
/// persist writes, not just this one.
///
/// Three conservative exclusions, all falling back to recomputation
/// rather than persisting a wrong answer:
/// - a vendor (non-reported) file's own bodies (above) — recorded
///   debt: persisting the vendor callees a reported file's own
///   inferred edges transitively reach is a possible future
///   optimization, revisited only if the numbers show the
///   cross-boundary vendor cutoff matters;
/// - a `DeclarationKind::Trait` class-like's own methods (a trait's
///   memo key carries the using class's context, which the trait's own
///   file cannot enumerate);
/// - a class-like with no stable folded key (an anonymous class — its
///   key is a synthetic, walk-relative `AstId`, not a name a caller in
///   another file could ever cite).
///
/// The persist key is derived from the member-tree entry itself
/// (`folded_symbol_key`/`folded_member_key`), never from the
/// crate-private `celerrate_types::inference::BodyOwner` — this crate
/// cannot see that type, and does not need to: the two agree by
/// construction, since both fold the same written name through the
/// same public helpers.
fn collect_signature_entries(
    inputs: &AnalysisInputs,
    panicked: &BTreeSet<FileId>,
) -> SignatureEntries {
    if !PERSIST_TYPED_ARTIFACTS {
        return Vec::new();
    }
    let database = &inputs.database;
    let mut signatures: SignatureEntries = Vec::new();

    for &file in inputs.reported.iter() {
        let file_id = file.file_id(database);
        if panicked.contains(&file_id) {
            continue;
        }
        let content = celerrate_db::content_hash(database, file);
        let tree = celerrate_semantics::member_tree(database, file);

        for function in &tree.functions {
            let Some(inferred) = inferred_body_types(
                database,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
                file,
                BodyQuery::new(database, function.ast_id),
            ) else {
                continue;
            };
            let key = folded_symbol_key(
                SymbolSpace::Function,
                &fully_qualified_name(&function.namespace, &function.name),
            );
            signatures.push((
                StoredSignatureKey::Function { key },
                stored_signature_of(database, inputs, content, inferred),
            ));
        }

        for class in &tree.classes {
            // The trait exclusion: a trait's memo key includes
            // the using class's context, which this file-local walk has
            // no way to enumerate.
            if class.kind == DeclarationKind::Trait {
                continue;
            }
            // The anonymous-class exclusion: no stable folded key for a
            // caller in another file to ever cite.
            let Some(name) = class.name.as_deref() else {
                continue;
            };
            let class_key = folded_symbol_key(
                SymbolSpace::ClassLike,
                &fully_qualified_name(&class.namespace, name),
            );
            for member in &class.members {
                if member.kind != MemberKind::Method {
                    continue;
                }
                let Some(inferred) = inferred_body_types(
                    database,
                    inputs.files,
                    inputs.stubs,
                    inputs.configuration,
                    file,
                    BodyQuery::new(database, member.ast_id),
                ) else {
                    continue;
                };
                let member_key = folded_member_key(MemberKind::Method, &member.name);
                signatures.push((
                    StoredSignatureKey::Method {
                        class_key: class_key.clone(),
                        member_key,
                    },
                    stored_signature_of(database, inputs, content, inferred),
                ));
            }
        }
    }

    sort_entries(&mut signatures);
    signatures
}

/// One body's `InferredBody` mirrored into its persisted form: the
/// return through `StoredType::of`, and every class, function, and
/// inferred-tier callee `inferred.dependencies` names, read verbatim —
/// never re-derived — with a digest stamped on each recorded class and
/// function key.
fn stored_signature_of<'db>(
    database: &'db AnalysisDatabase,
    inputs: &AnalysisInputs,
    content: ContentHash,
    inferred: &InferredBody<'db>,
) -> StoredInferredSignature {
    let classes = inferred
        .dependencies
        .classes
        .iter()
        .map(|key| StoredClassDependency {
            key: key.clone(),
            digest: class_surface_digest(
                database,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
                ClassQuery::new(database, key.clone()),
            ),
        })
        .collect();
    let functions = inferred
        .dependencies
        .functions
        .iter()
        .map(|key| StoredFunctionDependency {
            key: key.clone(),
            digest: function_signature_digest(
                database,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
                FunctionQuery::new(database, key.clone()),
            ),
        })
        .collect();
    let inferred_edges = inferred
        .dependencies
        .inferred_functions
        .iter()
        .map(|(key, of)| StoredInferredEdge {
            callee: StoredSignatureKey::Function { key: key.clone() },
            return_type: StoredType::of(database, *of),
        })
        .chain(inferred.dependencies.inferred_methods.iter().map(
            |((class_key, member_key), of)| StoredInferredEdge {
                callee: StoredSignatureKey::Method {
                    class_key: class_key.clone(),
                    member_key: member_key.clone(),
                },
                return_type: StoredType::of(database, *of),
            },
        ))
        .collect();
    StoredInferredSignature {
        content,
        return_type: StoredType::of(database, inferred.return_type),
        classes,
        functions,
        inferred: inferred_edges,
    }
}

/// One reported file's verdict — its diagnostics through the
/// cache-servable composition point, with the records the entry must
/// revalidate against, and (when the lever is on) the typed half
/// alongside them. Delegates to [`composed_verdict_with_lever`] with
/// [`PERSIST_TYPED_ARTIFACTS`]; the lever is parameterized into that
/// function, rather than read inside it, so its two branches are
/// unit-testable without a runtime toggle (`the_lever_persists_untyped_
/// only_verdicts`, this module's own tests).
fn composed_verdict(inputs: &AnalysisInputs, file: celerrate_db::SourceFile) -> StoredVerdict {
    composed_verdict_with_lever(inputs, file, PERSIST_TYPED_ARTIFACTS)
}

/// [`composed_verdict`]'s body, with the persist lever threaded in as a
/// parameter. The untyped half is computed exactly as it always was
/// (`persistable_diagnostics` plus `resolution_records`'s projection,
/// unconditionally); the typed half is `Some` only when `persist_typed`
/// is set, through [`composed_typed_verdict`].
fn composed_verdict_with_lever(
    inputs: &AnalysisInputs,
    file: celerrate_db::SourceFile,
    persist_typed: bool,
) -> StoredVerdict {
    let database = &inputs.database;
    let portion = crate::analysis::persistable_diagnostics(inputs, file);
    let records = celerrate_semantics::resolution_records(
        database,
        file,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
    );
    let directives = celerrate_semantics::suppression_directives(database, file);
    let typed = if persist_typed {
        Some(composed_typed_verdict(inputs, file))
    } else {
        None
    };
    StoredVerdict {
        diagnostics: portion
            .diagnostics
            .iter()
            .map(StoredDiagnostic::of)
            .collect(),
        records: records.iter().map(StoredRecord::of).collect(),
        directives: directives
            .iter()
            .enumerate()
            .map(|(index, directive)| {
                let matched = u32::try_from(index)
                    .map(|index| portion.matched.binary_search(&index).is_ok())
                    .unwrap_or(false);
                StoredDirective::of(directive, matched)
            })
            .collect(),
        typed,
    }
}

/// The typed half of one reported file's persisted verdict:
/// `typed_portion`'s post-suppression diagnostics, stored,
/// alongside the class, function, and inferred-edge records
/// `typed_file_verdicts(...).dependencies` recorded, the file-level
/// mirror of [`stored_signature_of`] above, reading the exact same
/// `FileDependencies` shape `analyze_one`'s recompute path already
/// produces (never a separate walk), digests stamped through the same
/// queries [`stored_signature_of`] stamps them through, and every
/// inferred edge's `StoredType` carried as `FileDependencies` already
/// recorded it (no re-derivation, no `TypeId` involved:
/// `FileDependencies::extend_from_body` already mirrored it at the
/// walk's own boundary).
fn composed_typed_verdict(
    inputs: &AnalysisInputs,
    file: celerrate_db::SourceFile,
) -> StoredTypedVerdict {
    let database = &inputs.database;
    let portion = crate::analysis::typed_portion(inputs, file);
    let result = celerrate_types::typed_file_verdicts(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        file,
    );
    let classes = result
        .dependencies
        .classes
        .iter()
        .map(|key| StoredClassDependency {
            key: key.clone(),
            digest: class_surface_digest(
                database,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
                ClassQuery::new(database, key.clone()),
            ),
        })
        .collect();
    let functions = result
        .dependencies
        .functions
        .iter()
        .map(|key| StoredFunctionDependency {
            key: key.clone(),
            digest: function_signature_digest(
                database,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
                FunctionQuery::new(database, key.clone()),
            ),
        })
        .collect();
    let inferred = result
        .dependencies
        .inferred_functions
        .iter()
        .map(|(key, return_type)| StoredInferredEdge {
            callee: StoredSignatureKey::Function { key: key.clone() },
            return_type: return_type.clone(),
        })
        .chain(result.dependencies.inferred_methods.iter().map(
            |((class_key, member_key), return_type)| StoredInferredEdge {
                callee: StoredSignatureKey::Method {
                    class_key: class_key.clone(),
                    member_key: member_key.clone(),
                },
                return_type: return_type.clone(),
            },
        ))
        .collect();
    StoredTypedVerdict {
        diagnostics: portion
            .diagnostics
            .iter()
            .map(StoredDiagnostic::of)
            .collect(),
        classes,
        functions,
        inferred,
        matched_directives: portion.matched,
    }
}

/// Deterministic pack order: by key, one entry per key. Generic over the
/// key type (the signature pack keys by `StoredSignatureKey`, not by
/// `ContentHash`), `dedup_by` keeps the
/// FIRST of any run of equal keys, which is what lets a duplicate
/// definition (two files declaring the same function name, an
/// already-diagnosed unknown-symbol condition upstream) resolve
/// deterministically by sorted-key order rather than by traversal
/// happenstance.
fn sort_entries<Key: Ord, Entry>(entries: &mut Vec<(Key, Entry)>) {
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.dedup_by(|left, right| left.0 == right.0);
}

/// Creates the cache directory and its self-ignoring `.gitignore`, and
/// sweeps crash debris from both `.celerrate/cache/` and its parent
/// `.celerrate/`. The `.gitignore` goes through the atomic write: a
/// plain write torn by a crash left a half-written file that was
/// never repaired, since only existence is checked. Its temporary
/// lands in `.celerrate/` (the `.gitignore`'s own parent), not in
/// `.celerrate/cache/`, so the parent gets the same best-effort sweep
/// or a crash during first-time `.gitignore` creation leaves a
/// `.tmp*` orphan forever.
fn prepare_directory(cache_directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_directory)?;
    sweep_crash_debris(cache_directory);
    if let Some(dot_celerrate) = cache_directory.parent() {
        sweep_crash_debris(dot_celerrate);
        let gitignore = dot_celerrate.join(".gitignore");
        if !gitignore.exists() {
            pack::write_atomically(&gitignore, b"*\n")?;
        }
    }
    Ok(())
}

/// Best-effort removal of temporary files a crash mid-write left
/// behind: `write_atomically`'s temporaries carry
/// `pack::TEMPORARY_FILE_PREFIX`, survive SIGKILL and power loss, and
/// nothing else ever removes them. A concurrent process mid-persist can
/// lose its temporary to this sweep; its rename then fails, that persist
/// is skipped, and its next pass rewrites — the same best-effort answer
/// as any other write failure.
fn sweep_crash_debris(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(pack::TEMPORARY_FILE_PREFIX)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Rewrites a pack only when its entries differ from the loaded state,
/// or when `header_moved` says the header the file was last confirmed to
/// hold is not the current one. Answers how `path` came to hold `entries`
/// under `header`: `Unchanged` when it was already unchanged, present,
/// and under the current header; `Written` when the write just
/// succeeded; `Failed` when encoding or the atomic write failed, in
/// which case whatever was on disk before (if anything) is untouched.
/// `persist` only swaps the session's snapshot when neither pack failed.
fn write_when_changed<
    Key: Eq + std::hash::Hash + Serialize + Clone,
    Entry: Serialize + PartialEq + Clone,
>(
    path: &Path,
    header: &PackHeader,
    entries: &[(Key, Entry)],
    loaded: &HashMap<Key, Entry>,
    header_moved: bool,
) -> PackWrite {
    let unchanged = !header_moved
        && entries.len() == loaded.len()
        && entries
            .iter()
            .all(|(key, value)| loaded.get(key) == Some(value));
    if unchanged && path.is_file() {
        return PackWrite::Unchanged;
    }
    let Some(bytes) = pack::encode(&Pack {
        header: header.clone(),
        entries: entries.to_vec(),
    }) else {
        return PackWrite::Failed;
    };
    if pack::write_atomically(path, &bytes).is_ok() {
        PackWrite::Written
    } else {
        PackWrite::Failed
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_project::{PhpVersion, PhpVersionRange};
    use salsa::Setter as _;

    use crate::analysis::AnalysisOutcome;
    use crate::session::Session;

    /// A project on disk, written into a temporary directory.
    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let path = root.path().join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        root
    }

    /// The invariant a parallel rewrite of `collect_entries` must
    /// preserve: two collections of the same inputs, run back to back,
    /// yield byte-identical, identically sorted entries. Several files
    /// (with a dependency edge between two of them) give the collection
    /// more than one task to interleave, so this pins cross-run identity
    /// rather than a single-file case where reordering could never
    /// surface; it is a modest fixture, not a stand-in for fan-out at
    /// corpus scale.
    #[test]
    fn collecting_entries_twice_yields_identical_sorted_entries() {
        let root = project(&[
            (
                "composer.json",
                r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            ("src/Alpha.php", "<?php\nnamespace App;\nclass Alpha {}\n"),
            ("src/Beta.php", "<?php\nnamespace App;\nclass Beta {}\n"),
            ("src/Gamma.php", "<?php\nnamespace App;\nclass Gamma {}\n"),
            ("src/Delta.php", "<?php\nnamespace App;\nnew Alpha();\n"),
        ]);
        let session = Session::start(root.path());
        let inputs = session.inputs();
        let panicked = std::collections::BTreeSet::new();

        let first = super::collect_entries(&session.sources, &inputs, &panicked);
        let second = super::collect_entries(&session.sources, &inputs, &panicked);

        assert_eq!(first.0, second.0, "item-tree entries stay identical");
        assert_eq!(first.1, second.1, "member-tree entries stay identical");
        assert_eq!(first.2, second.2, "verdict entries stay identical");
        assert!(
            !first.0.is_empty(),
            "the fixture must actually collect trees"
        );
    }

    /// A file the pass reported as panicked yields no verdict entry, and
    /// no item-tree entry either: nothing a panic touched enters the
    /// persistent cache. A panicked file's `item_tree` query was never
    /// memoized by the pass (`guarded` catches the panic before salsa
    /// ever caches a result), so recomputing it here would deterministically
    /// reproduce the same panic, unguarded, and it would escape
    /// `run`/`watch` as a raw abort rather than the internal-error report
    /// the pass itself already produced.
    #[test]
    fn a_panicked_file_is_never_persisted() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        std::fs::write(root.path().join("b.php"), "<?php class B {}").unwrap();
        let mut session = Session::start(root.path());
        let panicked = *session.sources.keys().next().unwrap();

        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: vec![panicked],
        };
        super::persist(&mut session, &outcome);

        assert_eq!(
            session.cache.verdicts.len(),
            1,
            "the healthy file has a verdict, the panicked one does not",
        );
        assert_eq!(
            session.cache.item_trees.len(),
            1,
            "the panicked file's tree is absent too: its query was never memoized",
        );
        assert_eq!(
            session.cache.member_trees.len(),
            1,
            "and its member tree is absent for the same reason",
        );
    }

    /// The persist lever: `PERSIST_TYPED_ARTIFACTS` is fixed `true`
    /// today, so this pins the ON branch, the inferred-signature pack
    /// IS written and the snapshot IS populated from it. The OFF
    /// branch (`StoredVerdict.typed` staying `None`) is
    /// `the_lever_persists_untyped_only_verdicts` just below: no
    /// runtime toggle exists yet to drive it end to end from
    /// `persist`, and a `const` cannot be mutated from a test, so
    /// that test pins `composed_verdict_with_lever`'s two branches
    /// directly instead. `PERSIST_TYPED_ARTIFACTS` and
    /// `collect_signature_entries` are `pub(crate)`/private, invisible
    /// to the external `tests/cache_seeding.rs` integration crate, so
    /// this lever test lives here instead of there.
    #[test]
    fn the_persist_lever_drops_the_typed_artifacts() {
        // `PERSIST_TYPED_ARTIFACTS` is a `const`, fixed on until a later
        // task threads a runtime toggle; this exercises exactly the
        // value it is fixed to today (`assert!` on a compile-time
        // constant is itself flagged, so the const's value is asserted
        // through its effect below instead).
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("a.php"),
            "<?php function f() { return 1; }",
        )
        .unwrap();
        let mut session = Session::start(root.path());
        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: Vec::new(),
        };
        super::persist(&mut session, &outcome);

        assert!(
            root.path()
                .join(".celerrate/cache")
                .join(super::snapshot::INFERRED_SIGNATURES_PACK)
                .is_file(),
            "the lever is on: the pack is written",
        );
        assert_eq!(session.cache.signatures.len(), 1, "one free function");
    }

    /// The persist lever test for the typed half: `composed_verdict_with_
    /// lever`'s two branches, pinned directly since `composed_verdict`
    /// and the lever are both private/`pub(crate)`, invisible to the
    /// external `tests/cache_seeding.rs` integration crate, exactly the
    /// same visibility seam `the_persist_lever_drops_the_typed_artifacts`
    /// above already worked around for its own lever. Off,
    /// `StoredVerdict.typed` stays `None`; on, it is populated — the two
    /// branches a runtime toggle would otherwise flip, exercised here as
    /// a plain function argument instead.
    #[test]
    fn the_lever_persists_untyped_only_verdicts() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("a.php"),
            "<?php function f() { return 1; }",
        )
        .unwrap();
        let session = Session::start(root.path());
        let inputs = session.inputs();
        let &file = inputs.reported.first().unwrap();

        let off = super::composed_verdict_with_lever(&inputs, file, false);
        assert!(
            off.typed.is_none(),
            "the lever off: StoredVerdict.typed must stay None",
        );

        let on = super::composed_verdict_with_lever(&inputs, file, true);
        assert!(
            on.typed.is_some(),
            "the lever on: StoredVerdict.typed must be populated",
        );
        assert_eq!(
            off.diagnostics, on.diagnostics,
            "the untyped half is identical either way: the lever only ever \
             touches the typed field",
        );
        assert_eq!(
            off.records, on.records,
            "the untyped half's records are identical either way too",
        );
    }

    /// When one pack cannot be written, neither the pack that could not
    /// be replaced nor the snapshot may pretend the write happened: the
    /// old snapshot stays so the next pass recomputes and retries.
    /// `write_atomically`'s rename fails on Linux when the destination
    /// is a directory, which lets this be reproduced deterministically,
    /// with no chmod and no root privileges.
    #[test]
    fn a_write_failure_leaves_the_old_snapshot_in_place() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        assert!(
            session.cache.verdicts.is_empty(),
            "a fresh project starts with no snapshot",
        );

        let cache_directory = root.path().join(".celerrate/cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        std::fs::create_dir(cache_directory.join(super::snapshot::ITEM_TREES_PACK)).unwrap();

        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: Vec::new(),
        };
        super::persist(&mut session, &outcome);

        assert!(
            session.cache.verdicts.is_empty(),
            "the item-trees pack could not be written, so the snapshot must not swap",
        );
        assert!(
            cache_directory
                .join(super::snapshot::ITEM_TREES_PACK)
                .is_dir(),
            "the obstruction is untouched: the failed rename never replaced it",
        );

        std::fs::remove_dir(cache_directory.join(super::snapshot::ITEM_TREES_PACK)).unwrap();
        super::persist(&mut session, &outcome);

        assert!(
            cache_directory
                .join(super::snapshot::ITEM_TREES_PACK)
                .is_file(),
            "the obstruction is gone, so this pass writes the pack",
        );
        assert_eq!(
            session.cache.verdicts.len(),
            1,
            "every pack confirmed, so the snapshot now swaps",
        );
    }

    /// A range moved between two persists whose entries happen to be
    /// byte-equal (item trees are range-independent, so this is exactly
    /// what a `--watch` cycle sees right after a `composer.json` edit
    /// moves the PHP range). The unchanged-entries fast path must not
    /// skip the write in that case, or the pack on disk keeps the OLD
    /// header forever: every later cycle would keep comparing against the
    /// swapped in-memory snapshot and keep skipping, and a fresh process
    /// would reject the pack and start cold every time.
    #[test]
    fn a_moved_range_rewrites_the_pack_even_with_unchanged_entries() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());

        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: Vec::new(),
        };
        super::persist(&mut session, &outcome);
        let original_range = session.cache_loaded_range;
        let original_header = super::pack::PackHeader::current(
            original_range,
            session.plugin_set_digest,
            session.configuration_digest,
        );

        let item_trees_path = session
            .cache_directory
            .join(super::snapshot::ITEM_TREES_PACK);
        let written = std::fs::read(&item_trees_path).unwrap();
        assert!(
            super::pack::decode::<Vec<(celerrate_db::ContentHash, super::stored::StoredItemTree)>>(
                &written,
                &original_header,
            )
            .is_some(),
            "sanity: the first persist wrote under the original range's header",
        );

        // Simulate a `composer.json` edit moving the range mid-watch,
        // without touching the analyzed files: the entries the next
        // persist computes are byte-equal to what is already loaded.
        let moved_range = PhpVersionRange::point(PhpVersion::new(9, 9));
        session
            .configuration
            .set_php_version_range(&mut session.database)
            .to(moved_range);
        // Simulate a `celerrate.toml` edit moving the configuration
        // digest in the same cycle: without the sync below, a mid-watch
        // configuration edit would disable the cache for the rest of the
        // process, comparing against the stale digest forever.
        let moved_digest = [0xab; 32];
        session.configuration_digest = moved_digest;
        super::persist(&mut session, &outcome);

        assert_eq!(
            session.cache_loaded_range, moved_range,
            "persist adopts the new range once the rewrite is confirmed",
        );
        assert_eq!(
            session.cache_loaded_configuration_digest, moved_digest,
            "persist adopts the new configuration digest once the rewrite is confirmed",
        );
        let moved_header = super::pack::PackHeader::current(
            moved_range,
            session.plugin_set_digest,
            session.configuration_digest,
        );
        let rewritten = std::fs::read(&item_trees_path).unwrap();
        assert!(
            super::pack::decode::<Vec<(celerrate_db::ContentHash, super::stored::StoredItemTree)>>(
                &rewritten,
                &moved_header,
            )
            .is_some(),
            "the pack on disk now decodes under the moved header",
        );
        assert!(
            super::pack::decode::<Vec<(celerrate_db::ContentHash, super::stored::StoredItemTree)>>(
                &rewritten,
                &original_header,
            )
            .is_none(),
            "and no longer under the stale one",
        );
    }

    /// `write_atomically`'s temporary files (the
    /// `.tmp` prefix `tempfile` uses) survive SIGKILL and power loss in
    /// `.celerrate/cache/`, and nothing ever swept them. `persist` now
    /// sweeps them best-effort; anything not matching the prefix is
    /// someone else's file and stays. On the same whole-branch review:
    /// `write_atomically(&gitignore, ...)`'s temporary lands in
    /// `.celerrate/`, the target's parent, not in `.celerrate/cache/`, so a
    /// crash during first-time `.gitignore` creation left a `.tmp*` orphan
    /// no sweep ever reached; `prepare_directory` now sweeps the parent
    /// too, same prefix, same best-effort.
    #[test]
    fn crash_debris_is_swept_and_other_files_are_not() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());

        let cache_directory = root.path().join(".celerrate/cache");
        let dot_celerrate = root.path().join(".celerrate");
        std::fs::create_dir_all(&cache_directory).unwrap();
        std::fs::write(cache_directory.join(".tmpAbC123"), b"debris").unwrap();
        std::fs::write(cache_directory.join("unrelated.bin"), b"not ours").unwrap();
        std::fs::write(dot_celerrate.join(".tmpDeF456"), b"parent debris").unwrap();
        std::fs::write(
            dot_celerrate.join("unrelated-parent.bin"),
            b"not ours either",
        )
        .unwrap();

        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: Vec::new(),
        };
        super::persist(&mut session, &outcome);

        assert!(
            !cache_directory.join(".tmpAbC123").exists(),
            "the crash debris is gone",
        );
        assert!(
            cache_directory.join("unrelated.bin").exists(),
            "only the .tmp prefix is ours to sweep",
        );
        assert!(
            !dot_celerrate.join(".tmpDeF456").exists(),
            "the parent directory's crash debris is gone too",
        );
        assert!(
            dot_celerrate.join("unrelated-parent.bin").exists(),
            "the parent sweep only ever touches the .tmp prefix",
        );
    }

    /// A persist that writes, a
    /// persist that skips, and a persist that fails are each counted, so
    /// a permanently unwritable cache directory is at least observable.
    #[test]
    fn persist_outcomes_are_counted() {
        use std::sync::atomic::Ordering;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: Vec::new(),
        };

        super::persist(&mut session, &outcome);
        assert_eq!(
            session.statistics.persist_written.load(Ordering::Relaxed),
            4,
            "item trees, member trees, diagnostics, and inferred signatures",
        );

        super::persist(&mut session, &outcome);
        assert_eq!(
            session.statistics.persist_skipped.load(Ordering::Relaxed),
            4
        );

        // Obstruct one pack: its rename fails deterministically (rename
        // onto a directory), the other packs are unchanged.
        let cache_directory = root.path().join(".celerrate/cache");
        std::fs::remove_file(cache_directory.join(super::snapshot::ITEM_TREES_PACK)).unwrap();
        std::fs::create_dir(cache_directory.join(super::snapshot::ITEM_TREES_PACK)).unwrap();
        super::persist(&mut session, &outcome);
        assert_eq!(session.statistics.persist_failed.load(Ordering::Relaxed), 1);
    }
}
