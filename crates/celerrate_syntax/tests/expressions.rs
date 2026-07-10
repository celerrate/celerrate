//! Expression grammar tests: tree shapes through
//! `support::render_expression` (offsets omitted), diagnostics asserted
//! structurally. Every parse asserts the lossless invariant on the way.
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
fn multiplication_binds_tighter_than_addition() {
    insta::assert_snapshot!(support::render_expression("1 + 2 * 3"), @r#"
    BinaryExpression
      Literal
        IntegerLiteral "1"
      Plus "+"
      BinaryExpression
        Literal
          IntegerLiteral "2"
        Star "*"
        Literal
          IntegerLiteral "3"
    "#);
}

#[test]
fn same_level_operators_associate_left() {
    insta::assert_snapshot!(support::render_expression("1 - 2 - 3"), @r#"
    BinaryExpression
      BinaryExpression
        Literal
          IntegerLiteral "1"
        Minus "-"
        Literal
          IntegerLiteral "2"
      Minus "-"
      Literal
        IntegerLiteral "3"
    "#);
}

#[test]
fn power_associates_right() {
    insta::assert_snapshot!(support::render_expression("2 ** 3 ** 4"), @r#"
    BinaryExpression
      Literal
        IntegerLiteral "2"
      StarStar "**"
      BinaryExpression
        Literal
          IntegerLiteral "3"
        StarStar "**"
        Literal
          IntegerLiteral "4"
    "#);
}

#[test]
fn concatenation_sits_below_addition() {
    // The PHP 8.0 precedence change: `.` is looser than `+`.
    insta::assert_snapshot!(support::render_expression("'a' . 1 + 2"), @r#"
    BinaryExpression
      Literal
        SingleQuotedString "'a'"
      Dot "."
      BinaryExpression
        Literal
          IntegerLiteral "1"
        Plus "+"
        Literal
          IntegerLiteral "2"
    "#);
}

#[test]
fn the_pipe_operator_sits_between_comparison_and_concatenation() {
    insta::assert_snapshot!(support::render_expression("$x . 'a' |> $f == 4"), @r#"
    BinaryExpression
      BinaryExpression
        BinaryExpression
          VariableReference
            Variable "$x"
          Dot "."
          Literal
            SingleQuotedString "'a'"
        PipeGreater "|>"
        VariableReference
          Variable "$f"
      EqualsEquals "=="
      Literal
        IntegerLiteral "4"
    "#);
}

#[test]
fn coalesce_associates_right() {
    insta::assert_snapshot!(support::render_expression("$a ?? $b ?? $c"), @r#"
    BinaryExpression
      VariableReference
        Variable "$a"
      QuestionQuestion "??"
      BinaryExpression
        VariableReference
          Variable "$b"
        QuestionQuestion "??"
        VariableReference
          Variable "$c"
    "#);
}

#[test]
fn word_logical_operators_bind_loosest() {
    insta::assert_snapshot!(support::render_expression("$a && $b or $c && $d"), @r#"
    BinaryExpression
      BinaryExpression
        VariableReference
          Variable "$a"
        AmpersandAmpersand "&&"
        VariableReference
          Variable "$b"
      Or "or"
      BinaryExpression
        VariableReference
          Variable "$c"
        AmpersandAmpersand "&&"
        VariableReference
          Variable "$d"
    "#);
}

#[test]
fn instanceof_is_a_binary_operator() {
    insta::assert_snapshot!(support::render_expression("$a instanceof $class"), @r#"
    BinaryExpression
      VariableReference
        Variable "$a"
      InstanceOf "instanceof"
      VariableReference
        Variable "$class"
    "#);
}

#[test]
fn parentheses_regroup() {
    insta::assert_snapshot!(support::render_expression("(1 + 2) * 3"), @r#"
    BinaryExpression
      ParenthesizedExpression
        OpenParenthesis "("
        BinaryExpression
          Literal
            IntegerLiteral "1"
          Plus "+"
          Literal
            IntegerLiteral "2"
        CloseParenthesis ")"
      Star "*"
      Literal
        IntegerLiteral "3"
    "#);
}

#[test]
fn a_comparison_chain_is_diagnosed_and_still_parses() {
    // Zend rejects `1 < 2 < 3`; we parse it left-associatively and say so.
    let parse = support::parse_verified("<?php 1 < 2 < 3;");
    let kinds = parse
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![SyntaxDiagnosticKind::Parser(
            ParserDiagnosticKind::NonAssociativeOperator
        )]
    );
}

#[test]
fn equality_and_relational_are_different_levels() {
    assert!(parser_diagnostics("<?php $a == $b < $c;").is_empty());
}

#[test]
fn an_equality_chain_is_diagnosed_and_still_parses() {
    // Zend rejects `$a == $b == $c`; we parse it left-associatively and
    // say so, exactly like the relational chain above.
    assert_eq!(
        parser_diagnostics("<?php $a == $b == $c;"),
        vec![ParserDiagnosticKind::NonAssociativeOperator]
    );
}

#[test]
fn an_instanceof_chain_is_diagnosed_and_still_parses() {
    // Zend rejects `$a instanceof $b instanceof $c`; we parse it
    // left-associatively and say so, exactly like the other
    // non-associative levels.
    assert_eq!(
        parser_diagnostics("<?php $a instanceof $b instanceof $c;"),
        vec![ParserDiagnosticKind::NonAssociativeOperator]
    );
}

#[test]
fn a_missing_right_operand_is_diagnosed_and_the_node_completes() {
    let diagnostics = parser_diagnostics("<?php 1 +;");
    assert_eq!(diagnostics, vec![ParserDiagnosticKind::ExpectedExpression]);
    let parse = support::parse_verified("<?php 1 +;");
    let statement = parse.tree().children().next().expect("a statement");
    assert_eq!(
        statement.children().next().map(|node| node.kind()),
        Some(SyntaxKind::BinaryExpression)
    );
}

#[test]
fn an_unclosed_parenthesis_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php (1 + 2;").contains(&ParserDiagnosticKind::Expected(
            SyntaxKind::CloseParenthesis
        ))
    );
}

#[test]
fn pathological_nesting_trips_the_guard_without_panicking() {
    let source = format!("<?php {}1;", "(".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}

#[test]
fn power_binds_tighter_than_prefix_minus() {
    insta::assert_snapshot!(support::render_expression("-2 ** 3"), @r#"
    PrefixExpression
      Minus "-"
      BinaryExpression
        Literal
          IntegerLiteral "2"
        StarStar "**"
        Literal
          IntegerLiteral "3"
    "#);
}

#[test]
fn logical_not_binds_tighter_than_boolean_and() {
    insta::assert_snapshot!(support::render_expression("!$a && $b"), @r#"
    BinaryExpression
      PrefixExpression
        Bang "!"
        VariableReference
          Variable "$a"
      AmpersandAmpersand "&&"
      VariableReference
        Variable "$b"
    "#);
}

#[test]
fn a_cast_is_a_single_token_prefix() {
    insta::assert_snapshot!(support::render_expression("(int) $x + 1"), @r#"
    BinaryExpression
      CastExpression
        IntCast "(int)"
        VariableReference
          Variable "$x"
      Plus "+"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn increment_works_prefix_and_postfix() {
    insta::assert_snapshot!(support::render_expression("++$i"), @r#"
    PrefixExpression
      PlusPlus "++"
      VariableReference
        Variable "$i"
    "#);
    insta::assert_snapshot!(support::render_expression("$i++"), @r#"
    PostfixExpression
      VariableReference
        Variable "$i"
      PlusPlus "++"
    "#);
}

#[test]
fn error_suppression_wraps_its_operand() {
    insta::assert_snapshot!(support::render_expression("@$x + 1"), @r#"
    BinaryExpression
      PrefixExpression
        At "@"
        VariableReference
          Variable "$x"
      Plus "+"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn prefix_operators_nest() {
    insta::assert_snapshot!(support::render_expression("- -$x"), @r#"
    PrefixExpression
      Minus "-"
      PrefixExpression
        Minus "-"
        VariableReference
          Variable "$x"
    "#);
}

#[test]
fn assignment_associates_right() {
    insta::assert_snapshot!(support::render_expression("$a = $b = 1"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$a"
      Equals "="
      AssignmentExpression
        VariableReference
          Variable "$b"
        Equals "="
        Literal
          IntegerLiteral "1"
    "#);
}

#[test]
fn compound_assignment_operators_parse() {
    insta::assert_snapshot!(support::render_expression("$x ??= 1"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$x"
      QuestionQuestionEquals "??="
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn assignment_by_reference_keeps_the_ampersand() {
    insta::assert_snapshot!(support::render_expression("$a = &$b"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$a"
      Equals "="
      Ampersand "&"
      VariableReference
        Variable "$b"
    "#);
}

#[test]
fn the_ternary_parses_long_and_short_forms() {
    insta::assert_snapshot!(support::render_expression("$a ? 'y' : 'n'"), @r#"
    TernaryExpression
      VariableReference
        Variable "$a"
      Question "?"
      Literal
        SingleQuotedString "'y'"
      Colon ":"
      Literal
        SingleQuotedString "'n'"
    "#);
    insta::assert_snapshot!(support::render_expression("$a ?: 'n'"), @r#"
    TernaryExpression
      VariableReference
        Variable "$a"
      Question "?"
      Colon ":"
      Literal
        SingleQuotedString "'n'"
    "#);
}

#[test]
fn coalesce_binds_tighter_than_the_ternary() {
    insta::assert_snapshot!(support::render_expression("$a ?? $b ? 1 : 2"), @r#"
    TernaryExpression
      BinaryExpression
        VariableReference
          Variable "$a"
        QuestionQuestion "??"
        VariableReference
          Variable "$b"
      Question "?"
      Literal
        IntegerLiteral "1"
      Colon ":"
      Literal
        IntegerLiteral "2"
    "#);
}

#[test]
fn assignment_binds_looser_than_the_ternary() {
    insta::assert_snapshot!(support::render_expression("$a = $b ? 1 : 2"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$a"
      Equals "="
      TernaryExpression
        VariableReference
          Variable "$b"
        Question "?"
        Literal
          IntegerLiteral "1"
        Colon ":"
        Literal
          IntegerLiteral "2"
    "#);
}

#[test]
fn an_unparenthesized_ternary_chain_is_diagnosed() {
    // A compile error in Zend since 8.0; parsed left-associatively here.
    let diagnostics = parser_diagnostics("<?php $a ? 1 : $b ? 2 : 3;");
    assert_eq!(
        diagnostics,
        vec![ParserDiagnosticKind::NonAssociativeOperator]
    );
}

#[test]
fn a_ternary_missing_its_colon_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php $a ? 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Colon))
    );
}
