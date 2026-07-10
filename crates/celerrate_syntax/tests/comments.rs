mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{kinds, lex_verified, texts};

#[test]
fn line_comments_stop_before_the_newline() {
    assert_eq!(
        texts("<?php // hello\n# world"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (LineComment, "// hello".to_owned()),
            (Whitespace, "\n".to_owned()),
            (LineComment, "# world".to_owned()),
        ]
    );
}

#[test]
fn line_comments_stop_before_a_close_tag() {
    assert_eq!(
        texts("<?php // note ?>x"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (LineComment, "// note ".to_owned()),
            (CloseTag, "?>".to_owned()),
            (InlineHtml, "x".to_owned()),
        ]
    );
}

#[test]
fn block_comments_span_lines() {
    assert_eq!(
        kinds("<?php /* a\nb */ ;"),
        [OpenTag, Whitespace, BlockComment, Whitespace, Semicolon]
    );
}

#[test]
fn docblocks_are_distinct_from_block_comments() {
    assert_eq!(
        kinds("<?php /** @param int $x */"),
        [OpenTag, Whitespace, DocComment]
    );
    // "/**/" is an empty block comment, and "/***/" has no whitespace
    // after the doc opener: both stay plain block comments, as in Zend.
    assert_eq!(kinds("<?php /**/"), [OpenTag, Whitespace, BlockComment]);
    assert_eq!(kinds("<?php /***/"), [OpenTag, Whitespace, BlockComment]);
}

#[test]
fn unterminated_block_comment_runs_to_the_end() {
    let (tokens, diagnostics) = lex_verified("<?php /* open");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, BlockComment]
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics.first().map(|d| d.kind),
        Some(LexerDiagnosticKind::UnterminatedBlockComment)
    );
    // The diagnostic points at the opening "/*".
    assert_eq!(
        diagnostics
            .first()
            .map(|d| (u32::from(d.range.start()), u32::from(d.range.end()))),
        Some((6, 8))
    );
}

#[test]
fn attribute_opener_is_not_a_comment() {
    assert_eq!(
        kinds("<?php #[Attribute] # comment"),
        [
            OpenTag,
            Whitespace,
            AttributeOpen,
            Identifier,
            CloseBracket,
            Whitespace,
            LineComment
        ]
    );
}

#[test]
fn a_form_feed_after_the_doc_opener_is_a_plain_block_comment() {
    // Zend's docblock rule requires real whitespace after `/**`; a form
    // feed is not PHP whitespace, so this stays an ordinary comment.
    assert_eq!(
        kinds("<?php /**\u{C} x */"),
        [OpenTag, Whitespace, BlockComment]
    );
}
