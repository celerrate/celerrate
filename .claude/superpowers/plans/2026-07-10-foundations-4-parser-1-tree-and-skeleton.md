# Foundations Part 4, Plan 1: Tree and Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `celerrate_syntax` a lossless syntax tree and a minimal but complete parsing pipeline: `parse(source)` produces a `SourceFile` tree covering inline HTML, tags, `echo` statements, and expression statements, with merged lexer + parser diagnostics.

**Architecture:** The tree is `rowan` (red-green), wrapped behind crate-owned types (`PhpLanguage`, `SyntaxNode`, ...). The parser is hand-written recursive descent that never touches the tree: it emits events (`Start`/`Token`/`Finish`, with forward-parent support) over a trivia-free token view; a `TreeBuilder` replays the events against the full token stream, reinserting trivia between siblings. Spec: `.claude/superpowers/specs/2026-07-10-foundations-4-parser-design.md`.

**Tech Stack:** Rust 1.94 (edition 2024), `rowan` 0.16, `text-size` 1, `insta` (snapshots), `cargo-fuzz` (libFuzzer).

## Global Constraints

Copied from the parent spec and `CLAUDE.md`; every task's requirements include them.

- Zero panic, mechanically enforced: workspace denies `clippy::unwrap_used`, `clippy::expect_used`, `clippy::indexing_slicing`, `clippy::panic`; `unsafe_code` is forbidden. Production code returns totals (`Option`, fallbacks); test modules may locally `#[allow]` these lints (see existing test files for the idiom). `debug_assert!` is permitted (compiled out in release).
- TDD: every step of behavior starts from a failing test. No production code without a test that demanded it.
- Layering: `celerrate_syntax` depends only on `celerrate_source` (plus external `rowan`, `text-size`). No bare `rowan` type in any public signature.
- The lossless invariant: `parse(source).tree().text() == source` for every input, including degenerate input.
- The parser performs no version or semantic judgment and never fails: worst case is `ErrorNode` wreckage plus diagnostics.
- Everything in English, full words, no abbreviated names (standard acronyms fine). No em-dashes. Comments state constraints the code cannot show, never narration.
- Commits: gitmoji + Conventional Commits (`✨ feat(syntax): ...`), repository-configured identity, no AI attribution of any kind.
- Before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` must all pass.

## File Structure

```
Cargo.toml                                    modify: add rowan to workspace dependencies
crates/celerrate_syntax/Cargo.toml            modify: add rowan dependency
crates/celerrate_syntax/src/lib.rs            modify: new modules and exports, crate docs
crates/celerrate_syntax/src/syntax_kind.rs    modify: macro-declared kinds, node kinds, raw conversion
crates/celerrate_syntax/src/diagnostic.rs     modify: ParserDiagnostic(Kind), SyntaxDiagnostic(Kind)
crates/celerrate_syntax/src/tree.rs           create: PhpLanguage, SyntaxNode/SyntaxToken/SyntaxElement aliases
crates/celerrate_syntax/src/tree/builder.rs   create: TreeBuilder (event replay, trivia reinsertion)
crates/celerrate_syntax/src/parser.rs         create: Parser, Marker, CompletedMarker, run()
crates/celerrate_syntax/src/parser/event.rs   create: Event
crates/celerrate_syntax/src/parser/token_source.rs  create: TokenSource (trivia-free view)
crates/celerrate_syntax/src/parser/grammar.rs create: source_file, statements, minimal expressions
crates/celerrate_syntax/src/parse.rs          create: Parse, parse()
crates/celerrate_syntax/tests/syntax_kind.rs  modify: raw conversion tests
crates/celerrate_syntax/tests/parse.rs        create: pipeline behavior tests
crates/celerrate_syntax/tests/parse_corpus.rs create: snapshot corpus for parse trees
crates/celerrate_syntax/tests/parse_corpus/   create: seed .php files
crates/celerrate_syntax/tests/support/mod.rs  modify: parse_verified, render_parse helpers
fuzz/Cargo.toml                               modify: parse fuzz target
fuzz/fuzz_targets/parse.rs                    create: parse fuzz target
fuzz/corpus/parse/                            create: seed corpus (copied from lex seeds)
.github/workflows/ci.yml                      modify: run the parse fuzz target
```

The `lexer.rs` + `lexer/` file-plus-directory layout is the crate's existing convention; `tree.rs`/`tree/` and `parser.rs`/`parser/` follow it.

---

### Task 1: Safe raw conversion for `SyntaxKind`, plus the first node kinds

`rowan` stores kinds as raw `u16`; reading a tree back needs `u16 -> SyntaxKind` without `unsafe` and without a hand-maintained table that could drift. A declarative macro becomes the single source of truth: it declares the enum and derives an `ALL` array in declaration order, so index equals discriminant.

**Files:**
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs`
- Test: `crates/celerrate_syntax/tests/syntax_kind.rs`

**Interfaces:**
- Consumes: the existing `SyntaxKind` enum (variants unchanged, same order).
- Produces: `SyntaxKind::from_raw(raw: u16) -> Option<SyntaxKind>`, `SyntaxKind::into_raw(self) -> u16`, and new node-kind variants `SourceFile`, `EchoStatement`, `ExpressionStatement`, `Literal`, `VariableReference`, `ErrorNode` (appended after `Error`, in this order). Everything else (`is_trivia`, `from_keyword`) keeps its exact signature and behavior.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/syntax_kind.rs`:

```rust
#[test]
fn raw_conversion_roundtrips_every_kind() {
    let mut raw = 0u16;
    while let Some(kind) = SyntaxKind::from_raw(raw) {
        assert_eq!(kind.into_raw(), raw, "discriminant order must match ALL order");
        raw += 1;
    }
    assert!(raw > 0, "at least one kind exists");
    assert_eq!(SyntaxKind::from_raw(raw), None);
    assert_eq!(SyntaxKind::from_raw(u16::MAX), None);
}

#[test]
fn node_kinds_exist_and_are_not_trivia() {
    for kind in [
        SyntaxKind::SourceFile,
        SyntaxKind::EchoStatement,
        SyntaxKind::ExpressionStatement,
        SyntaxKind::Literal,
        SyntaxKind::VariableReference,
        SyntaxKind::ErrorNode,
    ] {
        assert!(!kind.is_trivia());
    }
}

