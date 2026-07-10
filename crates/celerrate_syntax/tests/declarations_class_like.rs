#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_expression, render_statement};

#[test]
fn a_class_declaration_with_modifiers_and_heritage() {
    insta::assert_snapshot!(render_statement("abstract class A extends B implements C, D {}"), @r#"
    ClassDeclaration
      Abstract "abstract"
      Class "class"
      Identifier "A"
      ExtendsClause
        Extends "extends"
        Name
          Identifier "B"
      ImplementsClause
        Implements "implements"
        Name
          Identifier "C"
        Comma ","
        Name
          Identifier "D"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_readonly_class_parses() {
    assert_eq!(
        parser_diagnostics("<?php final readonly class Value {}"),
        vec![]
    );
}

#[test]
fn an_interface_extends_a_list() {
    insta::assert_snapshot!(render_statement("interface Shape extends HasArea, HasPerimeter {}"), @r#"
    InterfaceDeclaration
      Interface "interface"
      Identifier "Shape"
      ExtendsClause
        Extends "extends"
        Name
          Identifier "HasArea"
        Comma ","
        Name
          Identifier "HasPerimeter"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_trait_declaration_parses() {
    insta::assert_snapshot!(render_statement("trait Greets {}"), @r#"
    TraitDeclaration
      Trait "trait"
      Identifier "Greets"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_keyword_class_name_parses_and_is_judged_upstairs() {
    // Zend rejects `class List {}` (reserved word); structurally it is
    // one analyzable declaration, so it parses clean here.
    assert_eq!(parser_diagnostics("<?php class List {}"), vec![]);
}

#[test]
fn a_missing_class_name_before_extends_is_diagnosed_not_eaten() {
    // `extends` must stay the clause keyword: taking it as the name
    // would destroy the heritage structure.
    assert_eq!(
        parser_diagnostics("<?php class extends B {}"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Identifier)]
    );
}

#[test]
fn an_anonymous_class_parses_inside_new() {
    insta::assert_snapshot!(render_expression("new class(1) extends Base {}"), @r#"
    NewExpression
      New "new"
      ClassDeclaration
        Class "class"
        ArgumentList
          OpenParenthesis "("
          Argument
            Literal
              IntegerLiteral "1"
          CloseParenthesis ")"
        ExtendsClause
          Extends "extends"
          Name
            Identifier "Base"
        MemberList
          OpenBrace "{"
          CloseBrace "}"
    "#);
}

#[test]
fn a_readonly_anonymous_class_parses() {
    // `new readonly class {}` is 8.3; availability is semantic.
    assert_eq!(
        parser_diagnostics("<?php $o = new readonly class {};"),
        vec![]
    );
}

#[test]
fn new_readonly_with_a_call_stays_a_plain_new_target() {
    // Zend backward compatibility: `readonly` directly followed by `(`
    // is a call target, even right after `new`; only `readonly class`
    // routes to the anonymous-class form.
    assert_eq!(parser_diagnostics("<?php $x = new readonly(1);"), vec![]);
}

#[test]
fn readonly_stays_callable_as_a_function_name() {
    // Zend backward compatibility: `readonly` is not reserved as a
    // function name.
    insta::assert_snapshot!(render_statement("readonly($flag);"), @r#"
    ExpressionStatement
      CallExpression
        NameExpression
          Name
            Readonly "readonly"
        ArgumentList
          OpenParenthesis "("
          Argument
            VariableReference
              Variable "$flag"
          CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn modifiers_without_a_class_become_wreckage_and_the_rest_recovers() {
    assert_eq!(
        parser_diagnostics("<?php abstract 1;"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Class)]
    );
}

#[test]
fn junk_inside_a_member_list_is_swept_and_the_list_recovers() {
    assert_eq!(
        parser_diagnostics("<?php class A { 1 + 2; } echo 3;"),
        vec![
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::UnexpectedToken,
        ]
    );
}

#[test]
fn an_unclosed_member_list_is_diagnosed() {
    let diagnostics = parser_diagnostics("<?php class A {");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace)),
        "got {diagnostics:?}"
    );
}
