#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{lex_verified, texts};

#[test]
fn single_quoted_strings_are_one_token() {
    assert_eq!(
        texts("<?php 'hello';"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "'hello'".to_owned()),
            (Semicolon, ";".to_owned()),
        ]
    );
}

#[test]
fn single_quoted_escapes_do_not_terminate() {
    assert_eq!(
        texts(r"<?php 'a\'b\\'"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, r"'a\'b\\'".to_owned()),
        ]
    );
}

#[test]
fn single_quoted_strings_do_not_interpolate() {
    assert_eq!(
        texts("<?php '$name {$x}'"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "'$name {$x}'".to_owned()),
        ]
    );
}

#[test]
fn binary_prefix_belongs_to_the_string_token() {
    assert_eq!(
        texts("<?php b'x' B'y'"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "b'x'".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "B'y'".to_owned()),
        ]
    );
}

#[test]
fn unterminated_single_quoted_string_keeps_its_kind() {
    let (tokens, diagnostics) = lex_verified("<?php 'open");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, SingleQuotedString]
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnterminatedString);
    // Points at the opening quote.
    assert_eq!(u32::from(diagnostic.range.start()), 6);
    assert_eq!(u32::from(diagnostic.range.end()), 7);
}
