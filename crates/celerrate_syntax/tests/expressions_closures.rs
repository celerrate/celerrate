//! Closures and arrow functions.
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
fn a_closure_holds_parameters_use_clause_and_block() {
    insta::assert_snapshot!(
        support::render_expression("function ($a, $b = 1) use (&$total) { echo $a; }"),
        @r#"
    ClosureExpression
      Function "function"
      ParameterList
        OpenParenthesis "("
        Parameter
          Variable "$a"
        Comma ","
        Parameter
          Variable "$b"
          Equals "="
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
      ClosureUseClause
        Use "use"
        OpenParenthesis "("
        Ampersand "&"
        VariableReference
          Variable "$total"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        EchoStatement
          Echo "echo"
          VariableReference
            Variable "$a"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn typed_variadic_and_by_reference_parameters_parse() {
    assert!(
        parser_diagnostics("<?php function (int $x, ?\\Foo\\Bar $y, callable ...$rest) {};")
            .is_empty()
    );
    assert!(parser_diagnostics("<?php function (&$byReference) {};").is_empty());
    assert!(parser_diagnostics("<?php function &($x) { $x; };").is_empty());
}

#[test]
fn return_types_parse_on_both_forms() {
    assert!(parser_diagnostics("<?php function (): int {};").is_empty());
    insta::assert_snapshot!(support::render_expression("static fn (): int => 1"), @r#"
    ArrowFunctionExpression
      Static "static"
      Fn "fn"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      NamedType
        Name
          Identifier "int"
      FatArrow "=>"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn an_arrow_function_body_extends_as_far_as_possible() {
    insta::assert_snapshot!(support::render_expression("fn ($x) => $x * 2"), @r#"
    ArrowFunctionExpression
      Fn "fn"
      ParameterList
        OpenParenthesis "("
        Parameter
          Variable "$x"
        CloseParenthesis ")"
      FatArrow "=>"
      BinaryExpression
        VariableReference
          Variable "$x"
        Star "*"
        Literal
          IntegerLiteral "2"
    "#);
}

#[test]
fn arrow_functions_nest_and_stop_at_argument_commas() {
    assert!(parser_diagnostics("<?php $add = fn ($x) => fn ($y) => $x + $y;").is_empty());
    assert!(parser_diagnostics("<?php usort($list, fn ($a, $b) => $a <=> $b);").is_empty());
}

#[test]
fn static_closures_and_immediate_calls_parse() {
    assert!(parser_diagnostics("<?php static function () {};").is_empty());
    assert!(parser_diagnostics("<?php (function () { 1; })();").is_empty());
    // `static` alone keeps its scoped-access meaning.
    assert!(parser_diagnostics("<?php static::helper();").is_empty());
}

#[test]
fn closures_nest_statements_and_inline_html() {
    assert!(parser_diagnostics("<?php function () { echo 1; ?>raw<?php echo 2; };").is_empty());
}

#[test]
fn a_parameter_missing_its_variable_is_diagnosed() {
    // `int` is deliberately avoided here: `(int)` alone lexes as a cast
    // token (task 5's rule matches a bare `(`, a cast word, and `)`
    // with only whitespace between), so it would never reach the
    // parameter grammar at all. `Foo` is not a cast word.
    assert!(
        parser_diagnostics("<?php function (Foo) {};")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Variable))
    );
}

#[test]
fn an_arrow_function_missing_its_body_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php fn () => ;").contains(&ParserDiagnosticKind::ExpectedExpression)
    );
}
