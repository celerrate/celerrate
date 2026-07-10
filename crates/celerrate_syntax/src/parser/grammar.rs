//! The grammar's top level: the source file loop and the shared
//! statement-list step. Statements and expressions each own a module.

use crate::syntax_kind::SyntaxKind;

use super::{CompletedMarker, Marker, Parser};

mod declarations;
mod expressions;
mod statements;
mod types;

pub(super) fn source_file(parser: &mut Parser) {
    let marker = parser.start();
    while parser.current().is_some() {
        statement_list_step(parser);
    }
    marker.complete(parser, SyntaxKind::SourceFile);
}

/// One step of a statement list: inline HTML and tags stay tokens;
/// everything else is a statement. Shared by the source file and every
/// nested statement list.
pub(super) fn statement_list_step(parser: &mut Parser) {
    match parser.current() {
        Some(
            SyntaxKind::InlineHtml
            | SyntaxKind::OpenTag
            | SyntaxKind::OpenTagEcho
            | SyntaxKind::ShortOpenTag
            | SyntaxKind::CloseTag,
        ) => parser.bump(),
        _ => statements::statement(parser),
    }
}
