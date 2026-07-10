#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_expression, render_statement};

#[test]
fn an_attributed_function_keeps_its_groups_inside_the_declaration() {
    insta::assert_snapshot!(render_statement("#[Route('/home')] function home() {}"), @r##"
    FunctionDeclaration
      AttributeGroup
        AttributeOpen "#["
        Attribute
          Name
            Identifier "Route"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                SingleQuotedString "'/home'"
            CloseParenthesis ")"
        CloseBracket "]"
      Function "function"
      Identifier "home"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "##);
}

#[test]
fn one_group_carries_several_attributes() {
    insta::assert_snapshot!(render_statement("#[First, Second(1)] class A {}"), @r##"
    ClassDeclaration
      AttributeGroup
        AttributeOpen "#["
        Attribute
          Name
            Identifier "First"
        Comma ","
        Attribute
          Name
            Identifier "Second"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                IntegerLiteral "1"
            CloseParenthesis ")"
        CloseBracket "]"
      Class "class"
      Identifier "A"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "##);
}

#[test]
fn stacked_groups_parse_on_every_declaration_kind() {
    for source in [
        "<?php #[A] #[B] final class C {}",
        "<?php #[A] interface I {}",
        "<?php #[A] trait T {}",
        "<?php #[A] enum E { case One; }",
        "<?php #[A] const X = 1;",
    ] {
        assert_eq!(parser_diagnostics(source), vec![], "source: {source}");
    }
}

#[test]
fn members_cases_and_parameters_take_attributes() {
    for source in [
        "<?php class A { #[Override] public function handle(#[SensitiveParameter] string $token): void {} }",
        "<?php class A { #[Marker] public int $x = 1; }",
        "<?php class A { #[Marker] const X = 1; }",
        "<?php enum E { #[Marker] case One; }",
        "<?php class A { public int $x { #[Marker] get => 1; } }",
    ] {
        assert_eq!(parser_diagnostics(source), vec![], "source: {source}");
    }
}

#[test]
fn closures_and_arrow_functions_take_attributes() {
    insta::assert_snapshot!(render_expression("#[Pure] static fn (int $x): int => $x"), @r##"
    ArrowFunctionExpression
      AttributeGroup
        AttributeOpen "#["
        Attribute
          Name
            Identifier "Pure"
        CloseBracket "]"
      Static "static"
      Fn "fn"
      ParameterList
        OpenParenthesis "("
        Parameter
          NamedType
            Name
              Identifier "int"
          Variable "$x"
        CloseParenthesis ")"
      Colon ":"
      NamedType
        Name
          Identifier "int"
      FatArrow "=>"
      VariableReference
        Variable "$x"
    "##);
}

#[test]
fn an_anonymous_class_takes_attributes_after_new() {
    assert_eq!(
        parser_diagnostics("<?php $o = new #[Marker] class {};"),
        vec![]
    );
}

#[test]
fn attributes_before_a_non_declaration_become_wreckage() {
    // Zend rejects `#[A] echo 1;` too; the groups keep their structure
    // inside an ErrorNode and the statement parses on its own.
    assert_eq!(
        parser_diagnostics("<?php #[Marker] echo 1;"),
        vec![ParserDiagnosticKind::ExpectedDeclaration]
    );
}

#[test]
fn an_unterminated_group_is_diagnosed_and_recovers() {
    let diagnostics = parser_diagnostics("<?php #[Marker function f() {}");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(
            celerrate_syntax::SyntaxKind::CloseBracket
        )),
        "got {diagnostics:?}"
    );
}
