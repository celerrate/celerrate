//! Behavior tests for `SourceText` decoding. `expect` is allowed here:
//! failing loudly is exactly what a test should do.
#![allow(clippy::expect_used)]

use celerrate_source::SourceText;

#[test]
fn plain_ascii_decodes_unchanged() {
    let source = SourceText::from_bytes(b"<?php echo 1;").expect("fits the cap");
    assert_eq!(source.text(), "<?php echo 1;");
    assert!(!source.had_utf8_bom());
    assert!(source.replacements().is_empty());
    assert!(source.is_pristine());
}

#[test]
fn multibyte_utf8_decodes_unchanged() {
    let source = SourceText::from_bytes("héllo 🐘".as_bytes()).expect("fits the cap");
    assert_eq!(source.text(), "héllo 🐘");
    assert!(source.is_pristine());
}

#[test]
fn empty_input_decodes_to_empty_pristine_text() {
    let source = SourceText::from_bytes(b"").expect("fits the cap");
    assert_eq!(source.text(), "");
    assert!(source.is_pristine());
}

#[test]
fn utf8_bom_is_stripped_and_recorded() {
    let source = SourceText::from_bytes(b"\xEF\xBB\xBF<?php").expect("fits the cap");
    assert_eq!(source.text(), "<?php");
    assert!(source.had_utf8_bom());
    assert!(source.replacements().is_empty());
    assert!(!source.is_pristine());
}

#[test]
fn utf8_bom_alone_decodes_to_empty_text() {
    let source = SourceText::from_bytes(b"\xEF\xBB\xBF").expect("fits the cap");
    assert_eq!(source.text(), "");
    assert!(source.had_utf8_bom());
    assert!(!source.is_pristine());
}

#[test]
fn bom_bytes_after_the_start_are_kept_as_text() {
    let source = SourceText::from_bytes(b"a\xEF\xBB\xBFb").expect("fits the cap");
    // U+FEFF in the middle of the text is a zero-width no-break space,
    // not a byte-order mark: it stays in the text.
    assert_eq!(source.text(), "a\u{FEFF}b");
    assert!(!source.had_utf8_bom());
}

#[test]
fn line_endings_and_nul_bytes_pass_through() {
    let source = SourceText::from_bytes(b"a\r\nb\0c").expect("fits the cap");
    assert_eq!(source.text(), "a\r\nb\0c");
    assert!(source.is_pristine());
}
