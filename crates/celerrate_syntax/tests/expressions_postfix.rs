//! Names, calls, member access, scoped access, and indexing.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxDiagnosticKind, SyntaxKind};

fn parser_diagnostics(source: &str) -> Vec<ParserDiagnosticKind> {
    support::parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}

#[test]
fn a_bare_identifier_is_a_name_expression() {
    insta::assert_snapshot!(support::render_expression("PHP_EOL"), @r#"
    NameExpression
      Name
        Identifier "PHP_EOL"
    "#);
}

#[test]
fn qualified_and_fully_qualified_names_parse() {
    insta::assert_snapshot!(support::render_expression("\\Foo\\Bar"), @r#"
    NameExpression
      Name
        Backslash "\\"
        Identifier "Foo"
        Backslash "\\"
        Identifier "Bar"
    "#);
    insta::assert_snapshot!(support::render_expression("namespace\\Foo"), @r#"
    NameExpression
      Name
        Namespace "namespace"
        Backslash "\\"
        Identifier "Foo"
    "#);
}

#[test]
fn true_false_null_are_plain_names() {
    // The design routes them through semantic resolution, not the lexer.
    insta::assert_snapshot!(support::render_expression("true"), @r#"
    NameExpression
      Name
        Identifier "true"
    "#);
}

#[test]
fn instanceof_accepts_a_name_on_the_right() {
    insta::assert_snapshot!(support::render_expression("$a instanceof Foo\\Bar"), @r#"
    BinaryExpression
      VariableReference
        Variable "$a"
      InstanceOf "instanceof"
      NameExpression
        Name
          Identifier "Foo"
          Backslash "\\"
          Identifier "Bar"
    "#);
}

#[test]
fn dynamic_variables_parse_recursively() {
    insta::assert_snapshot!(support::render_expression("$$x"), @r#"
    DynamicVariableExpression
      Dollar "$"
      VariableReference
        Variable "$x"
    "#);
    insta::assert_snapshot!(support::render_expression("${'a' . 'b'}"), @r#"
    DynamicVariableExpression
      Dollar "$"
      OpenBrace "{"
      BinaryExpression
        Literal
          SingleQuotedString "'a'"
        Dot "."
        Literal
          SingleQuotedString "'b'"
      CloseBrace "}"
    "#);
}

#[test]
fn a_lone_dollar_is_diagnosed_but_wrapped() {
    assert!(parser_diagnostics("<?php $ + 1;").contains(&ParserDiagnosticKind::ExpectedExpression));
}

#[test]
fn pathological_dollar_chains_trip_the_guard_without_panicking() {
    let source = format!("<?php {}x;", "$".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}

#[test]
fn a_refused_expression_start_still_advances() {
    // `namespace` not followed by `\` is a declaration keyword this
    // plan does not parse; the statement loop must not livelock on it.
    let parse = support::parse_verified("<?php namespace Foo; $x;");
    assert!(!parse.diagnostics().is_empty());
    assert!(
        parse
            .tree()
            .children()
            .any(|node| node.kind() == SyntaxKind::ExpressionStatement)
    );
}
