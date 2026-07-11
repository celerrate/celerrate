use celerrate_source::FileId;
use std::fmt;

/// One analyzed file: its identifier (assigned by the virtual file
/// system) and its raw bytes. Decoding is a derived query, so decode
/// provenance and failures stay incremental, not input state.
#[salsa::input]
pub struct SourceFile {
    pub file_id: FileId,
    #[returns(ref)]
    pub bytes: Vec<u8>,
}

impl fmt::Debug for SourceFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceFile").finish()
    }
}

/// The analyzed file set: every file the current analysis covers, in
/// the deterministic order the composition root established. Whole-
/// project queries depend on this input, so membership changes (a file
/// created or deleted) invalidate them; editing one member's bytes
/// changes that file's input, never the set itself.
#[salsa::input]
pub struct AnalyzedFileSet {
    #[returns(ref)]
    pub files: Vec<SourceFile>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::FileId;
    use salsa::Setter;

    use crate::SourceFile;
    use crate::testing::TestDatabase;

    #[test]
    fn a_source_file_stores_its_identifier_and_bytes() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(3), b"<?php".to_vec());
        assert_eq!(file.file_id(&db), FileId::new(3));
        assert_eq!(file.bytes(&db), b"<?php");
    }

    #[test]
    fn setting_bytes_replaces_them() {
        let mut db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"before".to_vec());
        file.set_bytes(&mut db).to(b"after".to_vec());
        assert_eq!(file.bytes(&db), b"after");
    }

    use crate::AnalyzedFileSet;

    #[test]
    fn the_analyzed_file_set_stores_and_updates_its_files() {
        let mut db = TestDatabase::default();
        let first = SourceFile::new(&db, FileId::new(0), b"<?php".to_vec());
        let second = SourceFile::new(&db, FileId::new(1), b"<?php".to_vec());
        let set = AnalyzedFileSet::new(&db, vec![first]);
        assert_eq!(set.files(&db), &vec![first]);

        set.set_files(&mut db).to(vec![first, second]);
        assert_eq!(set.files(&db), &vec![first, second]);
    }
}
