//! The statement grammar: dispatch over the leading token, one rule
//! per statement form. Every rule keeps the parser's guarantees: it
//! always makes progress and completes the node it opened.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::Parser;
use super::expressions::{
    argument_list, error_element, expression, name, simple_variable, starts_expression,
};

pub(super) fn statement(parser: &mut Parser) {
    if !parser.enter_nesting() {
        // The budget refused the whole statement without consuming;
        // wrap one token so every enclosing statement list keeps
        // progressing (the same contract the expression lists use).
        if parser.current().is_some() {
            error_element(parser);
        }
        return;
    }
    dispatch_statement(parser);
    parser.leave_nesting();
}

fn dispatch_statement(parser: &mut Parser) {
    match parser.current() {
        Some(SyntaxKind::OpenBrace) => block(parser),
        Some(SyntaxKind::Semicolon) => empty_statement(parser),
        Some(SyntaxKind::Echo) => echo_statement(parser),
        Some(SyntaxKind::Return) => {
            keyword_optional_expression_statement(parser, SyntaxKind::ReturnStatement)
        }
        Some(SyntaxKind::Break) => {
            keyword_optional_expression_statement(parser, SyntaxKind::BreakStatement)
        }
        Some(SyntaxKind::Continue) => {
            keyword_optional_expression_statement(parser, SyntaxKind::ContinueStatement)
        }
        Some(SyntaxKind::Global) => global_statement(parser),
        Some(SyntaxKind::Unset) => unset_statement(parser),
        Some(SyntaxKind::Goto) => goto_statement(parser),
        Some(SyntaxKind::If) => if_statement(parser),
        Some(SyntaxKind::While) => while_statement(parser),
        Some(SyntaxKind::Do) => do_while_statement(parser),
        Some(SyntaxKind::For) => for_statement(parser),
        Some(SyntaxKind::Foreach) => foreach_statement(parser),
        Some(SyntaxKind::Switch) => switch_statement(parser),
        Some(SyntaxKind::Try) => try_statement(parser),
        Some(SyntaxKind::Declare) => declare_statement(parser),
        Some(SyntaxKind::Static) if parser.nth(1) == Some(SyntaxKind::Variable) => {
            static_statement(parser)
        }
        Some(SyntaxKind::Identifier) if parser.nth(1) == Some(SyntaxKind::Colon) => {
            label_statement(parser)
        }
        Some(SyntaxKind::Function) if super::declarations::at_function_declaration(parser) => {
            super::declarations::declaration(parser)
        }
        Some(SyntaxKind::Const) => super::declarations::declaration(parser),
        Some(SyntaxKind::Namespace) if parser.nth(1) != Some(SyntaxKind::Backslash) => {
            super::declarations::declaration(parser)
        }
        Some(SyntaxKind::Use) => super::declarations::declaration(parser),
        Some(
            SyntaxKind::Class
            | SyntaxKind::Interface
            | SyntaxKind::Trait
            | SyntaxKind::Abstract
            | SyntaxKind::Final,
        ) => super::declarations::declaration(parser),
        Some(SyntaxKind::Readonly) if parser.nth(1) != Some(SyntaxKind::OpenParenthesis) => {
            super::declarations::declaration(parser)
        }
        Some(kind) if starts_expression(kind) => expression_statement(parser),
        Some(_) => error_statement(parser),
        None => {}
    }
}

/// A lone `;`: a complete statement in PHP.
fn empty_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump();
    marker.complete(parser, SyntaxKind::EmptyStatement);
}

/// Statements until end of input or a statement-list terminator. The
/// guard observes `current` (not `at_end`): once the fuse blows,
/// `current` reports `None` while real unconsumed tokens can still sit
/// past the raw position, and this loop must unwind, not spin. The
/// test module below drives exactly that state. Every nested list
/// (blocks included) stops at any terminator in the shared set, not
/// just its own closer: the construct that owns a terminator consumes
/// it, and an orphan unwinds to the source-file loop.
pub(super) fn statement_list(parser: &mut Parser) {
    while parser.current().is_some() && !at_statement_list_terminator(parser) {
        super::statement_list_step(parser);
    }
}

