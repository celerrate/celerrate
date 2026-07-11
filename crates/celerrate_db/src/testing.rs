//! Test support: an instrumented database for this crate's tests, the
//! invalidation-scope tests, and the incremental harness. The concrete
//! production database is assembled at the composition root (the CLI
//! binary, a later part), not here.

use std::sync::{Arc, Mutex, PoisonError};

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
