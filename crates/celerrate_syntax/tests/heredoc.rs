#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{lex_verified, texts};

#[test]
fn a_basic_heredoc() {
    assert_eq!(
        texts("<?php <<<EOT\nhello\nEOT;"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "hello\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
            (Semicolon, ";".to_owned()),
        ]
    );
}

#[test]
fn a_double_quoted_label_is_a_heredoc() {
    assert_eq!(
        texts("<?php <<<\"EOT\"\nx\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<\"EOT\"\n".to_owned()),
            (StringFragment, "x\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn heredocs_interpolate() {
    assert_eq!(
        texts("<?php <<<EOT\na $name b {$x}\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "a ".to_owned()),
            (Variable, "$name".to_owned()),
            (StringFragment, " b ".to_owned()),
            (OpenBrace, "{".to_owned()),
            (Variable, "$x".to_owned()),
            (CloseBrace, "}".to_owned()),
            (StringFragment, "\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn flexible_indentation_belongs_to_the_end_token() {
    assert_eq!(
        texts("<?php <<<EOT\n    body\n    EOT;"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "    body\n".to_owned()),
            (HeredocEnd, "    EOT".to_owned()),
            (Semicolon, ";".to_owned()),
        ]
    );
}

#[test]
fn a_label_prefix_inside_the_body_does_not_close() {
    // "EOTX" starts with the label but continues with a name character,
    // so the heredoc stays open until the bare "EOT" line.
    assert_eq!(
        texts("<?php <<<EOT\nEOTX\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "EOTX\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn nowdocs_do_not_interpolate() {
    assert_eq!(
        texts("<?php <<<'EOT'\na $name {$x}\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<'EOT'\n".to_owned()),
            (StringFragment, "a $name {$x}\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn spaces_are_allowed_between_the_arrows_and_the_label() {
    assert_eq!(
        texts("<?php <<<  EOT\nx\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<  EOT\n".to_owned()),
            (StringFragment, "x\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn triple_less_without_a_label_is_shifts_not_heredoc() {
    assert_eq!(
        texts("<?php 1 <<< 2").last(),
        Some(&(IntegerLiteral, "2".to_owned()))
    );
    let (_tokens, diagnostics) = lex_verified("<?php 1 <<< 2");
    assert!(diagnostics.is_empty());
}

#[test]
fn an_unterminated_heredoc_diagnoses_the_start() {
    let (tokens, diagnostics) = lex_verified("<?php <<<EOT\nbody");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, HeredocStart, StringFragment]
    );
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnterminatedHeredoc);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
    assert_eq!(u32::from(diagnostic.range.end()), 13);
}

#[test]
fn an_empty_heredoc_closes_immediately() {
    assert_eq!(
        texts("<?php <<<EOT\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}
