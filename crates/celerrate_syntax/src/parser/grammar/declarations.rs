//! The declaration grammar: `const`, `namespace`, `use` imports,
//! functions, and the class-likes with their members. Every rule takes
//! an already-open [`Marker`], and the [`declaration`] dispatcher owns
//! opening it, so attribute groups (a later task of this plan) can
//! open the node before the dispatch runs.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::expressions::{expression, name, parameter_list};
use super::statements::{block, terminate_statement};
use super::{Marker, Parser};

/// Dispatch for declaration statements. The statement dispatcher only
/// routes here on tokens it has already vetted (with lookahead where a
/// keyword is overloaded), so the fallback arm is defensive: it is
/// reachable only behind consumed tokens (attribute groups, from a
/// later task), never at a standstill.
pub(super) fn declaration(parser: &mut Parser) {
    let marker = parser.start();
    match parser.current() {
        Some(SyntaxKind::Function) if at_function_declaration(parser) => {
            function_declaration(parser, marker);
        }
        Some(SyntaxKind::Const) => constant_declaration(parser, marker),
        Some(SyntaxKind::Namespace) if parser.nth(1) != Some(SyntaxKind::Backslash) => {
            namespace_declaration(parser, marker);
        }
        Some(_) => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedDeclaration);
            marker.complete(parser, SyntaxKind::ErrorNode);
        }
        None => marker.abandon(parser),
    }
}

/// `function` declares only when a name follows, with an optional `&`
/// between: `function (` and `function &(` are closure expressions.
pub(super) fn at_function_declaration(parser: &mut Parser) -> bool {
    match parser.nth(1) {
        Some(SyntaxKind::Identifier) => true,
        Some(SyntaxKind::Ampersand) => parser.nth(2) == Some(SyntaxKind::Identifier),
        _ => false,
    }
}

fn function_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `function`
    parser.eat(SyntaxKind::Ampersand); // by-reference return
    parser.expect(SyntaxKind::Identifier);
    parameter_list(parser);
    if parser.eat(SyntaxKind::Colon) {
        super::types::type_expression(parser);
    }
    block(parser);
    marker.complete(parser, SyntaxKind::FunctionDeclaration);
}

/// `const FOO = 1, BAR = 2;`, optionally typed since 8.3
/// (`const int FOO = 1;`). The type is absent exactly when the next
/// token is a name directly followed by `=`. Constant names accept any
/// keyword (semi-reserved: `const FOR = 1;`); which names and types
/// are legal where is semantic. Also the class-constant rule: the
/// member path parses its modifiers into `marker` first.
///
/// Terminates: every iteration bumps a name or breaks.
pub(super) fn constant_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `const`
    let at_untyped_name = parser
        .current()
        .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword())
        && parser.nth(1) == Some(SyntaxKind::Equals);
    if !at_untyped_name && parser.current().is_some_and(super::types::starts_type) {
        super::types::type_expression(parser);
    }
    loop {
        let at_name = parser
            .current()
            .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword());
        if !at_name {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
            break;
        }
        let element = parser.start();
        parser.bump();
        parser.expect(SyntaxKind::Equals);
        expression(parser);
        element.complete(parser, SyntaxKind::ConstantElement);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::ConstantDeclaration);
}

/// `namespace A\B;`, `namespace A\B { ... }`, `namespace { ... }`.
/// Only dispatched when the next token is not `\` (`namespace\Foo` is
/// a name expression). Where namespaces may appear and nest is
/// semantic.
fn namespace_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `namespace`
    if parser.at(SyntaxKind::Identifier) {
        name(parser);
    }
    if parser.at(SyntaxKind::OpenBrace) {
        block(parser);
    } else {
        terminate_statement(parser);
    }
    marker.complete(parser, SyntaxKind::NamespaceDeclaration);
}
