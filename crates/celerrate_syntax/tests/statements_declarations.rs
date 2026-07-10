#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn declare_takes_directives_and_a_body() {
    insta::assert_snapshot!(render_statement("declare(strict_types=1);"), @r#"
    DeclareStatement
      Declare "declare"
      OpenParenthesis "("
      DeclareDirective
        Identifier "strict_types"
        Equals "="
        Literal
          IntegerLiteral "1"
      CloseParenthesis ")"
      EmptyStatement
        Semicolon ";"
    "#);
}

#[test]
fn declare_accepts_every_body_form() {
    assert!(parser_diagnostics("<?php declare(ticks=1) { echo 1; }").is_empty());
    assert!(parser_diagnostics("<?php declare(ticks=1): echo 1; enddeclare;").is_empty());
    assert!(parser_diagnostics("<?php declare(encoding='UTF-8', ticks=1);").is_empty());
}

#[test]
fn a_directive_without_a_value_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php declare(strict_types);")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Equals))
    );
}

#[test]
fn a_named_function_declares() {
    insta::assert_snapshot!(render_statement("function add(int $a, int $b = 0) { return $a + $b; }"), @r#"
    FunctionDeclaration
      Function "function"
      Identifier "add"
      ParameterList
        OpenParenthesis "("
        Parameter
          TypeReference
            Identifier "int"
          Variable "$a"
        Comma ","
        Parameter
          TypeReference
            Identifier "int"
          Variable "$b"
          Equals "="
          Literal
            IntegerLiteral "0"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        ReturnStatement
          Return "return"
          BinaryExpression
            VariableReference
              Variable "$a"
            Plus "+"
            VariableReference
              Variable "$b"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn by_reference_returns_and_return_types_parse() {
    insta::assert_snapshot!(render_statement("function &all(): ?iterable { }"), @r#"
    FunctionDeclaration
      Function "function"
      Ampersand "&"
      Identifier "all"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      TypeReference
        Question "?"
        Identifier "iterable"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn an_anonymous_function_stays_a_closure_expression() {
    insta::assert_snapshot!(render_statement("function () { };"), @r#"
    ExpressionStatement
      ClosureExpression
        Function "function"
        ParameterList
          OpenParenthesis "("
          CloseParenthesis ")"
        Block
          OpenBrace "{"
          CloseBrace "}"
      Semicolon ";"
    "#);
}

#[test]
fn a_by_reference_closure_also_stays_an_expression() {
    assert!(parser_diagnostics("<?php $f = function &() { return $x; };").is_empty());
}
