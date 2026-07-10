#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_expression, render_statement};

#[test]
fn a_backed_enum_with_heritage_and_cases() {
    insta::assert_snapshot!(render_statement("enum Suit: string implements HasColor { case Hearts = 'H'; }"), @r#"
    EnumDeclaration
      Enum "enum"
      Identifier "Suit"
      Colon ":"
      NamedType
        Name
          Identifier "string"
      ImplementsClause
        Implements "implements"
        Name
          Identifier "HasColor"
      MemberList
        OpenBrace "{"
        EnumCase
          Case "case"
          Identifier "Hearts"
          Equals "="
          Literal
            SingleQuotedString "'H'"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn a_pure_enum_case_has_no_value() {
    insta::assert_snapshot!(render_statement("enum Direction { case North; }"), @r#"
    EnumDeclaration
      Enum "enum"
      Identifier "Direction"
      MemberList
        OpenBrace "{"
        EnumCase
          Case "case"
          Identifier "North"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn enums_carry_ordinary_members_too() {
    assert_eq!(
        parser_diagnostics(
            "<?php enum Suit: string { case Hearts = 'H'; const WILD = '*'; public function color(): string { return 'red'; } }"
        ),
        vec![]
    );
}

#[test]
fn enum_stays_callable_as_a_function_name() {
    // Zend backward compatibility: `enum` is not reserved; it only
    // declares when a name follows.
    assert_eq!(parser_diagnostics("<?php enum(1);"), vec![]);
}

#[test]
fn a_case_member_in_a_class_parses_and_is_judged_upstairs() {
    // Structurally fine anywhere a member is; enums-only is semantic.
    assert_eq!(parser_diagnostics("<?php class A { case North; }"), vec![]);
}

#[test]
fn a_semi_reserved_case_name_parses() {
    assert_eq!(parser_diagnostics("<?php enum Ops { case List; }"), vec![]);
}

#[test]
fn a_case_without_a_name_is_diagnosed_and_the_enum_recovers() {
    let diagnostics = parser_diagnostics("<?php enum Broken { case = 1; case Ok; }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(
            celerrate_syntax::SyntaxKind::Identifier
        )),
        "got {diagnostics:?}"
    );
}

#[test]
fn a_bare_enum_with_no_name_is_diagnosed_and_wrapped() {
    // `enum` with no name behind it: the declaration dispatch requires
    // a following identifier, so this token reaches the expression
    // grammar instead, which refuses it and wraps it in an `ErrorNode`.
    // Losslessness is covered by `parse_verified` inside `parser_diagnostics`.
    let diagnostics = parser_diagnostics("<?php enum;");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::ExpectedExpression),
        "got {diagnostics:?}"
    );
}

#[test]
fn new_enum_parenthesized_stays_a_constructor_call() {
    // Zend keeps `enum` usable as a plain class or function name for
    // backward compatibility: directly followed by `(`, it is never the
    // declaration keyword, even right after `new`. Same hack as
    // `new readonly(...)`, pinned by the test beside this one.
    insta::assert_snapshot!(render_expression("new enum(1)"), @r#"
    NewExpression
      New "new"
      Name
        Enum "enum"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
    "#);
}
