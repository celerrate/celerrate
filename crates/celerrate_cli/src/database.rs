//! The concrete salsa database: the first in the workspace outside
//! `celerrate_db::testing`, and the one the composition root owns.
//!
//! Cloning is how rayon fans out and how `--watch` runs an analysis
//! beside the main thread: the storage is shared, the handle is
//! per-thread. A setter on `&mut AnalysisDatabase` is what cancels every
//! other handle, which is the invariant `--watch` is built on.

/// The analysis database.
#[salsa::db]
#[derive(Clone)]
pub struct AnalysisDatabase {
    storage: salsa::Storage<Self>,
}

impl Default for AnalysisDatabase {
    fn default() -> Self {
        Self {
            storage: salsa::Storage::new(None),
        }
    }
}

#[salsa::db]
impl salsa::Database for AnalysisDatabase {}

#[cfg(test)]
mod tests {
    use super::AnalysisDatabase;

    #[test]
    fn the_database_answers_a_query() {
        use celerrate_db::{SourceFile, source_text};
        use celerrate_source::FileId;

        let db = AnalysisDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        assert!(source_text(&db, file).is_ok());
    }

    #[test]
    fn a_clone_shares_the_storage() {
        let db = AnalysisDatabase::default();
        let snapshot = db.clone();
        drop(snapshot);
    }
}
