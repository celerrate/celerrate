#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn an_empty_statement_is_a_node() {
    insta::assert_snapshot!(render_statement(";"), @r#"
    EmptyStatement
      Semicolon ";"
    "#);
}

#[test]
fn a_brace_block_is_a_statement() {
    insta::assert_snapshot!(render_statement("{ echo 1; }"), @r#"
    Block
      OpenBrace "{"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      CloseBrace "}"
    "#);
}

#[test]
fn an_unclosed_block_is_diagnosed_and_completes() {
    assert!(
        parser_diagnostics("<?php { echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace))
    );
}

#[test]
fn pathological_block_nesting_trips_the_guard_without_overflowing() {
    // 300 nested blocks: the statement guard must refuse past the
    // budget and keep consuming, never overflow the stack.
    let source = format!("<?php {}", "{".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        celerrate_syntax::SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}

#[test]
fn a_bare_return_terminates() {
    insta::assert_snapshot!(render_statement("return;"), @r#"
    ReturnStatement
      Return "return"
      Semicolon ";"
    "#);
}

#[test]
fn return_carries_an_optional_expression() {
    insta::assert_snapshot!(render_statement("return 1 + 2;"), @r#"
    ReturnStatement
      Return "return"
      BinaryExpression
        Literal
          IntegerLiteral "1"
        Plus "+"
        Literal
          IntegerLiteral "2"
      Semicolon ";"
    "#);
}

#[test]
fn break_and_continue_carry_an_optional_level() {
    insta::assert_snapshot!(render_statement("break 2;"), @r#"
    BreakStatement
      Break "break"
      Literal
        IntegerLiteral "2"
      Semicolon ";"
    "#);
    insta::assert_snapshot!(render_statement("continue;"), @r#"
    ContinueStatement
      Continue "continue"
      Semicolon ";"
    "#);
}

#[test]
fn a_missing_statement_terminator_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php return 1").contains(&ParserDiagnosticKind::ExpectedSemicolon)
    );
}
