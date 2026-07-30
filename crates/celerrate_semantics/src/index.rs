//! The global symbol index: both the source half (every declaration
//! of the analyzed file set under its case-folded key, built from item
//! trees only) and the stub half (`StubSymbolTable` over the
//! version-filtered stub view). The source table is sorted and
//! `Eq`-comparable so consumers backdate; lookups themselves go
//! through the per-name query in `crate::lookup`, never through a
//! direct dependency on the whole table.

use std::collections::{HashSet, VecDeque};

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::{
    StubClassSurface, StubIndexInput, StubSignature, StubSymbol, stubs_in_range,
};

use crate::ast_id::AstId;
use crate::items::{DeclarationKind, DefineId};
use crate::queries::item_tree;
use crate::symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};

/// Where a source symbol was declared: an item the item tree numbers, or
/// a `define()` call the item tree cannot see. Items sort before defines,
/// so a `const FOO` and a `define('FOO')` under one key resolve to the
/// `const`, deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SymbolOrigin {
    Item(AstId),
    Define(DefineId),
}

/// One declared symbol: its lookup key, the original spelling (the
/// "did you mean" diagnostics will need it), and the
/// declaration it names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolEntry {
    pub space: SymbolSpace,
    pub key: String,
    pub original: String,
    pub kind: DeclarationKind,
    pub origin: SymbolOrigin,
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
                left.origin,
                left.original.as_str(),
            )
                .cmp(&(
                    right.space,
                    right.key.as_str(),
                    right.origin,
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

/// The source symbol table of the analyzed file set: every item tree's
/// declarations, plus every `define()` the file introduces. Depends on
/// a single per-file query (the item tree), and on nothing that a body
/// edit can move: a span never enters an entry, so the table still
/// backdates.
#[salsa::tracked(returns(ref))]
pub fn source_symbol_table(db: &dyn salsa::Database, files: AnalyzedFileSet) -> SymbolTable {
    let mut entries = Vec::new();
    for &file in files.files(db) {
        let tree = item_tree(db, file);
        for declaration in &tree.declarations {
            let space = SymbolSpace::of_declaration(declaration.kind);
            let original = fully_qualified_name(&declaration.namespace, &declaration.name);
            entries.push(SymbolEntry {
                space,
                key: folded_symbol_key(space, &original),
                original,
                kind: declaration.kind,
                origin: SymbolOrigin::Item(declaration.ast_id),
            });
        }
        for (position, name) in tree.defines.iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                break;
            };
            // The name is literal: no namespace is prepended, a leading
            // root qualifier is only spelling.
            let original = name.strip_prefix('\\').unwrap_or(name.as_str()).to_owned();
            let space = SymbolSpace::Constant;
            entries.push(SymbolEntry {
                space,
                key: folded_symbol_key(space, &original),
                original,
                kind: DeclarationKind::Constant,
                origin: SymbolOrigin::Define(DefineId {
                    file: file.file_id(db),
                    index,
                }),
            });
        }
    }
    SymbolTable::from_entries(entries)
}

/// One stub symbol under its lookup key. The whole `StubSymbol` rides
/// along: the version-gating rule reads its availability window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubSymbolEntry {
    pub space: SymbolSpace,
    pub key: String,
    pub symbol: StubSymbol,
}

/// The stub half of the global symbol index: the version-filtered stub
/// view under case-folded keys. Rebuilt only when the stubs or the
/// configuration change, never on a source edit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StubSymbolTable {
    entries: Vec<StubSymbolEntry>,
}

impl StubSymbolTable {
    /// Builds the table: a deterministic sort, duplicates retained.
    pub fn from_entries(mut entries: Vec<StubSymbolEntry>) -> Self {
        entries.sort_by(|left, right| {
            (
                left.space,
                left.key.as_str(),
                left.symbol.name.as_str(),
                left.symbol.kind,
            )
                .cmp(&(
                    right.space,
                    right.key.as_str(),
                    right.symbol.name.as_str(),
                    right.symbol.kind,
                ))
        });
        Self { entries }
    }

    /// The first entry under `key` in `space`, when one exists.
    pub fn lookup(&self, space: SymbolSpace, key: &str) -> Option<&StubSymbolEntry> {
        let start = self
            .entries
            .partition_point(|entry| (entry.space, entry.key.as_str()) < (space, key));
        self.entries
            .get(start)
            .filter(|entry| entry.space == space && entry.key == key)
    }

    pub fn entries(&self) -> &[StubSymbolEntry] {
        &self.entries
    }
}