#[test]
fn node_kinds_come_after_token_kinds() {
    assert!(SyntaxKind::SourceFile.into_raw() > SyntaxKind::Error.into_raw());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test syntax_kind`
Expected: FAIL to compile with `no function or associated item named 'from_raw'` and `no variant or associated item named 'SourceFile'`.

- [ ] **Step 3: Refactor `syntax_kind.rs` around the macro**

In `crates/celerrate_syntax/src/syntax_kind.rs`, define the macro above the enum, then wrap the **existing variant list verbatim** (same order, doc comments and section comments preserved — doc comments pass through the `$(#[$attribute])*` capture, plain `//` comments simply stay at the invocation site) and append the node kinds at the end:

```rust
/// Declares `SyntaxKind` and derives the raw `u16` conversion from the
/// same variant list, so the enum and the conversion can never drift
/// apart: `ALL` mirrors the declaration order, and declaration order is
/// discriminant order.
macro_rules! syntax_kinds {
    ( $( $(#[$attribute:meta])* $variant:ident, )* ) => {
        /// Every kind of token and node in PHP syntax.
        ///
        /// One vocabulary shared by the whole syntax layer, `#[repr(u16)]`
        /// so the rowan tree stores it directly. Token kinds first, node
        /// kinds after them.
        ///
        /// Keywords each get their own kind, resolved case-insensitively by
        /// the lexer. Semi-reserved uses (`$object->list()`, `const FOR = 1;`,
        /// `enum` as a plain name) are the parser's business: it re-treats
        /// keyword kinds as identifiers where the grammar allows. `true`,
        /// `false`, `null`, `self`, `parent`, and the magic constants are
        /// plain identifiers, resolved semantically.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u16)]
        pub enum SyntaxKind {
            $( $(#[$attribute])* $variant, )*
        }

        impl SyntaxKind {
            /// Every kind, in declaration (and therefore discriminant) order.
            const ALL: &'static [SyntaxKind] = &[ $(SyntaxKind::$variant,)* ];
        }
    };
}

syntax_kinds! {
    // ... the existing variants, moved verbatim, `Whitespace` through
    // `Error`, unchanged and in the same order ...

    // Node kinds, appended after every token kind and hand-maintained
    // until the ungrammar code generation of a later plan takes
    // ownership of this list.
    /// The root node: one parsed PHP file.
    SourceFile,
    /// `echo expression, expression;`.
    EchoStatement,
    /// An expression used as a statement, terminator included.
    ExpressionStatement,
    /// A literal expression: integer, float, or single-quoted string.
    Literal,
    /// A `$variable` used as an expression.
    VariableReference,
    /// Recovery wreckage: tokens no grammar rule accepted.
    ErrorNode,
}
```

Note the enum-level doc comment moves into the macro expansion (shown above); delete the old free-standing enum declaration. Then add the conversion next to the existing `impl SyntaxKind` block (same block or a new one):

```rust
    /// The inverse of [`SyntaxKind::into_raw`]. Total and panic-free:
    /// out-of-range values return `None`.
    pub fn from_raw(raw: u16) -> Option<Self> {
        Self::ALL.get(usize::from(raw)).copied()
    }

    /// The `u16` the tree stores; the discriminant.
    pub fn into_raw(self) -> u16 {
        self as u16
    }
```

`is_trivia` and `from_keyword` are untouched.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax --test syntax_kind`
Expected: PASS (all pre-existing tests too — the refactor must not change any behavior).

- [ ] **Step 5: Full check and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/celerrate_syntax/src/syntax_kind.rs crates/celerrate_syntax/tests/syntax_kind.rs
git commit -m "✨ feat(syntax): add node kinds and safe raw kind conversion"
```

---

### Task 2: `rowan` behind `PhpLanguage` and crate-owned aliases

**Files:**
- Modify: `Cargo.toml` (workspace), `crates/celerrate_syntax/Cargo.toml`, `crates/celerrate_syntax/src/lib.rs`
- Create: `crates/celerrate_syntax/src/tree.rs`
- Test: unit tests in `crates/celerrate_syntax/src/tree.rs`

**Interfaces:**
- Consumes: `SyntaxKind::from_raw` / `into_raw` (Task 1).
- Produces: `pub enum PhpLanguage {}` implementing `rowan::Language` with `type Kind = SyntaxKind`; `pub type SyntaxNode = rowan::SyntaxNode<PhpLanguage>`; `pub type SyntaxToken = rowan::SyntaxToken<PhpLanguage>`; `pub type SyntaxElement = rowan::SyntaxElement<PhpLanguage>`. All re-exported from `lib.rs`. (`SyntaxNodePtr` is deliberately deferred: its first consumer is the salsa layer.)

- [ ] **Step 1: Add the dependency**

In the workspace `Cargo.toml`, under `[workspace.dependencies]`:

```toml
rowan = "0.16"
```

In `crates/celerrate_syntax/Cargo.toml`, under `[dependencies]`:

```toml
rowan = { workspace = true }
```

`rowan` 0.16 uses the same `text-size` 1.x types, so `rowan`'s `TextRange`/`TextSize` are the `celerrate_source` ones — no conversion layer.

Run: `cargo deny check`
Expected: PASS (rowan is `MIT OR Apache-2.0`; its `unsafe` internals are outside our `forbid`, which governs our code only — the parent spec records this).

- [ ] **Step 2: Write the failing test**

Create `crates/celerrate_syntax/src/tree.rs` with only a test module for now (it will not compile until step 3 adds the items — that is the failing state):

```rust
#[cfg(test)]
mod tests {
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
        builder.token(PhpLanguage::kind_to_raw(SyntaxKind::InlineHtml), "<p>hi</p>");
        builder.finish_node();
        let tree = SyntaxNode::new_root(builder.finish());
        assert_eq!(tree.kind(), SyntaxKind::SourceFile);
        assert_eq!(tree.text(), "<p>hi</p>");
    }
}
```

Declare the module in `crates/celerrate_syntax/src/lib.rs` (keep the module list alphabetical):

```rust
mod tree;
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p celerrate_syntax tree`
Expected: FAIL to compile with `cannot find type 'PhpLanguage'`.

- [ ] **Step 4: Implement the tree layer**

Prepend to `crates/celerrate_syntax/src/tree.rs`:

```rust
//! The tree layer: rowan wrapped behind crate-owned types. Upper crates
//! import these aliases; no bare rowan type appears in any public
//! signature.

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
```

Re-export from `lib.rs`:

```rust
pub use tree::{PhpLanguage, SyntaxElement, SyntaxNode, SyntaxToken};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax tree`
Expected: PASS.

- [ ] **Step 6: Full check and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add Cargo.toml Cargo.lock crates/celerrate_syntax/Cargo.toml crates/celerrate_syntax/src/tree.rs crates/celerrate_syntax/src/lib.rs
git commit -m "✨ feat(syntax): integrate rowan behind PhpLanguage aliases"
```

---

### Task 3: Parser events and the `TreeBuilder`

The builder is where losslessness is engineered: the parser's events cover only significant tokens; the builder walks the **full** token stream and reinserts trivia. Attachment rule (spec section 2): pending trivia flush just before the next node or token starts, into the node open at that point — so trivia sit between siblings and a node's range starts at its first significant token. Leftover raw tokens flush into the root before it closes, which keeps the tree lossless even in the face of a parser bug.

**Files:**
- Create: `crates/celerrate_syntax/src/parser/event.rs`, `crates/celerrate_syntax/src/parser.rs` (module shell only), `crates/celerrate_syntax/src/tree/builder.rs`
- Modify: `crates/celerrate_syntax/src/lib.rs` (declare `mod parser;`), `crates/celerrate_syntax/src/tree.rs` (declare `pub(crate) mod builder;`)
- Test: unit tests in `crates/celerrate_syntax/src/tree/builder.rs`

**Interfaces:**
- Consumes: `Token`, `SyntaxKind` (including `is_trivia`), `PhpLanguage`.
- Produces:
  - `pub(crate) enum Event { Start { kind: Option<SyntaxKind>, forward_parent: Option<usize> }, Token, Finish }` with `Event::tombstone() -> Event` (a `Start` with `kind: None`), in `parser/event.rs`.
  - `pub(crate) fn build_tree(source: &str, tokens: &[Token], events: Vec<Event>) -> rowan::GreenNode` in `tree/builder.rs`.
  - Contract: one `Event::Token` consumes exactly one significant (non-trivia) raw token, in order. `forward_parent` holds an **absolute** index into the event buffer.

- [ ] **Step 1: Create the event type and the parser module shell**

`crates/celerrate_syntax/src/parser/event.rs`:

```rust
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
```

`crates/celerrate_syntax/src/parser.rs` (shell; grows in Task 4):

```rust
//! The hand-written recursive-descent parser. Event-based: it reads a
//! trivia-free view of the token stream and records [`Event`]s; the
//! tree builder replays them. The parser never fails and never touches
//! the tree.

mod event;

pub(crate) use event::Event;
```

(Task 4 adds a temporary module-wide `#![allow(dead_code)]` here; Task 5 removes it.)

In `lib.rs`, declare `mod parser;` (alphabetical order). In `tree.rs`, add at the top:

```rust
pub(crate) mod builder;
```

- [ ] **Step 2: Write the failing builder tests**

Create `crates/celerrate_syntax/src/tree/builder.rs` containing only the test module (compile failure is the failing state). The tests hand-craft token streams and event sequences:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

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

    /// `<?php echo 1;` — trivia (one space between tokens) must land in
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
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax builder`
Expected: FAIL to compile with `cannot find function 'build_tree'`.

- [ ] **Step 4: Implement the builder**

Prepend to `crates/celerrate_syntax/src/tree/builder.rs`:

```rust
//! Replays parser events against the full token stream, reinserting
//! trivia, and materializes the green tree.
//!
//! Trivia attachment (spec section 2): pending trivia flush just before
//! the next node or token starts, into the node open at that point, so
//! trivia sit between siblings and a node's range starts at its first
//! significant token. Losslessness is structural: every raw token is
//! pushed exactly once, in order, and leftovers flush into the root
//! before it closes — a parser bug can cost tree shape, never text.

use celerrate_source::TextSize;
use rowan::{GreenNode, GreenNodeBuilder, Language as _};

use crate::parser::Event;
use crate::token::Token;
use crate::tree::PhpLanguage;

// TEMPORARY until Task 5's `parse()` consumes this: keeps the
// intermediate commit clean under `-D warnings`. Task 5 removes it.
#[allow(dead_code)]
pub(crate) fn build_tree(source: &str, tokens: &[Token], mut events: Vec<Event>) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    let mut raw = RawTokens {
        source,
        tokens,
        index: 0,
        offset: TextSize::from(0),
    };
    let mut depth = 0usize;
    let mut forward_kinds = Vec::new();
    for index in 0..events.len() {
        match take_event(&mut events, index) {
            Event::Start { kind: None, .. } => {}
            Event::Start {
                kind: Some(kind),
                forward_parent,
            } => {
                // Collect the forward-parent chain: each target must
                // open before the node that points at it, so the chain
                // is replayed outermost-first (reverse collection
                // order). Taking each target tombstones it at its own
                // position.
                forward_kinds.push(kind);
                let mut next = forward_parent;
                while let Some(target) = next {
                    next = None;
                    if let Event::Start {
                        kind,
                        forward_parent,
                    } = take_event(&mut events, target)
                    {
                        if let Some(kind) = kind {
                            forward_kinds.push(kind);
                        }
                        next = forward_parent;
                    }
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
            }
            Event::Token => {
                raw.flush_trivia(&mut builder);
                raw.push_next(&mut builder);
            }
            Event::Finish => {
                if depth == 1 {
                    raw.flush_remaining(&mut builder);
                }
                builder.finish_node();
                depth = depth.saturating_sub(1);
            }
        }
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax builder`
Expected: PASS (4 tests).

- [ ] **Step 6: Full check and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/celerrate_syntax/src/parser.rs crates/celerrate_syntax/src/parser/event.rs crates/celerrate_syntax/src/tree.rs crates/celerrate_syntax/src/tree/builder.rs crates/celerrate_syntax/src/lib.rs
git commit -m "✨ feat(syntax): build green trees from parser events"
```

---

### Task 4: Parser core — `TokenSource`, `Parser`, `Marker`

**Files:**
- Create: `crates/celerrate_syntax/src/parser/token_source.rs`
- Modify: `crates/celerrate_syntax/src/parser.rs`, `crates/celerrate_syntax/src/diagnostic.rs`
- Test: unit tests in both created/modified `parser` files

**Interfaces:**
- Consumes: `Token`, `SyntaxKind`, `Event` (Task 3).
- Produces (all `pub(crate)`, used by Task 5's grammar):
  - `TokenSource::new(tokens: &[Token]) -> TokenSource`; `fn kind(&self, position: usize) -> Option<SyntaxKind>`; `fn range(&self, position: usize) -> Option<TextRange>`; `fn significant_count(&self) -> usize` (named to sidestep `clippy::len_without_is_empty`).
  - `Parser` with `fn current(&self) -> Option<SyntaxKind>`, `fn at(&self, kind: SyntaxKind) -> bool`, `fn at_end(&self) -> bool`, `fn bump(&mut self)`, `fn start(&mut self) -> Marker`, `fn diagnose_current(&mut self, kind: ParserDiagnosticKind)` (range = current token, or zero-width at the previous token's end when at end of input), `fn diagnose_missing(&mut self, kind: ParserDiagnosticKind)` (always zero-width at the previous token's end).
  - `Marker::complete(self, parser: &mut Parser, kind: SyntaxKind) -> CompletedMarker`; `Marker::abandon(self, parser: &mut Parser)`. `CompletedMarker` is a unit struct for now (`precede` arrives with the Pratt plan).
  - In `diagnostic.rs`: `pub enum ParserDiagnosticKind { ExpectedExpression, ExpectedSemicolon, UnexpectedToken }` and `pub(crate)`-constructed `pub struct ParserDiagnostic { pub kind: ParserDiagnosticKind, pub range: TextRange }` (public type, not re-exported from `lib.rs` in this task).

- [ ] **Step 1: Write the failing `TokenSource` tests**

Create `crates/celerrate_syntax/src/parser/token_source.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    use crate::syntax_kind::SyntaxKind;

    use super::TokenSource;

    #[test]
    fn trivia_are_invisible_and_ranges_accumulate() {
        // "<?php echo 1;": trivia (whitespace) must vanish from the
        // significant view while ranges stay absolute.
        let (tokens, _diagnostics) = crate::lexer::lex("<?php echo 1;");
        let source = TokenSource::new(&tokens);
        assert_eq!(source.significant_count(), 4);
        assert_eq!(source.kind(0), Some(SyntaxKind::OpenTag));
        assert_eq!(source.kind(1), Some(SyntaxKind::Echo));
        assert_eq!(source.kind(2), Some(SyntaxKind::IntegerLiteral));
        assert_eq!(source.kind(3), Some(SyntaxKind::Semicolon));
        assert_eq!(source.kind(4), None);
        let range = source.range(2).unwrap_or_default();
        assert_eq!(u32::from(range.start()), 11);
        assert_eq!(u32::from(range.end()), 12);
    }
}
```

Declare it in `parser.rs`: `mod token_source;`

- [ ] **Step 2: Run to verify failure, then implement `TokenSource`**

Run: `cargo test -p celerrate_syntax token_source` — expected: compile FAIL (`TokenSource` not found).

Prepend the implementation:

```rust
use celerrate_source::{TextRange, TextSize};

use crate::syntax_kind::SyntaxKind;
use crate::token::Token;

/// The parser's trivia-free view of the token stream. Positions index
/// significant tokens only; ranges stay absolute in the source, so
/// diagnostics point at real text. The builder reconciles with the full
/// stream by consuming one significant token per `Token` event.
pub(crate) struct TokenSource {
    significant: Vec<(SyntaxKind, TextRange)>,
}

impl TokenSource {
    pub(crate) fn new(tokens: &[Token]) -> Self {
        let mut significant = Vec::new();
        let mut offset = TextSize::from(0);
        for token in tokens {
            if !token.kind.is_trivia() {
                significant.push((token.kind, TextRange::at(offset, token.length)));
            }
            offset += token.length;
        }
        Self { significant }
    }

    pub(crate) fn kind(&self, position: usize) -> Option<SyntaxKind> {
        self.significant.get(position).map(|(kind, _)| *kind)
    }

    pub(crate) fn range(&self, position: usize) -> Option<TextRange> {
        self.significant.get(position).map(|(_, range)| *range)
    }

    pub(crate) fn significant_count(&self) -> usize {
        self.significant.len()
    }
}
```

Run: `cargo test -p celerrate_syntax token_source` — expected: PASS.

- [ ] **Step 3: Write the failing parser-core tests**

Append to `crates/celerrate_syntax/src/parser.rs` (the test module exercises markers and events without any grammar):

```rust
#[cfg(test)]
mod tests {
    use crate::syntax_kind::SyntaxKind;

    use super::*;

    fn parser_over(source: &str) -> Parser {
        let (tokens, _diagnostics) = crate::lexer::lex(source);
        Parser::new(token_source::TokenSource::new(&tokens))
    }

    #[test]
    fn markers_wrap_bumped_tokens_into_a_node() {
        let mut parser = parser_over("<?php echo 1;");
        let marker = parser.start();
        while !parser.at_end() {
            parser.bump();
        }
        marker.complete(&mut parser, SyntaxKind::SourceFile);
        assert_eq!(
            parser.events,
            vec![
                Event::Start {
                    kind: Some(SyntaxKind::SourceFile),
                    forward_parent: None
                },
                Event::Token,
                Event::Token,
                Event::Token,
                Event::Token,
                Event::Finish,
            ],
        );
    }

    #[test]
    fn abandoning_the_last_marker_removes_its_event() {
        let mut parser = parser_over("<?php");
        let marker = parser.start();
        marker.abandon(&mut parser);
        assert!(parser.events.is_empty());
    }

    #[test]
    fn abandoning_an_older_marker_leaves_a_tombstone() {
        let mut parser = parser_over("<?php");
        let outer = parser.start();
        parser.bump();
        outer.abandon(&mut parser);
        assert_eq!(parser.events, vec![Event::tombstone(), Event::Token]);
    }

    #[test]
    fn missing_diagnostics_are_zero_width_after_the_previous_token() {
        let mut parser = parser_over("<?php echo");
        parser.bump(); // <?php
        parser.bump(); // echo
        parser.diagnose_missing(ParserDiagnosticKind::ExpectedSemicolon);
        let diagnostic = parser.diagnostics.first().copied();
        assert!(matches!(
            diagnostic,
            Some(ParserDiagnostic {
                kind: ParserDiagnosticKind::ExpectedSemicolon,
                range,
            }) if range.is_empty() && u32::from(range.start()) == 10
        ));
    }
}
```

Run: `cargo test -p celerrate_syntax parser` — expected: compile FAIL (`Parser` not found, `ParserDiagnosticKind` not found).

- [ ] **Step 4: Implement diagnostics and the parser core**

Append to `crates/celerrate_syntax/src/diagnostic.rs`:

```rust
/// What the parser expected or could not place, structurally. Rendering
/// into messages is an upper layer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserDiagnosticKind {
    /// An expression position holds no expression.
    ExpectedExpression,
    /// A statement misses its terminator (`;`, or `?>` / end of input
    /// only where PHP itself allows the omission).
    ExpectedSemicolon,
    /// A token no grammar rule accepts; wrapped in an `ErrorNode`.
    UnexpectedToken,
}

/// A parser diagnostic: a structured kind and the range it points at.
/// Zero-width ranges mark something missing at that offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParserDiagnostic {
    pub kind: ParserDiagnosticKind,
    pub range: TextRange,
}
```

Extend `crates/celerrate_syntax/src/parser.rs` (above the test module). The inner `#![allow(dead_code)]` sits right under the module docs; it cascades to the child modules and keeps this intermediate commit clean under `-D warnings` until Task 5's `parse()` consumes the parser:

```rust
// TEMPORARY until Task 5's `parse()` consumes the parser: keeps the
// intermediate commit clean under `-D warnings`. Task 5 removes it.
#![allow(dead_code)]

mod event;
mod token_source;

pub(crate) use event::Event;

use celerrate_source::{TextRange, TextSize};

use crate::diagnostic::{ParserDiagnostic, ParserDiagnosticKind};
use crate::syntax_kind::SyntaxKind;
use token_source::TokenSource;

struct Parser {
    source: TokenSource,
    position: usize,
    events: Vec<Event>,
    diagnostics: Vec<ParserDiagnostic>,
}

impl Parser {
    fn new(source: TokenSource) -> Self {
        Self {
            source,
            position: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn current(&self) -> Option<SyntaxKind> {
        self.source.kind(self.position)
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == Some(kind)
    }

    fn at_end(&self) -> bool {
        self.position >= self.source.significant_count()
    }

    /// Consumes the current token into the events. A no-op at end of
    /// input, so recovery loops can never run past the stream.
    fn bump(&mut self) {
        if self.at_end() {
            return;
        }
        self.position += 1;
        self.events.push(Event::Token);
    }

    fn start(&mut self) -> Marker {
        let event_index = self.events.len();
        self.events.push(Event::tombstone());
        Marker::new(event_index)
    }

    /// The end of the last consumed token; offset zero before any.
    fn previous_end(&self) -> TextSize {
        self.position
            .checked_sub(1)
            .and_then(|position| self.source.range(position))
            .map(TextRange::end)
            .unwrap_or_default()
    }

    /// Points at the current token, or zero-width after the last one
    /// when at end of input.
    fn diagnose_current(&mut self, kind: ParserDiagnosticKind) {
        let range = self
            .source
            .range(self.position)
            .unwrap_or_else(|| TextRange::empty(self.previous_end()));
        self.diagnostics.push(ParserDiagnostic { kind, range });
    }

    /// Marks something missing: zero-width at the previous token's end.
    fn diagnose_missing(&mut self, kind: ParserDiagnosticKind) {
        self.diagnostics.push(ParserDiagnostic {
            kind,
            range: TextRange::empty(self.previous_end()),
        });
    }
}

/// An open node. Must be completed or abandoned; the tripwire makes a
/// forgotten marker fail tests (`debug_assert!` compiles out in
/// release, where the worst case is a tombstone, not a panic).
struct Marker {
    event_index: usize,
    defused: bool,
}

impl Marker {
    fn new(event_index: usize) -> Self {
        Self {
            event_index,
            defused: false,
        }
    }

    fn complete(mut self, parser: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        self.defused = true;
        if let Some(Event::Start { kind: slot, .. }) = parser.events.get_mut(self.event_index) {
            *slot = Some(kind);
        }
        parser.events.push(Event::Finish);
        CompletedMarker
    }

    fn abandon(mut self, parser: &mut Parser) {
        self.defused = true;
        if self.event_index + 1 == parser.events.len() {
            parser.events.pop();
        }
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        debug_assert!(self.defused, "a marker must be completed or abandoned");
    }
}

/// A finished node. Grows `precede` (the forward-parent producer) with
/// the expressions plan.
struct CompletedMarker;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax parser`
Expected: PASS (token_source + parser tests). Note: `Parser`, `Marker`, `CompletedMarker` stay module-private (`struct`, not `pub(crate) struct`) — Task 5's grammar lives inside the `parser` module, so nothing else needs them. The temporary module-wide `#![allow(dead_code)]` (see step 4) is what keeps `cargo clippy --workspace --all-targets -- -D warnings` clean until `parse()` consumes the parser.

- [ ] **Step 6: Full check and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/celerrate_syntax/src/parser.rs crates/celerrate_syntax/src/parser/token_source.rs crates/celerrate_syntax/src/diagnostic.rs
git commit -m "✨ feat(syntax): add the parser core and token source"
```

---

### Task 5: `parse()`, `Parse`, and the minimal grammar

The public entry point plus the plan's grammar: `SourceFile` covering inline HTML and tags as direct token children, `echo` statements with expression lists, expression statements over minimal primaries (integer, float, single-quoted string, variable), Zend-faithful terminator handling (`;` required, except before `?>`), and single-token `ErrorNode` recovery.

**Files:**
- Create: `crates/celerrate_syntax/src/parser/grammar.rs`, `crates/celerrate_syntax/src/parse.rs`
- Modify: `crates/celerrate_syntax/src/parser.rs`, `crates/celerrate_syntax/src/diagnostic.rs`, `crates/celerrate_syntax/src/lib.rs`
- Test: `crates/celerrate_syntax/tests/parse.rs`, helper in `crates/celerrate_syntax/tests/support/mod.rs`

**Interfaces:**
- Consumes: `Parser`/`Marker` (Task 4), `build_tree` (Task 3), `lex`.
- Produces (public API):
  - `pub fn parse(source: &str) -> Parse`
  - `pub struct Parse` with `fn tree(&self) -> SyntaxNode` and `fn diagnostics(&self) -> &[SyntaxDiagnostic]`
  - `pub struct SyntaxDiagnostic { pub kind: SyntaxDiagnosticKind, pub range: TextRange }` and `pub enum SyntaxDiagnosticKind { Lexer(LexerDiagnosticKind), Parser(ParserDiagnosticKind) }`
  - `pub(crate) fn run(tokens: &[Token]) -> (Vec<Event>, Vec<ParserDiagnostic>)` in the `parser` module.

- [ ] **Step 1: Write the failing integration tests**

Add to `crates/celerrate_syntax/tests/support/mod.rs`:

```rust
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn parse_verified(source: &str) -> celerrate_syntax::Parse {
    let parse = celerrate_syntax::parse(source);
    assert_eq!(
        parse.tree().text().to_string(),
        source,
        "the tree must be lossless"
    );
    parse
}
```

Create `crates/celerrate_syntax/tests/parse.rs`:

```rust
//! Behavior tests for the parsing pipeline: tree shapes, terminator
//! rules, recovery, and diagnostic merging. Every parse goes through
//! the lossless assertion in `support::parse_verified`.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{SyntaxDiagnosticKind, SyntaxKind};

#[test]
fn empty_input_yields_an_empty_source_file() {
    let parse = support::parse_verified("");
    assert_eq!(parse.tree().kind(), SyntaxKind::SourceFile);
    assert_eq!(parse.tree().children_with_tokens().count(), 0);
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn inline_html_and_tags_are_token_children_of_the_source_file() {
    let parse = support::parse_verified("<p>a</p><?php ?><p>b</p>");
    let kinds: Vec<SyntaxKind> = parse
        .tree()
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .map(|token| token.kind())
        .collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::InlineHtml,
            SyntaxKind::OpenTag,
            SyntaxKind::Whitespace,
            SyntaxKind::CloseTag,
            SyntaxKind::InlineHtml,
        ],
    );
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn echo_wraps_its_comma_separated_expressions() {
    let parse = support::parse_verified("<?php echo 'a', 1, $x;");
    let statement = parse
        .tree()
        .children()
        .find(|node| node.kind() == SyntaxKind::EchoStatement)
        .expect("an echo statement");
    let expression_kinds: Vec<SyntaxKind> =
        statement.children().map(|node| node.kind()).collect();
    assert_eq!(
        expression_kinds,
        vec![
            SyntaxKind::Literal,
            SyntaxKind::Literal,
            SyntaxKind::VariableReference,
        ],
    );
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn an_expression_statement_holds_its_expression_and_semicolon() {
    let parse = support::parse_verified("<?php $x;");
    let statement = parse
        .tree()
        .children()
        .find(|node| node.kind() == SyntaxKind::ExpressionStatement)
        .expect("an expression statement");
    assert_eq!(
        statement.children().next().map(|node| node.kind()),
        Some(SyntaxKind::VariableReference),
    );
    assert!(parse.diagnostics().is_empty());
}

#[test]
fn a_close_tag_terminates_a_statement_without_a_semicolon() {
    let parse = support::parse_verified("<?php echo 1 ?>");
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
}

#[test]
fn a_missing_semicolon_is_diagnosed_and_the_statement_completes() {
    // Zend rejects a missing `;` at end of input too; we diagnose it
    // but still deliver both complete statements.
    let parse = support::parse_verified("<?php echo 1 echo 2;");
    assert_eq!(
        parse
            .tree()
            .children()
            .filter(|node| node.kind() == SyntaxKind::EchoStatement)
            .count(),
        2,
    );
    assert_eq!(parse.diagnostics().len(), 1);
}

#[test]
fn an_unexpected_token_becomes_an_error_node_and_parsing_continues() {
    let parse = support::parse_verified("<?php + echo 1;");
    let kinds: Vec<SyntaxKind> = parse.tree().children().map(|node| node.kind()).collect();
    assert_eq!(kinds, vec![SyntaxKind::ErrorNode, SyntaxKind::EchoStatement]);
    assert_eq!(parse.diagnostics().len(), 1);
}

#[test]
fn echo_without_an_expression_is_diagnosed() {
    let parse = support::parse_verified("<?php echo ;");
    assert_eq!(parse.diagnostics().len(), 1);
    assert!(
        parse
            .tree()
            .children()
            .any(|node| node.kind() == SyntaxKind::EchoStatement),
    );
}

#[test]
fn lexer_and_parser_diagnostics_merge_in_source_order() {
    // Unterminated string: a lexer diagnostic at the opening quote,
    // then the parser's missing terminator at end of input.
    let parse = support::parse_verified("<?php echo 'open");
    let kinds: Vec<&SyntaxDiagnosticKind> =
        parse.diagnostics().iter().map(|diagnostic| &diagnostic.kind).collect();
    assert_eq!(kinds.len(), 2);
    assert!(matches!(kinds.first(), Some(SyntaxDiagnosticKind::Lexer(_))));
    assert!(matches!(kinds.get(1), Some(SyntaxDiagnosticKind::Parser(_))));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test parse`
Expected: FAIL to compile with `unresolved import` (`celerrate_syntax::parse`, `SyntaxDiagnosticKind` not found).

- [ ] **Step 3: Implement the merged diagnostic, the grammar, and `parse()`**

Append to `crates/celerrate_syntax/src/diagnostic.rs`:

```rust
/// One diagnostic from the syntax layer, wherever it arose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxDiagnosticKind {
    Lexer(LexerDiagnosticKind),
    Parser(ParserDiagnosticKind),
}

/// A syntax diagnostic: lexer and parser findings merged into one
/// stream, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxDiagnostic {
    pub kind: SyntaxDiagnosticKind,
    pub range: TextRange,
}
```

Create `crates/celerrate_syntax/src/parser/grammar.rs`:

```rust
//! The grammar rules of this plan: a source file over inline HTML,
//! tags, `echo` statements, and expression statements with minimal
//! primary expressions. Every rule keeps the parser's guarantees: it
//! always makes progress and completes the node it opened.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::{CompletedMarker, Parser};

pub(super) fn source_file(parser: &mut Parser) {
    let marker = parser.start();
    while let Some(kind) = parser.current() {
        match kind {
            SyntaxKind::InlineHtml
            | SyntaxKind::OpenTag
            | SyntaxKind::OpenTagEcho
            | SyntaxKind::ShortOpenTag
            | SyntaxKind::CloseTag => parser.bump(),
            _ => statement(parser),
        }
    }
    marker.complete(parser, SyntaxKind::SourceFile);
}

fn statement(parser: &mut Parser) {
    match parser.current() {
        Some(SyntaxKind::Echo) => echo_statement(parser),
        Some(kind) if is_expression_start(kind) => expression_statement(parser),
        Some(_) => error_statement(parser),
        None => {}
    }
}

fn echo_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump();
    loop {
        if expression(parser).is_none() {
            break;
        }
        if parser.at(SyntaxKind::Comma) {
            parser.bump();
        } else {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::EchoStatement);
}

fn expression_statement(parser: &mut Parser) {
    let marker = parser.start();
    if expression(parser).is_none() {
        // Unreachable through `statement`'s dispatch, but recovery must
        // never leave an open marker or an empty node behind.
        marker.abandon(parser);
        return;
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::ExpressionStatement);
}

/// One token no rule accepts, wrapped and reported; the guaranteed
/// progress of the statement loop.
fn error_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.diagnose_current(ParserDiagnosticKind::UnexpectedToken);
    parser.bump();
    marker.complete(parser, SyntaxKind::ErrorNode);
}

/// PHP requires `;` after a statement except immediately before `?>`,
/// where it is optional (end of input does not exempt it: Zend rejects
/// that too, so we diagnose it — zero-width, after the last token).
fn terminate_statement(parser: &mut Parser) {
    if parser.at(SyntaxKind::Semicolon) {
        parser.bump();
        return;
    }
    if parser.at(SyntaxKind::CloseTag) {
        return;
    }
    parser.diagnose_missing(ParserDiagnosticKind::ExpectedSemicolon);
}

fn is_expression_start(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IntegerLiteral
            | SyntaxKind::FloatLiteral
            | SyntaxKind::SingleQuotedString
            | SyntaxKind::Variable
    )
}

/// A minimal primary expression; the Pratt machinery of the next plan
/// replaces this dispatch.
fn expression(parser: &mut Parser) -> Option<CompletedMarker> {
    let kind = match parser.current() {
        Some(kind) if is_expression_start(kind) => kind,
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
            return None;
        }
    };
    let marker = parser.start();
    parser.bump();
    let node_kind = match kind {
        SyntaxKind::Variable => SyntaxKind::VariableReference,
        _ => SyntaxKind::Literal,
    };
    Some(marker.complete(parser, node_kind))
}
```

In `crates/celerrate_syntax/src/parser.rs`: declare `mod grammar;` and add the entry point:

```rust
/// Runs the parser over a token stream: events for the builder plus
/// structured diagnostics. Never fails; degenerate input yields
/// `ErrorNode` wreckage and diagnostics.
pub(crate) fn run(tokens: &[crate::token::Token]) -> (Vec<Event>, Vec<ParserDiagnostic>) {
    let mut parser = Parser::new(TokenSource::new(tokens));
    grammar::source_file(&mut parser);
    (parser.events, parser.diagnostics)
}
```

Create `crates/celerrate_syntax/src/parse.rs`:

```rust
use crate::diagnostic::{SyntaxDiagnostic, SyntaxDiagnosticKind};
use crate::tree::SyntaxNode;
use crate::tree::builder::build_tree;

/// The result of parsing one source file: the lossless syntax tree and
/// every diagnostic, lexer and parser merged, in source order.
#[derive(Debug, Clone)]
pub struct Parse {
    root: rowan::GreenNode,
    diagnostics: Vec<SyntaxDiagnostic>,
}

impl Parse {
    /// The root of the red tree: always a `SourceFile`.
    pub fn tree(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.root.clone())
    }

    pub fn diagnostics(&self) -> &[SyntaxDiagnostic] {
        &self.diagnostics
    }
}