/// `{ statements }`.
pub(super) fn block(parser: &mut Parser) {
    let marker = parser.start();
    if parser.at(SyntaxKind::OpenBrace) {
        parser.bump();
        statement_list(parser);
        parser.expect(SyntaxKind::CloseBrace);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::Block);
}

fn echo_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump();
    loop {
        if expression(parser).is_none() {
            break;
        }
        if parser.at(SyntaxKind::Comma) {
            parser.bump();
        } else {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::EchoStatement);
}

/// A keyword, an optional expression, the terminator: `return`,
/// `break`, and `continue` share the shape.
fn keyword_optional_expression_statement(parser: &mut Parser, kind: SyntaxKind) {
    let marker = parser.start();
    parser.bump();
    if parser.current().is_some_and(starts_expression) {
        expression(parser);
    }
    terminate_statement(parser);
    marker.complete(parser, kind);
}

fn expression_statement(parser: &mut Parser) {
    let marker = parser.start();
    if expression(parser).is_none() {
        // The dispatcher saw an expression start but the grammar
        // refused. The refusal already carried its diagnostic; consume
        // the token so the statement loop always advances.
        if parser.at_end() {
            marker.abandon(parser);
            return;
        }
        parser.bump();
        marker.complete(parser, SyntaxKind::ErrorNode);
        return;
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::ExpressionStatement);
}

/// One token no rule accepts, wrapped and reported; the guaranteed
/// progress of the statement loop.
fn error_statement(parser: &mut Parser) {
    error_element(parser);
}

/// PHP requires `;` after a statement except immediately before `?>`,
/// where it is optional. End of input does not exempt it (Zend rejects
/// that too), so we diagnose it: zero-width, after the last token.
pub(super) fn terminate_statement(parser: &mut Parser) {
    if parser.at(SyntaxKind::Semicolon) {
        parser.bump();
        return;
    }
    if parser.at(SyntaxKind::CloseTag) {
        return;
    }
    parser.diagnose_missing(ParserDiagnosticKind::ExpectedSemicolon);
}

/// `global $a, $$b;`. Terminates: each iteration either consumed a
/// variable form or breaks; the comma is consumed before looping.
fn global_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `global`
    loop {
        if simple_variable(parser).is_none() {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
            break;
        }
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::GlobalStatement);
}

/// `static $a = 1, $b;`: dispatched only when `static` is directly
/// followed by a variable; `static::`, `static function`, and
/// `static fn` stay expressions.
fn static_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `static`
    loop {
        if !parser.at(SyntaxKind::Variable) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
            break;
        }
        let variable = parser.start();
        parser.bump();
        if parser.eat(SyntaxKind::Equals) {
            expression(parser);
        }
        variable.complete(parser, SyntaxKind::StaticVariable);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::StaticStatement);
}

/// `unset( targets );`. The shared argument list brings its recovery;
/// which targets are unsettable is semantic.
fn unset_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `unset`
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::UnsetStatement);
}

fn goto_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `goto`
    parser.expect(SyntaxKind::Identifier);
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::GotoStatement);
}

/// The dispatcher guaranteed the identifier-colon shape.
fn label_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // the label
    parser.bump(); // `:`
    marker.complete(parser, SyntaxKind::LabelStatement);
}

/// The tokens that end a nested statement list. A nested list never
/// consumes one of these: the construct that owns it eats it, and an
/// orphan unwinds to the source-file loop, which consumes anything.
/// This is the plan's contextual recovery set, and the reason every
/// nested list terminates: unwinding consumes nothing, but the top
/// level always progresses.
fn at_statement_list_terminator(parser: &mut Parser) -> bool {
    matches!(
        parser.current(),
        Some(
            SyntaxKind::CloseBrace
                | SyntaxKind::EndIf
                | SyntaxKind::EndWhile
                | SyntaxKind::EndFor
                | SyntaxKind::EndForeach
                | SyntaxKind::EndSwitch
                | SyntaxKind::EndDeclare
                | SyntaxKind::Else
                | SyntaxKind::ElseIf
                | SyntaxKind::Case
                | SyntaxKind::Default
        )
    )
}

