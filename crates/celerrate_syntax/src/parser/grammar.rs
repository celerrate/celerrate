//! The grammar rules of this plan: a source file over inline HTML,
//! tags, `echo` statements, and expression statements with minimal
//! primary expressions. Every rule keeps the parser's guarantees: it
//! always makes progress and completes the node it opened.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::{CompletedMarker, Parser};

pub(super) fn source_file(parser: &mut Parser) {
    let marker = parser.start();
    while let Some(kind) = parser.current() {
        match kind {
            SyntaxKind::InlineHtml
            | SyntaxKind::OpenTag
            | SyntaxKind::OpenTagEcho
            | SyntaxKind::ShortOpenTag
            | SyntaxKind::CloseTag => parser.bump(),
            _ => statement(parser),
        }
    }
    marker.complete(parser, SyntaxKind::SourceFile);
}

fn statement(parser: &mut Parser) {
    match parser.current() {
        Some(SyntaxKind::Echo) => echo_statement(parser),
        Some(kind) if is_expression_start(kind) => expression_statement(parser),
        Some(_) => error_statement(parser),
        None => {}
    }
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
        // Unreachable through `statement`'s dispatch, but recovery must
        // never leave an open marker or an empty node behind.
        marker.abandon(parser);
        return;
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::ExpressionStatement);
}

/// One token no rule accepts, wrapped and reported; the guaranteed
/// progress of the statement loop.
fn error_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.diagnose_current(ParserDiagnosticKind::UnexpectedToken);
    parser.bump();
    marker.complete(parser, SyntaxKind::ErrorNode);
}

/// PHP requires `;` after a statement except immediately before `?>`,
/// where it is optional (end of input does not exempt it: Zend rejects
/// that too, so we diagnose it — zero-width, after the last token).
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

fn is_expression_start(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IntegerLiteral
            | SyntaxKind::FloatLiteral
            | SyntaxKind::SingleQuotedString
            | SyntaxKind::Variable
    )
}

/// A minimal primary expression; the Pratt machinery of the next plan
/// replaces this dispatch.
fn expression(parser: &mut Parser) -> Option<CompletedMarker> {
    let kind = match parser.current() {
        Some(kind) if is_expression_start(kind) => kind,
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
            return None;
        }
    };
    let marker = parser.start();
    parser.bump();
    let node_kind = match kind {
        SyntaxKind::Variable => SyntaxKind::VariableReference,
        _ => SyntaxKind::Literal,
    };
    Some(marker.complete(parser, node_kind))
}
