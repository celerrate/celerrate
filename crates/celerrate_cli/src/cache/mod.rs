//! The persistent artifact cache: a content-addressed derived-artifact
//! cache above salsa, persisted to `.celerrate/cache/` and used to
//! re-seed a fresh database at startup. Nothing here is ever fatal:
//! every failure mode of a cache file answers by recomputation.
//!
//! The typed families (CEL0030-CEL0038) are plan 9a's artifact class,
//! not this one's: `StoredVerdict` persists only the syntax, decode,
//! and semantic families, and stays that way with no format bump. The
//! typed portion is recomputed fresh on every path, cold or warm.

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
use celerrate_source::FileId;
use serde::Serialize;

use crate::analysis::{AnalysisInputs, AnalysisOutcome};
use crate::session::Session;

use pack::{Pack, PackHeader};
use snapshot::{CacheSnapshot, DIAGNOSTICS_PACK, ITEM_TREES_PACK, MEMBER_TREES_PACK};
use stored::{StoredDiagnostic, StoredItemTree, StoredMemberTree, StoredRecord, StoredVerdict};

/// One pack's entries in memory: content-addressed, sorted, deduplicated.
type TreeEntries = Vec<(ContentHash, StoredItemTree)>;
type MemberTreeEntries = Vec<(ContentHash, StoredMemberTree)>;
type VerdictEntries = Vec<(ContentHash, StoredVerdict)>;

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
    let inputs = session.inputs();
    let database = &inputs.database;
    let current_range = session.configuration.php_version_range(database);
    let header = PackHeader::current(current_range);
    let panicked: BTreeSet<FileId> = outcome.panicked.iter().copied().collect();

    let Ok((trees, member_trees, verdicts)) =
        crate::analysis::isolated(|| collect_entries(&session.sources, &inputs, &panicked))
    else {
        return;
    };

    if prepare_directory(&session.cache_directory).is_err() {
        session
            .statistics
            .persist_failed
            .fetch_add(3, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    // The header the on-disk packs were last confirmed to hold, derived
    // from the range the snapshot was loaded or last written under: under
    // `--watch` a manifest edit can move the range at runtime, and a
    // cycle whose entries happen to be byte-equal (item trees are
    // range-independent) must not skip the write in that case, or the
    // disk keeps a stale header that no later cycle ever revisits. The
    // schema, binary, and stub hash cannot move mid-process, so comparing
    // the range alone is exactly comparing the header.
    let header_moved = current_range != session.cache_loaded_range;
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
    for write in [&trees_written, &member_trees_written, &verdicts_written] {
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
    {
        session.cache = Arc::new(CacheSnapshot {
            item_trees: trees.into_iter().collect(),
            member_trees: member_trees.into_iter().collect(),
            verdicts: verdicts.into_iter().collect(),
        });
        session.cache_loaded_range = current_range;
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
    let database = &inputs.database;

    let mut trees: TreeEntries = sources
        .iter()
        .filter(|(file_id, _)| !panicked.contains(file_id))
        .map(|(_, &file)| {
            (
                celerrate_db::content_hash(database, file),
                StoredItemTree::of(celerrate_semantics::item_tree(database, file)),
            )
        })
        .collect();
    sort_entries(&mut trees);

    let mut member_trees: MemberTreeEntries = sources
        .iter()
        .filter(|(file_id, _)| !panicked.contains(file_id))
        .map(|(_, &file)| {
            (
                celerrate_db::content_hash(database, file),
                StoredMemberTree::of(celerrate_semantics::member_tree(database, file)),
            )
        })
        .collect();
    sort_entries(&mut member_trees);

    let mut verdicts: VerdictEntries = Vec::new();
    for &file in inputs.reported.iter() {
        let file_id = file.file_id(database);
        if panicked.contains(&file_id) {
            continue;
        }
        let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
        // Mirrors `analyze_one`: a validated hit is only reused when
        // every stored diagnostic still re-interns, or `persist` would
        // re-persist an entry the pass itself refused to serve.
        let stored = match verdict::validated_verdict(inputs, file) {
            Some(stored)
                if stored.diagnostics.iter().all(|diagnostic| {
                    diagnostic.to_diagnostic(file_id, content_length).is_some()
                }) =>
            {
                stored.clone()
            }
            _ => composed_verdict(inputs, file),
        };
        verdicts.push((celerrate_db::content_hash(database, file), stored));
    }
    sort_entries(&mut verdicts);

    (trees, member_trees, verdicts)
}

/// One reported file's verdict — its diagnostics through the
/// cache-servable composition point, with the records the entry must
/// revalidate against. Every query here is memoized from the pass. The
/// typed families never reach this: `persistable_diagnostics` is
/// exactly what a pack may carry (module doc above).
fn composed_verdict(inputs: &AnalysisInputs, file: celerrate_db::SourceFile) -> StoredVerdict {
    let database = &inputs.database;
    let diagnostics = crate::analysis::persistable_diagnostics(inputs, file);
    let records = celerrate_semantics::resolution_records(
        database,
        file,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
    );
    StoredVerdict {
        diagnostics: diagnostics.iter().map(StoredDiagnostic::of).collect(),
        records: records.iter().map(StoredRecord::of).collect(),
    }
}

/// Deterministic pack order: by key, one entry per key.
fn sort_entries<Entry>(entries: &mut Vec<(ContentHash, Entry)>) {
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.dedup_by(|left, right| left.0 == right.0);
}

/// Creates the cache directory and its self-ignoring `.gitignore`, and
/// sweeps crash debris from both `.celerrate/cache/` and its parent
/// `.celerrate/`. The `.gitignore` goes through the atomic write (audit
/// finding M8): a plain write torn by a crash left a half-written file
/// that was never repaired, since only existence is checked. Its
/// temporary lands in `.celerrate/` (the `.gitignore`'s own parent), not
/// in `.celerrate/cache/`, so the parent gets the same best-effort sweep
/// or a crash during first-time `.gitignore` creation leaves a `.tmp*`
/// orphan forever (whole-branch review finding, closing the M2/M8 seam).
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

/// Best-effort removal of temporary files a crash mid-write left behind
/// (audit finding M2): `write_atomically`'s temporaries carry
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
fn write_when_changed<Entry: Serialize + PartialEq + Clone>(
    path: &Path,
    header: &PackHeader,
    entries: &[(ContentHash, Entry)],
    loaded: &HashMap<ContentHash, Entry>,
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
        let original_header = super::pack::PackHeader::current(original_range);

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
        super::persist(&mut session, &outcome);

        assert_eq!(
            session.cache_loaded_range, moved_range,
            "persist adopts the new range once the rewrite is confirmed",
        );
        let moved_header = super::pack::PackHeader::current(moved_range);
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

    /// Audit finding M2: `write_atomically`'s temporary files (the
    /// `.tmp` prefix `tempfile` uses) survive SIGKILL and power loss in
    /// `.celerrate/cache/`, and nothing ever swept them. `persist` now
    /// sweeps them best-effort; anything not matching the prefix is
    /// someone else's file and stays. Whole-branch review finding M2/M8:
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

    /// Audit finding M5 through I8's counters: a persist that writes, a
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
            3
        );

        super::persist(&mut session, &outcome);
        assert_eq!(
            session.statistics.persist_skipped.load(Ordering::Relaxed),
            3
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