/// Parses decoded PHP source text into a lossless syntax tree plus
/// structured diagnostics. Always terminates, never fails: degenerate
/// input yields `ErrorNode`s and diagnostics, never a crash or a hole
/// in the tree; `parse(source).tree().text() == source`, always.
pub fn parse(source: &str) -> Parse {
    let (tokens, lexer_diagnostics) = crate::lexer::lex(source);
    let (events, parser_diagnostics) = crate::parser::run(&tokens);
    let root = build_tree(source, &tokens, events);
    let mut diagnostics: Vec<SyntaxDiagnostic> = lexer_diagnostics
        .into_iter()
        .map(|diagnostic| SyntaxDiagnostic {
            kind: SyntaxDiagnosticKind::Lexer(diagnostic.kind),
            range: diagnostic.range,
        })
        .chain(parser_diagnostics.into_iter().map(|diagnostic| SyntaxDiagnostic {
            kind: SyntaxDiagnosticKind::Parser(diagnostic.kind),
            range: diagnostic.range,
        }))
        .collect();
    // Stable sort: on equal ranges, lexer diagnostics stay first.
    diagnostics.sort_by_key(|diagnostic| (diagnostic.range.start(), diagnostic.range.end()));
    Parse { root, diagnostics }
}
```

Update `crates/celerrate_syntax/src/lib.rs` — crate docs no longer defer the parser, new module, new exports:

```rust
//! PHP syntax for the Celerrate toolchain: [`lex`] turns decoded source
//! text into a lossless token stream, and [`parse`] builds the lossless
//! concrete syntax tree on top of it, plus structured diagnostics.
//! Nothing here ever fails: degenerate input yields error tokens,
//! `ErrorNode`s, and diagnostics, never a crash.

