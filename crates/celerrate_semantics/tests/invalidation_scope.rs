//! Invalidation-scope tests for the boundary: after each canonical
//! edit class, assert exactly which queries re-executed. The
//! consistency harness verifies the result; these tests verify how
//! little work produced it — the direct proof of the item tree's
//! early cutoff, which no correctness test can observe.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::AnalyzedFileSet;
use celerrate_db::SourceFile;
use celerrate_db::testing::TestDatabase;
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    SymbolSources, SymbolSpace, UseTables, ast_id_map, item_tree, resolve_name,
};
use celerrate_source::FileId;
use celerrate_stubs::{StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind};
use salsa::Setter;

/// A stand-in for part 5's consumers: any query that reads the item
/// tree and nothing else syntactic. If the tree backdates, this must
/// never re-run.
#[salsa::tracked]
fn declared_names(db: &dyn salsa::Database, file: SourceFile) -> Vec<String> {
    item_tree(db, file)
        .declarations
        .iter()
        .map(|declaration| declaration.name.clone())
        .collect()
}

fn executions_of(log: &[String], query: &str) -> usize {
    let prefix = format!("{query}(");
    log.iter()
        .filter(|entry| entry.contains(prefix.as_str()))
        .count()
}

#[test]
fn a_body_edit_reaches_the_item_tree_and_stops_there() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function greet() { return 1; }".to_vec(),
    );
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function greet() { return 2; }".to_vec());
    let _ = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "parse"),
        1,
        "the edited file reparses: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "the projection re-runs over the new tree: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "declared_names"),
        0,
        "an identical item tree must backdate, sparing every consumer: {log:?}",
    );
}

#[test]
fn a_comment_only_edit_spares_every_consumer() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function greet() { return 1; }".to_vec(),
    );
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php // a note\nfunction greet() { return 1; }".to_vec());
    let _ = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(executions_of(&log, "item_tree"), 1, "{log:?}");
    assert_eq!(executions_of(&log, "declared_names"), 0, "{log:?}");
}

#[test]
fn a_whitespace_shift_renumbers_without_reprojecting_consumers() {
    // The split-volatility design, observed directly: ranges shifted,
    // so the numbering's value changes — but the item tree is
    // range-free, equal, backdated.
    let mut db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), b"<?php function greet() {}".to_vec());
    let _ = declared_names(&db, file);
    let _ = ast_id_map(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php\n\nfunction greet() {}".to_vec());
    let _ = declared_names(&db, file);
    let _ = ast_id_map(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "ast_id_map"),
        1,
        "ranges shifted, the numbering re-runs: {log:?}",
    );
    assert_eq!(executions_of(&log, "item_tree"), 1, "{log:?}");
    assert_eq!(
        executions_of(&log, "declared_names"),
        0,
        "a range-free item tree must backdate: {log:?}",
    );
}

#[test]
fn a_signature_edit_reaches_the_consumers() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function greet() { return 1; }".to_vec(),
    );
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function hello() { return 1; }".to_vec());
    let names = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "declared_names"),
        1,
        "a renamed declaration must reach the consumers: {log:?}",
    );
    assert_eq!(names, vec!["hello".to_owned()]);
}

#[test]
fn adding_a_declaration_reaches_the_consumers() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php class A {} class B {}".to_vec());
    let names = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(executions_of(&log, "declared_names"), 1, "{log:?}");
    assert_eq!(names, vec!["A".to_owned(), "B".to_owned()]);
}

#[test]
fn editing_one_file_reprojects_only_that_file() {
    let mut db = TestDatabase::default();
    let edited = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
    let untouched = SourceFile::new(&db, FileId::new(1), b"<?php class B {}".to_vec());
    let _ = declared_names(&db, edited);
    let _ = declared_names(&db, untouched);
    db.take_executed();

    edited
        .set_bytes(&mut db)
        .to(b"<?php class A {} class C {}".to_vec());
    let _ = declared_names(&db, edited);
    let _ = declared_names(&db, untouched);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "only the edited file reprojects: {log:?}",
    );
    assert_eq!(executions_of(&log, "declared_names"), 1, "{log:?}");
}

#[test]
fn a_new_file_does_not_reanalyze_existing_files() {
    let db = TestDatabase::default();
    let existing = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
    let _ = declared_names(&db, existing);
    db.take_executed();

    let added = SourceFile::new(&db, FileId::new(1), b"<?php class B {}".to_vec());
    let _ = declared_names(&db, added);
    let _ = declared_names(&db, existing);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "only the new file lowers: {log:?}",
    );
    assert_eq!(executions_of(&log, "declared_names"), 1, "{log:?}");
}

