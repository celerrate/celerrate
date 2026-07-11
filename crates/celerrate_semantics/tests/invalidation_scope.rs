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

use celerrate_db::SourceFile;
use celerrate_db::testing::TestDatabase;
use celerrate_semantics::{ast_id_map, item_tree};
use celerrate_source::FileId;
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