mod cursor;
mod diagnostic;
mod lexer;
mod parse;
mod parser;
mod syntax_kind;
mod token;
mod tree;

pub use diagnostic::{
    LexerDiagnostic, LexerDiagnosticKind, ParserDiagnosticKind, SyntaxDiagnostic,
    SyntaxDiagnosticKind,
};
pub use lexer::lex;
pub use parse::{Parse, parse};
pub use syntax_kind::SyntaxKind;
pub use token::Token;
pub use tree::{PhpLanguage, SyntaxElement, SyntaxNode, SyntaxToken};
```

(`ParserDiagnostic` the struct stays internal: the public merged stream is `SyntaxDiagnostic`.)

Finally, remove the two temporary `dead_code` allows now that `parse()` consumes everything: the inner `#![allow(dead_code)]` block at the top of `parser.rs` (Task 4) and the `#[allow(dead_code)]` on `build_tree` in `tree/builder.rs` (Task 3), each with its `TEMPORARY` comment.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax --test parse`
Expected: PASS (9 tests).

- [ ] **Step 5: Full check and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/celerrate_syntax/src crates/celerrate_syntax/tests/parse.rs crates/celerrate_syntax/tests/support/mod.rs
git commit -m "✨ feat(syntax): parse inline HTML, echo, and expression statements"
```

---

### Task 6: Parse-tree snapshot corpus

A dedicated corpus for the parser (separate from the lexer's `tests/corpus/`), seeded with files the current grammar covers and grown by every later plan. Snapshots render the indented tree plus the merged diagnostics.

**Files:**
- Create: `crates/celerrate_syntax/tests/parse_corpus.rs`, `crates/celerrate_syntax/tests/parse_corpus/hello.php`, `.../tags.php`, `.../statements.php`, `.../recovery.php`
- Modify: `crates/celerrate_syntax/tests/support/mod.rs`

**Interfaces:**
- Consumes: `parse`, `SyntaxElement`, `SyntaxDiagnostic` (Task 5).
- Produces: `support::render_parse(source: &str) -> String` — the rendering shared by this corpus and any later behavior test that wants a readable tree.

- [ ] **Step 1: Add the renderer and the corpus test**

Append to `crates/celerrate_syntax/tests/support/mod.rs`:

```rust
/// Renders a parse as an indented tree (`Kind@start..end`, token text
/// quoted) plus a diagnostics footer, asserting losslessness on the way.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_parse(source: &str) -> String {
    use std::fmt::Write as _;

    let parse = parse_verified(source);
    let mut output = String::new();
    render_element(&mut output, parse.tree().into(), 0);
    if !parse.diagnostics().is_empty() {
        let _ = writeln!(output, "---");
        for diagnostic in parse.diagnostics() {
            let _ = writeln!(
                output,
                "{:?} @ {}..{}",
                diagnostic.kind,
                u32::from(diagnostic.range.start()),
                u32::from(diagnostic.range.end()),
            );
        }
    }
    output
}

