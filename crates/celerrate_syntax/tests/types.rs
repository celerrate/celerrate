#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_statement, render_type};

#[test]
fn a_named_type_wraps_one_name() {
    insta::assert_snapshot!(render_type("int"), @r#"
    NamedType
      Name
        Identifier "int"
    "#);
}

#[test]
fn a_qualified_type_is_one_name_node() {
    insta::assert_snapshot!(render_type("\\App\\Value"), @r#"
    NamedType
      Name
        Backslash "\\"
        Identifier "App"
        Backslash "\\"
        Identifier "Value"
    "#);
}

#[test]
fn keyword_types_stay_bare_tokens() {
    insta::assert_snapshot!(render_type("static"), @r#"
    NamedType
      Static "static"
    "#);
}

#[test]
fn a_nullable_type_prefixes_its_question_mark() {
    insta::assert_snapshot!(render_type("?Foo"), @r#"
    NullableType
      Question "?"
      NamedType
        Name
          Identifier "Foo"
    "#);
}

#[test]
fn union_types_are_one_flat_node() {
    insta::assert_snapshot!(render_type("int|string|null"), @r#"
    UnionType
      NamedType
        Name
          Identifier "int"
      Pipe "|"
      NamedType
        Name
          Identifier "string"
      Pipe "|"
      NamedType
        Name
          Identifier "null"
    "#);
}

#[test]
fn intersections_bind_tighter_than_unions() {
    insta::assert_snapshot!(render_type("A&B|C"), @r#"
    UnionType
      IntersectionType
        NamedType
          Name
            Identifier "A"
        Ampersand "&"
        NamedType
          Name
            Identifier "B"
      Pipe "|"
      NamedType
        Name
          Identifier "C"
    "#);
}

#[test]
fn dnf_types_keep_their_parentheses_as_a_node() {
    insta::assert_snapshot!(render_type("(A&B)|C"), @r#"
    UnionType
      ParenthesizedType
        OpenParenthesis "("
        IntersectionType
          NamedType
            Name
              Identifier "A"
          Ampersand "&"
          NamedType
            Name
              Identifier "B"
        CloseParenthesis ")"
      Pipe "|"
      NamedType
        Name
          Identifier "C"
    "#);
}

#[test]
fn a_parameter_ampersand_before_a_variable_is_by_reference_not_intersection() {
    insta::assert_snapshot!(render_statement("function f(A&$x) {}"), @r#"
    FunctionDeclaration
      Function "function"
      Identifier "f"
      ParameterList
        OpenParenthesis "("
        Parameter
          NamedType
            Name
              Identifier "A"
          Ampersand "&"
          Variable "$x"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_parameter_ampersand_before_a_variadic_stays_the_parameter_marker() {
    assert_eq!(
        parser_diagnostics("<?php function f(A&...$rest) {}"),
        vec![]
    );
}

#[test]
fn a_dnf_parameter_type_parses() {
    assert_eq!(
        parser_diagnostics("<?php function f((Countable&ArrayAccess)|null $x) {}"),
        vec![]
    );
}

#[test]
fn a_missing_union_member_is_diagnosed() {
    assert_eq!(
        parser_diagnostics("<?php function f(): int| {}"),
        vec![ParserDiagnosticKind::ExpectedType]
    );
}

#[test]
fn a_refused_parameter_type_never_stalls_the_parameter_list() {
    // A closure buried under deep parenthesized-expression nesting
    // reaches its parameter list with the nesting budget nearly spent,
    // so `type_expression` can refuse the parameter's type without
    // consuming a token; the list's position guard must force one
    // error bump. The broken loop re-diagnosed the same missing
    // parameter variable once per spin until the step fuse blew, so
    // the spin signature is that diagnostic repeating; the band of
    // depths keeps the test on the exact exhaustion point even if the
    // grammar's nesting arithmetic drifts.
    for depth in 120..=130 {
        let source = format!(
            "<?php $x = {}function(int $y){{}}{};",
            "(".repeat(depth),
            ")".repeat(depth)
        );
        let diagnostics = parser_diagnostics(&source);
        assert!(
            !diagnostics.contains(&ParserDiagnosticKind::NoProgress),
            "the parse must never abandon tokens at depth {depth}"
        );
        let missing_variable_count = diagnostics
            .iter()
            .filter(|kind| {
                **kind == ParserDiagnosticKind::Expected(celerrate_syntax::SyntaxKind::Variable)
            })
            .count();
        assert!(
            missing_variable_count <= 1,
            "one refused parameter reports once at depth {depth}, \
             got {missing_variable_count}"
        );
    }
}

#[test]
fn pathological_nullable_nesting_stays_a_diagnostic() {
    // `??` lexes as one coalesce token, so the chain must be spaced to
    // reach the parser as repeated `?` tokens.
    let source = format!("<?php function f(): {}int {{}}", "? ".repeat(300));
    let diagnostics = parser_diagnostics(&source);
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::NestingTooDeep),
        "the nesting guard must refuse, got {diagnostics:?}"
    );
}
