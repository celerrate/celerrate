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
    SymbolQuery, SymbolResolution, SymbolSources, SymbolSpace, UseTables, ast_id_map,
    folded_symbol_key, item_tree, lookup_symbol, reference_diagnostics, resolve_name,
    source_symbol_table, syntax_version_diagnostics,
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

impl ResolutionFixture {
    /// Adds a file to the analyzed set: a new `SourceFile`, and the set
    /// input rewritten. This is the mutation `--watch` performs when a
    /// file appears.
    fn add_file(&mut self, source: &str) -> SourceFile {
        let file_id = FileId::new(u32::try_from(self.files.len()).unwrap());
        let file = SourceFile::new(&self.db, file_id, source.as_bytes().to_vec());
        self.files.push(file);
        self.set.set_files(&mut self.db).to(self.files.clone());
        file
    }

    /// Drops a file from the analyzed set. `SourceFile` has no deleted
    /// state, and a tombstone (empty bytes) would leave the set lying
    /// about what it contains, so the member leaves the set outright.
    fn remove_file(&mut self, index: usize) {
        self.files.remove(index);
        self.set.set_files(&mut self.db).to(self.files.clone());
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

#[test]
fn an_unrelated_declaration_spares_other_files_reference_checks() {
    let mut db = TestDatabase::default();
    let library = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php namespace Lib; class Helper {}".to_vec(),
    );
    let consumer = SourceFile::new(
        &db,
        FileId::new(1),
        b"<?php namespace App; use Lib\\Helper; $x = new Helper();".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![library, consumer]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);
    db.take_executed();

    library
        .set_bytes(&mut db)
        .to(b"<?php namespace Lib; class Helper {} class Extra {}".to_vec());
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "reference_diagnostics"),
        1,
        "only the edited file re-checks; the consumer's lookups backdate: {log:?}",
    );
}

#[test]
fn a_version_range_change_re_runs_the_gating_queries() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php readonly class Point {}".to_vec(),
    );
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    assert_eq!(
        syntax_version_diagnostics(&db, file, configuration).len(),
        1
    );
    db.take_executed();

    configuration
        .set_php_version_range(&mut db)
        .to(PhpVersionRange::new(
            PhpVersion::new(8, 2),
            PhpVersion::new(8, 5),
        ));
    let diagnostics = syntax_version_diagnostics(&db, file, configuration);

    assert_eq!(diagnostics, &vec![]);
    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "syntax_version_diagnostics"),
        1,
        "the configuration is an input of the gating query: {log:?}",
    );
}

#[test]
fn a_comment_only_edit_elsewhere_spares_the_consumer() {
    let mut db = TestDatabase::default();
    let library = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php namespace Lib; class Helper {}".to_vec(),
    );
    let consumer = SourceFile::new(
        &db,
        FileId::new(1),
        b"<?php namespace App; use Lib\\Helper; $x = new Helper();".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![library, consumer]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);
    db.take_executed();

    library
        .set_bytes(&mut db)
        .to(b"<?php namespace Lib; class Helper { /* note */ }".to_vec());
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "reference_diagnostics"),
        1,
        "the edited file re-checks over its new tree; the consumer's \
         lookups backdate behind the unchanged symbol table: {log:?}",
    );
}

#[test]
fn adding_a_file_that_declares_nothing_invalidates_nothing_downstream() {
    // The benign case backdating is supposed to absorb: the symbol table
    // re-runs once because its input set changed, produces an identical
    // value, and no consumer of a lookup re-runs.
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base {}",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.add_file("<?php echo 1;");
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        1,
        "the set changed, so the table rebuilds once: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        0,
        "the table backdates, so no consumer re-runs: {log:?}",
    );
}

#[test]
fn adding_a_file_that_declares_the_missing_symbol_reaches_the_consumer() {
    let mut fixture =
        ResolutionFixture::new(&["<?php namespace App; class Consumer extends Base {}"]);
    assert_eq!(fixture.unresolved(0), vec!["Base".to_owned()]);
    fixture.db.take_executed();

    fixture.add_file("<?php namespace App; class Base {}");
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        1,
        "a changed lookup answer must reach the consumer: {log:?}",
    );
}

/// The coverage gap the architecture audit named directly: no test
/// exercised a `define()` edit's effect on the table. A `define()` added
/// inside a body is invisible to the item traversal itself, but visible
/// to the item tree's separate `defines` list (see `items.rs`'s module
/// doc), so the tree value changes, the table rebuilds once, and a
/// lookup that used to answer `None` now answers `Source`.
#[test]
fn a_body_edit_adding_a_define_reaches_the_table_and_the_lookup() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function boot() { return 1; }".to_vec(),
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
    fn query(db: &TestDatabase) -> SymbolQuery<'_> {
        SymbolQuery::new(
            db,
            SymbolSpace::Constant,
            folded_symbol_key(SymbolSpace::Constant, "APP_ROOT"),
        )
    }
    assert_eq!(
        lookup_symbol(&db, files, stubs, configuration, query(&db)),
        None
    );
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function boot() { define('APP_ROOT', 1); return 1; }".to_vec());
    let resolution = lookup_symbol(&db, files, stubs, configuration, query(&db));

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "the edited file reprojects, now with a define: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        1,
        "a define appearing must rebuild the table: {log:?}",
    );
    assert!(
        matches!(resolution, Some(SymbolResolution::Source { .. })),
        "the new define must resolve: {resolution:?}",
    );
}

/// The other half of the same gap: a body edit that leaves a file's set
/// of `define()` names unchanged must still backdate at the item-tree
/// level, exactly like any other define-free body edit, and spare the
/// table and every lookup behind it.
#[test]
fn a_define_free_body_edit_in_a_define_carrying_file_backdates_the_table() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function boot() { define('APP_ROOT', 1); return 1; }".to_vec(),
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
    fn query(db: &TestDatabase) -> SymbolQuery<'_> {
        SymbolQuery::new(
            db,
            SymbolSpace::Constant,
            folded_symbol_key(SymbolSpace::Constant, "APP_ROOT"),
        )
    }
    assert!(matches!(
        lookup_symbol(&db, files, stubs, configuration, query(&db)),
        Some(SymbolResolution::Source { .. })
    ));
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function boot() { define('APP_ROOT', 1); return 2; }".to_vec());
    let _ = source_symbol_table(&db, files);
    let _ = lookup_symbol(&db, files, stubs, configuration, query(&db));

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "the edited file still reprojects: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        0,
        "an unchanged set of defines must backdate the item tree, \
         sparing the table entirely: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "lookup_symbol"),
        0,
        "and every lookup behind it: {log:?}",
    );
}

#[test]
fn deleting_the_file_that_declares_the_symbol_reaches_the_consumer() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base {}",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.remove_file(1);
    assert_eq!(fixture.unresolved(0), vec!["Base".to_owned()]);

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        1,
        "a deleted declaration must reach the consumer: {log:?}",
    );
}