#[allow(dead_code)]
fn render_element(output: &mut String, element: celerrate_syntax::SyntaxElement, depth: usize) {
    use std::fmt::Write as _;

    let indent = "  ".repeat(depth);
    match element {
        celerrate_syntax::SyntaxElement::Node(node) => {
            let range = node.text_range();
            let _ = writeln!(
                output,
                "{indent}{:?}@{}..{}",
                node.kind(),
                u32::from(range.start()),
                u32::from(range.end()),
            );
            for child in node.children_with_tokens() {
                render_element(output, child, depth + 1);
            }
        }
        celerrate_syntax::SyntaxElement::Token(token) => {
            let range = token.text_range();
            let _ = writeln!(
                output,
                "{indent}{:?}@{}..{} {:?}",
                token.kind(),
                u32::from(range.start()),
                u32::from(range.end()),
                token.text(),
            );
        }
    }
}
```

Create `crates/celerrate_syntax/tests/parse_corpus.rs`:

```rust
//! Snapshot corpus for the parser: every `tests/parse_corpus/*.php`
//! file is parsed and snapshotted as an indented tree plus diagnostics.
//! The corpus grows with the grammar; each plan adds the files its
//! rules cover. The lossless invariant is asserted on every file.
#![allow(clippy::expect_used)]

