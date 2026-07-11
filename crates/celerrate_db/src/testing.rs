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
    let mut incremental = TestDatabase::default();
    let mut current: Vec<Vec<u8>> = initial.iter().map(|bytes| bytes.to_vec()).collect();
    let files: Vec<SourceFile> = current
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            SourceFile::new(&incremental, FileId::new(index as u32), bytes.clone())
        })
        .collect();

    assert_matches_from_scratch(&incremental, &files, &current);
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
        assert_matches_from_scratch(&incremental, &files, &current);
    }
}

fn assert_matches_from_scratch(
    incremental: &TestDatabase,
    files: &[SourceFile],
    current: &[Vec<u8>],
) {
    let from_scratch = TestDatabase::default();
    for (index, (file, bytes)) in files.iter().zip(current).enumerate() {
        let fresh_file = SourceFile::new(&from_scratch, FileId::new(index as u32), bytes.clone());
        assert_eq!(
            parse(incremental, *file).tree().text().to_string(),
            parse(&from_scratch, fresh_file).tree().text().to_string(),
            "tree text diverged for file {index}",
        );
        assert_eq!(
            file_diagnostics(incremental, *file),
            file_diagnostics(&from_scratch, fresh_file),
            "diagnostics diverged for file {index}",
        );
    }
}
