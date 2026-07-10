#![allow(clippy::expect_used)] // test assertions require expect for diagnosing failures

mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{kinds, lex_verified};

#[test]
fn a_stray_control_byte_is_a_one_character_error_token() {
    let (tokens, diagnostics) = lex_verified("<?php \u{1};");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, Error, Semicolon]
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnexpectedCharacter);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
    assert_eq!(u32::from(diagnostic.range.end()), 7);
}

#[test]
fn lexing_continues_after_an_error() {
    assert_eq!(
        kinds("<?php \u{1}\u{2} echo"),
        [OpenTag, Whitespace, Error, Error, Whitespace, Echo]
    );
}

#[test]
fn ascii_delete_is_an_unexpected_character() {
    // Non-ASCII characters are all name starts under PHP's
    // byte-oriented rule, so unexpected characters are always ASCII:
    // assert the DEL control byte.
    assert_eq!(kinds("<?php \u{7F}"), [OpenTag, Whitespace, Error]);
}

#[test]
fn degenerate_input_terminates_and_stays_lossless() {
    // A pathological soup of control bytes, quotes-free: every char
    // must come back out, one Error token each, no hang.
    let soup: String = ('\u{0}'..='\u{8}').cycle().take(300).collect();
    let source = format!("<?php {soup}");
    let (tokens, diagnostics) = lex_verified(&source);
    assert_eq!(tokens.len(), 302);
    assert_eq!(diagnostics.len(), 300);
}

#[test]
fn form_feed_is_not_whitespace_in_scripting_mode() {
    // Zend's whitespace is space, tab, \n, and \r only; a form feed in
    // PHP code is an unexpected character.
    let (tokens, diagnostics) = lex_verified("<?php \u{C};");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, Error, Semicolon]
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnexpectedCharacter);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
}

#[test]
fn form_feed_stays_ordinary_content_outside_scripting() {
    let (tokens, diagnostics) = lex_verified("a\u{C}b<?php '\u{C}' ?>\u{C}");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [
            InlineHtml,
            OpenTag,
            Whitespace,
            SingleQuotedString,
            Whitespace,
            CloseTag,
            InlineHtml,
        ]
    );
    assert!(diagnostics.is_empty());
}
