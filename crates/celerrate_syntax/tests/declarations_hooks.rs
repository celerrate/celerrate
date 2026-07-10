#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_member};

#[test]
fn abstract_hooks_are_name_and_semicolon() {
    insta::assert_snapshot!(render_member("public string $name { get; set; }"), @r#"
    PropertyDeclaration
      Public "public"
      NamedType
        Name
          Identifier "string"
      PropertyElement
        Variable "$name"
        PropertyHookList
          OpenBrace "{"
          PropertyHook
            Identifier "get"
            Semicolon ";"
          PropertyHook
            Identifier "set"
            Semicolon ";"
          CloseBrace "}"
    "#);
}

#[test]
fn hook_bodies_take_arrow_expressions_parameters_and_blocks() {
    insta::assert_snapshot!(render_member("public string $name { get => $this->raw; set(string $value) { $this->raw = $value; } }"), @r#"
    PropertyDeclaration
      Public "public"
      NamedType
        Name
          Identifier "string"
      PropertyElement
        Variable "$name"
        PropertyHookList
          OpenBrace "{"
          PropertyHook
            Identifier "get"
            FatArrow "=>"
            MemberAccessExpression
              VariableReference
                Variable "$this"
              Arrow "->"
              MemberName
                Identifier "raw"
            Semicolon ";"
          PropertyHook
            Identifier "set"
            ParameterList
              OpenParenthesis "("
              Parameter
                NamedType
                  Name
                    Identifier "string"
                Variable "$value"
              CloseParenthesis ")"
            Block
              OpenBrace "{"
              ExpressionStatement
                AssignmentExpression
                  MemberAccessExpression
                    VariableReference
                      Variable "$this"
                    Arrow "->"
                    MemberName
                      Identifier "raw"
                  Equals "="
                  VariableReference
                    Variable "$value"
                Semicolon ";"
              CloseBrace "}"
          CloseBrace "}"
    "#);
}

#[test]
fn a_by_reference_final_hook_parses() {
    assert_eq!(
        parser_diagnostics(
            "<?php class A { public array $items { final &get { return $this->items; } } }"
        ),
        vec![]
    );
}

#[test]
fn a_hooked_property_needs_no_semicolon() {
    assert_eq!(
        parser_diagnostics("<?php class A { public string $x { get; } public int $y = 1; }"),
        vec![]
    );
}

#[test]
fn hooks_in_an_interface_parse() {
    assert_eq!(
        parser_diagnostics("<?php interface Named { public string $name { get; set; } }"),
        vec![]
    );
}

#[test]
fn promoted_constructor_parameters_take_modifiers() {
    insta::assert_snapshot!(render_member("public function __construct(public readonly int $x, private(set) string $y = 'a') {}"), @r#"
    MethodDeclaration
      Public "public"
      Function "function"
      Identifier "__construct"
      ParameterList
        OpenParenthesis "("
        Parameter
          Public "public"
          Readonly "readonly"
          NamedType
            Name
              Identifier "int"
          Variable "$x"
        Comma ","
        Parameter
          Private "private"
          OpenParenthesis "("
          Identifier "set"
          CloseParenthesis ")"
          NamedType
            Name
              Identifier "string"
          Variable "$y"
          Equals "="
          Literal
            SingleQuotedString "'a'"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn hooks_on_a_promoted_parameter_parse() {
    // 8.4 allows hooks in constructor promotion; availability is
    // semantic.
    assert_eq!(
        parser_diagnostics(
            "<?php class A { public function __construct(public string $full { get => $this->first; }) {} }"
        ),
        vec![]
    );
}

#[test]
fn junk_inside_a_hook_list_is_swept_and_the_list_recovers() {
    let diagnostics = parser_diagnostics("<?php class A { public int $x { 42 get; } }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken),
        "got {diagnostics:?}"
    );
}

#[test]
fn an_unclosed_hook_list_terminates() {
    let diagnostics = parser_diagnostics("<?php class A { public int $x { get;");
    assert!(!diagnostics.is_empty());
}

#[test]
fn constructor_promotion_accepts_final_alongside_visibility() {
    // php-src's `optional_cpp_modifiers` is the full member-modifier
    // set; legality of a given modifier on a parameter is judged at
    // compile time, not by the parser.
    assert_eq!(
        parser_diagnostics(
            "<?php class A { public function __construct(public final string $name) {} }"
        ),
        vec![]
    );
    assert_eq!(
        parser_diagnostics(
            "<?php class A { public function __construct(final public string $name) {} }"
        ),
        vec![]
    );
}
