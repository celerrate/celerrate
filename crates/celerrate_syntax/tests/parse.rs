//! Behavior tests for the parsing pipeline: tree shapes, terminator
//! rules, recovery, and diagnostic merging. Every parse goes through
//! the lossless assertion in `support::parse_verified`.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{SyntaxDiagnosticKind, SyntaxKind};

#[test]
fn empty_input_yields_an_empty_source_file() {
    let parse = support::parse_verified("");
    assert_eq!(parse.tree().kind(), SyntaxKind::SourceFile);
    assert_eq!(parse.tree().children_with_tokens().count(), 0);
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn inline_html_and_tags_are_token_children_of_the_source_file() {
    let parse = support::parse_verified("<p>a</p><?php ?><p>b</p>");
    let kinds: Vec<SyntaxKind> = parse
        .tree()
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .map(|token| token.kind())
        .collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::InlineHtml,
            SyntaxKind::OpenTag,
            SyntaxKind::Whitespace,
            SyntaxKind::CloseTag,
            SyntaxKind::InlineHtml,
        ],
    );
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn echo_wraps_its_comma_separated_expressions() {
    let parse = support::parse_verified("<?php echo 'a', 1, $x;");
    let statement = parse
        .tree()
        .children()
        .find(|node| node.kind() == SyntaxKind::EchoStatement)
        .expect("an echo statement");
    let expression_kinds: Vec<SyntaxKind> = statement.children().map(|node| node.kind()).collect();
    assert_eq!(
        expression_kinds,
        vec![
            SyntaxKind::Literal,
            SyntaxKind::Literal,
            SyntaxKind::VariableReference,
        ],
    );
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn an_expression_statement_holds_its_expression_and_semicolon() {
    let parse = support::parse_verified("<?php $x;");
    let statement = parse
        .tree()
        .children()
        .find(|node| node.kind() == SyntaxKind::ExpressionStatement)
        .expect("an expression statement");
    assert_eq!(
        statement.children().next().map(|node| node.kind()),
        Some(SyntaxKind::VariableReference),
    );
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn a_close_tag_terminates_a_statement_without_a_semicolon() {
    let parse = support::parse_verified("<?php echo 1 ?>");
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
}

#[test]
fn a_missing_semicolon_is_diagnosed_and_the_statement_completes() {
    // Zend rejects a missing `;` at end of input too; we diagnose it
    // but still deliver both complete statements.
    let parse = support::parse_verified("<?php echo 1 echo 2;");
    assert_eq!(
        parse
            .tree()
            .children()
            .filter(|node| node.kind() == SyntaxKind::EchoStatement)
            .count(),
        2,
    );
    assert_eq!(parse.diagnostics().len(), 1);
}

#[test]
fn an_unexpected_token_becomes_an_error_node_and_parsing_continues() {
    let parse = support::parse_verified("<?php ) echo 1;");
    let kinds: Vec<SyntaxKind> = parse.tree().children().map(|node| node.kind()).collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::ErrorNode, SyntaxKind::EchoStatement]
    );
    assert_eq!(parse.diagnostics().len(), 1);
}

#[test]
fn echo_without_an_expression_is_diagnosed() {
    let parse = support::parse_verified("<?php echo ;");
    assert_eq!(parse.diagnostics().len(), 1);
    assert!(
        parse
            .tree()
            .children()
            .any(|node| node.kind() == SyntaxKind::EchoStatement),
    );
}

#[test]
fn lexer_and_parser_diagnostics_merge_in_source_order() {
    // Unterminated string: a lexer diagnostic at the opening quote,
    // then the parser's missing terminator at end of input.
    let parse = support::parse_verified("<?php echo 'open");
    let kinds: Vec<&SyntaxDiagnosticKind> = parse
        .diagnostics()
        .iter()
        .map(|diagnostic| &diagnostic.kind)
        .collect();
    assert_eq!(kinds.len(), 2);
    assert!(matches!(
        kinds.first(),
        Some(SyntaxDiagnosticKind::Lexer(_))
    ));
    assert!(matches!(
        kinds.get(1),
        Some(SyntaxDiagnosticKind::Parser(_))
    ));
}

#[test]
#[allow(clippy::indexing_slicing)]
fn exhausted_nesting_reports_each_finding_once() {
    // Unwinding out of an exhausted nesting budget used to emit one
    // identical Expected(CloseParenthesis) per unwound level, all
    // zero-width at the same offset: 127 adjacent duplicates on this
    // input.
    let source = format!("<?php {}1;", "(".repeat(140));
    let parse = support::parse_verified(&source);
    assert!(
        parse
            .diagnostics()
            .windows(2)
            .all(|pair| pair[0] != pair[1]),
        "no diagnostic may repeat its immediate predecessor"
    );
}
