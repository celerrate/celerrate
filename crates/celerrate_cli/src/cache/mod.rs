//! The persistent artifact cache: a content-addressed derived-artifact
//! cache above salsa, persisted to `.celerrate/cache/` and used to
//! re-seed a fresh database at startup. Nothing here is ever fatal:
//! every failure mode of a cache file answers by recomputation.

pub mod pack;
pub mod snapshot;
pub mod stored;
pub mod verdict;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use celerrate_db::ContentHash;
use celerrate_source::FileId;
use serde::Serialize;

use crate::analysis::{AnalysisInputs, AnalysisOutcome};
use crate::session::Session;

use pack::{Pack, PackHeader};
use snapshot::{CacheSnapshot, DIAGNOSTICS_PACK, ITEM_TREES_PACK};
use stored::{StoredDiagnostic, StoredItemTree, StoredRecord, StoredVerdict};

/// Persists the packs after one completed pass, best-effort: an I/O
/// failure skips the write and nothing else. The session's snapshot is
/// replaced by what was actually WRITTEN, and only when both packs
/// confirm — whole or nothing, so the next cycle's equality check never
/// compares against a snapshot the disk does not hold. On failure the
/// old snapshot stays, the next pass recomputes the same entries and
/// retries the write; an occasional redundant rewrite of the healthy
/// pack alongside a retried failing one is harmless and best-effort.
pub fn persist(session: &mut Session, outcome: &AnalysisOutcome) {
    let inputs = session.inputs();
    let database = &inputs.database;
    let header = PackHeader::current(session.configuration.php_version_range(database));

    let mut trees: Vec<(ContentHash, StoredItemTree)> = session
        .sources
        .values()
        .map(|&file| {
            (
                celerrate_db::content_hash(database, file),
                StoredItemTree::of(celerrate_semantics::item_tree(database, file)),
            )
        })
        .collect();
    sort_entries(&mut trees);

    let panicked: BTreeSet<FileId> = outcome.panicked.iter().copied().collect();
    let mut verdicts: Vec<(ContentHash, StoredVerdict)> = Vec::new();
    for &file in inputs.reported.iter() {
        let file_id = file.file_id(database);
        if panicked.contains(&file_id) {
            continue;
        }
        // Mirrors `analyze_one`: a validated hit is only reused when
        // every stored diagnostic still re-interns, or `persist` would
        // re-persist an entry the pass itself refused to serve.
        let stored = match verdict::validated_verdict(&inputs, file) {
            Some(stored)
                if stored
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.to_diagnostic(file_id).is_some()) =>
            {
                stored.clone()
            }
            _ => composed_verdict(&inputs, file),
        };
        verdicts.push((celerrate_db::content_hash(database, file), stored));
    }
    sort_entries(&mut verdicts);

    if prepare_directory(&session.cache_directory).is_err() {
        return;
    }
    let trees_written = write_when_changed(
        &session.cache_directory.join(ITEM_TREES_PACK),
        &header,
        &trees,
        &session.cache.item_trees,
    );
    let verdicts_written = write_when_changed(
        &session.cache_directory.join(DIAGNOSTICS_PACK),
        &header,
        &verdicts,
        &session.cache.verdicts,
    );
    if trees_written && verdicts_written {
        session.cache = Arc::new(CacheSnapshot {
            item_trees: trees.into_iter().collect(),
            verdicts: verdicts.into_iter().collect(),
        });
        session.cache_loaded_range = session.configuration.php_version_range(database);
    }
}

/// One reported file's verdict, composed exactly as `analyze_one`
/// composes its diagnostics, with the records the entry must
/// revalidate against. Every query here is memoized from the pass.
fn composed_verdict(inputs: &AnalysisInputs, file: celerrate_db::SourceFile) -> StoredVerdict {
    let database = &inputs.database;
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

/// Creates the cache directory and its self-ignoring `.gitignore`.
fn prepare_directory(cache_directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_directory)?;
    if let Some(dot_celerrate) = cache_directory.parent() {
        let gitignore = dot_celerrate.join(".gitignore");
        if !gitignore.exists() {
            std::fs::write(gitignore, "*\n")?;
        }
    }
    Ok(())
}

/// Rewrites a pack only when its entries differ from the loaded state.
/// Answers whether `path` now holds exactly `entries`: true when it was
/// already unchanged and present, or the write just succeeded; false
/// when encoding or the atomic write failed, in which case whatever was
/// on disk before (if anything) is untouched. `persist` only swaps the
/// session's snapshot when this returns true for every pack.
fn write_when_changed<Entry: Serialize + PartialEq + Clone>(
    path: &Path,
    header: &PackHeader,
    entries: &[(ContentHash, Entry)],
    loaded: &HashMap<ContentHash, Entry>,
) -> bool {
    let unchanged = entries.len() == loaded.len()
        && entries
            .iter()
            .all(|(key, value)| loaded.get(key) == Some(value));
    if unchanged && path.is_file() {
        return true;
    }
    let Some(bytes) = pack::encode(&Pack {
        header: header.clone(),
        entries: entries.to_vec(),
    }) else {
        return false;
    };
    pack::write_atomically(path, &bytes).is_ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crate::analysis::AnalysisOutcome;
    use crate::session::Session;

    /// A file the pass reported as panicked yields no verdict entry:
    /// nothing a panic touched enters the persistent cache.
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
            2,
            "item trees are content projections and stay cacheable",
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
            "both packs confirmed, so the snapshot now swaps",
        );
    }
}
