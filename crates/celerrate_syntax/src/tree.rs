//! The tree layer: rowan wrapped behind crate-owned types. Upper crates
//! import these aliases; no bare rowan type appears in any public
//! signature.

pub(crate) mod builder;

use crate::syntax_kind::SyntaxKind;

/// PHP for rowan: ties [`SyntaxKind`] to the untyped green tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PhpLanguage {}

impl rowan::Language for PhpLanguage {
    type Kind = SyntaxKind;

    /// Total and panic-free: every raw kind inside a tree was produced
    /// by `kind_to_raw`, so the fallback is unreachable in practice.
    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from_raw(raw.0).unwrap_or(SyntaxKind::Error)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.into_raw())
    }
}

/// A red-tree node of the PHP syntax tree.
pub type SyntaxNode = rowan::SyntaxNode<PhpLanguage>;
/// A red-tree token of the PHP syntax tree.
pub type SyntaxToken = rowan::SyntaxToken<PhpLanguage>;
/// A node or a token.
pub type SyntaxElement = rowan::SyntaxElement<PhpLanguage>;

/// A stable, lightweight pointer to a node: its kind and range, no
/// tree handle. Upper layers (the salsa database) key derived data
/// with it instead of holding red nodes.
pub type SyntaxNodePtr = rowan::ast::SyntaxNodePtr<PhpLanguage>;

#[cfg(test)]
mod tests {
    //! `expect` is fine here: failing loudly is what a test should do.
    #![allow(clippy::expect_used)]

    use rowan::Language as _;

    use super::{PhpLanguage, SyntaxNode};
    use crate::syntax_kind::SyntaxKind;

    #[test]
    fn kinds_roundtrip_through_the_language() {
        let raw = PhpLanguage::kind_to_raw(SyntaxKind::EchoStatement);
        assert_eq!(PhpLanguage::kind_from_raw(raw), SyntaxKind::EchoStatement);
    }

    #[test]
    fn a_typed_tree_preserves_text_and_kinds() {
        let mut builder = rowan::GreenNodeBuilder::new();
        builder.start_node(PhpLanguage::kind_to_raw(SyntaxKind::SourceFile));
        builder.token(
            PhpLanguage::kind_to_raw(SyntaxKind::InlineHtml),
            "<p>hi</p>",
        );
        builder.finish_node();
        let tree = SyntaxNode::new_root(builder.finish());
        assert_eq!(tree.kind(), SyntaxKind::SourceFile);
        assert_eq!(tree.text(), "<p>hi</p>");
    }

    #[test]
    fn a_node_pointer_resolves_back_to_its_node() {
        let parse = crate::parse::parse("<?php echo 1;");
        let root = parse.tree();
        let statement = root.first_child().expect("a first statement");
        let pointer = super::SyntaxNodePtr::new(&statement);
        assert_eq!(pointer.to_node(&root), statement);
    }
}
