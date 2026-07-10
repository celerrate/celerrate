#![allow(dead_code)]

use crate::syntax_kind::SyntaxKind;

/// One step of tree construction, recorded by the parser and replayed
/// by the builder. The parser never touches the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    /// Opens a node. `kind: None` is a tombstone: an abandoned or
    /// already-replayed start, skipped by the builder. `forward_parent`
    /// chains to a later `Start` that must open **before** this one
    /// (absolute index into the event buffer).
    Start {
        kind: Option<SyntaxKind>,
        forward_parent: Option<usize>,
    },
    /// Consumes the next significant token.
    Token,
    /// Closes the innermost open node.
    Finish,
}

impl Event {
    pub(crate) fn tombstone() -> Self {
        Self::Start {
            kind: None,
            forward_parent: None,
        }
    }
}
