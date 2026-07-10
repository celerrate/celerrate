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

#[test]
fn a_plain_double_quoted_string_is_delimiters_around_one_fragment() {
    assert_eq!(
        texts(r#"<?php "hello""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "hello".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn an_empty_double_quoted_string_is_two_delimiters() {
    assert_eq!(
        texts(r#"<?php """#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn simple_variable_interpolation() {
    assert_eq!(
        texts(r#"<?php "a $name b""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "a ".to_owned()),
            (Variable, "$name".to_owned()),
            (StringFragment, " b".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn escaped_dollars_and_quotes_stay_in_the_fragment() {
    assert_eq!(
        texts(r#"<?php "a \" \$x b""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, r#"a \" \$x b"#.to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn simple_property_interpolation() {
    assert_eq!(
        texts(r#"<?php "$user->name and $user?->name""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$user".to_owned()),
            (Arrow, "->".to_owned()),
            (Identifier, "name".to_owned()),
            (StringFragment, " and ".to_owned()),
            (Variable, "$user".to_owned()),
            (NullsafeArrow, "?->".to_owned()),
            (Identifier, "name".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn only_one_property_level_interpolates() {
    // "$a->b->c" interpolates $a->b; "->c" is literal, as in Zend.
    assert_eq!(
        texts(r#"<?php "$a->b->c""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$a".to_owned()),
            (Arrow, "->".to_owned()),
            (Identifier, "b".to_owned()),
            (StringFragment, "->c".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn simple_offset_interpolation() {
    assert_eq!(
        texts(r#"<?php "$items[0] $map[key] $grid[$x] $list[-1]""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$items".to_owned()),
            (OpenBracket, "[".to_owned()),
            (IntegerLiteral, "0".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$map".to_owned()),
            (OpenBracket, "[".to_owned()),
            (Identifier, "key".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$grid".to_owned()),
            (OpenBracket, "[".to_owned()),
            (Variable, "$x".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$list".to_owned()),
            (OpenBracket, "[".to_owned()),
            (Minus, "-".to_owned()),
            (IntegerLiteral, "1".to_owned()),
            (CloseBracket, "]".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn brace_interpolation_opens_nested_scripting() {
    assert_eq!(
        texts(r#"<?php "x {$a->b(1)} y""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "x ".to_owned()),
            (OpenBrace, "{".to_owned()),
            (Variable, "$a".to_owned()),
            (Arrow, "->".to_owned()),
            (Identifier, "b".to_owned()),
            (OpenParenthesis, "(".to_owned()),
            (IntegerLiteral, "1".to_owned()),
            (CloseParenthesis, ")".to_owned()),
            (CloseBrace, "}".to_owned()),
            (StringFragment, " y".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn nested_braces_inside_brace_interpolation_balance() {
    assert_eq!(
        texts(r#"<?php "{$f(['k' => 1])}""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (OpenBrace, "{".to_owned()),
            (Variable, "$f".to_owned()),
            (OpenParenthesis, "(".to_owned()),
            (OpenBracket, "[".to_owned()),
            (SingleQuotedString, "'k'".to_owned()),
            (Whitespace, " ".to_owned()),
            (FatArrow, "=>".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "1".to_owned()),
            (CloseBracket, "]".to_owned()),
            (CloseParenthesis, ")".to_owned()),
            (CloseBrace, "}".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn deprecated_dollar_brace_interpolation_still_lexes() {
    assert_eq!(
        texts(r#"<?php "${name}""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (DollarOpenBrace, "${".to_owned()),
            (Identifier, "name".to_owned()),
            (CloseBrace, "}".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn a_lone_dollar_or_brace_stays_in_the_fragment() {
    assert_eq!(
        texts(r#"<?php "a $ b { c""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "a $ b { c".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn backtick_strings_interpolate_like_double_quotes() {
    assert_eq!(
        texts("<?php `ls $dir`"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (Backtick, "`".to_owned()),
            (StringFragment, "ls ".to_owned()),
            (Variable, "$dir".to_owned()),
            (Backtick, "`".to_owned()),
        ]
    );
}

#[test]
fn binary_prefix_on_double_quotes() {
    assert_eq!(
        texts(r#"<?php b"x""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "b\"".to_owned()),
            (StringFragment, "x".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn unterminated_double_quoted_string_diagnoses_the_opening() {
    let (tokens, diagnostics) = lex_verified(r#"<?php "open $x"#);
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, DoubleQuote, StringFragment, Variable]
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnterminatedString);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
}

#[test]
fn unterminated_brace_interpolation_diagnoses_the_opening() {
    let (_tokens, diagnostics) = lex_verified(r#"<?php "a {$x"#);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == LexerDiagnosticKind::UnterminatedInterpolation
            && u32::from(diagnostic.range.start()) == 9
    }));
    // The string opening is reported too.
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.kind == LexerDiagnosticKind::UnterminatedString })
    );
}
