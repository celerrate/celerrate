use celerrate_source::{LineIndex, SourceText, SourceTooLarge};
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
}
