#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxKind};
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

#[test]
fn a_use_import_with_an_alias() {
    insta::assert_snapshot!(render_statement("use App\\Service as Servicing;"), @r#"
    UseDeclaration
      Use "use"
      UseClause
        Name
          Identifier "App"
          Backslash "\\"
          Identifier "Service"
        As "as"
        Identifier "Servicing"
      Semicolon ";"
    "#);
}

#[test]
fn a_function_import_types_the_whole_clause_list() {
    insta::assert_snapshot!(render_statement("use function strlen, strrev;"), @r#"
    UseDeclaration
      Use "use"
      Function "function"
      UseClause
        Name
          Identifier "strlen"
      Comma ","
      UseClause
        Name
          Identifier "strrev"
      Semicolon ";"
    "#);
}

#[test]
fn a_group_import_nests_typed_and_aliased_items() {
    insta::assert_snapshot!(render_statement("use App\\{Service, function helper as aid, const LIMIT};"), @r#"
    UseDeclaration
      Use "use"
      UseClause
        Name
          Identifier "App"
        UseGroup
          Backslash "\\"
          OpenBrace "{"
          UseClause
            Name
              Identifier "Service"
          Comma ","
          UseClause
            Function "function"
            Name
              Identifier "helper"
            As "as"
            Identifier "aid"
          Comma ","
          UseClause
            Const "const"
            Name
              Identifier "LIMIT"
          CloseBrace "}"
      Semicolon ";"
    "#);
}

#[test]
fn a_trailing_comma_inside_a_group_parses_clean() {
    // Zend allows the trailing comma in group imports.
    assert_eq!(parser_diagnostics("<?php use App\\{Service,};"), vec![]);
}

#[test]
fn an_unclosed_group_recovers_at_the_semicolon() {
    assert_eq!(
        parser_diagnostics("<?php use App\\{Service; echo 1;"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace)]
    );
}

#[test]
fn a_use_without_a_name_is_diagnosed_and_recovers() {
    assert_eq!(
        parser_diagnostics("<?php use ; echo 1;"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Identifier)]
    );
}