mod support;

#[test]
fn parse_corpus() {
    insta::glob!("parse_corpus/*.php", |path| {
        let source = std::fs::read_to_string(path).expect("corpus file is readable");
        insta::assert_snapshot!(support::render_parse(&source));
    });
}
```

Create the seed files.

`crates/celerrate_syntax/tests/parse_corpus/hello.php`:

```php
<p>Before</p>
<?php

echo 'Hello, Celerrate!';

?>
<p>After</p>
```

`crates/celerrate_syntax/tests/parse_corpus/tags.php`:

```php
<?= 42 ?>
<? echo 3.14; ?>
```

`crates/celerrate_syntax/tests/parse_corpus/statements.php`:

```php
<?php
echo 'a', 'b', 'c';
$user;
echo $count;
1.5;
'string';
```

`crates/celerrate_syntax/tests/parse_corpus/recovery.php`:

```php
<?php
echo ;
echo 1 echo 2;
+
echo 'unterminated;
```

- [ ] **Step 2: Run, review, accept the snapshots**

Run: `cargo test -p celerrate_syntax --test parse_corpus`
Expected: FAIL — insta writes pending `.snap.new` files for the four corpus files.

Review each pending snapshot **against the grammar's promises** before accepting: trees lossless (implicitly asserted), trivia between siblings, `EchoStatement`/`ExpressionStatement`/`Literal`/`VariableReference`/`ErrorNode` where expected, `recovery.php` showing `ExpectedExpression`, `ExpectedSemicolon`, `UnexpectedToken`, and the lexer's `UnterminatedString` merged in source order. Then:

Run: `cargo insta accept` (from the repository root; installs with `cargo install cargo-insta` if absent)
Run: `cargo test -p celerrate_syntax --test parse_corpus`
Expected: PASS.

- [ ] **Step 3: Full check and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/celerrate_syntax/tests
git commit -m "✅ test(syntax): snapshot the parse corpus"
```

