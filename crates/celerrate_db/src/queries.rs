use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_source::{LineIndex, SourceText, SourceTooLarge, TextRange, TextSize};
use celerrate_syntax::Parse;

use crate::input::SourceFile;

/// Decodes a file's bytes into engine-ready text. The only failure is
/// an oversized input; everything else (byte-order mark, invalid
/// UTF-8) is provenance on the decoded text.
#[salsa::tracked(returns(ref))]
pub fn source_text(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> Result<SourceText, SourceTooLarge> {
    SourceText::from_bytes(file.bytes(db))
}

/// Parses a file's decoded text into the lossless syntax tree. A file
/// that fails to decode parses as empty: the decode failure itself is
/// reported by `file_diagnostics`, and every consumer still receives a
/// well-formed tree.
#[salsa::tracked(returns(ref))]
pub fn parse(db: &dyn salsa::Database, file: SourceFile) -> Parse {
    match source_text(db, file) {
        Ok(text) => celerrate_syntax::parse(text.text()),
        Err(_) => celerrate_syntax::parse(""),
    }
}

/// The line/column index of a file's decoded text. A file that fails
/// to decode indexes as empty, mirroring `parse`.
#[salsa::tracked(returns(ref))]
pub fn line_index(db: &dyn salsa::Database, file: SourceFile) -> LineIndex {
    match source_text(db, file) {
        Ok(text) => LineIndex::new(text.text()),
        Err(_) => LineIndex::new(""),
    }
}

/// The content address of one file: the blake3 hash of its raw bytes.
/// Every persistent-cache entry is keyed by it. A tracked query so one
/// revision hashes a file at most once, wherever the address is needed.
pub type ContentHash = [u8; 32];

/// Hashes a file's raw bytes into its content address.
#[salsa::tracked]
pub fn content_hash(db: &dyn salsa::Database, file: SourceFile) -> ContentHash {
    *blake3::hash(file.bytes(db)).as_bytes()
}

/// The file's decoded bytes would exceed the 4 GiB engine cap.
pub const SOURCE_TOO_LARGE: DiagnosticId = DiagnosticId::new("CEL0001");

/// Every identifier this crate allocates, for the registry check at the
/// composition root.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[SOURCE_TOO_LARGE];

/// Every diagnostic of one file, in deterministic source order: the
/// decode failure when the file could not be decoded, the projected
/// syntax diagnostics otherwise. Semantic families join in later parts.
#[salsa::tracked(returns(ref))]
pub fn file_diagnostics(db: &dyn salsa::Database, file: SourceFile) -> Vec<Diagnostic> {
    let file_id = file.file_id(db);
    match source_text(db, file) {
        Err(_) => vec![Diagnostic::spanned(
            SOURCE_TOO_LARGE,
            Severity::Error,
            file_id,
            TextRange::empty(TextSize::from(0)),
            "the file exceeds the 4 GiB source size limit".to_owned(),
        )],
        Ok(_) => parse(db, file)
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.to_diagnostic(file_id))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, LineColumn, TextSize};
    use salsa::Setter;

    use crate::testing::TestDatabase;
    use crate::{SourceFile, line_index, parse, source_text};

    #[test]
    fn source_text_decodes_the_bytes() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"\xEF\xBB\xBF<?php".to_vec());
        let decoded = source_text(&db, file).as_ref().unwrap();
        assert_eq!(decoded.text(), "<?php");
        assert!(decoded.had_utf8_bom());
    }

    #[test]
    fn parse_produces_a_lossless_tree_over_the_decoded_text() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        let parsed = parse(&db, file);
        assert_eq!(parsed.tree().text().to_string(), "<?php echo 1;");
    }

    #[test]
    fn line_index_maps_offsets_over_the_decoded_text() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php\necho 1;".to_vec());
        let index = line_index(&db, file);
        assert_eq!(
            index.line_column(TextSize::from(6)),
            LineColumn { line: 1, column: 0 },
        );
    }

    #[test]
    fn editing_bytes_reparses() {
        let mut db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        assert_eq!(parse(&db, file).tree().text().to_string(), "<?php echo 1;");
        file.set_bytes(&mut db).to(b"<?php echo 2;".to_vec());
        assert_eq!(parse(&db, file).tree().text().to_string(), "<?php echo 2;");
    }

    use crate::{SOURCE_TOO_LARGE, file_diagnostics};

    #[test]
    fn clean_files_have_no_diagnostics() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        assert!(file_diagnostics(&db, file).is_empty());
    }

    #[test]
    fn syntax_diagnostics_project_with_the_file_identifier() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(9), b"<?php echo ;".to_vec());
        let diagnostics = file_diagnostics(&db, file);
        assert!(!diagnostics.is_empty());
        for diagnostic in diagnostics {
            assert_eq!(
                diagnostic.span().map(|(file, _)| file),
                Some(FileId::new(9))
            );
            assert_eq!(diagnostic.severity, celerrate_diagnostics::Severity::Error);
            assert!(diagnostic.id.as_str().starts_with("CEL"));
        }
    }

    #[test]
    fn source_too_large_is_stable() {
        assert_eq!(SOURCE_TOO_LARGE.as_str(), "CEL0001");
    }

    use crate::content_hash;

    #[test]
    fn the_content_hash_is_a_function_of_the_bytes_alone() {
        let db = TestDatabase::default();
        let first = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        let second = SourceFile::new(&db, FileId::new(9), b"<?php echo 1;".to_vec());
        let different = SourceFile::new(&db, FileId::new(2), b"<?php echo 2;".to_vec());
        assert_eq!(content_hash(&db, first), content_hash(&db, second));
        assert_ne!(content_hash(&db, first), content_hash(&db, different));
    }

    #[test]
    fn editing_bytes_changes_the_hash() {
        let mut db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        let before = content_hash(&db, file);
        file.set_bytes(&mut db).to(b"<?php echo 2;".to_vec());
        assert_ne!(before, content_hash(&db, file));
    }
}