/// The stub symbol table over the version-filtered view.
#[salsa::tracked(returns(ref))]
pub fn stub_symbol_table(
    db: &dyn salsa::Database,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> StubSymbolTable {
    StubSymbolTable::from_entries(
        stubs_in_range(db, stubs, configuration)
            .symbols()
            .iter()
            .map(|symbol| {
                let space = SymbolSpace::of_stub_kind(symbol.kind);
                StubSymbolEntry {
                    space,
                    key: folded_symbol_key(space, &symbol.name),
                    symbol: symbol.clone(),
                }
            })
            .collect(),
    )
}

/// The folded consultation surface over the compiled blob payloads: the
/// stub function signatures and class surfaces under their folded keys.
/// Sorted, so accessors binary-search. Rebuilt only when the stub input
/// changes, never on a source edit or a configuration change: the
/// payloads are version-agnostic (per-member availability is filtered at
/// consultation time, not here).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StubSignatureTable {
    functions: Vec<(String, StubSignature)>,
    classes: Vec<(String, StubClassSurface)>,
}

impl StubSignatureTable {
    /// The signature filed under one folded Function-space key.
    pub fn function(&self, key: &str) -> Option<&StubSignature> {
        self.functions
            .binary_search_by(|(existing, _)| existing.as_str().cmp(key))
            .ok()
            .and_then(|position| self.functions.get(position))
            .map(|(_, signature)| signature)
    }

    /// The class surface filed under one folded ClassLike-space key.
    pub fn class(&self, key: &str) -> Option<&StubClassSurface> {
        self.classes
            .binary_search_by(|(existing, _)| existing.as_str().cmp(key))
            .ok()
            .and_then(|position| self.classes.get(position))
            .map(|(_, surface)| surface)
    }
}

/// The folded signature table over the compiled blob payloads. Keyed by
/// the same folding rule as the symbol tables, so a resolved edge's
/// folded key consults it directly.
#[salsa::tracked(returns(ref))]
pub fn stub_signature_table(db: &dyn salsa::Database, stubs: StubIndexInput) -> StubSignatureTable {
    let index = stubs.index(db);
    let mut functions: Vec<(String, StubSignature)> = index
        .functions()
        .iter()
        .map(|(name, signature)| {
            (
                folded_symbol_key(SymbolSpace::Function, name),
                signature.clone(),
            )
        })
        .collect();
    functions.sort_by(|left, right| left.0.cmp(&right.0));
    let mut classes: Vec<(String, StubClassSurface)> = index
        .classes()
        .iter()
        .map(|(name, surface)| {
            (
                folded_symbol_key(SymbolSpace::ClassLike, name),
                surface.clone(),
            )
        })
        .collect();
    classes.sort_by(|left, right| left.0.cmp(&right.0));
    StubSignatureTable { functions, classes }
}

/// What one stub-frontier walk reached. The single answer shape both
/// consumers of [`stub_frontier`] read: linearization folds `reached`
/// into a class's `stub_ancestors` and `opaque` into its
/// `has_opaque_edge`; iteration typing only asks whether `reached`
/// contains a protocol interface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StubFrontier {
    /// Every folded ClassLike key the walk reached, in breadth-first
    /// order, each exactly once. A key with no compiled surface is
    /// reached like any other — its own parents are simply unknown.
    pub reached: Vec<String>,
    /// Some reached key had no compiled surface, so the hierarchy
    /// behind it is not fully walked: a genuinely opaque boundary.
    pub opaque: bool,
}

/// Breadth-first over the compiled parent links from `seeds`: every
/// stub class-like transitively reachable, in walk order. The single
/// implementation of that walk — linearization's stub-frontier
/// expansion ([`crate::linearize::linearized_class`]) and iteration
/// typing's protocol check (`celerrate_types`' `flow.rs`) both route
/// through here rather than each keeping a copy.
///
/// **Self-match semantics**: this walk answers ANCESTRY, never
/// identity. `seeds` are the ancestors to start from, and the walk
/// never adds the class being asked about — a caller holding a class
/// key wants [`stub_ancestors_of`], which seeds from that class's
/// PARENTS. So `stub_ancestors_of(table, "iterator")` does not report
/// `iterator`, exactly as a source-declared `Iterator`'s linearized
/// `ancestry` does not list itself. Both callers want that same
/// meaning, so it is fixed here rather than left to each one's seeding
/// (it previously differed between them: the `flow.rs` copy seeded the
/// queue with the queried name itself, so it uniquely reported a class
/// as its own ancestor).
///
/// A key already visited is skipped, so a cycle among stub parents
/// (`A parent B`, `B parent A`) terminates rather than looping; a key
/// genuinely reachable from itself through such a cycle does appear,
/// which is a real ancestry fact, not a self-match.
///
/// `reached` is ordered by the queue, a pure function of `seeds`' order
/// and each surface's recorded `parents` order — deterministic, never
/// dependent on the visited set's iteration.
pub fn stub_frontier(
    table: &StubSignatureTable,
    seeds: impl IntoIterator<Item = String>,
) -> StubFrontier {
    let mut queue: VecDeque<String> = seeds.into_iter().collect();
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier = StubFrontier::default();
    while let Some(key) = queue.pop_front() {
        if !visited.insert(key.clone()) {
            continue;
        }
        match table.class(&key) {
            Some(surface) => {
                for parent in &surface.parents {
                    queue.push_back(folded_symbol_key(SymbolSpace::ClassLike, parent));
                }
            }
            None => frontier.opaque = true,
        }
        frontier.reached.push(key);
    }
    frontier
}

