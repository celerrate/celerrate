#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn declare_takes_directives_and_a_body() {
    insta::assert_snapshot!(render_statement("declare(strict_types=1);"), @r#"
    DeclareStatement
      Declare "declare"
      OpenParenthesis "("
      DeclareDirective
        Identifier "strict_types"
        Equals "="
        Literal
          IntegerLiteral "1"
      CloseParenthesis ")"
      EmptyStatement
        Semicolon ";"
    "#);
}

#[test]
fn declare_accepts_every_body_form() {
    assert!(parser_diagnostics("<?php declare(ticks=1) { echo 1; }").is_empty());
    assert!(parser_diagnostics("<?php declare(ticks=1): echo 1; enddeclare;").is_empty());
    assert!(parser_diagnostics("<?php declare(encoding='UTF-8', ticks=1);").is_empty());
}

#[test]
fn a_directive_without_a_value_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php declare(strict_types);")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Equals))
    );
}
