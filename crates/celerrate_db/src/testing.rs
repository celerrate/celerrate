//! Test support: an instrumented database for this crate's tests, the
//! invalidation-scope tests, and the incremental harness. The concrete
//! production database is assembled at the composition root (the CLI
//! binary, a later part), not here.

use std::sync::{Arc, Mutex, PoisonError};

use salsa::Setter;

/// A salsa database that records every query execution.
///
/// Each `WillExecute` event is captured as its Debug rendering (for
/// example `parse(Id(400))`); invalidation-scope tests assert on those
/// strings to pin exactly which queries re-ran after an edit.
#[salsa::db]
#[derive(Clone)]
pub struct TestDatabase {
    storage: salsa::Storage<Self>,
    executed: Arc<Mutex<Vec<String>>>,
}

impl Default for TestDatabase {
    fn default() -> Self {
        let executed: Arc<Mutex<Vec<String>>> = Arc::default();
        let storage = salsa::Storage::new(Some(Box::new({
            let executed = executed.clone();
            move |event: salsa::Event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    let mut log = executed.lock().unwrap_or_else(PoisonError::into_inner);
                    log.push(format!("{database_key:?}"));
                }
            }
        })));
        Self { storage, executed }
    }
}

impl TestDatabase {
    /// Drains the executions recorded since the last call.
    pub fn take_executed(&self) -> Vec<String> {
        let mut log = self.executed.lock().unwrap_or_else(PoisonError::into_inner);
        core::mem::take(&mut *log)
    }
}

#[salsa::db]
impl salsa::Database for TestDatabase {}

use celerrate_source::FileId;

use crate::{SourceFile, file_diagnostics, parse};

/// Replays an edit sequence against one incremental database and, after
/// every edit, asserts each file's analysis is byte-for-byte identical
/// to a from-scratch database built on the current state.
///
/// `initial` provides the starting bytes of file 0, 1, 2, ...; each
/// edit is `(file index, new bytes)`. Panics (test-style assertions)
/// on any divergence or out-of-range file index.
pub fn assert_incremental_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])]) {
    assert_incremental_consistency_with(
        initial,
        edits,
        &|incremental, file, from_scratch, fresh_file, index| {
            assert_eq!(
                parse(incremental, file).tree().text().to_string(),
                parse(from_scratch, fresh_file).tree().text().to_string(),
                "tree text diverged for file {index}",
            );
            assert_eq!(
                file_diagnostics(incremental, file),
                file_diagnostics(from_scratch, fresh_file),
                "diagnostics diverged for file {index}",
            );
        },
    );
}

/// The closure form of the replay: upper layers extend the same
/// harness with their own per-file comparison, without any dependency
/// from this crate upward. The closure receives the incremental
/// database and its file handle, the from-scratch database and its
/// fresh handle, and the file index; it runs for every file after the
/// initial state and after every edit.
pub fn assert_incremental_consistency_with(
    initial: &[&[u8]],
    edits: &[(usize, &[u8])],
    assert_file_matches: &dyn Fn(&TestDatabase, SourceFile, &TestDatabase, SourceFile, usize),
) {
    assert_incremental_consistency_with_context(
        initial,
        edits,
        &|_, files| files.to_vec(),
        &|incremental, files, from_scratch, fresh_files| {
            for (index, (file, fresh_file)) in files.iter().zip(fresh_files).enumerate() {
                assert_file_matches(incremental, *file, from_scratch, *fresh_file, index);
            }
        },
    );
}

/// The state-level form of the replay: `make_context` creates the
/// whole-project inputs (the analyzed file set, the stub index, the
/// configuration) once for the incremental database and once per
/// from-scratch database, and `assert_state_matches` compares the two
/// databases after the initial state and after every edit.
pub fn assert_incremental_consistency_with_context<Context>(
    initial: &[&[u8]],
    edits: &[(usize, &[u8])],
    make_context: &dyn Fn(&TestDatabase, &[SourceFile]) -> Context,
    assert_state_matches: &dyn Fn(&TestDatabase, &Context, &TestDatabase, &Context),
) {
    let mut incremental = TestDatabase::default();
    let mut current: Vec<Vec<u8>> = initial.iter().map(|bytes| bytes.to_vec()).collect();
    let files: Vec<SourceFile> = current
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            SourceFile::new(&incremental, FileId::new(index as u32), bytes.clone())
        })
        .collect();
    let context = make_context(&incremental, &files);

    assert_state_against_scratch(
        &incremental,
        &context,
        &current,
        make_context,
        assert_state_matches,
    );
    for &(file_index, new_bytes) in edits {
        assert!(
            file_index < files.len(),
            "edit targets unknown file index {file_index}",
        );
        let (Some(slot), Some(file)) = (current.get_mut(file_index), files.get(file_index)) else {
            // Unreachable: guarded by the assertion above.
            return;
        };
        *slot = new_bytes.to_vec();
        file.set_bytes(&mut incremental).to(new_bytes.to_vec());
        assert_state_against_scratch(
            &incremental,
            &context,
            &current,
            make_context,
            assert_state_matches,
        );
    }
}

fn assert_state_against_scratch<Context>(
    incremental: &TestDatabase,
    context: &Context,
    current: &[Vec<u8>],
    make_context: &dyn Fn(&TestDatabase, &[SourceFile]) -> Context,
    assert_state_matches: &dyn Fn(&TestDatabase, &Context, &TestDatabase, &Context),
) {
    let from_scratch = TestDatabase::default();
    let fresh_files: Vec<SourceFile> = current
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            SourceFile::new(&from_scratch, FileId::new(index as u32), bytes.clone())
        })
        .collect();
    let fresh_context = make_context(&from_scratch, &fresh_files);
    assert_state_matches(incremental, context, &from_scratch, &fresh_context);
}