/// `( expression )` after a control-flow keyword. The tokens stay
/// flat under the statement node, like `match`'s subject.
fn parenthesized_condition(parser: &mut Parser) {
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        expression(parser);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
}

/// The single embedded statement of a control-flow body
/// (`if (c) body`). A statement-list terminator or end of input means
/// the body is missing: diagnosed, never consumed, so the enclosing
/// construct recovers its own closer.
fn embedded_statement(parser: &mut Parser) {
    if parser.current().is_none() || at_statement_list_terminator(parser) {
        parser.diagnose_missing(ParserDiagnosticKind::ExpectedStatement);
        return;
    }
    super::statement_list_step(parser);
}

/// The alternative-syntax body shared by `while`, `for`, `foreach`,
/// and `declare` once their `:` is consumed: the statement list, the
/// closing keyword, the statement terminator. `if` and `switch` place
/// clauses or case sections between the list and the closer, so they
/// keep their own sequences.
fn alternative_body(parser: &mut Parser, closing: SyntaxKind) {
    statement_list(parser);
    parser.expect(closing);
    terminate_statement(parser);
}

fn if_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `if`
    parenthesized_condition(parser);
    if parser.eat(SyntaxKind::Colon) {
        statement_list(parser);
        while parser.at(SyntaxKind::ElseIf) {
            let clause = parser.start();
            parser.bump();
            parenthesized_condition(parser);
            parser.expect(SyntaxKind::Colon);
            statement_list(parser);
            clause.complete(parser, SyntaxKind::ElseIfClause);
        }
        if parser.at(SyntaxKind::Else) {
            let clause = parser.start();
            parser.bump();
            parser.expect(SyntaxKind::Colon);
            statement_list(parser);
            clause.complete(parser, SyntaxKind::ElseClause);
        }
        parser.expect(SyntaxKind::EndIf);
        terminate_statement(parser);
    } else {
        embedded_statement(parser);
        while parser.at(SyntaxKind::ElseIf) {
            let clause = parser.start();
            parser.bump();
            parenthesized_condition(parser);
            embedded_statement(parser);
            clause.complete(parser, SyntaxKind::ElseIfClause);
        }
        if parser.at(SyntaxKind::Else) {
            let clause = parser.start();
            parser.bump();
            embedded_statement(parser);
            clause.complete(parser, SyntaxKind::ElseClause);
        }
    }
    marker.complete(parser, SyntaxKind::IfStatement);
}

fn while_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `while`
    parenthesized_condition(parser);
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndWhile);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::WhileStatement);
}

fn do_while_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `do`
    embedded_statement(parser);
    parser.expect(SyntaxKind::While);
    parenthesized_condition(parser);
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::DoWhileStatement);
}

fn for_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `for`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        for_expression_list(parser);
        parser.expect(SyntaxKind::Semicolon);
        for_expression_list(parser);
        parser.expect(SyntaxKind::Semicolon);
        for_expression_list(parser);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndFor);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::ForStatement);
}

/// One `for` section. Progress is enforced mechanically: the nesting
/// guard can refuse an expression without consuming (this loop can be
/// entered while the budget is exhausted, through a pathological
/// condition chain); an unmoved position breaks out and leaves the
/// token to the section's caller.
fn for_expression_list(parser: &mut Parser) {
    let marker = parser.start();
    while parser.current().is_some_and(starts_expression) {
        let position_before_expression = parser.position();
        expression(parser);
        if parser.position() == position_before_expression {
            break;
        }
        if !parser.eat(SyntaxKind::Comma) && parser.current().is_some_and(starts_expression) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Comma));
        }
    }
    marker.complete(parser, SyntaxKind::ForExpressionList);
}

