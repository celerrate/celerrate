//! The statement grammar: dispatch over the leading token, one rule
//! per statement form. Every rule keeps the parser's guarantees: it
//! always makes progress and completes the node it opened.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::Parser;
use super::expressions::{
    argument_list, error_element, expression, simple_variable, starts_expression,
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
        Some(SyntaxKind::Static) if parser.nth(1) == Some(SyntaxKind::Variable) => {
            static_statement(parser)
        }
        Some(SyntaxKind::Identifier) if parser.nth(1) == Some(SyntaxKind::Colon) => {
            label_statement(parser)
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
        // refused (for example `namespace` without `\`). The refusal
        // already carried its diagnostic; consume the token so the
        // statement loop always advances.
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
fn terminate_statement(parser: &mut Parser) {
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
}
