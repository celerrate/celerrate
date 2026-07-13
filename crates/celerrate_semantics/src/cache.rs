//! The artifact-cache extension point: the dependency-inverted trait
//! this layer consults, the handle the composition root registers as a
//! salsa singleton input, and nothing about disks. The persistent cache
//! is a CLI concern; this layer only asks "is the boundary artifact of
//! these bytes already known?".

use std::fmt;
use std::sync::Arc;

use celerrate_db::ContentHash;
use celerrate_source::FileId;

use crate::items::ItemTree;

/// What a registered artifact cache can answer. The contract is exact:
/// a `Some` must be byte-for-byte what the computation would produce
/// for a file with identity `file` whose bytes hash to `content` —
/// `ItemTree::from_root(file, &parse(bytes).tree())`, `defines` field
/// included — or `None`. A tree that omits or misreports a `define()`
/// the bytes actually declare is not a valid cache entry: `defines` is
/// as much a part of the exact value as `declarations` and `imports`,
/// and consumers such as `source_symbol_table` read it straight from
/// this query. The cross-process harness holds implementations to it.
pub trait ArtifactCache: Send + Sync {
    /// The cached item tree of the file whose content hashes to
    /// `content`, already remapped to `file`.
    fn item_tree(&self, file: FileId, content: ContentHash) -> Option<ItemTree>;
}

/// The registered cache, as the cloneable handle a salsa input field
/// requires.
#[derive(Clone)]
pub struct CacheHandle(pub Arc<dyn ArtifactCache>);

impl fmt::Debug for CacheHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("CacheHandle").finish()
    }
}

/// The singleton input the composition root registers once, before any
/// query runs, and never mutates: reading it therefore never
/// invalidates anything. Databases that register nothing (every test
/// database) take the compute path through `try_get`'s `None`.
#[salsa::input(singleton)]
pub struct ArtifactCacheInput {
    #[returns(ref)]
    pub cache: CacheHandle,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::sync::Arc;

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{ContentHash, SourceFile};
    use celerrate_source::FileId;

    use crate::cache::{ArtifactCache, ArtifactCacheInput, CacheHandle};
    use crate::items::ItemTree;
    use crate::queries::item_tree;

    /// A probe cache: returns a fixed tree for every lookup. This
    /// deliberately violates the trait's exactness contract, which is
    /// the point: the only way to observe that the query consulted the
    /// cache is to hand it a value the computation would never produce.
    struct Probe(ItemTree);

    impl ArtifactCache for Probe {
        fn item_tree(&self, _file: FileId, _content: ContentHash) -> Option<ItemTree> {
            Some(self.0.clone())
        }
    }

    /// A cache that never has anything.
    struct Empty;

    impl ArtifactCache for Empty {
        fn item_tree(&self, _file: FileId, _content: ContentHash) -> Option<ItemTree> {
            None
        }
    }

    #[test]
    fn a_registered_cache_is_consulted_before_lowering() {
        let db = TestDatabase::default();
        let _ = ArtifactCacheInput::builder(CacheHandle(Arc::new(Probe(ItemTree::default()))))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        let _ = db.take_executed();
        let tree = item_tree(&db, file);
        assert!(
            tree.declarations.is_empty(),
            "the probe's empty tree is served, not the lowered one",
        );
        let executed = db.take_executed();
        assert!(
            executed.iter().all(|query| !query.contains("parse(")),
            "a hit never parses: {executed:?}",
        );
    }

    #[test]
    fn a_hit_carries_its_defines_into_the_symbol_table_without_parsing() {
        // The whole point of moving defines into the ItemTree: a warm
        // process must build `source_symbol_table` from pack-served trees
        // alone, never by re-parsing to answer `defined_constants`
        // (which no longer exists as a query at all).
        use celerrate_db::AnalyzedFileSet;

        use crate::index::source_symbol_table;
        use crate::symbols::SymbolSpace;

        let mut probe_tree = ItemTree::default();
        probe_tree.defines.push("APP_ROOT".to_owned());

        let db = TestDatabase::default();
        let _ = ArtifactCacheInput::builder(CacheHandle(Arc::new(Probe(probe_tree))))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        // The source bytes declare nothing: only the probe's `defines`
        // may explain a symbol-table hit under `APP_ROOT`.
        let file = SourceFile::new(&db, FileId::new(0), b"<?php".to_vec());
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let _ = db.take_executed();

        let table = source_symbol_table(&db, files);
        assert!(
            table.lookup(SymbolSpace::Constant, "APP_ROOT").is_some(),
            "the probe's define reaches the table",
        );
        let executed = db.take_executed();
        assert!(
            executed.iter().all(|query| !query.contains("parse(")),
            "the table build must never parse when every tree is cache-served: {executed:?}",
        );
    }

    #[test]
    fn a_cache_miss_computes_normally() {
        let db = TestDatabase::default();
        let _ = ArtifactCacheInput::builder(CacheHandle(Arc::new(Empty)))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 1);
    }

    #[test]
    fn no_registered_cache_computes_normally() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 1);
    }
}
