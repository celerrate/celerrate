//! The statement grammar: dispatch over the leading token, one rule
//! per statement form. Every rule keeps the parser's guarantees: it
//! always makes progress and completes the node it opened.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::Parser;
use super::expressions::{error_element, expression, starts_expression};

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

/// Statements until end of input or the list's closing brace. The
/// guard observes `current` (not `at_end`): once the fuse blows,
/// `current` reports `None` while real unconsumed tokens can still sit
/// past the raw position, and this loop must unwind, not spin. The
/// test module below drives exactly that state.
pub(super) fn statement_list(parser: &mut Parser) {
    while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
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