---

### Task 7: Fuzz the whole pipeline

**Files:**
- Create: `fuzz/fuzz_targets/parse.rs`, `fuzz/corpus/parse/` (seeded)
- Modify: `fuzz/Cargo.toml`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `celerrate_syntax::parse`, `celerrate_source::SourceText::from_bytes`.
- Produces: the `parse` fuzz target; CI runs it beside `lex`.

- [ ] **Step 1: Add the target**

Create `fuzz/fuzz_targets/parse.rs`:

```rust
//! Arbitrary bytes through `SourceText::from_bytes` then the full
//! parsing pipeline. Invariants: no panic anywhere, the tree is
//! lossless, and parsing terminates (libFuzzer's timeout catches
//! hangs — guaranteed progress is the property under test).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = celerrate_source::SourceText::from_bytes(data) else {
        return;
    };
    let parse = celerrate_syntax::parse(source.text());
    assert_eq!(
        parse.tree().text().to_string(),
        source.text(),
        "the tree must be lossless"
    );
});
```

Append to `fuzz/Cargo.toml`:

```toml
[[bin]]
name = "parse"
path = "fuzz_targets/parse.rs"
test = false
doc = false
bench = false
```

Seed the corpus from the lexer's committed seeds (same corpus policy as the lexer part: seeds are committed, findings become seeds):

