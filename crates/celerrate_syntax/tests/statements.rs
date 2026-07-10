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

#[test]
fn global_lists_variables() {
    insta::assert_snapshot!(render_statement("global $configuration, $$indirect;"), @r#"
    GlobalStatement
      Global "global"
      VariableReference
        Variable "$configuration"
      Comma ","
      DynamicVariableExpression
        Dollar "$"
        VariableReference
          Variable "$indirect"
      Semicolon ";"
    "#);
}

#[test]
fn global_without_a_variable_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php global;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Variable))
    );
}

#[test]
fn static_variables_declare_with_optional_initializers() {
    insta::assert_snapshot!(render_statement("static $count = 0, $names;"), @r#"
    StaticStatement
      Static "static"
      StaticVariable
        Variable "$count"
        Equals "="
        Literal
          IntegerLiteral "0"
      Comma ","
      StaticVariable
        Variable "$names"
      Semicolon ";"
    "#);
}

#[test]
fn static_scoped_access_stays_an_expression_statement() {
    insta::assert_snapshot!(render_statement("static::create();"), @r#"
    ExpressionStatement
      CallExpression
        ScopedAccessExpression
          NameExpression
            Static "static"
          ColonColon "::"
          MemberName
            Identifier "create"
        ArgumentList
          OpenParenthesis "("
          CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn a_static_closure_stays_an_expression_statement() {
    assert!(parser_diagnostics("<?php static fn () => 1;").is_empty());
}

#[test]
fn unset_takes_a_parenthesized_list() {
    insta::assert_snapshot!(render_statement("unset($map['key'], $other);"), @r#"
    UnsetStatement
      Unset "unset"
      ArgumentList
        OpenParenthesis "("
        Argument
          IndexExpression
            VariableReference
              Variable "$map"
            OpenBracket "["
            Literal
              SingleQuotedString "'key'"
            CloseBracket "]"
        Comma ","
        Argument
          VariableReference
            Variable "$other"
        CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn unset_without_parentheses_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php unset;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis))
    );
}

#[test]
fn goto_names_a_label() {
    insta::assert_snapshot!(render_statement("goto cleanup;"), @r#"
    GotoStatement
      Goto "goto"
      Identifier "cleanup"
      Semicolon ";"
    "#);
}

#[test]
fn an_identifier_followed_by_a_colon_is_a_label() {
    insta::assert_snapshot!(render_statement("cleanup: echo 1;"), @r#"
    LabelStatement
      Identifier "cleanup"
      Colon ":"
    "#);
}

#[test]
fn a_call_statement_is_not_mistaken_for_a_label() {
    assert!(parser_diagnostics("<?php cleanup();").is_empty());
}

#[test]
fn goto_without_a_label_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php goto ;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Identifier))
    );
}
