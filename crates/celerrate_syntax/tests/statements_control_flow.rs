#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn a_classic_if_wraps_condition_and_body() {
    insta::assert_snapshot!(render_statement("if ($x) echo 1;"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
    "#);
}

#[test]
fn elseif_and_else_are_clauses() {
    insta::assert_snapshot!(render_statement("if ($a) { } elseif ($b) { } else { }"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$a"
      CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
      ElseIfClause
        ElseIf "elseif"
        OpenParenthesis "("
        VariableReference
          Variable "$b"
        CloseParenthesis ")"
        Block
          OpenBrace "{"
          CloseBrace "}"
      ElseClause
        Else "else"
        Block
          OpenBrace "{"
          CloseBrace "}"
    "#);
}

#[test]
fn else_if_with_a_space_nests_an_if_inside_the_else() {
    insta::assert_snapshot!(render_statement("if ($a) echo 1; else if ($b) echo 2;"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$a"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      ElseClause
        Else "else"
        IfStatement
          If "if"
          OpenParenthesis "("
          VariableReference
            Variable "$b"
          CloseParenthesis ")"
          EchoStatement
            Echo "echo"
            Literal
              IntegerLiteral "2"
            Semicolon ";"
    "#);
}

#[test]
fn a_dangling_else_binds_to_the_innermost_if() {
    let rendered = render_statement("if ($a) if ($b) echo 1; else echo 2;");
    // The outer if has no ElseClause child of its own: the else sits
    // inside the inner IfStatement.
    let inner_holds_else = rendered
        .lines()
        .any(|line| line.trim() == "ElseClause" && line.starts_with("    "));
    assert!(
        inner_holds_else,
        "the else must nest inside the inner if:\n{rendered}"
    );
}

#[test]
fn a_missing_condition_parenthesis_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php if $x echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis))
    );
}

#[test]
fn a_missing_body_is_diagnosed_without_consuming() {
    assert!(parser_diagnostics("<?php if ($x)").contains(&ParserDiagnosticKind::ExpectedStatement));
}