/// A stand-in for part 6's checks: the inheritance names of one file
/// that do not resolve. It reads the file's item tree and the
/// per-name lookups, nothing else; the firewall must spare it from
/// every unrelated change.
#[salsa::tracked]
fn unresolved_inheritance_names(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
) -> Vec<String> {
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let tree = item_tree(db, file);
    let mut unresolved = Vec::new();
    for declaration in &tree.declarations {
        let tables = UseTables::for_namespace(tree, &declaration.namespace);
        for name in declaration
            .extends
            .iter()
            .chain(&declaration.implements)
            .chain(&declaration.trait_uses)
        {
            let resolution = resolve_name(
                db,
                sources,
                &declaration.namespace,
                &tables,
                name,
                SymbolSpace::ClassLike,
            );
            if resolution.is_none() {
                unresolved.push(name.clone());
            }
        }
    }
    unresolved
}

struct ResolutionFixture {
    db: TestDatabase,
    files: Vec<SourceFile>,
    set: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

impl ResolutionFixture {
    fn new(sources: &[&str]) -> Self {
        Self::with_stubs(sources, StubIndex::default())
    }

    /// Like [`Self::new`], but with a caller-supplied stub index: the
    /// version-range scope test needs a reference that resolves
    /// through the stub table (never through source) to observe
    /// `stub_symbol_table` react to a configuration edit.
    fn with_stubs(sources: &[&str], stub_index: StubIndex) -> Self {
        let db = TestDatabase::default();
        let files: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let set = AnalyzedFileSet::new(&db, files.clone());
        let stubs = StubIndexInput::builder(stub_index)
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Self {
            db,
            files,
            set,
            stubs,
            configuration,
        }
    }

    fn unresolved(&self, file_index: usize) -> Vec<String> {
        unresolved_inheritance_names(
            &self.db,
            self.set,
            self.stubs,
            self.configuration,
            self.files[file_index],
        )
    }
}

#[test]
fn adding_an_unrelated_symbol_spares_every_consumer() {
    // The spec's firewall sentence, observed directly: adding a symbol
    // in one file does not invalidate the checks of files that never
    // reference it.
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base {}",
        "<?php namespace Elsewhere; class Unrelated {}",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.files[2]
        .set_bytes(&mut fixture.db)
        .to(b"<?php namespace Elsewhere; class Unrelated {} class Another {}".to_vec());
    let _ = fixture.unresolved(0);

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        1,
        "a new signature rebuilds the table: {log:?}",
    );
    assert!(
        executions_of(&log, "lookup_symbol") >= 1,
        "the cached lookups re-run cheaply: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        0,
        "an unchanged lookup answer must backdate, sparing the consumer: {log:?}",
    );
}

#[test]
fn a_body_edit_in_another_file_stops_at_its_item_tree() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base { public function greet() { return 1; } }",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.files[1]
        .set_bytes(&mut fixture.db)
        .to(b"<?php namespace App; class Base { public function greet() { return 2; } }".to_vec());
    let _ = fixture.unresolved(0);

    let log = fixture.db.take_executed();
    assert_eq!(executions_of(&log, "item_tree"), 1, "{log:?}");
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        0,
        "an identical item tree must never rebuild the table: {log:?}",
    );
    assert_eq!(executions_of(&log, "lookup_symbol"), 0, "{log:?}");
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        0,
        "{log:?}",
    );
}

#[test]
fn deleting_the_referenced_declaration_reaches_the_consumer() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base {}",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.files[1]
        .set_bytes(&mut fixture.db)
        .to(b"<?php namespace App;".to_vec());
    assert_eq!(fixture.unresolved(0), vec!["Base".to_owned()]);

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        1,
        "a changed lookup answer must reach the consumer: {log:?}",
    );
}

#[test]
fn declaring_a_previously_missing_symbol_reaches_the_consumer() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App;",
    ]);
    assert_eq!(fixture.unresolved(0), vec!["Base".to_owned()]);
    fixture.db.take_executed();

    fixture.files[1]
        .set_bytes(&mut fixture.db)
        .to(b"<?php namespace App; class Base {}".to_vec());
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        1,
        "{log:?}",
    );
}

#[test]
fn a_version_range_change_never_touches_the_source_table() {
    // Base is only a stub symbol here (never a source declaration):
    // `lookup_symbol` shadows the stub table behind the source table,
    // so a fixture where the source side already answers would never
    // exercise the stub side at all. Routing the reference through the
    // stub table is what lets this test observe `stub_symbol_table`
    // react to the configuration edit. A second, version-gated stub
    // symbol is included purely so the filtered view's *content*
    // actually differs between the two ranges; Base itself stays
    // always-available, so the consumer's final answer never changes.
    let mut fixture = ResolutionFixture::with_stubs(
        &[
            "<?php namespace App; class Consumer extends Base {}",
            "<?php namespace App;",
        ],
        StubIndex::from_symbols(vec![
            StubSymbol {
                name: "App\\Base".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "App\\Newer".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 4)),
                    ..StubAvailability::ALWAYS
                },
            },
        ]),
    );
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture
        .configuration
        .set_php_version_range(&mut fixture.db)
        .to(PhpVersionRange::point(PhpVersion::new(8, 3)));
    let _ = fixture.unresolved(0);

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "stub_symbol_table"),
        1,
        "the stub side recomputes: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        0,
        "the source side must not: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        0,
        "unchanged stub answers backdate: {log:?}",
    );
}
