//! The declaration grammar: `const`, `namespace`, `use` imports,
//! functions, and the class-likes with their members. Every rule takes
//! an already-open [`Marker`], and the [`declaration`] dispatcher owns
//! opening it, so attribute groups (a later task of this plan) can
//! open the node before the dispatch runs.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::expressions::{error_element, expect_list_separator, expression, name, parameter_list};
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
        Some(SyntaxKind::Use) => use_declaration(parser, marker),
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

/// `use A\B;`, `use A\B as C;`, `use function a\b;`,
/// `use const A\B;`, comma-separated clause lists, and the group form
/// `use A\{B, function c as d};`. Collisions and resolution are
/// semantic. Terminates: every iteration parses a clause that consumed
/// at least one name token, or breaks.
fn use_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `use`
    if matches!(
        parser.current(),
        Some(SyntaxKind::Function | SyntaxKind::Const)
    ) {
        parser.bump(); // the import type, for the whole clause list
    }
    loop {
        if !use_clause(parser) {
            break;
        }
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::UseDeclaration);
}

/// One top-level import clause: a name, then a group (`\{ ... }`) or
/// an optional alias. Returns false without consuming when no name can
/// start here.
fn use_clause(parser: &mut Parser) -> bool {
    if !matches!(
        parser.current(),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
    ) {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
        return false;
    }
    let marker = parser.start();
    name(parser);
    if parser.at(SyntaxKind::Backslash) && parser.nth(1) == Some(SyntaxKind::OpenBrace) {
        use_group(parser);
    } else {
        use_alias(parser);
    }
    marker.complete(parser, SyntaxKind::UseClause);
    true
}

/// `\{ B, function c as d, }`: the group of a grouped import. Same
/// recovery contract as the expression lists: unexpected tokens are
/// wrapped and consumed; `;`, `?>`, and end of input abort. The shared
/// separator helper tolerates the trailing comma Zend allows here.
/// Terminates: every iteration consumes through `use_group_item` (its
/// dispatch admits only kinds that item always bumps) or through
/// `error_element`.
fn use_group(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `\`
    parser.bump(); // `{`
    while !parser.at(SyntaxKind::CloseBrace) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        if !matches!(
            parser.current(),
            Some(
                SyntaxKind::Function
                    | SyntaxKind::Const
                    | SyntaxKind::Identifier
                    | SyntaxKind::Backslash
                    | SyntaxKind::Namespace
            )
        ) {
            error_element(parser);
            continue;
        }
        use_group_item(parser);
        expect_list_separator(parser, SyntaxKind::CloseBrace);
    }
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::UseGroup);
}

/// One item of a group: an optional per-item `function`/`const` type,
/// the name, an optional alias. Always consumes at least one token:
/// the group loop admitted only its leading kinds.
fn use_group_item(parser: &mut Parser) {
    let marker = parser.start();
    if matches!(
        parser.current(),
        Some(SyntaxKind::Function | SyntaxKind::Const)
    ) {
        parser.bump();
    }
    if matches!(
        parser.current(),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
    ) {
        name(parser);
        use_alias(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
    }
    marker.complete(parser, SyntaxKind::UseClause);
}

/// `as Alias`. Any keyword is accepted as the alias; validity is
/// semantic.
fn use_alias(parser: &mut Parser) {
    if !parser.eat(SyntaxKind::As) {
        return;
    }
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => parser.bump(),
        _ => parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
}
