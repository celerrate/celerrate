//! The source half of the global symbol index: every declaration of
//! the analyzed file set under its case-folded key. The table is built
//! from item trees only (the invalidation boundary holds), sorted and
//! `Eq`-comparable so consumers backdate; lookups themselves go
//! through the per-name query in `crate::lookup`, never through a
//! direct dependency on the whole table.

use celerrate_db::AnalyzedFileSet;

use crate::ast_id::AstId;
use crate::items::DeclarationKind;
use crate::queries::item_tree;
use crate::symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};

/// One declared symbol: its lookup key, the original spelling (the
/// "did you mean" diagnostics of sub-project 4 will need it), and the
/// declaration it names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolEntry {
    pub space: SymbolSpace,
    pub key: String,
    pub original: String,
    pub kind: DeclarationKind,
    pub ast_id: AstId,
}

/// The declared symbols of the analyzed file set, sorted by
/// (space, key) with declaration identity breaking ties: duplicates
/// keep a deterministic order and lookup answers the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolTable {
    entries: Vec<SymbolEntry>,
}

impl SymbolTable {
    /// Builds the table: a deterministic sort, duplicates retained.
    pub fn from_entries(mut entries: Vec<SymbolEntry>) -> Self {
        entries.sort_by(|left, right| {
            (
                left.space,
                left.key.as_str(),
                left.ast_id,
                left.original.as_str(),
            )
                .cmp(&(
                    right.space,
                    right.key.as_str(),
                    right.ast_id,
                    right.original.as_str(),
                ))
        });
        Self { entries }
    }

    /// The first entry under `key` in `space`, when one exists.
    pub fn lookup(&self, space: SymbolSpace, key: &str) -> Option<&SymbolEntry> {
        let start = self
            .entries
            .partition_point(|entry| (entry.space, entry.key.as_str()) < (space, key));
        self.entries
            .get(start)
            .filter(|entry| entry.space == space && entry.key == key)
    }

    pub fn entries(&self) -> &[SymbolEntry] {
        &self.entries
    }
}

/// The source symbol table of the analyzed file set. Depends on every
/// member's item tree: a signature change anywhere rebuilds it (the
/// merge is cheap), and the per-name lookups behind it backdate for
/// every name whose answer did not change.
#[salsa::tracked(returns(ref))]
pub fn source_symbol_table(db: &dyn salsa::Database, files: AnalyzedFileSet) -> SymbolTable {
    let mut entries = Vec::new();
    for &file in files.files(db) {
        for declaration in &item_tree(db, file).declarations {
            let space = SymbolSpace::of_declaration(declaration.kind);
            let original = fully_qualified_name(&declaration.namespace, &declaration.name);
            entries.push(SymbolEntry {
                space,
                key: folded_symbol_key(space, &original),
                original,
                kind: declaration.kind,
                ast_id: declaration.ast_id,
            });
        }
    }
    SymbolTable::from_entries(entries)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_source::FileId;
    use salsa::Setter;

    use super::{SymbolTable, source_symbol_table};
    use crate::items::DeclarationKind;
    use crate::symbols::SymbolSpace;

    fn set_of(db: &TestDatabase, sources: &[&str]) -> AnalyzedFileSet {
        let files: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        AnalyzedFileSet::new(db, files)
    }

    fn table_of(sources: &[&str]) -> SymbolTable {
        let db = TestDatabase::default();
        source_symbol_table(&db, set_of(&db, sources)).clone()
    }

    #[test]
    fn every_declaration_is_indexed_under_its_folded_key() {
        let table = table_of(&[
            "<?php namespace App; class Service {} function greet() {} const LIMIT = 1;",
        ]);
        let class = table
            .lookup(SymbolSpace::ClassLike, "app\\service")
            .unwrap();
        assert_eq!(class.original, "App\\Service");
        assert_eq!(class.kind, DeclarationKind::Class);
        assert_eq!(class.ast_id.file, FileId::new(0));

        let function = table.lookup(SymbolSpace::Function, "app\\greet").unwrap();
        assert_eq!(function.original, "App\\greet");

        let constant = table.lookup(SymbolSpace::Constant, "app\\LIMIT").unwrap();
        assert_eq!(constant.kind, DeclarationKind::Constant);
    }

    #[test]
    fn constant_lookups_are_case_sensitive() {
        let table = table_of(&["<?php namespace App; const LIMIT = 1;"]);
        assert!(table.lookup(SymbolSpace::Constant, "app\\limit").is_none());
        assert!(table.lookup(SymbolSpace::Constant, "app\\LIMIT").is_some());
    }

    #[test]
    fn spaces_never_bleed_into_each_other() {
        let table = table_of(&["<?php function shared() {} class Shared {}"]);
        assert_eq!(
            table
                .lookup(SymbolSpace::Function, "shared")
                .map(|e| e.kind),
            Some(DeclarationKind::Function),
        );
        assert_eq!(
            table
                .lookup(SymbolSpace::ClassLike, "shared")
                .map(|e| e.kind),
            Some(DeclarationKind::Class),
        );
        assert!(table.lookup(SymbolSpace::Constant, "shared").is_none());
    }

    #[test]
    fn duplicates_answer_the_first_declaration_in_set_order() {
        let table = table_of(&[
            "<?php namespace App; class Service {}",
            "<?php namespace App; class SERVICE {}",
        ]);
        let entry = table
            .lookup(SymbolSpace::ClassLike, "app\\service")
            .unwrap();
        assert_eq!(entry.ast_id.file, FileId::new(0));
        assert_eq!(entry.original, "App\\Service");
    }

    #[test]
    fn entries_are_sorted_deterministically() {
        let table = table_of(&[
            "<?php class Zulu {} class Alpha {}",
            "<?php function zulu() {} const A = 1;",
        ]);
        let keys: Vec<(SymbolSpace, &str)> = table
            .entries()
            .iter()
            .map(|entry| (entry.space, entry.key.as_str()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn an_unknown_key_answers_none() {
        let table = table_of(&["<?php class Known {}"]);
        assert!(table.lookup(SymbolSpace::ClassLike, "unknown").is_none());
    }

    #[test]
    fn a_body_edit_never_rebuilds_the_table() {
        // The item tree's early cutoff pays off here: the table depends
        // on item trees only, and a body edit backdates the tree.
        let mut db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function greet() { return 1; }".to_vec(),
        );
        let set = AnalyzedFileSet::new(&db, vec![file]);
        let _ = source_symbol_table(&db, set);
        db.take_executed();

        file.set_bytes(&mut db)
            .to(b"<?php function greet() { return 2; }".to_vec());
        let _ = source_symbol_table(&db, set);
        let log = db.take_executed();
        assert!(
            log.iter().any(|entry| entry.starts_with("item_tree")),
            "the edited file reprojects: {log:?}",
        );
        assert!(
            !log.iter()
                .any(|entry| entry.starts_with("source_symbol_table")),
            "a body edit must never rebuild the table: {log:?}",
        );
    }

    #[test]
    fn adding_a_file_to_the_set_rebuilds_the_table() {
        let mut db = TestDatabase::default();
        let first = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        let set = AnalyzedFileSet::new(&db, vec![first]);
        assert_eq!(source_symbol_table(&db, set).entries().len(), 1);

        let second = SourceFile::new(&db, FileId::new(1), b"<?php class B {}".to_vec());
        set.set_files(&mut db).to(vec![first, second]);
        assert_eq!(source_symbol_table(&db, set).entries().len(), 2);
    }
}