fn foreach_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `foreach`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        expression(parser);
        parser.expect(SyntaxKind::As);
        foreach_target(parser);
        if parser.eat(SyntaxKind::FatArrow) {
            foreach_target(parser);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndForeach);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::ForeachStatement);
}

/// One binding target: optional `&`, then an expression (a variable,
/// `[...]`/`list(...)` destructuring, a member chain). `=>` is not a
/// binary operator, so the expression stops before it. Assignability
/// is semantic.
fn foreach_target(parser: &mut Parser) {
    parser.eat(SyntaxKind::Ampersand);
    expression(parser);
}

fn switch_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `switch`
    parenthesized_condition(parser);
    if parser.eat(SyntaxKind::OpenBrace) {
        // Zend tolerates one stray `;` before the first case.
        parser.eat(SyntaxKind::Semicolon);
        switch_case_list(parser, SyntaxKind::CloseBrace);
        parser.expect(SyntaxKind::CloseBrace);
    } else if parser.eat(SyntaxKind::Colon) {
        parser.eat(SyntaxKind::Semicolon);
        switch_case_list(parser, SyntaxKind::EndSwitch);
        parser.expect(SyntaxKind::EndSwitch);
        terminate_statement(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::SwitchStatement);
}

/// Sections until `closing`. Terminates: the case and default arms
/// always bump their keyword before anything refusable, and the
/// fallback arm is an error element, which always bumps.
fn switch_case_list(parser: &mut Parser, closing: SyntaxKind) {
    while parser.current().is_some() && !parser.at(closing) {
        match parser.current() {
            Some(SyntaxKind::Case) => {
                let case = parser.start();
                parser.bump();
                expression(parser);
                switch_case_separator(parser);
                statement_list(parser);
                case.complete(parser, SyntaxKind::SwitchCase);
            }
            Some(SyntaxKind::Default) => {
                let case = parser.start();
                parser.bump();
                switch_case_separator(parser);
                statement_list(parser);
                case.complete(parser, SyntaxKind::SwitchCase);
            }
            _ => error_element(parser),
        }
    }
}

/// `:` or the Zend-legal `;` after a case label.
fn switch_case_separator(parser: &mut Parser) {
    if !parser.eat(SyntaxKind::Colon) && !parser.eat(SyntaxKind::Semicolon) {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Colon));
    }
}

fn try_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `try`
    block(parser);
    let mut caught = false;
    while parser.at(SyntaxKind::Catch) {
        caught = true;
        catch_clause(parser);
    }
    if parser.at(SyntaxKind::Finally) {
        let clause = parser.start();
        parser.bump();
        block(parser);
        clause.complete(parser, SyntaxKind::FinallyClause);
    } else if !caught {
        // Zend rejects `try` without a single catch or finally.
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Catch));
    }
    marker.complete(parser, SyntaxKind::TryStatement);
}

