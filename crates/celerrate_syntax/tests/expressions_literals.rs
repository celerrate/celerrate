//! Arrays, list destructuring, and string interpolation nodes.
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
fn short_arrays_hold_keyed_and_positional_elements() {
    insta::assert_snapshot!(support::render_expression("[1, 'k' => 2]"), @r#"
    ArrayExpression
      OpenBracket "["
      ArrayElement
        Literal
          IntegerLiteral "1"
      Comma ","
      ArrayElement
        Literal
          SingleQuotedString "'k'"
        FatArrow "=>"
        Literal
          IntegerLiteral "2"
      CloseBracket "]"
    "#);
}

#[test]
fn the_long_array_form_shares_the_node_kind() {
    insta::assert_snapshot!(support::render_expression("array(1)"), @r#"
    ArrayExpression
      Array "array"
      OpenParenthesis "("
      ArrayElement
        Literal
          IntegerLiteral "1"
      CloseParenthesis ")"
    "#);
}

#[test]
fn spread_and_by_reference_elements_parse() {
    insta::assert_snapshot!(support::render_expression("[...$xs, &$y]"), @r#"
    ArrayExpression
      OpenBracket "["
      ArrayElement
        Ellipsis "..."
        VariableReference
          Variable "$xs"
      Comma ","
      ArrayElement
        Ampersand "&"
        VariableReference
          Variable "$y"
      CloseBracket "]"
    "#);
}

#[test]
fn destructuring_shapes_parse() {
    assert!(parser_diagnostics("<?php [$a, [$b, $c]] = $nested;").is_empty());
    assert!(parser_diagnostics("<?php [, $second] = $pair;").is_empty());
    assert!(parser_diagnostics("<?php ['k' => $v] = $map;").is_empty());
    assert!(parser_diagnostics("<?php [1 => &$byReference] = $source;").is_empty());
}

#[test]
fn list_destructuring_parses() {
    insta::assert_snapshot!(support::render_expression("list($a, $b) = $pair"), @r#"
    AssignmentExpression
      ListExpression
        List "list"
        OpenParenthesis "("
        ArrayElement
          VariableReference
            Variable "$a"
        Comma ","
        ArrayElement
          VariableReference
            Variable "$b"
        CloseParenthesis ")"
      Equals "="
      VariableReference
        Variable "$pair"
    "#);
}

#[test]
fn trailing_commas_and_nested_arrays_parse() {
    assert!(parser_diagnostics("<?php [[1, 2], [3, 4],];").is_empty());
}

#[test]
fn an_array_literal_indexes_directly() {
    // `[` after a primary is indexing; at expression start it is an array.
    assert!(parser_diagnostics("<?php [1, 2][0];").is_empty());
}

#[test]
fn an_unclosed_array_stops_at_the_statement_boundary() {
    let diagnostics = parser_diagnostics("<?php [1, 2; $x;");
    assert!(diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBracket)));
    let parse = support::parse_verified("<?php [1, 2; $x;");
    assert!(
        parse
            .tree()
            .children()
            .filter(|node| node.kind() == SyntaxKind::ExpressionStatement)
            .count()
            >= 2
    );
}

#[test]
fn pathological_nesting_inside_an_array_terminates() {
    // Deeply nested `[` inside an array element can exhaust the nesting
    // guard while `array_element_list`'s own loop is still active (the
    // leftover `[` tokens surface through the postfix loop at every
    // unwinding level, not only at the top). Without a mechanical
    // progress guarantee in `array_element_list` itself, this spins
    // forever; the assertion that matters here is that this test
    // completes.
    let source = format!("<?php {}1;", "[".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}

#[test]
fn a_double_quoted_string_holds_fragments_and_interpolations() {
    insta::assert_snapshot!(support::render_expression(r#""a $name b""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      StringFragment "a "
      SimpleInterpolation
        Variable "$name"
      StringFragment " b"
      DoubleQuote "\""
    "#);
}

#[test]
fn simple_interpolation_takes_one_property_hop_or_one_offset() {
    insta::assert_snapshot!(support::render_expression(r#""$user->name""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      SimpleInterpolation
        Variable "$user"
        Arrow "->"
        Identifier "name"
      DoubleQuote "\""
    "#);
    insta::assert_snapshot!(support::render_expression(r#""$items[0] $list[-1]""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      SimpleInterpolation
        Variable "$items"
        OpenBracket "["
        IntegerLiteral "0"
        CloseBracket "]"
      StringFragment " "
      SimpleInterpolation
        Variable "$list"
        OpenBracket "["
        Minus "-"
        IntegerLiteral "1"
        CloseBracket "]"
      DoubleQuote "\""
    "#);
}

#[test]
fn brace_interpolation_holds_a_full_expression() {
    insta::assert_snapshot!(support::render_expression(r#""x {$a->b(1)} y""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      StringFragment "x "
      BraceInterpolation
        OpenBrace "{"
        CallExpression
          MemberAccessExpression
            VariableReference
              Variable "$a"
            Arrow "->"
            MemberName
              Identifier "b"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                IntegerLiteral "1"
            CloseParenthesis ")"
        CloseBrace "}"
      StringFragment " y"
      DoubleQuote "\""
    "#);
}

#[test]
fn the_deprecated_dollar_brace_form_still_parses() {
    insta::assert_snapshot!(support::render_expression(r#""${name}""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      DollarBraceInterpolation
        DollarOpenBrace "${"
        NameExpression
          Name
            Identifier "name"
        CloseBrace "}"
      DoubleQuote "\""
    "#);
}

#[test]
fn backticks_are_shell_executions() {
    insta::assert_snapshot!(support::render_expression("`ls $dir`"), @r#"
    ShellExecExpression
      Backtick "`"
      StringFragment "ls "
      SimpleInterpolation
        Variable "$dir"
      Backtick "`"
    "#);
}

#[test]
fn heredocs_interpolate_and_nowdocs_do_not() {
    let heredoc = support::parse_verified("<?php <<<TXT\nHello $name\nTXT;");
    let interpolations = heredoc
        .tree()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::SimpleInterpolation)
        .count();
    assert_eq!(interpolations, 1);
    assert!(
        heredoc
            .tree()
            .descendants()
            .any(|node| node.kind() == SyntaxKind::HeredocExpression)
    );

    let nowdoc = support::parse_verified("<?php <<<'TXT'\nHello $name\nTXT;");
    let interpolation_nodes = nowdoc
        .tree()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::SimpleInterpolation)
        .count();
    assert_eq!(interpolation_nodes, 0);
}

#[test]
fn an_unterminated_string_adds_no_parser_diagnostic_of_its_own() {
    // The lexer already reported the unterminated string; the parser
    // contributes only the ordinary missing terminator, nothing about
    // the missing quote.
    assert_eq!(
        parser_diagnostics("<?php \"open"),
        vec![ParserDiagnosticKind::ExpectedSemicolon]
    );
}
