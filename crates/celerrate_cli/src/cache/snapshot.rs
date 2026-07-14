//! The loaded cache: an immutable snapshot, fixed for the lifetime of
//! the process, of whatever validated on disk. Anything that fails any
//! check is silently absent; the run recomputes and the next persist
//! rewrites it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use celerrate_db::ContentHash;
use celerrate_semantics::{ArtifactCache, ItemTree};
use celerrate_source::FileId;
use serde::de::DeserializeOwned;

use super::pack::{PackHeader, decode};
use super::statistics::CacheStatistics;
use super::stored::{StoredItemTree, StoredVerdict};

pub const ITEM_TREES_PACK: &str = "item_trees.bin";
pub const DIAGNOSTICS_PACK: &str = "diagnostics.bin";

/// Whatever the packs validated to. Both maps may be empty; nothing
/// downstream distinguishes "no cache" from "no valid cache".
#[derive(Debug, Default)]
pub struct CacheSnapshot {
    pub item_trees: HashMap<ContentHash, StoredItemTree>,
    pub verdicts: HashMap<ContentHash, StoredVerdict>,
}

impl CacheSnapshot {
    pub fn load(cache_directory: &Path, expected: &PackHeader) -> Self {
        Self {
            item_trees: load_pack(&cache_directory.join(ITEM_TREES_PACK), expected),
            verdicts: load_pack(&cache_directory.join(DIAGNOSTICS_PACK), expected),
        }
    }
}

fn load_pack<Entry: DeserializeOwned>(
    path: &Path,
    expected: &PackHeader,
) -> HashMap<ContentHash, Entry> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashMap::new();
    };
    match decode::<Vec<(ContentHash, Entry)>>(&bytes, expected) {
        Some(pack) => pack.entries.into_iter().collect(),
        None => HashMap::new(),
    }
}

/// The snapshot as the artifact cache the semantics layer consults:
/// a lookup by content address, with the current file identity stamped
/// back in, counting hits and misses as it answers.
pub struct SnapshotCache {
    pub snapshot: Arc<CacheSnapshot>,
    pub statistics: Arc<CacheStatistics>,
}

impl ArtifactCache for SnapshotCache {
    fn item_tree(&self, file: FileId, content: ContentHash) -> Option<ItemTree> {
        use std::sync::atomic::Ordering;
        match self.snapshot.item_trees.get(&content) {
            Some(stored) => {
                self.statistics.tree_hits.fetch_add(1, Ordering::Relaxed);
                Some(stored.to_item_tree(file))
            }
            None => {
                self.statistics.tree_misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }
}
