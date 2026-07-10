//! Replays parser events against the full token stream, reinserting
//! trivia, and materializes the green tree.
//!
//! Trivia attachment (spec section 2): pending trivia flush just before
//! the next node or token starts, into the node open at that point, so
//! trivia sit between siblings and a node's range starts at its first
//! significant token. Losslessness is structural (every raw token is
//! pushed exactly once, in order), and leftovers flush into the root
//! before it closes; a parser bug can cost tree shape, never text.
//!
//! rowan requires exactly one top-level child when `finish()` runs and
//! panics on an unmatched `finish_node()`. A parser bug (or, defensively,
//! any malformed event stream) must never turn into a panic here, so the
//! replay tolerates structurally invalid input: a `Finish` with nothing
//! open is skipped, a `Token` with nothing open is left for a later
//! flush, a `Start` after the root has already closed is skipped (its
//! forward-parent chain included), nodes still open at the end are
//! flushed and closed, and an event stream that never opened anything
//! still gets wrapped in a synthetic `SourceFile` root.

use celerrate_source::TextSize;
use rowan::{GreenNode, GreenNodeBuilder, Language as _};

use crate::parser::Event;
use crate::syntax_kind::SyntaxKind;
use crate::token::Token;
use crate::tree::PhpLanguage;

pub(crate) fn build_tree(source: &str, tokens: &[Token], mut events: Vec<Event>) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    let mut raw = RawTokens {
        source,
        tokens,
        index: 0,
        offset: TextSize::from(0),
    };
    let mut depth = 0usize;
    // Set the first time a node opens and never cleared; `root_opened && depth == 0`
    // means the single root rowan allows has already closed.
    let mut root_opened = false;
    let mut forward_kinds = Vec::new();
    for index in 0..events.len() {
        match take_event(&mut events, index) {
            Event::Start { kind: None, .. } => {}
            Event::Start {
                kind: Some(kind),
                forward_parent,
            } => {
                let root_closed = root_opened && depth == 0;
                // Collect the forward-parent chain: each target must
                // open before the node that points at it, so the chain
                // is replayed outermost-first (reverse collection
                // order). Taking each target tombstones it at its own
                // position, even when the chain itself is discarded
                // below, so a later loop iteration cannot replay it.
                if !root_closed {
                    forward_kinds.push(kind);
                }
                let mut next = forward_parent;
                while let Some(target) = next {
                    next = None;
                    if let Event::Start {
                        kind,
                        forward_parent,
                    } = take_event(&mut events, target)
                    {
                        if !root_closed && let Some(kind) = kind {
                            forward_kinds.push(kind);
                        }
                        next = forward_parent;
                    }
                }
                if root_closed {
                    continue;
                }
                // The root opens before any token: nothing to flush and
                // nowhere to put trivia yet.
                if depth > 0 {
                    raw.flush_trivia(&mut builder);
                }
                for kind in forward_kinds.drain(..).rev() {
                    builder.start_node(PhpLanguage::kind_to_raw(kind));
                    depth += 1;
                }
                root_opened = true;
            }
            Event::Token => {
                // No node is open to receive this token: leave the raw
                // cursor untouched so the token flushes later instead of
                // rowan recording it as a stray top-level child.
                if depth > 0 {
                    raw.flush_trivia(&mut builder);
                    raw.push_next(&mut builder);
                }
            }
            Event::Finish => {
                // No node is open to close: skip rather than calling
                // rowan's `finish_node`, which panics with none open.
                if depth == 0 {
                    continue;
                }
                if depth == 1 {
                    raw.flush_remaining(&mut builder);
                }
                builder.finish_node();
                depth -= 1;
            }
        }
    }
    if depth > 0 {
        // Nodes are still open: flush what is left of the token stream
        // into the innermost one, then close every open ancestor so
        // rowan sees a single, complete top-level child.
        raw.flush_remaining(&mut builder);
        while depth > 0 {
            builder.finish_node();
            depth -= 1;
        }
    } else if !root_opened {
        // Nothing was ever opened (for example an empty event list):
        // rowan still requires exactly one root, so synthesize one and
        // hand it every raw token to keep the tree lossless.
        builder.start_node(PhpLanguage::kind_to_raw(SyntaxKind::SourceFile));
        raw.flush_remaining(&mut builder);
        builder.finish_node();
    }
    builder.finish()
}

/// Removes the event at `index`, leaving a tombstone in its place.
fn take_event(events: &mut [Event], index: usize) -> Event {
    events
        .get_mut(index)
        .map(|slot| core::mem::replace(slot, Event::tombstone()))
        .unwrap_or_else(Event::tombstone)
}

/// A cursor over the full (trivia-included) token stream.
struct RawTokens<'source> {
    source: &'source str,
    tokens: &'source [Token],
    index: usize,
    offset: TextSize,
}

