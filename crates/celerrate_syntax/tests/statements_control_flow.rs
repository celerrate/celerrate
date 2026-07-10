#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn a_classic_if_wraps_condition_and_body() {
    insta::assert_snapshot!(render_statement("if ($x) echo 1;"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
    "#);
}

#[test]
fn elseif_and_else_are_clauses() {
    insta::assert_snapshot!(render_statement("if ($a) { } elseif ($b) { } else { }"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$a"
      CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
      ElseIfClause
        ElseIf "elseif"
        OpenParenthesis "("
        VariableReference
          Variable "$b"
        CloseParenthesis ")"
        Block
          OpenBrace "{"
          CloseBrace "}"
      ElseClause
        Else "else"
        Block
          OpenBrace "{"
          CloseBrace "}"
    "#);
}

#[test]
fn else_if_with_a_space_nests_an_if_inside_the_else() {
    insta::assert_snapshot!(render_statement("if ($a) echo 1; else if ($b) echo 2;"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$a"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      ElseClause
        Else "else"
        IfStatement
          If "if"
          OpenParenthesis "("
          VariableReference
            Variable "$b"
          CloseParenthesis ")"
          EchoStatement
            Echo "echo"
            Literal
              IntegerLiteral "2"
            Semicolon ";"
    "#);
}

#[test]
fn a_dangling_else_binds_to_the_innermost_if() {
    let rendered = render_statement("if ($a) if ($b) echo 1; else echo 2;");
    // The outer if has no ElseClause child of its own: the else sits
    // inside the inner IfStatement.
    let inner_holds_else = rendered
        .lines()
        .any(|line| line.trim() == "ElseClause" && line.starts_with("    "));
    assert!(
        inner_holds_else,
        "the else must nest inside the inner if:\n{rendered}"
    );
}

#[test]
fn a_missing_condition_parenthesis_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php if $x echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis))
    );
}

#[test]
fn a_missing_body_is_diagnosed_without_consuming() {
    assert!(parser_diagnostics("<?php if ($x)").contains(&ParserDiagnosticKind::ExpectedStatement));
}

#[test]
fn an_alternative_if_closes_with_endif() {
    insta::assert_snapshot!(
        render_statement("if ($x): echo 1; elseif ($y): echo 2; else: echo 3; endif;"),
        @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      Colon ":"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      ElseIfClause
        ElseIf "elseif"
        OpenParenthesis "("
        VariableReference
          Variable "$y"
        CloseParenthesis ")"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "2"
          Semicolon ";"
      ElseClause
        Else "else"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "3"
          Semicolon ";"
      EndIf "endif"
      Semicolon ";"
    "#);
}

#[test]
fn inline_html_interrupts_an_alternative_body() {
    // The templating idiom: the body of the colon form is raw HTML
    // between a close tag and the next open tag.
    assert!(parser_diagnostics("<?php if ($x): ?><p>yes</p><?php endif;").is_empty());
}

#[test]
fn a_missing_endif_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php if ($x): echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::EndIf))
    );
}

#[test]
fn a_block_does_not_swallow_an_alternative_closer() {
    // `endif` inside braces belongs to nobody: the block stops,
    // diagnoses its missing brace, and the orphan surfaces to the
    // source-file loop as an error element.
    let diagnostics = parser_diagnostics("<?php { endif; }");
    assert!(diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace)));
    assert!(diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken));
}

#[test]
fn a_body_against_a_terminator_is_diagnosed_without_consuming() {
    // `else` right after the condition: the body is missing. The
    // embedded-statement rule diagnoses without consuming, so the if
    // still claims its else clause and recovery stays local.
    let diagnostics = parser_diagnostics("<?php if ($x) else echo 1;");
    assert_eq!(diagnostics, vec![ParserDiagnosticKind::ExpectedStatement]);
}

#[test]
fn while_takes_both_syntaxes() {
    insta::assert_snapshot!(render_statement("while ($x) echo 1;"), @r#"
    WhileStatement
      While "while"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
    "#);
    insta::assert_snapshot!(render_statement("while ($x): echo 1; endwhile;"), @r#"
    WhileStatement
      While "while"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      Colon ":"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      EndWhile "endwhile"
      Semicolon ";"
    "#);
}

#[test]
fn do_while_puts_the_condition_after_the_body() {
    insta::assert_snapshot!(render_statement("do echo 1; while ($x);"), @r#"
    DoWhileStatement
      Do "do"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      While "while"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn do_without_while_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php do echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::While))
    );
}

#[test]
fn for_holds_three_sections_and_a_body() {
    insta::assert_snapshot!(render_statement("for ($i = 0; $i < 3; $i++) echo $i;"), @r#"
    ForStatement
      For "for"
      OpenParenthesis "("
      ForExpressionList
        AssignmentExpression
          VariableReference
            Variable "$i"
          Equals "="
          Literal
            IntegerLiteral "0"
      Semicolon ";"
      ForExpressionList
        BinaryExpression
          VariableReference
            Variable "$i"
          Less "<"
          Literal
            IntegerLiteral "3"
      Semicolon ";"
      ForExpressionList
        PostfixExpression
          VariableReference
            Variable "$i"
          PlusPlus "++"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        VariableReference
          Variable "$i"
        Semicolon ";"
    "#);
}

#[test]
fn for_sections_may_be_empty_or_lists() {
    insta::assert_snapshot!(render_statement("for (;;) ;"), @r#"
    ForStatement
      For "for"
      OpenParenthesis "("
      ForExpressionList
      Semicolon ";"
      ForExpressionList
      Semicolon ";"
      ForExpressionList
      CloseParenthesis ")"
      EmptyStatement
        Semicolon ";"
    "#);
    assert!(parser_diagnostics("<?php for ($i = 0, $j = 9; $i < $j; $i++, $j--) ;").is_empty());
}

#[test]
fn an_alternative_for_closes_with_endfor() {
    assert!(parser_diagnostics("<?php for (;;): echo 1; endfor;").is_empty());
    assert!(
        parser_diagnostics("<?php for (;;): echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::EndFor))
    );
}
