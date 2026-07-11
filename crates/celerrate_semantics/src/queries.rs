//! The boundary as salsa queries. Two per-file queries with split
//! volatility: the numbering carries ranges and re-runs whenever they
//! shift; the item tree carries none, so a body edit backdates and
//! everything downstream is spared. Query definitions live here, in
//! their domain crate; the concrete database is assembled at the
//! composition root.

use celerrate_db::SourceFile;

use crate::ast_id::AstIdMap;
use crate::items::ItemTree;

/// The declaration numbering of one file. The value changes whenever
/// ranges shift: consume [`item_tree`] instead, and reconcile spans
/// through this map as late as possible.
#[salsa::tracked(returns(ref))]
pub fn ast_id_map(db: &dyn salsa::Database, file: SourceFile) -> AstIdMap {
    AstIdMap::from_root(&celerrate_db::parse(db, file).tree())
}

/// The item tree of one file: range-free, so a body edit produces an
/// equal value and salsa backdates it — the early-cutoff boundary
/// every cross-file consumer sits behind.
#[salsa::tracked(returns(ref))]
pub fn item_tree(db: &dyn salsa::Database, file: SourceFile) -> ItemTree {
    ItemTree::from_root(file.file_id(db), &celerrate_db::parse(db, file).tree())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_source::FileId;
    use celerrate_syntax::SyntaxKind;
    use salsa::Setter;

    use super::{ast_id_map, item_tree};

    #[test]
    fn the_item_tree_query_projects_a_file() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(3),
            b"<?php namespace App; class Service {}".to_vec(),
        );
        let tree = item_tree(&db, file);
        let declaration = tree.declarations.first().unwrap();
        assert_eq!(declaration.name, "Service");
        assert_eq!(declaration.namespace, "App");
        assert_eq!(declaration.ast_id.file, FileId::new(3));
    }

    #[test]
    fn the_map_and_the_tree_number_declarations_identically() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php namespace N; use A; class B extends C {} function d() {} const E = 1;".to_vec(),
        );
        let tree = item_tree(&db, file);
        let map = ast_id_map(&db, file);
        let root = celerrate_db::parse(&db, file).tree();
        assert!(!tree.declarations.is_empty());
        for declaration in &tree.declarations {
            let node = map
                .pointer(declaration.ast_id.index)
                .and_then(|pointer| pointer.try_to_node(&root));
            assert!(
                node.is_some(),
                "declaration {declaration:?} must reconcile through the map",
            );
        }
        for import in &tree.imports {
            let kind = map
                .pointer(import.ast_id.index)
                .and_then(|pointer| pointer.try_to_node(&root))
                .map(|node| node.kind());
            assert_eq!(kind, Some(SyntaxKind::UseDeclaration));
        }
    }

    #[test]
    fn editing_bytes_reprojects() {
        let mut db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 1);
        file.set_bytes(&mut db)
            .to(b"<?php class A {} class B {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 2);
    }

    #[test]
    fn an_undecodable_file_projects_an_empty_tree() {
        // Oversized or undecodable inputs parse as empty in
        // celerrate_db; the projection follows without failing.
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"\xFF\xFE<?php".to_vec());
        let _ = item_tree(&db, file);
    }
}
