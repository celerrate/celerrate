#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_member};

#[test]
fn a_typed_property_lists_its_declarators() {
    insta::assert_snapshot!(render_member("public int $balance = 0, $pending;"), @r#"
    PropertyDeclaration
      Public "public"
      NamedType
        Name
          Identifier "int"
      PropertyElement
        Variable "$balance"
        Equals "="
        Literal
          IntegerLiteral "0"
      Comma ","
      PropertyElement
        Variable "$pending"
      Semicolon ";"
    "#);
}

#[test]
fn a_var_property_parses() {
    insta::assert_snapshot!(render_member("var $legacy;"), @r#"
    PropertyDeclaration
      Var "var"
      PropertyElement
        Variable "$legacy"
      Semicolon ";"
    "#);
}

#[test]
fn asymmetric_visibility_stays_flat_modifier_tokens() {
    insta::assert_snapshot!(render_member("public private(set) string $name;"), @r#"
    PropertyDeclaration
      Public "public"
      Private "private"
      OpenParenthesis "("
      Identifier "set"
      CloseParenthesis ")"
      NamedType
        Name
          Identifier "string"
      PropertyElement
        Variable "$name"
      Semicolon ";"
    "#);
}

#[test]
fn a_nullable_static_property_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { protected static ?self $instance = null; }"),
        vec![]
    );
}

#[test]
fn a_class_constant_carries_modifiers_and_a_type() {
    insta::assert_snapshot!(render_member("final protected const int LIMIT = 10;"), @r#"
    ConstantDeclaration
      Final "final"
      Protected "protected"
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
fn a_semi_reserved_class_constant_name_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { const FOR = 'semi-reserved'; }"),
        vec![]
    );
}

#[test]
fn dangling_modifiers_become_wreckage_and_the_list_recovers() {
    // `public` wraps into an ErrorNode (ExpectedDeclaration); the `;`
    // it dangled on is then swept by the member list (UnexpectedToken);
    // the constant after it parses clean.
    assert_eq!(
        parser_diagnostics("<?php class A { public; const OK = 1; }"),
        vec![
            ParserDiagnosticKind::ExpectedDeclaration,
            ParserDiagnosticKind::UnexpectedToken,
        ]
    );
}

#[test]
fn a_property_without_a_variable_is_diagnosed() {
    let diagnostics = parser_diagnostics("<?php class A { public int = 5; }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::Variable)),
        "got {diagnostics:?}"
    );
}
