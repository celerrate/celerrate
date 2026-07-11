//! Accessor plumbing shared by the generated code and the extensions.

use super::{AstChildren, AstNode};
use crate::syntax_kind::SyntaxKind;
use crate::tree::{SyntaxNode, SyntaxToken};

/// The `index`th typed child of type `N`, counted among `N` children
/// only (this is what makes positional same-type accessors correct on
/// partial trees: a missing later child is `None`, never a shift of an
/// earlier one).
pub(crate) fn child<N: AstNode>(parent: &SyntaxNode, index: usize) -> Option<N> {
    parent.children().filter_map(N::cast).nth(index)
}

pub(crate) fn children<N: AstNode>(parent: &SyntaxNode) -> AstChildren<N> {
    AstChildren::new(parent)
}

/// The first direct token child whose kind is one of `kinds`.
pub(crate) fn token(parent: &SyntaxNode, kinds: &[SyntaxKind]) -> Option<SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| kinds.contains(&token.kind()))
}