```bash
cp -r fuzz/corpus/lex fuzz/corpus/parse
```

- [ ] **Step 2: Smoke-run locally (if nightly is available)**

Run: `cargo +nightly fuzz run parse -- -max_total_time=60`
Expected: no crash. If nightly is not installed locally, rely on CI (next step) and say so in the task summary.

- [ ] **Step 3: Wire CI**

In `.github/workflows/ci.yml`, in the `fuzz` job, directly after the `cargo +nightly fuzz run lex` line, add:

```yaml
      - run: cargo +nightly fuzz run parse -- -max_total_time=180
```

(The existing crash-artifact upload step already covers both targets: it grabs all of `fuzz/artifacts/`.)

- [ ] **Step 4: Full check and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add fuzz .github/workflows/ci.yml
git commit -m "✅ test(fuzz): fuzz the parser pipeline"
```

---

## Done means

- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean; `cargo deny check` clean.
- `parse("<?php echo 'Hello';")` returns a `SourceFile` tree whose text is the input, byte for byte — and so does every corpus file and fuzz input.
- Broken input produces partial trees plus merged diagnostics, never a failure.
- CI runs the `parse` fuzz target.
- The next plan (expressions) starts from `expression()` in `grammar.rs` and `CompletedMarker` gaining `precede`.
