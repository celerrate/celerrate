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
/// replaced by what this pass concluded, so the next cycle's equality
/// check compares against the last persisted state.
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
        if panicked.contains(&file.file_id(database)) {
            continue;
        }
        let stored = match verdict::validated_verdict(&inputs, file) {
            Some(stored) => stored.clone(),
            None => composed_verdict(&inputs, file),
        };
        verdicts.push((celerrate_db::content_hash(database, file), stored));
    }
    sort_entries(&mut verdicts);

    if prepare_directory(&session.cache_directory).is_err() {
        return;
    }
    write_when_changed(
        &session.cache_directory.join(ITEM_TREES_PACK),
        &header,
        &trees,
        &session.cache.item_trees,
    );
    write_when_changed(
        &session.cache_directory.join(DIAGNOSTICS_PACK),
        &header,
        &verdicts,
        &session.cache.verdicts,
    );
    session.cache = Arc::new(CacheSnapshot {
        item_trees: trees.into_iter().collect(),
        verdicts: verdicts.into_iter().collect(),
    });
    session.cache_loaded_range = session.configuration.php_version_range(database);
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
fn write_when_changed<Entry: Serialize + PartialEq + Clone>(
    path: &Path,
    header: &PackHeader,
    entries: &[(ContentHash, Entry)],
    loaded: &HashMap<ContentHash, Entry>,
) {
    let unchanged = entries.len() == loaded.len()
        && entries
            .iter()
            .all(|(key, value)| loaded.get(key) == Some(value));
    if unchanged && path.is_file() {
        return;
    }
    let Some(bytes) = pack::encode(&Pack {
        header: header.clone(),
        entries: entries.to_vec(),
    }) else {
        return;
    };
    let _ = pack::write_atomically(path, &bytes);
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
}
