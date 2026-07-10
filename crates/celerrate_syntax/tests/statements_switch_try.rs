#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxKind};
use support::{parser_diagnostics, render_statement};

#[test]
fn switch_holds_cases_and_a_default() {
    insta::assert_snapshot!(
        render_statement("switch ($x) { case 1: echo 1; break; default: echo 0; }"),
        @r#"
    SwitchStatement
      Switch "switch"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      OpenBrace "{"
      SwitchCase
        Case "case"
        Literal
          IntegerLiteral "1"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "1"
          Semicolon ";"
        BreakStatement
          Break "break"
          Semicolon ";"
      SwitchCase
        Default "default"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "0"
          Semicolon ";"
      CloseBrace "}"
    "#);
}

#[test]
fn cases_fall_through_and_accept_semicolon_terminators() {
    // `case 1;` is Zend-legal; an empty case body falls through.
    assert!(parser_diagnostics("<?php switch ($x) { case 1: case 2; echo 1; }").is_empty());
}

#[test]
fn switch_tolerates_one_leading_semicolon() {
    assert!(parser_diagnostics("<?php switch ($x) { ; case 1: echo 1; }").is_empty());
}

#[test]
fn an_alternative_switch_closes_with_endswitch() {
    assert!(parser_diagnostics("<?php switch ($x): case 1: echo 1; endswitch;").is_empty());
}

#[test]
fn junk_between_cases_is_wrapped_and_consumed() {
    let diagnostics = parser_diagnostics("<?php switch ($x) { junk case 1: echo 1; }");
    assert!(diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken));
}

#[test]
fn try_catch_finally_holds_clauses() {
    insta::assert_snapshot!(
        render_statement("try { } catch (LogicException | \\RuntimeException $error) { } finally { }"),
        @r#"
    TryStatement
      Try "try"
      Block
        OpenBrace "{"
        CloseBrace "}"
      CatchClause
        Catch "catch"
        OpenParenthesis "("
        Name
          Identifier "LogicException"
        Pipe "|"
        Name
          Backslash "\\"
          Identifier "RuntimeException"
        VariableReference
          Variable "$error"
        CloseParenthesis ")"
        Block
          OpenBrace "{"
          CloseBrace "}"
      FinallyClause
        Finally "finally"
        Block
          OpenBrace "{"
          CloseBrace "}"
    "#);
}

#[test]
fn catch_variables_are_optional_since_php_8() {
    assert!(parser_diagnostics("<?php try { } catch (Throwable) { }").is_empty());
}

#[test]
fn finally_alone_satisfies_a_try() {
    assert!(parser_diagnostics("<?php try { } finally { }").is_empty());
}

#[test]
fn a_bare_try_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php try { }")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Catch))
    );
}