impl RawTokens<'_> {
    fn flush_trivia(&mut self, builder: &mut GreenNodeBuilder<'_>) {
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            self.push_next(builder);
        }
    }

    fn push_next(&mut self, builder: &mut GreenNodeBuilder<'_>) {
        let Some(token) = self.tokens.get(self.index) else {
            return;
        };
        let start = usize::from(self.offset);
        let end = start + usize::from(token.length);
        // The lexer guarantees token lengths cover the source exactly;
        // the empty fallback would only follow a violated invariant
        // upstream, and losing text there is still panic-free.
        let text = self.source.get(start..end).unwrap_or_default();
        builder.token(PhpLanguage::kind_to_raw(token.kind), text);
        self.offset += token.length;
        self.index += 1;
    }

    fn flush_remaining(&mut self, builder: &mut GreenNodeBuilder<'_>) {
        while self.index < self.tokens.len() {
            self.push_next(builder);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::unwrap_used)]

    use crate::parser::Event;
    use crate::syntax_kind::SyntaxKind;
    use crate::token::Token;
    use crate::tree::SyntaxNode;

    use super::build_tree;

    fn token(kind: SyntaxKind, length: u32) -> Token {
        Token::new(kind, celerrate_source::TextSize::from(length))
    }

    fn start(kind: SyntaxKind) -> Event {
        Event::Start {
            kind: Some(kind),
            forward_parent: None,
        }
    }

    /// `<?php echo 1;`: trivia (one space between tokens) must land in
    /// the source file, between siblings, never inside the statement's
    /// leading edge.
    #[test]
    fn trivia_sit_between_siblings() {
        // "<?php echo 1;" = OpenTag(5) Ws(1) Echo(4) Ws(1) Integer(1) Semicolon(1)
        let source = "<?php echo 1;";
        let tokens = [
            token(SyntaxKind::OpenTag, 5),
            token(SyntaxKind::Whitespace, 1),
            token(SyntaxKind::Echo, 4),
            token(SyntaxKind::Whitespace, 1),
            token(SyntaxKind::IntegerLiteral, 1),
            token(SyntaxKind::Semicolon, 1),
        ];
        let events = vec![
            start(SyntaxKind::SourceFile),
            Event::Token, // <?php
            start(SyntaxKind::EchoStatement),
            Event::Token, // echo
            start(SyntaxKind::Literal),
            Event::Token, // 1
            Event::Finish,
            Event::Token, // ;
            Event::Finish,
            Event::Finish,
        ];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
        let statement = tree
            .children()
            .find(|node| node.kind() == SyntaxKind::EchoStatement)
            .unwrap();
        // The statement starts at `echo`, not at the space before it.
        assert_eq!(u32::from(statement.text_range().start()), 6);
        // Inside the statement, the space before `1` sits between the
        // `echo` token and the Literal node.
        let literal = statement.children().next().unwrap();
        assert_eq!(literal.kind(), SyntaxKind::Literal);
        assert_eq!(literal.text(), "1");
    }

    /// Trailing trivia (and any leftover raw token) flush into the root
    /// before it closes: lossless even after the last statement.
    #[test]
    fn trailing_trivia_land_in_the_root() {
        let source = "<?php ";
        let tokens = [
            token(SyntaxKind::OpenTag, 5),
            token(SyntaxKind::Whitespace, 1),
        ];
        let events = vec![start(SyntaxKind::SourceFile), Event::Token, Event::Finish];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
    }

    /// A tombstone start is skipped entirely.
    #[test]
    fn tombstones_are_skipped() {
        let source = "1";
        let tokens = [token(SyntaxKind::IntegerLiteral, 1)];
        let events = vec![
            start(SyntaxKind::SourceFile),
            Event::tombstone(),
            Event::Token,
            Event::Finish,
        ];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
        assert_eq!(tree.children().count(), 0);
    }

    /// Forward parents open before the node that points at them: the
    /// classic retroactive wrap of an already-parsed expression.
    /// Simulates `1 + 2` where the Literal `1` (event 1) is preceded by
    /// a BinaryExpression-like wrapper (here: ExpressionStatement, the
    /// only wrapping node kind this plan defines).
    #[test]
    fn forward_parents_wrap_completed_nodes() {
        let source = "1";
        let tokens = [token(SyntaxKind::IntegerLiteral, 1)];
        let events = vec![
            start(SyntaxKind::SourceFile),
            Event::Start {
                kind: Some(SyntaxKind::Literal),
                forward_parent: Some(4), // absolute index of the wrapper
            },
            Event::Token,
            Event::Finish, // closes Literal
            Event::Start {
                kind: Some(SyntaxKind::ExpressionStatement),
                forward_parent: None,
            },
            Event::Finish, // closes ExpressionStatement
            Event::Finish, // closes SourceFile
        ];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
        let statement = tree.children().next().unwrap();
        assert_eq!(statement.kind(), SyntaxKind::ExpressionStatement);
        let literal = statement.children().next().unwrap();
        assert_eq!(literal.kind(), SyntaxKind::Literal);
    }

    /// An empty event list still yields a single root: `build_tree` opens
    /// a synthetic `SourceFile` and flushes every raw token into it.
    #[test]
    fn empty_events_still_flush_the_token_stream() {
        let source = "1";
        let tokens = [token(SyntaxKind::IntegerLiteral, 1)];
        let events = vec![];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
    }

    /// A lone `Finish` at depth 0 has no open node to close: it is
    /// skipped rather than handed to rowan's `finish_node`.
    #[test]
    fn finish_at_depth_zero_is_skipped() {
        let source = "1";
        let tokens = [token(SyntaxKind::IntegerLiteral, 1)];
        let events = vec![Event::Finish];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
    }

    /// A node opened but never closed still yields a lossless, single-root
    /// tree: the builder closes it after flushing the remaining tokens.
    #[test]
    fn unclosed_node_is_closed_after_flushing() {
        let source = "1";
        let tokens = [token(SyntaxKind::IntegerLiteral, 1)];
        let events = vec![start(SyntaxKind::SourceFile), Event::Token];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
    }

    /// Events continuing after the root has already closed must not open
    /// a second top-level child: rowan requires exactly one root.
    #[test]
    fn events_after_root_closed_are_skipped() {
        let source = "1";
        let tokens = [token(SyntaxKind::IntegerLiteral, 1)];
        let events = vec![
            start(SyntaxKind::SourceFile),
            Event::Finish,
            start(SyntaxKind::Literal),
            Event::Token,
            Event::Finish,
        ];
        let tree = SyntaxNode::new_root(build_tree(source, &tokens, events));
        assert_eq!(tree.text(), source);
        assert_eq!(tree.children().count(), 0);
    }
}
