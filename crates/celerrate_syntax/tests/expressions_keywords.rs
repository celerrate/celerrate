//! Keyword-headed expressions: new, clone, the intrinsics, the
//! low-precedence prefixes, and match.
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

#[test]
fn isset_takes_a_variable_list() {
    insta::assert_snapshot!(support::render_expression("isset($a, $b->c)"), @r#"
    IssetExpression
      Isset "isset"
      ArgumentList
        OpenParenthesis "("
        Argument
          VariableReference
            Variable "$a"
        Comma ","
        Argument
          MemberAccessExpression
            VariableReference
              Variable "$b"
            Arrow "->"
            MemberName
              Identifier "c"
        CloseParenthesis ")"
    "#);
}

#[test]
fn empty_and_eval_require_their_parentheses() {
    assert!(parser_diagnostics("<?php empty($x);").is_empty());
    assert!(parser_diagnostics("<?php eval($code);").is_empty());
    assert!(
        parser_diagnostics("<?php isset;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis))
    );
}

#[test]
fn exit_and_die_take_optional_arguments() {
    insta::assert_snapshot!(support::render_expression("exit(1)"), @r#"
    ExitExpression
      Exit "exit"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
    "#);
    assert!(parser_diagnostics("<?php exit;").is_empty());
    assert!(parser_diagnostics("<?php die;").is_empty());
    assert!(parser_diagnostics("<?php $code = $failed ? exit(1) : 0;").is_empty());
}

#[test]
fn print_is_a_low_prefix_expression() {
    insta::assert_snapshot!(support::render_expression("print 'x' . 'y'"), @r#"
    PrintExpression
      Print "print"
      BinaryExpression
        Literal
          SingleQuotedString "'x'"
        Dot "."
        Literal
          SingleQuotedString "'y'"
    "#);
    assert!(parser_diagnostics("<?php $ok = print 'x';").is_empty());
}

#[test]
fn throw_works_as_a_coalesce_fallback() {
    insta::assert_snapshot!(support::render_expression("$x ?? throw new Error('missing')"), @r#"
    BinaryExpression
      VariableReference
        Variable "$x"
      QuestionQuestion "??"
      ThrowExpression
        Throw "throw"
        NewExpression
          New "new"
          Name
            Identifier "Error"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                SingleQuotedString "'missing'"
            CloseParenthesis ")"
    "#);
}

#[test]
fn yield_covers_bare_value_and_keyed_forms() {
    assert!(parser_diagnostics("<?php yield;").is_empty());
    assert!(parser_diagnostics("<?php yield $value;").is_empty());
    insta::assert_snapshot!(support::render_expression("yield $key => $value"), @r#"
    YieldExpression
      Yield "yield"
      VariableReference
        Variable "$key"
      FatArrow "=>"
      VariableReference
        Variable "$value"
    "#);
}

#[test]
fn yield_from_delegates_a_whole_generator() {
    insta::assert_snapshot!(support::render_expression("yield from $generator"), @r#"
    YieldExpression
      YieldFrom "yield from"
      VariableReference
        Variable "$generator"
    "#);
}

#[test]
fn yield_binds_tighter_than_the_word_operators() {
    insta::assert_snapshot!(support::render_expression("yield $a and $b"), @r#"
    BinaryExpression
      YieldExpression
        Yield "yield"
        VariableReference
          Variable "$a"
      And "and"
      VariableReference
        Variable "$b"
    "#);
}

#[test]
fn include_swallows_its_whole_operand() {
    insta::assert_snapshot!(support::render_expression("include $path . '.php'"), @r#"
    IncludeExpression
      Include "include"
      BinaryExpression
        VariableReference
          Variable "$path"
        Dot "."
        Literal
          SingleQuotedString "'.php'"
    "#);
    assert!(parser_diagnostics("<?php require_once __DIR__ . '/bootstrap.php';").is_empty());
}

#[test]
fn match_holds_arms_with_condition_lists_and_default() {
    insta::assert_snapshot!(
        support::render_expression("match ($status) { 1, 2 => 'low', default => 'other' }"),
        @r#"
    MatchExpression
      Match "match"
      OpenParenthesis "("
      VariableReference
        Variable "$status"
      CloseParenthesis ")"
      OpenBrace "{"
      MatchArm
        Literal
          IntegerLiteral "1"
        Comma ","
        Literal
          IntegerLiteral "2"
        FatArrow "=>"
        Literal
          SingleQuotedString "'low'"
      Comma ","
      MatchArm
        Default "default"
        FatArrow "=>"
        Literal
          SingleQuotedString "'other'"
      CloseBrace "}"
    "#);
}

#[test]
fn match_accepts_empty_bodies_and_trailing_commas() {
    assert!(parser_diagnostics("<?php match ($x) {};").is_empty());
    assert!(parser_diagnostics("<?php match ($x) { 1 => 'a', };").is_empty());
    assert!(parser_diagnostics("<?php match ($x) { 1, => 'a' };").is_empty());
}

#[test]
fn match_conditions_are_full_expressions() {
    assert!(
        parser_diagnostics("<?php match (true) { $age >= 18 => 'adult', default => 'minor' };")
            .is_empty()
    );
    assert!(parser_diagnostics("<?php $r = match ($x) { f($x) => g($x) };").is_empty());
}

#[test]
fn a_match_arm_missing_its_arrow_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php match ($x) { 1 'a' };")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::FatArrow))
    );
}

#[test]
fn garbage_inside_match_is_wrapped_and_the_tree_survives() {
    let parse = support::parse_verified("<?php match ($x) { => 'a', 2 => 'b' };");
    assert!(!parse.diagnostics().is_empty());
    assert!(
        parse
            .tree()
            .descendants()
            .any(|node| node.kind() == SyntaxKind::MatchExpression)
    );
}

#[test]
fn a_condition_list_trailing_comma_with_no_arrow_does_not_swallow_the_brace() {
    // `match ($x) { 1, }` has no `=>` at all: after the trailing comma,
    // `match_arm` used to force a parse of the arm list's own `}` as the
    // next condition, swallowing it into an `ErrorElement` and leaving a
    // spurious missing-`CloseBrace` diagnostic even though the brace was
    // right there in the source.
    let parse = support::parse_verified("<?php match ($x) { 1, };");
    let diagnostics = parse.diagnostics();
    assert!(
        diagnostics.len() <= 2,
        "recovery must stay bounded: {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace))
        )),
        "the arm list's closing brace must not be reported missing: {diagnostics:?}"
    );
    assert!(
        parse
            .tree()
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .any(|token| token.kind() == SyntaxKind::CloseBrace),
        "a CloseBrace token must survive in the tree, not be wrapped away"
    );
}

#[test]
fn pathological_nesting_inside_a_match_arm_condition_terminates() {
    // Deeply nested parentheses inside a match arm's condition list can
    // exhaust the nesting guard while `match_expression`'s own arm loop
    // is still active (the leftover `(` tokens surface through the
    // postfix loop at every unwinding level, not only at the top).
    // Without a mechanical progress guarantee in the arm loop itself,
    // this spins forever; the assertion that matters here is that this
    // test completes.
    let source = format!("<?php match ($x) {{ {}1 => 2 }};", "(".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}
