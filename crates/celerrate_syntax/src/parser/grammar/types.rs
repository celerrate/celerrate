//! The type grammar: nullable, union, intersection, and DNF forms,
//! transcribed from php-src's `zend_language_parser.y` (branch
//! PHP-8.5: `type_expr`, `union_type`, `intersection_type`). Which
//! compositions Zend accepts is a semantic judgment (`?A|B` and
//! `(A|B)&C` it rejects); the parser accepts every composition.
//! `starts_type` is deliberately narrower than `atomic_type`'s
//! any-keyword acceptance: it is a dispatch predicate (which callers
//! use to decide whether a type sits here at all), while `atomic_type`
//! itself, once dispatched into, parses resiliently and accepts any
//! keyword as a named type.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::expressions::name;
use super::{CompletedMarker, Parser};

/// Whether `kind` can start a type. `array`, `callable`, and `static`
/// are Zend's keyword types; `int`, `string`, `null`, `self`, and the
/// rest are plain identifiers.
pub(super) fn starts_type(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Question
            | SyntaxKind::OpenParenthesis
            | SyntaxKind::Identifier
            | SyntaxKind::Backslash
            | SyntaxKind::Namespace
            | SyntaxKind::Array
            | SyntaxKind::Callable
            | SyntaxKind::Static
    )
}

/// A full type expression: `?T`, `A|B`, `A&B`, `(A&B)|C`. Returns
/// `None` without consuming when no type can start here (diagnosed) or
/// when the nesting guard refuses; callers with loops position-guard
/// against the latter.
pub(super) fn type_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    if !parser.enter_nesting() {
        return None;
    }
    let result = union_type(parser);
    parser.leave_nesting();
    result
}

/// `A|B|C`, one flat node however long the chain. Terminates: every
/// iteration consumes its `|`.
fn union_type(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = intersection_type(parser)?;
    if parser.at(SyntaxKind::Pipe) {
        let marker = left.precede(parser);
        while parser.eat(SyntaxKind::Pipe) {
            // A missing member (`int|`) is diagnosed downstream; the
            // node still completes.
            intersection_type(parser);
        }
        left = marker.complete(parser, SyntaxKind::UnionType);
    }
    Some(left)
}

/// `A&B&C`, one flat node. Terminates: every iteration consumes its
/// `&`.
fn intersection_type(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = atomic_type(parser)?;
    if at_intersection_ampersand(parser) {
        let marker = left.precede(parser);
        while at_intersection_ampersand(parser) {
            parser.bump();
            atomic_type(parser);
        }
        left = marker.complete(parser, SyntaxKind::IntersectionType);
    }
    Some(left)
}

/// `&` continues an intersection only when a type can follow. In a
/// parameter list, the ampersand after a type is the by-reference
/// marker (`function f(A&$x)`, `function f(A&...$xs)`): it precedes a
/// variable or `...`, never a type start, so one token of lookahead
/// separates the readings.
fn at_intersection_ampersand(parser: &mut Parser) -> bool {
    parser.at(SyntaxKind::Ampersand) && parser.nth(1).is_some_and(starts_type)
}

fn atomic_type(parser: &mut Parser) -> Option<CompletedMarker> {
    match parser.current() {
        Some(SyntaxKind::Question) => {
            // `? ? T` recurses one level per token (`??` lexes as one
            // coalesce token, but spaced chains reach here), so the
            // nesting guard bounds the recursion like any other.
            if !parser.enter_nesting() {
                return None;
            }
            let marker = parser.start();
            parser.bump();
            atomic_type(parser);
            let completed = marker.complete(parser, SyntaxKind::NullableType);
            parser.leave_nesting();
            Some(completed)
        }
        Some(SyntaxKind::OpenParenthesis) => {
            if !parser.enter_nesting() {
                return None;
            }
            let marker = parser.start();
            parser.bump();
            type_expression(parser);
            parser.expect(SyntaxKind::CloseParenthesis);
            let completed = marker.complete(parser, SyntaxKind::ParenthesizedType);
            parser.leave_nesting();
            Some(completed)
        }
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace) => {
            let marker = parser.start();
            name(parser);
            Some(marker.complete(parser, SyntaxKind::NamedType))
        }
        Some(kind) if kind.is_keyword() => {
            // `array`, `callable`, `static` are Zend's keyword types;
            // any other keyword parses too and is judged upstairs.
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::NamedType))
        }
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedType);
            None
        }
    }
}
