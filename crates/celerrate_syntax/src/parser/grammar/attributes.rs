//! Attribute groups: `#[Name(arguments), Other\Name]`. Groups precede
//! declarations, members, enum cases, parameters, property hooks,
//! closures, and anonymous classes; each host rule calls
//! [`attribute_groups`] with its node marker already open, so the
//! groups become leading children of the declaration they decorate.

use crate::syntax_kind::SyntaxKind;

use super::Parser;
use super::expressions::{argument_list, error_element, expect_list_separator, name};

/// Zero or more `#[...]` groups. Progress: every group bumps its `#[`.
pub(super) fn attribute_groups(parser: &mut Parser) {
    while parser.at(SyntaxKind::AttributeOpen) {
        attribute_group(parser);
    }
}

/// One `#[ ... ]` group. Same recovery contract as the expression
/// lists: unexpected tokens are wrapped and consumed; `;`, `?>`, and
/// end of input abort so a runaway group cannot swallow the file.
fn attribute_group(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `#[`
    while !parser.at(SyntaxKind::CloseBracket) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        if !matches!(
            parser.current(),
            Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
        ) {
            error_element(parser);
            continue;
        }
        attribute(parser);
        expect_list_separator(parser, SyntaxKind::CloseBracket);
    }
    parser.expect(SyntaxKind::CloseBracket);
    marker.complete(parser, SyntaxKind::AttributeGroup);
}

/// One attribute: a qualified name and optional arguments. Attribute
/// names are class names, never keywords.
fn attribute(parser: &mut Parser) {
    let marker = parser.start();
    name(parser);
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    }
    marker.complete(parser, SyntaxKind::Attribute);
}
