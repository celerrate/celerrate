//! Invalidation-scope tests: after each canonical edit class, assert
//! exactly which queries re-executed. The incremental-consistency
//! harness verifies the result; these tests verify how little work
//! produced it, which is what the published incremental targets
//! depend on (parent spec, section 3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{SourceFile, file_diagnostics, parse};
use celerrate_source::FileId;
use salsa::Setter;

fn executions_of(log: &[String], query: &str) -> usize {
    let prefix = format!("{query}(");
    log.iter()
        .filter(|entry| entry.contains(prefix.as_str()))
        .count()
}

#[test]
fn editing_one_file_reanalyzes_only_that_file() {
    let mut db = TestDatabase::default();
    let edited = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
    let untouched = SourceFile::new(&db, FileId::new(1), b"<?php echo 2;".to_vec());
    let _ = file_diagnostics(&db, edited);
    let _ = file_diagnostics(&db, untouched);
    db.take_executed();

    edited.set_bytes(&mut db).to(b"<?php echo 3;".to_vec());
    let _ = file_diagnostics(&db, edited);
    let _ = file_diagnostics(&db, untouched);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "parse"),
        1,
        "only the edited file reparses: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "file_diagnostics"),
        1,
        "only the edited file recomputes diagnostics: {log:?}",
    );
}

#[test]
fn an_equal_decode_backdates_and_skips_the_reparse() {
    // `\xFF` and `\xFE` are both single invalid bytes: each decodes to
    // one U+FFFD at the same range, so the decoded `SourceText` is
    // identical. Salsa backdates the equal `source_text` result and
    // `parse` never re-executes: early cutoff, observed directly.
    let mut db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), b"<?php echo \xFF;".to_vec());
    let _ = parse(&db, file);
    db.take_executed();

    file.set_bytes(&mut db).to(b"<?php echo \xFE;".to_vec());
    let _ = parse(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "source_text"),
        1,
        "the decode re-runs on new bytes: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "parse"),
        0,
        "an identical decode must backdate, sparing the reparse: {log:?}",
    );
}
