//! Keyword-headed expressions: new, clone, the intrinsics, the
//! low-precedence prefixes, and match.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxDiagnosticKind};

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
fn new_takes_a_name_and_arguments() {
    insta::assert_snapshot!(support::render_expression("new Foo(1)"), @r#"
    NewExpression
      New "new"
      Name
        Identifier "Foo"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
    "#);
}

#[test]
fn new_accepts_every_class_reference_form() {
    assert!(parser_diagnostics("<?php new \\Foo\\Bar();").is_empty());
    assert!(parser_diagnostics("<?php new static();").is_empty());
    assert!(parser_diagnostics("<?php new $class;").is_empty());
    assert!(parser_diagnostics("<?php new $factory->product();").is_empty());
    assert!(parser_diagnostics("<?php new ($resolver->pick())($x);").is_empty());
}

#[test]
fn member_access_chains_on_new_since_php_84() {
    insta::assert_snapshot!(support::render_expression("new Foo()->bar()"), @r#"
    CallExpression
      MemberAccessExpression
        NewExpression
          New "new"
          Name
            Identifier "Foo"
          ArgumentList
            OpenParenthesis "("
            CloseParenthesis ")"
        Arrow "->"
        MemberName
          Identifier "bar"
      ArgumentList
        OpenParenthesis "("
        CloseParenthesis ")"
    "#);
}

#[test]
fn an_anonymous_class_is_deferred_with_recovery() {
    // `new class {}` belongs to the declarations plan; until then the
    // tokens survive through recovery.
    assert!(
        parser_diagnostics("<?php new class;").contains(&ParserDiagnosticKind::UnexpectedToken)
    );
}

#[test]
fn clone_keeps_its_prefix_form_and_precedence() {
    insta::assert_snapshot!(support::render_expression("clone $entity + 1"), @r#"
    BinaryExpression
      CloneExpression
        Clone "clone"
        VariableReference
          Variable "$entity"
      Plus "+"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn the_php_85_clone_function_form_parses_and_chains() {
    insta::assert_snapshot!(support::render_expression("clone($entity, ['id' => null])"), @r#"
    CloneExpression
      Clone "clone"
      ArgumentList
        OpenParenthesis "("
        Argument
          VariableReference
            Variable "$entity"
        Comma ","
        Argument
          ArrayExpression
            OpenBracket "["
            ArrayElement
              Literal
                SingleQuotedString "'id'"
              FatArrow "=>"
              NameExpression
                Name
                  Identifier "null"
            CloseBracket "]"
        CloseParenthesis ")"
    "#);
    assert!(parser_diagnostics("<?php clone($entity)->touch();").is_empty());
}
