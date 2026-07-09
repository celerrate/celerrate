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

use celerrate_source::{TextRange, TextSize};

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

#[test]
fn invalid_byte_at_start_is_replaced_and_recorded() {
    let source = SourceText::from_bytes(b"\xFFabc").expect("fits the cap");
    assert_eq!(source.text(), "\u{FFFD}abc");
    assert_eq!(source.replacements(), &[range(0, 3)]);
    assert!(!source.is_pristine());
}

#[test]
fn invalid_byte_in_the_middle_is_replaced_and_recorded() {
    let source = SourceText::from_bytes(b"ab\xFFcd").expect("fits the cap");
    assert_eq!(source.text(), "ab\u{FFFD}cd");
    assert_eq!(source.replacements(), &[range(2, 5)]);
}

#[test]
fn invalid_byte_at_the_end_is_replaced_and_recorded() {
    let source = SourceText::from_bytes(b"ab\xFF").expect("fits the cap");
    assert_eq!(source.text(), "ab\u{FFFD}");
    assert_eq!(source.replacements(), &[range(2, 5)]);
}

#[test]
fn consecutive_invalid_bytes_each_get_a_replacement() {
    let source = SourceText::from_bytes(b"a\xFF\xFEb").expect("fits the cap");
    assert_eq!(source.text(), "a\u{FFFD}\u{FFFD}b");
    assert_eq!(source.replacements(), &[range(1, 4), range(4, 7)]);
}

#[test]
fn truncated_multibyte_character_at_the_end_is_one_replacement() {
    // "é" is C3 A9; the input stops after C3.
    let source = SourceText::from_bytes(b"caf\xC3").expect("fits the cap");
    assert_eq!(source.text(), "caf\u{FFFD}");
    assert_eq!(source.replacements(), &[range(3, 6)]);
}

#[test]
fn truncated_multibyte_sequence_in_the_middle_is_one_replacement() {
    // E0 A0 is the truncated prefix of a three-byte sequence; decoding
    // resumes at the following valid byte.
    let source = SourceText::from_bytes(b"a\xE0\xA0b").expect("fits the cap");
    assert_eq!(source.text(), "a\u{FFFD}b");
    assert_eq!(source.replacements(), &[range(1, 4)]);
}

#[test]
fn literal_replacement_character_in_valid_input_is_not_recorded() {
    let source = SourceText::from_bytes("a\u{FFFD}b".as_bytes()).expect("fits the cap");
    assert_eq!(source.text(), "a\u{FFFD}b");
    assert!(source.replacements().is_empty());
    assert!(source.is_pristine());
}

#[test]
fn bom_and_replacements_combine_in_pristine() {
    let source = SourceText::from_bytes(b"\xEF\xBB\xBFa\xFF").expect("fits the cap");
    assert_eq!(source.text(), "a\u{FFFD}");
    assert!(source.had_utf8_bom());
    assert_eq!(source.replacements(), &[range(1, 4)]);
    assert!(!source.is_pristine());
}
