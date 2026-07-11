//! The typed AST: typed, zero-cost views over the concrete syntax
//! tree. The structs, enums, and accessors are generated from
//! `php.ungram` (`generated.rs`, `cargo xtask codegen`); logic a
//! generator cannot express (semi-reserved names, position-dependent
//! roles) is hand-written in `extensions.rs`. Every accessor returns
//! `Option` or an iterator: the partial trees error recovery produces
//! are normal citizens, not special cases.

mod generated;
pub(crate) mod support;

pub use generated::*;

use std::marker::PhantomData;

use crate::syntax_kind::SyntaxKind;
use crate::tree::{PhpLanguage, SyntaxNode};

/// A typed view over a syntax node of a known kind. Views are cheap:
/// one red-node handle, no copying.
pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(syntax: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

/// The typed children of one node, in source order; children of other
/// types (and wreckage) are skipped.
#[derive(Debug, Clone)]
pub struct AstChildren<N> {
    inner: rowan::SyntaxNodeChildren<PhpLanguage>,
    node_type: PhantomData<N>,
}

impl<N> AstChildren<N> {
    pub(crate) fn new(parent: &SyntaxNode) -> Self {
        AstChildren {
            inner: parent.children(),
            node_type: PhantomData,
        }
    }
}

impl<N: AstNode> Iterator for AstChildren<N> {
    type Item = N;

    fn next(&mut self) -> Option<N> {
        self.inner.by_ref().find_map(N::cast)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{AstNode, support};
    use crate::syntax_kind::SyntaxKind;
    use crate::tree::SyntaxNode;

    // Hand-rolled views: the generated code arrives in the next task;
    // the infrastructure must already carry any conforming type.
    struct EchoStatementView {
        syntax: SyntaxNode,
    }

    impl AstNode for EchoStatementView {
        fn can_cast(kind: SyntaxKind) -> bool {
            kind == SyntaxKind::EchoStatement
        }
        fn cast(syntax: SyntaxNode) -> Option<Self> {
            Self::can_cast(syntax.kind()).then(|| Self { syntax })
        }
        fn syntax(&self) -> &SyntaxNode {
            &self.syntax
        }
    }

    struct LiteralView {
        syntax: SyntaxNode,
    }

    impl AstNode for LiteralView {
        fn can_cast(kind: SyntaxKind) -> bool {
            kind == SyntaxKind::Literal
        }
        fn cast(syntax: SyntaxNode) -> Option<Self> {
            Self::can_cast(syntax.kind()).then(|| Self { syntax })
        }
        fn syntax(&self) -> &SyntaxNode {
            &self.syntax
        }
    }

    #[test]
    fn casting_children_and_tokens_work_over_a_real_parse() {
        let parse = crate::parse::parse("<?php echo 1, 2;");
        let root = parse.tree();
        let echo: EchoStatementView = support::child(&root, 0).expect("an echo statement");
        let literals: Vec<LiteralView> = support::children(echo.syntax()).collect();
        assert_eq!(literals.len(), 2, "typed children skip tokens and trivia");
        let second: LiteralView = support::child(echo.syntax(), 1).expect("a second literal");
        assert_eq!(second.syntax().text().to_string(), "2");
        let keyword = support::token(echo.syntax(), &[SyntaxKind::Echo]).expect("the echo keyword");
        assert_eq!(keyword.text(), "echo");
        assert!(
            LiteralView::cast(parse.tree()).is_none(),
            "a kind mismatch refuses the cast"
        );
    }
}
