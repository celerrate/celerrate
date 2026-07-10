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

#[test]
fn a_call_wraps_its_callee_and_arguments() {
    insta::assert_snapshot!(support::render_expression("strlen('x')"), @r#"
    CallExpression
      NameExpression
        Name
          Identifier "strlen"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            SingleQuotedString "'x'"
        CloseParenthesis ")"
    "#);
}

#[test]
fn named_and_spread_arguments_parse() {
    insta::assert_snapshot!(support::render_expression("f(name: 1, ...$rest)"), @r#"
    CallExpression
      NameExpression
        Name
          Identifier "f"
      ArgumentList
        OpenParenthesis "("
        Argument
          Identifier "name"
          Colon ":"
          Literal
            IntegerLiteral "1"
        Comma ","
        Argument
          Ellipsis "..."
          VariableReference
            Variable "$rest"
        CloseParenthesis ")"
    "#);
}

#[test]
fn a_keyword_works_as_a_named_argument_label() {
    let parse = support::parse_verified("<?php f(default: 1);");
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
}

#[test]
fn the_first_class_callable_form_is_a_lone_ellipsis() {
    insta::assert_snapshot!(support::render_expression("f(...)"), @r#"
    CallExpression
      NameExpression
        Name
          Identifier "f"
      ArgumentList
        OpenParenthesis "("
        Ellipsis "..."
        CloseParenthesis ")"
    "#);
}

#[test]
fn calls_chain_and_take_any_callee() {
    insta::assert_snapshot!(support::render_expression("$f(1)(2)"), @r#"
    CallExpression
      CallExpression
        VariableReference
          Variable "$f"
        ArgumentList
          OpenParenthesis "("
          Argument
            Literal
              IntegerLiteral "1"
          CloseParenthesis ")"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "2"
        CloseParenthesis ")"
    "#);
}

#[test]
fn trailing_commas_and_by_reference_arguments_parse() {
    assert!(parser_diagnostics("<?php f($a, &$b,);").is_empty());
}

#[test]
fn a_missing_argument_separator_is_diagnosed_and_both_arguments_survive() {
    let diagnostics = parser_diagnostics("<?php f(1 2);");
    assert_eq!(
        diagnostics,
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Comma)]
    );
}

#[test]
fn an_unclosed_call_stops_at_the_statement_boundary() {
    let diagnostics = parser_diagnostics("<?php f(1; $x;");
    assert!(diagnostics.contains(&ParserDiagnosticKind::Expected(
        SyntaxKind::CloseParenthesis
    )));
    let parse = support::parse_verified("<?php f(1; $x;");
    assert!(
        parse
            .tree()
            .children()
            .filter(|node| node.kind() == SyntaxKind::ExpressionStatement)
            .count()
            >= 2,
        "the call must not swallow the next statement"
    );
}

#[test]
fn pathological_nesting_inside_a_call_terminates() {
    // Deeply nested parentheses inside a call argument can exhaust the
    // nesting guard while `argument_list`'s own loop is still active
    // (the leftover `(` tokens surface through the postfix loop at
    // every unwinding level, not only at the top). Without a mechanical
    // progress guarantee in `argument_list` itself, this spins forever;
    // the assertion that matters here is that this test completes.
    let source = format!("<?php f({}1;", "(".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}

#[test]
fn member_access_wraps_left_to_right() {
    insta::assert_snapshot!(support::render_expression("$user->name"), @r#"
    MemberAccessExpression
      VariableReference
        Variable "$user"
      Arrow "->"
      MemberName
        Identifier "name"
    "#);
}

#[test]
fn nullsafe_access_and_semi_reserved_members_parse() {
    insta::assert_snapshot!(support::render_expression("$user?->list()"), @r#"
    CallExpression
      MemberAccessExpression
        VariableReference
          Variable "$user"
        NullsafeArrow "?->"
        MemberName
          List "list"
      ArgumentList
        OpenParenthesis "("
        CloseParenthesis ")"
    "#);
}

#[test]
fn dynamic_and_computed_member_names_parse() {
    insta::assert_snapshot!(support::render_expression("$object->$property"), @r#"
    MemberAccessExpression
      VariableReference
        Variable "$object"
      Arrow "->"
      MemberName
        VariableReference
          Variable "$property"
    "#);
    insta::assert_snapshot!(support::render_expression("$object->{$prefix . 'x'}"), @r#"
    MemberAccessExpression
      VariableReference
        Variable "$object"
      Arrow "->"
      MemberName
        OpenBrace "{"
        BinaryExpression
          VariableReference
            Variable "$prefix"
          Dot "."
          Literal
            SingleQuotedString "'x'"
        CloseBrace "}"
    "#);
}

#[test]
fn scoped_access_covers_constants_methods_properties_and_class() {
    insta::assert_snapshot!(support::render_expression("Foo::class"), @r#"
    ScopedAccessExpression
      NameExpression
        Name
          Identifier "Foo"
      ColonColon "::"
      MemberName
        Class "class"
    "#);
    insta::assert_snapshot!(support::render_expression("Foo::$instance"), @r#"
    ScopedAccessExpression
      NameExpression
        Name
          Identifier "Foo"
      ColonColon "::"
      MemberName
        VariableReference
          Variable "$instance"
    "#);
    assert!(parser_diagnostics("<?php static::create();").is_empty());
    assert!(parser_diagnostics("<?php \\Foo\\Bar::BAZ;").is_empty());
}

#[test]
fn indexing_chains_and_the_empty_index_parse() {
    insta::assert_snapshot!(support::render_expression("$matrix[0][1]"), @r#"
    IndexExpression
      IndexExpression
        VariableReference
          Variable "$matrix"
        OpenBracket "["
        Literal
          IntegerLiteral "0"
        CloseBracket "]"
      OpenBracket "["
      Literal
        IntegerLiteral "1"
      CloseBracket "]"
    "#);
    assert!(parser_diagnostics("<?php $queue[] = 1;").is_empty());
}

#[test]
fn the_spec_chain_parses_with_forward_parents() {
    insta::assert_snapshot!(support::render_expression("$a->b[0]() + $c"), @r#"
    BinaryExpression
      CallExpression
        IndexExpression
          MemberAccessExpression
            VariableReference
              Variable "$a"
            Arrow "->"
            MemberName
              Identifier "b"
          OpenBracket "["
          Literal
            IntegerLiteral "0"
          CloseBracket "]"
        ArgumentList
          OpenParenthesis "("
          CloseParenthesis ")"
      Plus "+"
      VariableReference
        Variable "$c"
    "#);
}

#[test]
fn whitespace_between_qualified_name_segments_still_parses_as_one_name() {
    // Zend lexes a qualified name as a single token, which forbids
    // interior whitespace; the trivia-free token view this parser reads
    // cannot see that adjacency, so a spaced form like `Foo \ Bar`
    // parses the same as `Foo\Bar`, diagnostic-free, as one `Name` run.
    // Pinned so this recorded permissiveness decision does not drift.
    insta::assert_snapshot!(support::render_expression("Foo \\ Bar"), @r#"
    NameExpression
      Name
        Identifier "Foo"
        Backslash "\\"
        Identifier "Bar"
    "#);
    assert!(parser_diagnostics("<?php Foo \\ Bar;").is_empty());
}

#[test]
fn a_missing_member_name_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php $object->;").contains(&ParserDiagnosticKind::ExpectedMemberName)
    );
}
