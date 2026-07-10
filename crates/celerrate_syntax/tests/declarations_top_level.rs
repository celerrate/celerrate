#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn a_constant_declaration_lists_its_elements() {
    insta::assert_snapshot!(render_statement("const GREETING = 'hello', ANSWER = 42;"), @r#"
    ConstantDeclaration
      Const "const"
      ConstantElement
        Identifier "GREETING"
        Equals "="
        Literal
          SingleQuotedString "'hello'"
      Comma ","
      ConstantElement
        Identifier "ANSWER"
        Equals "="
        Literal
          IntegerLiteral "42"
      Semicolon ";"
    "#);
}

#[test]
fn a_typed_constant_takes_a_type_before_its_name() {
    insta::assert_snapshot!(render_statement("const int LIMIT = 10;"), @r#"
    ConstantDeclaration
      Const "const"
      NamedType
        Name
          Identifier "int"
      ConstantElement
        Identifier "LIMIT"
        Equals "="
        Literal
          IntegerLiteral "10"
      Semicolon ";"
    "#);
}

#[test]
fn a_semi_reserved_constant_name_parses_clean() {
    // `const FOR = 1;`: keyword names are accepted wholesale; which
    // positions allow them is semantic.
    assert_eq!(parser_diagnostics("<?php const FOR = 1;"), vec![]);
}

#[test]
fn a_missing_constant_value_is_diagnosed_and_the_statement_recovers() {
    assert_eq!(
        parser_diagnostics("<?php const A = ; echo 1;"),
        vec![ParserDiagnosticKind::ExpectedExpression]
    );
}

#[test]
fn a_namespace_declaration_takes_a_name_and_a_terminator() {
    insta::assert_snapshot!(render_statement("namespace App\\Domain;"), @r#"
    NamespaceDeclaration
      Namespace "namespace"
      Name
        Identifier "App"
        Backslash "\\"
        Identifier "Domain"
      Semicolon ";"
    "#);
}

#[test]
fn a_braced_namespace_wraps_its_statements_in_a_block() {
    insta::assert_snapshot!(render_statement("namespace App { echo 1; }"), @r#"
    NamespaceDeclaration
      Namespace "namespace"
      Name
        Identifier "App"
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
fn a_global_namespace_block_has_no_name() {
    assert_eq!(parser_diagnostics("<?php namespace { echo 1; }"), vec![]);
}

#[test]
fn namespace_backslash_stays_a_name_expression() {
    // `namespace\helper()` is the relative-name call, not a namespace
    // declaration; the dispatcher separates the two on one lookahead.
    insta::assert_snapshot!(render_statement("namespace\\helper();"), @r#"
    ExpressionStatement
      CallExpression
        NameExpression
          Name
            Namespace "namespace"
            Backslash "\\"
            Identifier "helper"
        ArgumentList
          OpenParenthesis "("
          CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn function_declarations_still_parse_after_the_move() {
    assert_eq!(
        parser_diagnostics("<?php function f(int $x): void { return; }"),
        vec![]
    );
}
