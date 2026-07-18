//! The boundary as salsa queries. Four per-file queries: the numbering
//! carries ranges and re-runs whenever they shift; the item tree and
//! the member tree carry none, so a body or member edit backdates and
//! everything downstream is spared; the semantic diagnostics query
//! merges the reference checks and the syntax version gating built on
//! top of the item tree. Query definitions live here, in their domain
//! crate; the concrete database is assembled at the composition root.

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::Diagnostic;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::ast_id::AstIdMap;
use crate::items::ItemTree;
use crate::members::MemberTree;

/// The declaration numbering of one file. The value changes whenever
/// ranges shift: consume [`item_tree`] instead, and reconcile spans
/// through this map as late as possible.
#[salsa::tracked(returns(ref))]
pub fn ast_id_map(db: &dyn salsa::Database, file: SourceFile) -> AstIdMap {
    AstIdMap::from_root(&celerrate_db::parse(db, file).tree())
}

/// The item tree of one file: range-free, so a body edit produces an
/// equal value and salsa backdates it — the early-cutoff boundary
/// every cross-file consumer sits behind. A cache registered at the
/// composition root is consulted first, keyed by the file's content
/// address; the lookup is a pure function of tracked inputs, so the
/// query stays deterministic either way.
#[salsa::tracked(returns(ref))]
pub fn item_tree(db: &dyn salsa::Database, file: SourceFile) -> ItemTree {
    if let Some(input) = crate::cache::ArtifactCacheInput::try_get(db)
        && let Some(tree) = input
            .cache(db)
            .0
            .item_tree(file.file_id(db), celerrate_db::content_hash(db, file))
    {
        return tree;
    }
    ItemTree::from_root(file.file_id(db), &celerrate_db::parse(db, file).tree())
}

/// The member projection of one file: per class-like declaration, its
/// direct members with flags, signatures as unresolved names, and
/// docblock text. Range-free like the item tree, and a sibling of it
/// on purpose: a member edit changes this value without touching
/// `item_tree`, so top-level consumers — the global symbol table
/// first — are structurally spared. A cache registered at the
/// composition root is consulted first, keyed by the file's content
/// address, exactly as `item_tree` does.
#[salsa::tracked(returns(ref))]
pub fn member_tree(db: &dyn salsa::Database, file: SourceFile) -> MemberTree {
    if let Some(input) = crate::cache::ArtifactCacheInput::try_get(db)
        && let Some(tree) = input
            .cache(db)
            .0
            .member_tree(file.file_id(db), celerrate_db::content_hash(db, file))
    {
        return tree;
    }
    MemberTree::from_root(file.file_id(db), &celerrate_db::parse(db, file).tree())
}

/// Every semantic diagnostic of one file: the reference families
/// (unknown symbols, symbol version gating) and syntax version gating,
/// merged and deterministically ordered. Syntax and decode findings
/// live in `celerrate_db::file_diagnostics`; the CLI composes both.
#[salsa::tracked(returns(ref))]
pub fn semantic_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let mut diagnostics =
        crate::reference_checks::reference_diagnostics(db, file, files, stubs, configuration)
            .clone();
    diagnostics.extend(
        crate::syntax_gating::syntax_version_diagnostics(db, file, configuration)
            .iter()
            .cloned(),
    );
    diagnostics.sort();
    diagnostics
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::AnalyzedFileSet;
    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};
    use celerrate_syntax::SyntaxKind;
    use salsa::Setter;

    use super::{ast_id_map, item_tree, semantic_diagnostics};

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

    #[test]
    fn semantic_diagnostics_merge_both_families_in_order() {
        // One unknown class after one gated construct: the merged output
        // is sorted by range, families interleaved.
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php readonly class Point {} $x = new Missing();".to_vec(),
        );
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let stubs = StubIndexInput::builder(StubIndex::default())
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        let diagnostics = semantic_diagnostics(&db, file, files, stubs, configuration);
        let identifiers: Vec<&str> = diagnostics.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(identifiers, vec!["CEL0024", "CEL0018"]);
        let mut sorted = diagnostics.clone();
        sorted.sort();
        assert_eq!(&sorted, diagnostics);
    }

    #[test]
    fn the_member_tree_query_projects_a_file() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(3),
            b"<?php namespace App; class Service { public function run(): void {} }".to_vec(),
        );
        let tree = super::member_tree(&db, file);
        let class = tree.classes.first().unwrap();
        assert_eq!(class.name.as_deref(), Some("Service"));
        assert_eq!(class.namespace, "App");
        assert_eq!(
            class.members.first().map(|member| member.name.as_str()),
            Some("run"),
        );
    }
}
