use celerrate_source::FileId;

/// One analyzed file: its identifier (assigned by the virtual file
/// system) and its raw bytes. Decoding is a derived query, so decode
/// provenance and failures stay incremental, not input state.
#[salsa::input]
pub struct SourceFile {
    pub file_id: FileId,
    #[returns(ref)]
    pub bytes: Vec<u8>,
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
}