/// The stub ancestors of one class-like, transitively: [`stub_frontier`]
/// seeded from `class_key`'s own compiled parents. `class_key` itself is
/// not an ancestor of itself, so it is absent unless a genuine parent
/// cycle leads back to it. A key with no compiled surface has no known
/// parents and answers an empty, non-opaque frontier — the caller
/// already knows the key did not resolve.
pub fn stub_ancestors_of(table: &StubSignatureTable, class_key: &str) -> StubFrontier {
    let Some(surface) = table.class(class_key) else {
        return StubFrontier::default();
    };
    stub_frontier(
        table,
        surface
            .parents
            .iter()
            .map(|parent| folded_symbol_key(SymbolSpace::ClassLike, parent)),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_source::FileId;
    use salsa::Setter;

    use celerrate_stubs::StubClassSurface;

    use super::{
        StubSignatureTable, SymbolOrigin, SymbolTable, source_symbol_table, stub_ancestors_of,
        stub_frontier,
    };
    use crate::ast_id::AstId;
    use crate::items::{DeclarationKind, DefineId};
    use crate::symbols::{SymbolSpace, folded_symbol_key};

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
        assert!(matches!(
            class.origin,
            SymbolOrigin::Item(AstId { file, .. }) if file == FileId::new(0)
        ));

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
        assert!(matches!(
            entry.origin,
            SymbolOrigin::Item(AstId { file, .. }) if file == FileId::new(0)
        ));
        assert_eq!(entry.original, "App\\Service");
    }

    #[test]
    fn a_define_joins_the_symbol_table_in_the_global_namespace() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php namespace App; define('APP_ROOT', 1);".to_vec(),
        );
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let table = source_symbol_table(&db, files);
        let entry = table
            .lookup(SymbolSpace::Constant, "APP_ROOT")
            .expect("the define is indexed globally, not under App\\");
        assert_eq!(entry.original, "APP_ROOT");
        assert_eq!(entry.kind, DeclarationKind::Constant);
        assert_eq!(
            entry.origin,
            SymbolOrigin::Define(DefineId {
                file: FileId::new(0),
                index: 0,
            }),
        );
        assert!(
            table
                .lookup(SymbolSpace::Constant, "app\\APP_ROOT")
                .is_none()
        );
    }

    #[test]
    fn a_qualified_define_literal_declares_where_it_says() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php define('Foo\\\\Bar', 1); define('\\\\Root\\\\Baz', 2);".to_vec(),
        );
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let table = source_symbol_table(&db, files);
        assert!(table.lookup(SymbolSpace::Constant, "foo\\Bar").is_some());
        assert!(table.lookup(SymbolSpace::Constant, "root\\Baz").is_some());
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

    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::stub_symbol_table;

    fn stub_input(db: &TestDatabase) -> StubIndexInput {
        let index = StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Random\\Randomizer".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 2)),
                    ..StubAvailability::ALWAYS
                },
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "E_ALL".to_owned(),
                kind: StubSymbolKind::Constant,
                availability: StubAvailability::ALWAYS,
            },
        ]);
        StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(db)
    }

    fn configuration_of(
        db: &TestDatabase,
        minimum: (u8, u8),
        maximum: (u8, u8),
    ) -> ProjectConfiguration {
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(minimum.0, minimum.1),
            PhpVersion::new(maximum.0, maximum.1),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db)
    }

    #[test]
    fn stub_symbols_are_indexed_under_their_folded_keys() {
        let db = TestDatabase::default();
        let table = stub_symbol_table(&db, stub_input(&db), configuration_of(&db, (8, 1), (8, 5)));
        let class = table
            .lookup(SymbolSpace::ClassLike, "random\\randomizer")
            .unwrap();
        assert_eq!(class.symbol.name, "Random\\Randomizer");
        assert_eq!(
            class.symbol.availability.introduced,
            Some(PhpVersion::new(8, 2)),
        );
        assert!(table.lookup(SymbolSpace::Function, "strlen").is_some());
        assert!(table.lookup(SymbolSpace::Constant, "E_ALL").is_some());
        assert!(
            table.lookup(SymbolSpace::Constant, "e_all").is_none(),
            "constants stay case-sensitive",
        );
    }

    #[test]
    fn the_signature_table_folds_and_answers_by_key() {
        use celerrate_stubs::{StubClassSurface, StubIndex, StubIndexInput, StubSignature};

        use super::stub_signature_table;

        let index = StubIndex::new(
            vec![],
            vec![("Str\\Len".to_owned(), StubSignature::default())],
            vec![("RuntimeException".to_owned(), StubClassSurface::default())],
        );
        let db = TestDatabase::default();
        let input = StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let table = stub_signature_table(&db, input);
        assert!(table.function("str\\len").is_some(), "function keys fold");
        assert!(table.class("runtimeexception").is_some(), "class keys fold");
        assert!(
            table.class("RuntimeException").is_none(),
            "pre-folded keys only",
        );
    }

    #[test]
    fn the_table_respects_the_version_range() {
        let mut db = TestDatabase::default();
        let stubs = stub_input(&db);
        let configuration = configuration_of(&db, (8, 0), (8, 1));
        let table = stub_symbol_table(&db, stubs, configuration);
        assert!(
            table
                .lookup(SymbolSpace::ClassLike, "random\\randomizer")
                .is_none(),
            "introduced after the maximum: filtered out",
        );

        configuration
            .set_php_version_range(&mut db)
            .to(PhpVersionRange::new(
                PhpVersion::new(8, 1),
                PhpVersion::new(8, 5),
            ));
        let widened = stub_symbol_table(&db, stubs, configuration);
        assert!(
            widened
                .lookup(SymbolSpace::ClassLike, "random\\randomizer")
                .is_some(),
        );
    }

    /// A `StubSignatureTable` over the given `(name, parents)` pairs,
    /// folded exactly as `stub_signature_table` folds the real blob.
    fn surface_table(classes: &[(&str, &[&str])]) -> StubSignatureTable {
        let mut classes: Vec<(String, StubClassSurface)> = classes
            .iter()
            .map(|(name, parents)| {
                (
                    folded_symbol_key(SymbolSpace::ClassLike, name),
                    StubClassSurface {
                        parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
                        members: Vec::new(),
                    },
                )
            })
            .collect();
        classes.sort_by(|left, right| left.0.cmp(&right.0));
        StubSignatureTable {
            functions: Vec::new(),
            classes,
        }
    }

    #[test]
    fn the_stub_frontier_reaches_transitive_parents_in_walk_order() {
        let table = surface_table(&[
            ("ArrayIterator", &["Iterator", "Countable"]),
            ("Iterator", &["Traversable"]),
            ("Countable", &[]),
            ("Traversable", &[]),
        ]);
        let frontier = stub_frontier(&table, ["arrayiterator".to_owned()]);
        // Breadth-first from the seed, each key once, parents in their
        // recorded (declaration) order: deterministic, not the visited
        // set's iteration order.
        assert_eq!(
            frontier.reached,
            vec!["arrayiterator", "iterator", "countable", "traversable"],
        );
        assert!(!frontier.opaque);
    }

    /// The self-match contract, fixed in one place (the reviewer's
    /// Minor #5): `stub_ancestors_of` answers ANCESTRY, never identity.
    /// The two callers of this walk previously disagreed — iteration
    /// typing's own copy seeded its queue with the queried name itself,
    /// so it uniquely reported `Iterator` as implementing `Iterator`,
    /// where a source-declared `Iterator`'s linearized ancestry does
    /// not list itself.
    #[test]
    fn a_class_is_never_its_own_stub_ancestor() {
        let table = surface_table(&[("Iterator", &["Traversable"]), ("Traversable", &[])]);
        let frontier = stub_ancestors_of(&table, "iterator");
        assert_eq!(frontier.reached, vec!["traversable"]);
        assert!(!frontier.reached.iter().any(|key| key == "iterator"));
    }

    #[test]
    fn a_stub_parent_cycle_terminates() {
        // `A parent B`, `B parent A`: the visited set closes the loop
        // rather than queueing forever. A key genuinely reachable from
        // itself through the cycle is a real ancestry fact, so `a` does
        // appear here — unlike the self-match case above, which has no
        // cycle.
        let table = surface_table(&[("A", &["B"]), ("B", &["A"])]);
        let frontier = stub_ancestors_of(&table, "a");
        assert_eq!(frontier.reached, vec!["b", "a"]);
        assert!(!frontier.opaque);
    }

    #[test]
    fn a_parent_with_no_compiled_surface_leaves_the_frontier_opaque() {
        let table = surface_table(&[("Known", &["Missing"])]);
        let frontier = stub_ancestors_of(&table, "known");
        assert_eq!(frontier.reached, vec!["missing"]);
        assert!(frontier.opaque);
    }

    #[test]
    fn a_class_with_no_compiled_surface_has_no_known_ancestors() {
        let table = surface_table(&[("Known", &[])]);
        let frontier = stub_ancestors_of(&table, "ghost");
        assert!(frontier.reached.is_empty());
        assert!(!frontier.opaque);
    }
}