fn catch_clause(parser: &mut Parser) {
    let clause = parser.start();
    parser.bump(); // `catch`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        // `A | B\C`: qualified names separated by pipes. `name`
        // self-recovers (diagnoses a missing identifier) on absence,
        // and each loop iteration consumed its pipe: progress holds.
        name(parser);
        while parser.eat(SyntaxKind::Pipe) {
            name(parser);
        }
        if parser.at(SyntaxKind::Variable) {
            let variable = parser.start();
            parser.bump();
            variable.complete(parser, SyntaxKind::VariableReference);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    block(parser);
    clause.complete(parser, SyntaxKind::CatchClause);
}

fn declare_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `declare`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        // Terminates: each iteration bumps the directive's identifier
        // before anything refusable, or breaks.
        loop {
            if !parser.at(SyntaxKind::Identifier) {
                parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
                break;
            }
            let directive = parser.start();
            parser.bump();
            parser.expect(SyntaxKind::Equals);
            expression(parser);
            directive.complete(parser, SyntaxKind::DeclareDirective);
            if !parser.eat(SyntaxKind::Comma) {
                break;
            }
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndDeclare);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::DeclareStatement);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use crate::diagnostic::ParserDiagnosticKind;
    use crate::parser::Parser;
    use crate::parser::token_source::TokenSource;
    use crate::syntax_kind::SyntaxKind;
    use crate::tree::SyntaxNode;

    #[test]
    fn a_blown_fuse_terminates_a_statement_list_with_tokens_remaining() {
        // The historical bug: a statement-list loop gated on `at_end`
        // instead of observing `current` spins forever once the fuse
        // blows, because the fuse silences `current` without moving
        // the raw token position. This drives the real
        // `statement_list` in exactly that state; a regression hangs
        // this test rather than failing it.
        let source = "<?php { 1; 2; 3; }";
        let (tokens, _lexer_diagnostics) = crate::lexer::lex(source);
        let mut parser = Parser::new(TokenSource::new(&tokens));
        let root = parser.start();
        parser.bump(); // `<?php`
        parser.bump(); // `{`
        for _ in 0..=Parser::MAXIMUM_STEPS_WITHOUT_PROGRESS {
            parser.current();
        }
        assert_eq!(parser.current(), None, "the fuse must have blown");
        assert!(!parser.at_end(), "real tokens must still remain unconsumed");
        super::statement_list(&mut parser);
        parser.expect(SyntaxKind::CloseBrace);
        root.complete(&mut parser, SyntaxKind::SourceFile);
        parser.recover_unconsumed_tail();
        let tree = SyntaxNode::new_root(crate::tree::builder::build_tree(
            source,
            &tokens,
            parser.events,
        ));
        assert_eq!(
            tree.text().to_string(),
            source,
            "the tree must stay lossless even after the fuse blows mid-list"
        );
        let no_progress_count = parser
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == ParserDiagnosticKind::NoProgress)
            .count();
        assert_eq!(no_progress_count, 1, "exactly one NoProgress diagnostic");
    }

    #[test]
    fn a_refused_expression_start_is_wrapped_and_consumed() {
        // `expression_statement`'s refusal branch: `expression()` can
        // return `None` on a token the dispatcher vetted as an
        // expression start (bare `namespace` here: the primary rule
        // requires a following `\`). The branch must keep the
        // statement loop advancing: diagnose, consume exactly that one
        // token, and wrap it in an `ErrorNode`. No dispatcher arm
        // currently routes such a token here (bare `namespace` now
        // dispatches as a namespace declaration), so this drives the
        // rule directly; the enum dispatch of a later task makes the
        // branch reachable from source text again.
        let source = "<?php namespace + 1;";
        let (tokens, _lexer_diagnostics) = crate::lexer::lex(source);
        let mut parser = Parser::new(TokenSource::new(&tokens));
        let root = parser.start();
        parser.bump(); // `<?php`
        super::expression_statement(&mut parser);
        assert_eq!(
            parser.current(),
            Some(SyntaxKind::Plus),
            "exactly the refused `namespace` token must be consumed"
        );
        assert!(
            parser
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == ParserDiagnosticKind::ExpectedExpression),
            "the refusal must carry its ExpectedExpression diagnostic"
        );
        root.complete(&mut parser, SyntaxKind::SourceFile);
        parser.recover_unconsumed_tail();
        let tree = SyntaxNode::new_root(crate::tree::builder::build_tree(
            source,
            &tokens,
            parser.events,
        ));
        assert_eq!(
            tree.text().to_string(),
            source,
            "the tree must stay lossless around the wrapped token"
        );
        assert!(
            tree.children()
                .any(|node| node.kind() == SyntaxKind::ErrorNode
                    && node.text().to_string().contains("namespace")),
            "the refused token must sit wrapped in an ErrorNode"
        );
    }
}
