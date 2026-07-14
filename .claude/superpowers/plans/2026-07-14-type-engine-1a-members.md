# Type Engine 1a — Members Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the invalidation boundary to class members: the member projection (`MemberTree`), inheritance linearization with the per-(class, member, kind) lookup firewall, and the structural settlement of the `source_symbol_table` serial-rebuild debt.

**Architecture:** The shared declaration traversal (`item_nodes`) starts descending into member lists and numbering member nodes and anonymous classes, so `AstIdMap` and both projections keep agreeing by construction. The top-level `ItemTree` keeps its exact current payload; members land in a **sibling projection** (`member_tree`), so a member signature edit invalidates member consumers without touching `item_tree` — which is precisely what spares `source_symbol_table` from rebuilding on member edits (the inherited debt, settled structurally). Linearization is an iterative walk with a visited set inside one tracked query (never self-recursive: no salsa cycle by construction), and member lookups go through an interned per-(class, name, kind) query, the same firewall pattern as `lookup_symbol`.

**Tech Stack:** Rust workspace, salsa 0.27 (tracked/interned queries), rowan-style CST via `celerrate_syntax`, insta not needed here (plain assertions), TDD with `cargo test`.

**Spec:** `.claude/superpowers/specs/2026-07-14-type-engine-design.md` section 2 (the member boundary), section 9 (the `source_symbol_table` debt). Read it before starting.

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result`/`Option`; test modules open with `#![allow(clippy::unwrap_used)]` (add `expect_used`, `indexing_slicing`, `panic` only when used).
- TDD: every task writes its failing test first, watches it fail, then implements minimally.
- Strict layering: `celerrate_semantics` may depend on `celerrate_syntax`, `celerrate_db`, `celerrate_project`, `celerrate_stubs`, `celerrate_source`, `celerrate_diagnostics` — never upward. New code in this plan touches only `celerrate_syntax` (two pure accessors) and `celerrate_semantics`.
- Determinism: no wall clock, no randomness, no environment reads inside queries. Every collection that crosses a query boundary is deterministically ordered.
- Everything in English, full words (no abbreviated identifiers; standard acronyms fine).
- Commits: gitmoji + Conventional Commits (`✅ test(semantics): …`, `✨ feat(semantics): …`, `♻️ refactor(semantics): …`). Never add Claude attribution of any kind.
- Run before every commit: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`; run `cargo fmt --all` and re-stage when it changes files.
- PHP semantics reminders used throughout: method names are ASCII case-insensitive; property, class-constant, and enum-case names are case-sensitive.

## File Structure

- Modify: `crates/celerrate_semantics/src/item_nodes.rs` — traversal descends into member lists; members and anonymous class-likes become numbered declaration nodes; `ItemNode` gains `owner`.
- Modify: `crates/celerrate_semantics/src/ast_id.rs` — tests only (the numbering contract widens).
- Modify: `crates/celerrate_semantics/src/items.rs` — `lower()` skips member-owned and nameless nodes (they consume numbering, never a top-level `Declaration`).
- Modify: `crates/celerrate_syntax/src/ast/extensions.rs` — `docblock_token`, `type_text`, `expression_text` (pure accessors, no layering change).
- Create: `crates/celerrate_semantics/src/members.rs` — `MemberTree` projection: kinds, flags, signatures as unresolved names, docblock text, trait-use records with adaptations.
- Modify: `crates/celerrate_semantics/src/queries.rs` — the `member_tree` salsa query.
- Create: `crates/celerrate_semantics/src/linearize.rs` — `linearized_class` query, `LinearizedClass`, ancestry edges, cycle posture, magic markers.
- Create: `crates/celerrate_semantics/src/member_lookup.rs` — `MemberQuery` (interned) + `lookup_member`, the firewall.
- Modify: `crates/celerrate_semantics/src/lookup.rs` — `lookup_class_declaration` (origin-carrying source lookup) and `analyzed_file_index`.
- Modify: `crates/celerrate_semantics/src/lib.rs` — module wiring and exports.
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs` — the new edit classes, including the `source_symbol_table` debt proof.

Naming note used everywhere below: a **class key** is `folded_symbol_key(SymbolSpace::ClassLike, fully_qualified)` — the existing folding. A **member key** folds by kind: methods `to_ascii_lowercase()`, everything else verbatim.

---

### Task 1: Members and anonymous class-likes become declaration nodes

The shared traversal defines "the declaration nodes"; both `AstIdMap` and `ItemTree` consume it, so extending it extends both numberings in agreement. After this task: method, property, constant, and enum-case nodes inside member lists are numbered items carrying their owner; nameless class-likes (anonymous classes) are numbered items; the top-level `ItemTree` payload is byte-identical to before (members and nameless nodes consume an index and project nothing).

**Files:**
- Modify: `crates/celerrate_semantics/src/item_nodes.rs`
- Modify: `crates/celerrate_semantics/src/items.rs`
- Modify: `crates/celerrate_semantics/src/ast_id.rs` (tests)

**Interfaces:**
- Consumes: `celerrate_syntax::ast` accessors (`ClassDeclaration::name_token`, `member_list`, …), all existing.
- Produces: `pub(crate) struct ItemNode { pub(crate) node: SyntaxNode, pub(crate) namespace: String, pub(crate) owner: Option<u32> }` — `owner` is the item index of the enclosing class-like for direct members, `None` everywhere else. `pub(crate) fn item_nodes(root: &SyntaxNode) -> Vec<ItemNode>` keeps its signature. Tasks 3–5 group members by `owner`; Task 8 anchors anonymous-class identity on the numbered index.

- [ ] **Step 1: Write the failing tests for the widened numbering**

In `crates/celerrate_semantics/src/ast_id.rs`, replace the two tests that pin the old contract (`members_are_not_declaration_nodes`, `nameless_declarations_are_not_numbered`) with the new contract:

```rust
    #[test]
    fn members_are_numbered_declaration_nodes() {
        // Numbering: class = 0, const B = 1, method = 2, const C = 3.
        let (_parse, map) =
            map_of("<?php class A { const B = 1; public function method() {} } const C = 1;");
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::ClassDeclaration,
                SyntaxKind::ConstantDeclaration,
                SyntaxKind::MethodDeclaration,
                SyntaxKind::ConstantDeclaration,
            ],
        );
    }

    #[test]
    fn properties_and_enum_cases_are_numbered() {
        let (_parse, map) = map_of(
            "<?php class A { public int $count = 0; } enum Suit { case Hearts; case Spades; }",
        );
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::ClassDeclaration,
                SyntaxKind::PropertyDeclaration,
                SyntaxKind::EnumDeclaration,
                SyntaxKind::EnumCase,
                SyntaxKind::EnumCase,
            ],
        );
    }

    #[test]
    fn anonymous_classes_are_numbered_with_their_members() {
        // The synthetic identity the spec gives anonymous classes is
        // their numbered position; their members are owned like any
        // other class's.
        let (_parse, map) =
            map_of("<?php function wrapper() { return new class { public function f() {} }; }");
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::ClassDeclaration,
                SyntaxKind::MethodDeclaration,
            ],
        );
    }

    #[test]
    fn a_function_declared_inside_a_method_body_is_numbered() {
        // The old traversal skipped the whole member list, so a named
        // function declared inside a method body was invisible — a
        // false negative this task closes in passing.
        let (_parse, map) =
            map_of("<?php class A { function m() { function nested() {} } }");
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::ClassDeclaration,
                SyntaxKind::MethodDeclaration,
                SyntaxKind::FunctionDeclaration,
            ],
        );
    }

    #[test]
    fn statement_edits_renumber_nothing_and_member_insertion_renumbers_later_nodes() {
        // The spec's recorded trade-off: numbering counts declaration
        // nodes only, so statement edits still renumber nothing; adding
        // a member (or an anonymous class) renumbers later declarations
        // in the file. Accepted, and pinned here.
        let (_parse, before) = map_of("<?php class A { function m() { $x = 1; } } class B {}");
        let (_parse, statement_edit) =
            map_of("<?php class A { function m() { $x = 2; $y = 3; } } class B {}");
        assert_eq!(kinds_of(&before), kinds_of(&statement_edit));

        let (_parse, member_added) =
            map_of("<?php class A { function m() { $x = 1; } function n() {} } class B {}");
        assert_eq!(
            kinds_of(&member_added),
            vec![
                SyntaxKind::ClassDeclaration,
                SyntaxKind::MethodDeclaration,
                SyntaxKind::MethodDeclaration,
                SyntaxKind::ClassDeclaration,
            ],
        );
    }
```

In `crates/celerrate_semantics/src/items.rs`, the existing tests `members_are_not_projected`, `anonymous_classes_are_not_projected`, and `a_body_edit_produces_an_identical_item_tree` must keep passing unchanged (the top-level payload does not move). Update the one numbering-sensitive expectation and add the owner-skip pin:

```rust
    #[test]
    fn member_nodes_consume_numbering_but_project_nothing() {
        // Numbering: class = 0, const member = 1, method = 2, so the
        // constant after the class is item 3 — and the projection still
        // carries exactly the class and the top-level constant.
        let tree = tree_of("<?php class A { const B = 1; function m() {} } const C = 1;");
        let kinds_and_ids: Vec<(DeclarationKind, u32)> = tree
            .declarations
            .iter()
            .map(|declaration| (declaration.kind, declaration.ast_id.index))
            .collect();
        assert_eq!(
            kinds_and_ids,
            vec![(DeclarationKind::Class, 0), (DeclarationKind::Constant, 3)],
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics ast_id:: items::tests::member_nodes_consume 2>&1 | tail -20`
Expected: FAIL — the new `ast_id` tests see the old numbering (members absent), and `member_nodes_consume_numbering_but_project_nothing` sees index 1 for the constant.

- [ ] **Step 3: Widen the traversal**

Replace the body of `crates/celerrate_semantics/src/item_nodes.rs` below the module documentation (update the module documentation too: the old text says the walk "never descends into a `MemberList`", which stops being true):

```rust
//! The shared traversal defining "the declaration nodes" of one file.
//! `AstIdMap` and both projections consume it, so their numbering
//! agrees by construction: preorder tree order, the enclosing
//! namespace tracked as walk state.
//!
//! The walk descends into control-flow blocks, function bodies, and —
//! since the type-engine sub-project — member lists: methods,
//! properties, constants, and enum cases are numbered items carrying
//! their owning class-like's index, and nameless class-likes
//! (anonymous classes) are numbered items whose position is their
//! synthetic identity. Numbering counts declaration nodes only, so
//! statement edits renumber nothing; adding a member or an anonymous
//! class renumbers later declarations in the file — the recorded,
//! accepted trade-off (spec section 2).
//!
//! A statement-form `namespace Foo;` nested inside a control-flow
//! block (error-recovery input, invalid PHP) deliberately switches the
//! walk state for everything that follows the enclosing block:
//! tolerated and deterministic, never a failure.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

/// One declaration node with its enclosing namespace (`""` is global)
/// and, for a direct member of a class-like, the item index of that
/// class-like.
pub(crate) struct ItemNode {
    pub(crate) node: SyntaxNode,
    pub(crate) namespace: String,
    pub(crate) owner: Option<u32>,
}

/// Every declaration node of the file, in tree order.
pub(crate) fn item_nodes(root: &SyntaxNode) -> Vec<ItemNode> {
    let mut items = Vec::new();
    let mut namespace = String::new();
    collect(root, &mut namespace, None, &mut items);
    items
}

fn push(items: &mut Vec<ItemNode>, node: &SyntaxNode, namespace: &str, owner: Option<u32>) -> Option<u32> {
    let index = u32::try_from(items.len()).ok()?;
    items.push(ItemNode {
        node: node.clone(),
        namespace: namespace.to_owned(),
        owner,
    });
    Some(index)
}

fn collect(
    node: &SyntaxNode,
    namespace: &mut String,
    owner: Option<u32>,
    items: &mut Vec<ItemNode>,
) {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::NamespaceDeclaration => {
                push(items, &child, namespace, None);
                let Some(declaration) = ast::NamespaceDeclaration::cast(child.clone()) else {
                    continue;
                };
                let declared = declaration
                    .name()
                    .map(|name| name.text())
                    .unwrap_or_default();
                match declaration.block() {
                    // Brace form: the name scopes the block, nothing else.
                    Some(block) => {
                        let mut inner = declared;
                        collect(block.syntax(), &mut inner, None, items);
                    }
                    // Statement form: the name applies to what follows.
                    None => *namespace = declared,
                }
            }
            _ => {
                if is_class_like(&child) {
                    // Named or anonymous: both are numbered; the
                    // anonymous one's position is its identity. Direct
                    // members walk under this index; everything deeper
                    // (method bodies) walks ownerless again.
                    let index = push(items, &child, namespace, owner);
                    collect(&child, namespace, index, items);
                    continue;
                }
                if is_member(&child, owner) || is_ownerless_item(&child) {
                    push(items, &child, namespace, if is_member(&child, owner) { owner } else { None });
                }
                // Members own only their list-level children: a method
                // body's declarations are ownerless.
                let descend_owner = if child.kind() == SyntaxKind::MemberList {
                    owner
                } else {
                    None
                };
                collect(&child, namespace, descend_owner, items);
            }
        }
    }
}

/// Any class-like declaration node, named or not: classes, interfaces,
/// traits, enums. Interfaces, traits, and enums are always named in
/// valid PHP, but error recovery may produce nameless ones; a nameless
/// class-like is numbered (anonymous classes need the identity) and
/// projected by nobody unless a projection wants it.
fn is_class_like(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TraitDeclaration
            | SyntaxKind::EnumDeclaration
    )
}

/// A direct member of the enclosing class-like: numbered under its
/// owner. `owner` is set exactly while walking a member list.
fn is_member(node: &SyntaxNode, owner: Option<u32>) -> bool {
    owner.is_some()
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == SyntaxKind::MemberList)
        && matches!(
            node.kind(),
            SyntaxKind::MethodDeclaration
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::ConstantDeclaration
                | SyntaxKind::EnumCase
        )
}

/// The ownerless item kinds, exactly the old `is_item` minus the
/// class-likes (handled above). A `ConstantDeclaration` here is a
/// top-level `const`: the member form is caught by `is_member` first.
fn is_ownerless_item(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::UseDeclaration | SyntaxKind::ConstantDeclaration => true,
        SyntaxKind::FunctionDeclaration => ast::FunctionDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        _ => false,
    }
}
```

Two consequences to wire in `crates/celerrate_semantics/src/items.rs`:

1. `ItemTree::from_root` keeps enumerating `item_nodes`, but `lower()` must skip what is not a top-level declaration. Add the guard at the top of `lower()`:

```rust
fn lower(item: &ItemNode, ast_id: AstId, tree: &mut ItemTree) {
    // Members consume numbering but never project a top-level
    // declaration; the member projection (`MemberTree`) owns them.
    if item.owner.is_some() {
        return;
    }
    match item.node.kind() {
        // … existing arms unchanged …
```

2. Nameless class-likes already project nothing (`push_declaration` returns on a missing name token) — no change needed there, but the anonymous class now consumes an index, which the `member_nodes_consume_numbering_but_project_nothing` style of assertion pins.

- [ ] **Step 4: Run the crate's tests, fix the numbering-sensitive stragglers**

Run: `cargo test --package celerrate_semantics 2>&1 | tail -20`

Expected: the new tests pass. Two kinds of stragglers are legitimate and must be updated to the new numbering, not worked around: (a) any test asserting a literal `ast_id.index` over a source sample containing members or anonymous classes; (b) the `ast_id.rs` doc-order test if a sample gains numbered nodes. `crates/celerrate_semantics/tests/incremental_consistency.rs` and the cache tests in `celerrate_cli` must pass **unchanged** — if one fails, the traversal broke an invariant; stop and fix the traversal, not the test. Persisted-cache compatibility is not a concern: the pack key includes the binary self-hash, so packs written by the old numbering never seed the new one.

- [ ] **Step 5: Run the full workspace gates**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): number members and anonymous classes as declaration nodes"
```

---

### Task 2: Docblock and written-text accessors on the syntax layer

The member projection needs three pure readings the typed AST does not offer yet: the docblock token attached to a declaration node, the written text of a type, and the comparable written text of a default-value expression. They are hand-written accessor logic, so they live in `extensions.rs` next to their kin.

**Files:**
- Modify: `crates/celerrate_syntax/src/ast/extensions.rs`

**Interfaces:**
- Produces: `pub fn docblock_token(node: &SyntaxNode) -> Option<SyntaxToken>` — the nearest preceding sibling `DocComment` with only whitespace between it and the node; `pub fn type_text(ty: &Type) -> String` — non-trivia descendant tokens joined with no separator; `pub fn expression_text(expression: &Expression) -> String` — non-trivia descendant tokens joined with one space. Tasks 3–5 consume all three.

- [ ] **Step 1: Write the failing tests**

Append a test module to `crates/celerrate_syntax/src/ast/extensions.rs` (the file has none yet):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::super::generated::{ClassDeclaration, MethodDeclaration};
    use super::{docblock_token, expression_text, type_text};
    use crate::ast::AstNode;
    use crate::{SyntaxKind, SyntaxNode};

    fn first_node(source: &str, kind: SyntaxKind) -> SyntaxNode {
        celerrate_syntax_parse(source)
            .descendants()
            .find(|node| node.kind() == kind)
            .unwrap()
    }

    fn celerrate_syntax_parse(source: &str) -> SyntaxNode {
        crate::parse(source).tree()
    }

    #[test]
    fn the_docblock_directly_above_a_declaration_attaches() {
        let class = first_node(
            "<?php\n/** @template T */\nclass Collection {}",
            SyntaxKind::ClassDeclaration,
        );
        assert_eq!(
            docblock_token(&class).map(|token| token.text().to_owned()),
            Some("/** @template T */".to_owned()),
        );
    }

    #[test]
    fn a_member_docblock_attaches_inside_the_member_list() {
        // Trivia flushes into the open node, so a member's docblock is
        // its preceding sibling inside the `MemberList`.
        let method = first_node(
            "<?php class A {\n    /** @return int */\n    public function f() {}\n}",
            SyntaxKind::MethodDeclaration,
        );
        assert_eq!(
            docblock_token(&method).map(|token| token.text().to_owned()),
            Some("/** @return int */".to_owned()),
        );
    }

    #[test]
    fn a_line_comment_or_a_sibling_between_breaks_attachment() {
        // Only whitespace may sit between the docblock and the node:
        // the PHPDoc convention, and what keeps attachment unambiguous.
        let class = first_node(
            "<?php\n/** doc */\n// not for the class\nclass A {}",
            SyntaxKind::ClassDeclaration,
        );
        assert_eq!(docblock_token(&class), None);

        let second = celerrate_syntax_parse("<?php /** doc */ class First {} class Second {}")
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::ClassDeclaration)
            .nth(1)
            .unwrap();
        assert_eq!(docblock_token(&second), None);
    }

    #[test]
    fn type_text_strips_trivia_and_joins_without_separator() {
        let method = first_node(
            "<?php class A { function f(): Foo\\Bar /* trailing */ | null {} }",
            SyntaxKind::MethodDeclaration,
        );
        let method = MethodDeclaration::cast(method).unwrap();
        assert_eq!(type_text(&method.return_type().unwrap()), "Foo\\Bar|null");
    }

    #[test]
    fn expression_text_joins_tokens_with_one_space() {
        // The comparable form: token boundaries preserved (so `new Foo`
        // never collides with an identifier `newFoo`), trivia and
        // formatting collapsed (so a formatting-only edit is equal).
        let class = first_node(
            "<?php class A { public $x = new  Foo( 1,   2 ); }",
            SyntaxKind::ClassDeclaration,
        );
        let class = ClassDeclaration::cast(class).unwrap();
        let element = class
            .member_list()
            .unwrap()
            .member_declarations()
            .find_map(|member| match member {
                crate::ast::MemberDeclaration::PropertyDeclaration(property) => Some(property),
                _ => None,
            })
            .unwrap()
            .property_elements()
            .next()
            .unwrap();
        assert_eq!(
            expression_text(&element.expression().unwrap()),
            "new Foo ( 1 , 2 )",
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax extensions 2>&1 | tail -10`
Expected: FAIL to compile — `docblock_token`, `type_text`, `expression_text` do not exist.

- [ ] **Step 3: Implement the three accessors**

Add to `crates/celerrate_syntax/src/ast/extensions.rs` (top level, near the other free helpers), and extend the imports at the top of the file with `Type` from generated and `NodeOrToken` handling via the tree types already imported:

```rust
use super::generated::Type;

/// The docblock attached to one declaration node: the nearest
/// preceding sibling `DocComment` token with only whitespace between
/// it and the node. Trivia flushes into the node open at that point
/// (tree-builder policy), so a declaration's docblock is always a
/// preceding sibling, never a child. Anything else in between — a line
/// comment, another node — breaks attachment, the PHPDoc convention.
pub fn docblock_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut current = node.prev_sibling_or_token();
    while let Some(element) = current {
        let Some(token) = element.as_token() else {
            return None;
        };
        match token.kind() {
            SyntaxKind::Whitespace => current = element.prev_sibling_or_token(),
            SyntaxKind::DocComment => return element.into_token(),
            _ => return None,
        }
    }
    None
}

/// The written text of a type, trivia stripped, tokens joined with no
/// separator: `Foo\Bar|null`. Native type grammar never places two
/// name tokens adjacently, so the joined form is unambiguous.
pub fn type_text(ty: &Type) -> String {
    written_tokens(ty.syntax()).collect()
}

/// The comparable written form of an expression: trivia stripped,
/// tokens joined with one space. Token boundaries survive (so `new
/// Foo` never collides with an identifier `newFoo`), formatting does
/// not (so a formatting-only edit produces an equal value). This is
/// the projection typed judgments read for default values — its
/// *content* is the contract, not its prettiness.
pub fn expression_text(expression: &Expression) -> String {
    written_tokens(expression.syntax())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every non-trivia token text under `node`, in order.
fn written_tokens(node: &SyntaxNode) -> impl Iterator<Item = String> + use<> {
    node.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia())
        .map(|token| token.text().to_owned())
}
```

Check the actual names before relying on them: `prev_sibling_or_token`, `descendants_with_tokens`, and `as_token`/`into_token` are the rowan cursor API re-exported through `crate::tree` — mirror whatever `tokens_of` and the builder already use in this file and in `crates/celerrate_syntax/src/tree/`. If `Whitespace` is not the single whitespace kind (check `SyntaxKind::is_trivia` in `crates/celerrate_syntax/src/syntax_kind.rs` — newlines may be a distinct kind), match on `kind.is_trivia() && kind != SyntaxKind::DocComment && kind != SyntaxKind::LineComment && kind != SyntaxKind::BlockComment` instead, i.e. *whitespace-like trivia continues the scan, comment trivia breaks it, a `DocComment` resolves it*. The test in Step 1 (line comment breaks attachment) is the contract; implement to it.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax extensions 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): docblock attachment and written-text accessors"
```

---

### Task 3: The member projection — kinds, names, flags, grouping

`MemberTree` is the sibling projection of `ItemTree`: per class-like declaration, its direct members with kind, name, flags, and `AstId` — range-free and `Eq`-comparable, so a method body edit produces an identical value. Signatures and docblocks join in Task 4; trait-use records in Task 5.

**Files:**
- Create: `crates/celerrate_semantics/src/members.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (add `mod members;` and `pub use members::{ClassMembers, Member, MemberFlags, MemberKind, MemberTree, Visibility};`)

**Interfaces:**
- Consumes: `item_nodes` / `ItemNode.owner` (Task 1); `celerrate_syntax::ast` member accessors.
- Produces:

```rust
pub enum MemberKind { Method, Property, ClassConstant, EnumCase }
pub enum Visibility { Public, Protected, Private }
pub struct MemberFlags {
    pub visibility: Visibility, // Public when unwritten (also `var`)
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
}
pub struct Member {
    pub kind: MemberKind,
    pub name: String,      // original spelling; property names WITHOUT the `$`
    pub flags: MemberFlags,
    pub ast_id: AstId,
    // Task 4 adds: signature: MemberSignature, docblock: Option<String>
}
pub struct ClassMembers {
    pub kind: DeclarationKind,     // Class | Interface | Trait | Enum
    pub name: Option<String>,      // None for anonymous class-likes
    pub namespace: String,
    pub ast_id: AstId,             // the synthetic identity of an anonymous class
    pub members: Vec<Member>,      // tree order
    // Task 4 adds: docblock: Option<String>; Task 5 adds: trait_uses: Vec<TraitUse>
}
pub struct MemberTree { pub classes: Vec<ClassMembers> } // tree order
impl MemberTree { pub fn from_root(file: FileId, root: &SyntaxNode) -> Self }
```

All types derive `Debug, Clone, PartialEq, Eq` (and `Hash` on the leaf enums/structs, matching `items.rs`).

- [ ] **Step 1: Write the failing tests**

In the new `crates/celerrate_semantics/src/members.rs`, write the types as stubs plus this test module (mirror the `items.rs` test style):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::FileId;

    use super::{MemberKind, MemberTree, Visibility};
    use crate::items::DeclarationKind;

    fn tree_of(source: &str) -> MemberTree {
        MemberTree::from_root(FileId::new(0), &celerrate_syntax::parse(source).tree())
    }

    fn only_class(source: &str) -> super::ClassMembers {
        let tree = tree_of(source);
        assert_eq!(tree.classes.len(), 1, "expected one class-like");
        tree.classes.into_iter().next().unwrap()
    }

    #[test]
    fn every_member_kind_is_projected_in_tree_order() {
        let class = only_class(
            "<?php class A {\n\
                 const LIMIT = 1;\n\
                 public int $count = 0;\n\
                 public function method(): void {}\n\
             }",
        );
        let kinds_and_names: Vec<(MemberKind, &str)> = class
            .members
            .iter()
            .map(|member| (member.kind, member.name.as_str()))
            .collect();
        assert_eq!(
            kinds_and_names,
            vec![
                (MemberKind::ClassConstant, "LIMIT"),
                (MemberKind::Property, "count"),
                (MemberKind::Method, "method"),
            ],
        );
        assert_eq!(class.kind, DeclarationKind::Class);
        assert_eq!(class.name.as_deref(), Some("A"));
    }

    #[test]
    fn enum_cases_are_members_of_their_enum() {
        let class = only_class("<?php enum Suit: string { case Hearts = 'h'; case Spades = 's'; }");
        assert_eq!(class.kind, DeclarationKind::Enum);
        let names: Vec<&str> = class.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["Hearts", "Spades"]);
        assert!(class.members.iter().all(|m| m.kind == MemberKind::EnumCase));
    }

    #[test]
    fn flags_are_read_from_the_modifier_list() {
        let class = only_class(
            "<?php abstract class A {\n\
                 protected static readonly int $x;\n\
                 abstract public function f(): void;\n\
                 final private function g() {}\n\
                 var $legacy;\n\
             }",
        );
        let property = &class.members[0];
        assert_eq!(property.flags.visibility, Visibility::Protected);
        assert!(property.flags.is_static);
        assert!(property.flags.is_readonly);
        let abstract_method = &class.members[1];
        assert_eq!(abstract_method.flags.visibility, Visibility::Public);
        assert!(abstract_method.flags.is_abstract);
        let final_method = &class.members[2];
        assert_eq!(final_method.flags.visibility, Visibility::Private);
        assert!(final_method.flags.is_final);
        let legacy = &class.members[3];
        assert_eq!(legacy.flags.visibility, Visibility::Public);
    }

    #[test]
    fn a_grouped_property_projects_one_member_per_element_sharing_identity() {
        let class = only_class("<?php class A { public $first, $second; }");
        let names: Vec<&str> = class.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second"]);
        assert_eq!(class.members[0].ast_id, class.members[1].ast_id);
    }

    #[test]
    fn a_grouped_class_constant_projects_one_member_per_element() {
        let class = only_class("<?php class A { const B = 1, C = 2; }");
        let names: Vec<&str> = class.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["B", "C"]);
        assert!(class.members.iter().all(|m| m.kind == MemberKind::ClassConstant));
    }

    #[test]
    fn an_anonymous_class_is_a_nameless_group_with_identity() {
        let tree = tree_of("<?php function wrapper() { return new class { public function f() {} }; }");
        assert_eq!(tree.classes.len(), 1);
        let class = tree.classes.first().unwrap();
        assert_eq!(class.name, None);
        // Numbering: wrapper = 0, anonymous class = 1, method = 2.
        assert_eq!(class.ast_id.index, 1);
        assert_eq!(class.members.first().map(|m| m.name.as_str()), Some("f"));
    }

    #[test]
    fn a_nested_class_owns_its_own_members() {
        // An anonymous class inside a method body belongs to itself,
        // not to the enclosing class.
        let tree = tree_of(
            "<?php class Outer { function f() { return new class { function inner() {} }; } }",
        );
        assert_eq!(tree.classes.len(), 2);
        let outer = &tree.classes[0];
        let inner = &tree.classes[1];
        assert_eq!(outer.name.as_deref(), Some("Outer"));
        assert_eq!(
            outer.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["f"],
        );
        assert_eq!(inner.name, None);
        assert_eq!(
            inner.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["inner"],
        );
    }

    #[test]
    fn a_method_body_edit_produces_an_identical_member_tree() {
        let before = tree_of("<?php class A { function f() { return 1; } }");
        let body_edit = tree_of("<?php class A { function f() { return 2; } }");
        assert_eq!(before, body_edit);
    }

    #[test]
    fn interfaces_and_traits_group_their_members() {
        let tree = tree_of(
            "<?php interface I { public function f(); const K = 1; }\n\
             trait T { public function helper() {} }",
        );
        assert_eq!(tree.classes.len(), 2);
        assert_eq!(tree.classes[0].kind, DeclarationKind::Interface);
        assert_eq!(tree.classes[1].kind, DeclarationKind::Trait);
    }

    #[test]
    fn malformed_input_projects_what_the_parser_recovered() {
        let tree = tree_of("<?php class Broken { public function ok() {}");
        assert_eq!(tree.classes.len(), 1);
        assert_eq!(
            tree.classes[0].members.first().map(|m| m.name.as_str()),
            Some("ok"),
        );
    }

    #[test]
    fn empty_and_memberless_files_project_nothing_surprising() {
        assert_eq!(tree_of("").classes, Vec::new());
        assert_eq!(tree_of("<?php function free() {}").classes, Vec::new());
        let class = only_class("<?php class Empty {}");
        assert_eq!(class.members, Vec::new());
    }
}
```

Note the deliberate contract choices the tests pin: property names are stored **without** the `$` (member lookup and PHP reflection both use the bare name); `var` maps to public; a free function creates no class group; **every class-like gets a group, members or not** (linearization needs the group to exist to know the class's file-local members).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics members 2>&1 | tail -10`
Expected: FAIL to compile (the types and `from_root` are stubs or absent).

- [ ] **Step 3: Implement the projection**

`crates/celerrate_semantics/src/members.rs`:

```rust
//! The member projection: the sibling of the item tree at member
//! granularity. Per class-like declaration of one file, its direct
//! members — kind, name, flags, identity, and (Task 4) signature as
//! unresolved names plus docblock text. Range-free and
//! `Eq`-comparable: a method body edit produces an identical value,
//! salsa backdates it, and member consumers are spared, while the
//! top-level `ItemTree` never changes at all — which is what spares
//! `source_symbol_table` (the settled serial-rebuild debt).

use celerrate_source::FileId;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::ast_id::AstId;
use crate::item_nodes::{ItemNode, item_nodes};
use crate::items::DeclarationKind;

/// The kind of one class member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Method,
    Property,
    ClassConstant,
    EnumCase,
}

/// PHP member visibility. Unwritten and `var` are public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

/// The modifier flags of one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemberFlags {
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
}

impl Default for MemberFlags {
    fn default() -> Self {
        Self {
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: false,
            is_readonly: false,
        }
    }
}

/// One member: original spelling (property names without the `$`),
/// flags, and stable identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Member {
    pub kind: MemberKind,
    pub name: String,
    pub flags: MemberFlags,
    pub ast_id: AstId,
}

/// One class-like declaration and its direct members, in tree order.
/// `name` is `None` for anonymous class-likes; their `ast_id` is the
/// synthetic identity the spec gives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMembers {
    pub kind: DeclarationKind,
    pub name: Option<String>,
    pub namespace: String,
    pub ast_id: AstId,
    pub members: Vec<Member>,
}

/// The member projection of one file, in tree order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemberTree {
    pub classes: Vec<ClassMembers>,
}

impl MemberTree {
    /// Projects one file's syntax tree. Shares the item-node traversal
    /// with `AstIdMap` and `ItemTree`, so all three numberings agree
    /// by construction.
    pub fn from_root(file: FileId, root: &SyntaxNode) -> Self {
        let nodes = item_nodes(root);
        let mut classes: Vec<(u32, ClassMembers)> = Vec::new();
        for (position, item) in nodes.iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                break;
            };
            let ast_id = AstId { file, index };
            if let Some(group) = class_group(item, ast_id) {
                classes.push((index, group));
                continue;
            }
            let Some(owner) = item.owner else { continue };
            let Some((_, group)) = classes
                .iter_mut()
                .rev()
                .find(|(class_index, _)| *class_index == owner)
            else {
                continue;
            };
            lower_member(&item.node, ast_id, group);
        }
        Self {
            classes: classes.into_iter().map(|(_, group)| group).collect(),
        }
    }
}

/// The group of a class-like item node; `None` for anything else.
fn class_group(item: &ItemNode, ast_id: AstId) -> Option<ClassMembers> {
    let (kind, name_token) = match item.node.kind() {
        SyntaxKind::ClassDeclaration => {
            let declaration = ast::ClassDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Class, declaration.name_token())
        }
        SyntaxKind::InterfaceDeclaration => {
            let declaration = ast::InterfaceDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Interface, declaration.name_token())
        }
        SyntaxKind::TraitDeclaration => {
            let declaration = ast::TraitDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Trait, declaration.name_token())
        }
        SyntaxKind::EnumDeclaration => {
            let declaration = ast::EnumDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Enum, declaration.name_token())
        }
        _ => return None,
    };
    Some(ClassMembers {
        kind,
        name: name_token.map(|token| token.text().to_owned()),
        namespace: item.namespace.clone(),
        ast_id,
        members: Vec::new(),
    })
}

fn lower_member(node: &SyntaxNode, ast_id: AstId, group: &mut ClassMembers) {
    match node.kind() {
        SyntaxKind::MethodDeclaration => {
            let Some(method) = ast::MethodDeclaration::cast(node.clone()) else {
                return;
            };
            let Some(name) = method.name_token() else { return };
            group.members.push(Member {
                kind: MemberKind::Method,
                name: name.text().to_owned(),
                flags: flags_of(method.modifiers()),
                ast_id,
            });
        }
        SyntaxKind::PropertyDeclaration => {
            let Some(property) = ast::PropertyDeclaration::cast(node.clone()) else {
                return;
            };
            let flags = flags_of(property.modifiers());
            for element in property.property_elements() {
                let Some(name) = element.name_token() else {
                    continue;
                };
                group.members.push(Member {
                    kind: MemberKind::Property,
                    name: property_name(&name),
                    flags,
                    ast_id,
                });
            }
        }
        SyntaxKind::ConstantDeclaration => {
            let Some(constant) = ast::ConstantDeclaration::cast(node.clone()) else {
                return;
            };
            let flags = flags_of(constant.modifiers());
            for element in constant.constant_elements() {
                let Some(name) = element.name_token() else {
                    continue;
                };
                group.members.push(Member {
                    kind: MemberKind::ClassConstant,
                    name: name.text().to_owned(),
                    flags,
                    ast_id,
                });
            }
        }
        SyntaxKind::EnumCase => {
            let Some(case) = ast::EnumCase::cast(node.clone()) else {
                return;
            };
            let Some(name) = case.name_token() else { return };
            group.members.push(Member {
                kind: MemberKind::EnumCase,
                name: name.text().to_owned(),
                flags: MemberFlags::default(),
                ast_id,
            });
        }
        _ => {}
    }
}

/// The bare property name: the `$` sigil stripped from the variable
/// token. Lookup and reflection both use the bare name.
fn property_name(token: &SyntaxToken) -> String {
    token.text().trim_start_matches('$').to_owned()
}

fn flags_of(modifiers: impl Iterator<Item = SyntaxToken>) -> MemberFlags {
    let mut flags = MemberFlags::default();
    for token in modifiers {
        match token.kind() {
            SyntaxKind::Public | SyntaxKind::Var => flags.visibility = Visibility::Public,
            SyntaxKind::Protected => flags.visibility = Visibility::Protected,
            SyntaxKind::Private => flags.visibility = Visibility::Private,
            SyntaxKind::Static => flags.is_static = true,
            SyntaxKind::Abstract => flags.is_abstract = true,
            SyntaxKind::Final => flags.is_final = true,
            SyntaxKind::Readonly => flags.is_readonly = true,
            _ => {}
        }
    }
    flags
}
```

Wire the module in `crates/celerrate_semantics/src/lib.rs`: add `mod members;` in alphabetical position and `pub use members::{ClassMembers, Member, MemberFlags, MemberKind, MemberTree, Visibility};`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics members 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): project members per class in the member tree"
```

---

### Task 4: Signatures as unresolved names, and docblock text

Members gain their signature (parameter list, return type, property type, default values in comparable form) and their docblock text; class groups gain the class-level docblock. All of it stays range-free written text: resolution is plan 3's job, the docblock grammar is plan 4's. This is also where the spec's accepted cost lands: an edit inside a docblock changes the member tree (the raw text is a field), by design.

**Files:**
- Modify: `crates/celerrate_semantics/src/members.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (export `MemberSignature`, `ParameterSignature`)

**Interfaces:**
- Consumes: `docblock_token`, `type_text`, `expression_text` (Task 2).
- Produces, added to Task 3's types:

```rust
pub struct ParameterSignature {
    pub name: String,               // without the `$`
    pub type_text: Option<String>,  // written type, unresolved
    pub default_text: Option<String>, // comparable form
    pub by_reference: bool,
    pub variadic: bool,
    pub is_promoted: bool,          // constructor promotion: any modifier present
}
pub struct MemberSignature {
    pub parameters: Vec<ParameterSignature>, // methods; empty otherwise
    pub type_text: Option<String>,  // return type (methods) or property/constant type
    pub default_text: Option<String>, // property/constant initializer or enum-case value
    pub by_reference: bool,         // `function &f()`
}
// Member gains:      pub signature: MemberSignature, pub docblock: Option<String>
// ClassMembers gains: pub docblock: Option<String>
```

- [ ] **Step 1: Write the failing tests**

Append to the test module of `crates/celerrate_semantics/src/members.rs`:

```rust
    #[test]
    fn a_method_signature_carries_parameters_return_type_and_reference() {
        let class = only_class(
            "<?php class A {\n\
                 public function f(int $count, Foo\\Bar|null $subject = null, string ...$rest): static {}\n\
                 public function &g() {}\n\
             }",
        );
        let f = &class.members[0];
        let parameters = &f.signature.parameters;
        assert_eq!(parameters.len(), 3);
        assert_eq!(parameters[0].name, "count");
        assert_eq!(parameters[0].type_text.as_deref(), Some("int"));
        assert_eq!(parameters[0].default_text, None);
        assert_eq!(parameters[1].type_text.as_deref(), Some("Foo\\Bar|null"));
        assert_eq!(parameters[1].default_text.as_deref(), Some("null"));
        assert!(parameters[2].variadic);
        assert_eq!(f.signature.type_text.as_deref(), Some("static"));
        assert!(!f.signature.by_reference);
        assert!(class.members[1].signature.by_reference);
    }

    #[test]
    fn promoted_constructor_parameters_are_marked() {
        let class = only_class(
            "<?php class A { public function __construct(private readonly int $id, string $plain) {} }",
        );
        let constructor = &class.members[0];
        assert!(constructor.signature.parameters[0].is_promoted);
        assert!(!constructor.signature.parameters[1].is_promoted);
    }

    #[test]
    fn property_and_constant_signatures_carry_type_and_default() {
        let class = only_class(
            "<?php class A {\n\
                 public ?Logger $logger = null;\n\
                 public array $bare = [];\n\
                 final const int LIMIT = 10;\n\
             }",
        );
        let logger = &class.members[0];
        assert_eq!(logger.signature.type_text.as_deref(), Some("?Logger"));
        assert_eq!(logger.signature.default_text.as_deref(), Some("null"));
        let limit = &class.members[2];
        assert_eq!(limit.signature.type_text.as_deref(), Some("int"));
        assert_eq!(limit.signature.default_text.as_deref(), Some("10"));
    }

    #[test]
    fn an_enum_case_value_is_its_default_text() {
        let class = only_class("<?php enum Suit: string { case Hearts = 'h'; case Clubs; }");
        assert_eq!(
            class.members[0].signature.default_text.as_deref(),
            Some("'h'"),
        );
        assert_eq!(class.members[1].signature.default_text, None);
    }

    #[test]
    fn docblocks_attach_to_members_and_to_the_class() {
        let class = only_class(
            "<?php\n\
             /** @template T */\n\
             class Collection {\n\
                 /** @return T|null */\n\
                 public function first() {}\n\
                 public function undocumented() {}\n\
             }",
        );
        assert_eq!(class.docblock.as_deref(), Some("/** @template T */"));
        assert_eq!(
            class.members[0].docblock.as_deref(),
            Some("/** @return T|null */"),
        );
        assert_eq!(class.members[1].docblock, None);
    }

    #[test]
    fn a_docblock_edit_changes_the_member_tree_a_body_comment_does_not() {
        // The spec's accepted cost, pinned: docblock text is a field,
        // so editing it changes the value; a comment inside a body is
        // still invisible.
        let before = tree_of("<?php class A { /** @return int */ function f() { return 1; } }");
        let docblock_edit =
            tree_of("<?php class A { /** @return string */ function f() { return 1; } }");
        let body_comment_edit =
            tree_of("<?php class A { /** @return int */ function f() { /* note */ return 1; } }");
        assert_ne!(before, docblock_edit);
        assert_eq!(before, body_comment_edit);
    }

    #[test]
    fn a_default_value_edit_changes_the_member_tree_formatting_does_not() {
        // The comparable form is the projection typed judgments read:
        // content changes invalidate, formatting does not.
        let before = tree_of("<?php class A { public $x = new Foo(1, 2); }");
        let content_edit = tree_of("<?php class A { public $x = new Foo(1, 3); }");
        let formatting_edit = tree_of("<?php class A { public $x = new  Foo( 1,   2 ); }");
        assert_ne!(before, content_edit);
        assert_eq!(before, formatting_edit);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics members 2>&1 | tail -10`
Expected: FAIL to compile — `signature` and `docblock` fields do not exist.

- [ ] **Step 3: Implement signatures and docblocks**

In `crates/celerrate_semantics/src/members.rs`: add the two structs, add the fields (`signature: MemberSignature`, `docblock: Option<String>` on `Member`; `docblock: Option<String>` on `ClassMembers`), extend the imports with `celerrate_syntax::ast::extensions` accessors (`docblock_token`, `type_text`, `expression_text` — check `lib.rs` of `celerrate_syntax` re-exports them; if `ast::extensions` is private, re-export the three from `celerrate_syntax::ast`), and fill them in `class_group` and `lower_member`:

```rust
/// One parameter of a method signature, as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParameterSignature {
    pub name: String,
    pub type_text: Option<String>,
    pub default_text: Option<String>,
    pub by_reference: bool,
    pub variadic: bool,
    pub is_promoted: bool,
}

/// One member's signature, every type an unresolved written text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct MemberSignature {
    pub parameters: Vec<ParameterSignature>,
    pub type_text: Option<String>,
    pub default_text: Option<String>,
    pub by_reference: bool,
}

fn method_signature(method: &ast::MethodDeclaration) -> MemberSignature {
    MemberSignature {
        parameters: method
            .parameter_list()
            .into_iter()
            .flat_map(|list| list.parameters())
            .filter_map(|parameter| {
                let name = parameter.name_token()?;
                Some(ParameterSignature {
                    name: name.text().trim_start_matches('$').to_owned(),
                    type_text: parameter.ty().map(|ty| ast::type_text(&ty)),
                    default_text: parameter
                        .default_value()
                        .map(|expression| ast::expression_text(&expression)),
                    by_reference: parameter.by_reference_token().is_some(),
                    variadic: parameter.variadic_token().is_some(),
                    is_promoted: parameter.modifiers().next().is_some(),
                })
            })
            .collect(),
        type_text: method.return_type().map(|ty| ast::type_text(&ty)),
        default_text: None,
        by_reference: method.by_reference_token().is_some(),
    }
}
```

In `lower_member`, per arm: methods get `signature: method_signature(&method)`; properties get `signature: MemberSignature { type_text: property.ty().map(|ty| ast::type_text(&ty)), default_text: element.expression().map(|e| ast::expression_text(&e)), ..MemberSignature::default() }` (per element — the type is shared, the default is per element); class constants get `default_text` from `element.expression()` and `type_text` from the declaration's typed-constant type if `ConstantDeclaration` has a `ty()` accessor (check `generated.rs`; if there is none, typed constants keep `type_text: None` here and a `// typed-constant type: plan 3 reads it when the accessor exists` note is honest); enum cases get `default_text: case.value().map(|e| ast::expression_text(&e))`. Every member and the class group get `docblock: ast::docblock_token(node_or_class_node).map(|token| token.text().to_owned())`.

One trap: `Parameter::default_value()` and `PropertyElement::expression()` are `support::child(…, 0)` accessors — for a parameter, the *first* `Expression` child is the default (the type is a `Type`, not an `Expression`), so this is correct; verify with the Step 1 tests rather than reasoning further.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics members 2>&1 | tail -10`
Expected: PASS. If `type_text`/`expression_text`/`docblock_token` are not reachable as `ast::…`, add `pub use extensions::{docblock_token, expression_text, type_text};` to `crates/celerrate_syntax/src/ast.rs` and re-run.

- [ ] **Step 5: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_semantics crates/celerrate_syntax
git commit -m "✨ feat(semantics): carry signatures and docblocks in the member tree"
```

---

### Task 5: Trait-use records with adaptations

Linearization needs, per class, which traits it uses and how `insteadof`/`as` adapt them. The names inside adaptation bodies were the semantic core's recorded false negative; capturing them here closes it.

**Files:**
- Modify: `crates/celerrate_semantics/src/members.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (export `TraitAdaptation`, `TraitUse`)

**Interfaces:**
- Consumes: `TraitUseClause::{names, trait_adaptation_list}`, `TraitPrecedence::{reference_name, member_token, excluded_names}`, `TraitAlias::{reference_name, member_token, visibility_token, alias_token}` (all existing accessors, verified).
- Produces, added to `ClassMembers`:

```rust
pub enum TraitAdaptation {
    // `A::m insteadof B, C;` — trait_name is None for the unqualified form.
    Precedence { trait_name: Option<String>, member: String, excluded: Vec<String> },
    // `A::m as protected n;` — visibility and alias each optional.
    Alias { trait_name: Option<String>, member: String, visibility: Option<Visibility>, alias: Option<String> },
}
pub struct TraitUse { pub names: Vec<String>, pub adaptations: Vec<TraitAdaptation> }
// ClassMembers gains: pub trait_uses: Vec<TraitUse>
```

- [ ] **Step 1: Write the failing tests**

Append to the test module of `members.rs`:

```rust
    use super::{TraitAdaptation, TraitUse};

    #[test]
    fn trait_uses_carry_their_names() {
        let class = only_class("<?php class A { use First, Concerns\\Second; use \\Third; }");
        assert_eq!(
            class.trait_uses,
            vec![
                TraitUse {
                    names: vec!["First".to_owned(), "Concerns\\Second".to_owned()],
                    adaptations: Vec::new(),
                },
                TraitUse {
                    names: vec!["\\Third".to_owned()],
                    adaptations: Vec::new(),
                },
            ],
        );
    }

    #[test]
    fn insteadof_and_as_adaptations_are_captured() {
        let class = only_class(
            "<?php class A {\n\
                 use B, C {\n\
                     B::hello insteadof C;\n\
                     C::hello as protected hi;\n\
                     bye as farewell;\n\
                 }\n\
             }",
        );
        let adaptations = &class.trait_uses.first().unwrap().adaptations;
        assert_eq!(
            adaptations[0],
            TraitAdaptation::Precedence {
                trait_name: Some("B".to_owned()),
                member: "hello".to_owned(),
                excluded: vec!["C".to_owned()],
            },
        );
        assert_eq!(
            adaptations[1],
            TraitAdaptation::Alias {
                trait_name: Some("C".to_owned()),
                member: "hello".to_owned(),
                visibility: Some(Visibility::Protected),
                alias: Some("hi".to_owned()),
            },
        );
        assert_eq!(
            adaptations[2],
            TraitAdaptation::Alias {
                trait_name: None,
                member: "bye".to_owned(),
                visibility: None,
                alias: Some("farewell".to_owned()),
            },
        );
    }

    #[test]
    fn a_visibility_only_alias_has_no_new_name() {
        let class = only_class("<?php class A { use B { hello as protected; } }");
        assert_eq!(
            class.trait_uses.first().unwrap().adaptations.first(),
            Some(&TraitAdaptation::Alias {
                trait_name: None,
                member: "hello".to_owned(),
                visibility: Some(Visibility::Protected),
                alias: None,
            }),
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics members 2>&1 | tail -10`
Expected: FAIL to compile (`trait_uses` does not exist).

- [ ] **Step 3: Implement the trait-use capture**

In `members.rs`, add the types, the `trait_uses: Vec<TraitUse>` field, and populate it in `class_group` by walking the class node's `member_list()` for `TraitUseClause` members (traits are not numbered items, so this read happens at group creation):

```rust
/// One `insteadof` or `as` adaptation, as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TraitAdaptation {
    Precedence {
        trait_name: Option<String>,
        member: String,
        excluded: Vec<String>,
    },
    Alias {
        trait_name: Option<String>,
        member: String,
        visibility: Option<Visibility>,
        alias: Option<String>,
    },
}

/// One `use Trait, …;` clause of a class body, adaptations included.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraitUse {
    pub names: Vec<String>,
    pub adaptations: Vec<TraitAdaptation>,
}

fn trait_uses_of(member_list: Option<ast::MemberList>) -> Vec<TraitUse> {
    member_list
        .into_iter()
        .flat_map(|list| list.member_declarations())
        .filter_map(|member| match member {
            ast::MemberDeclaration::TraitUseClause(clause) => Some(clause),
            _ => None,
        })
        .map(|clause| TraitUse {
            names: clause.names().map(|name| name.text()).collect(),
            adaptations: clause
                .trait_adaptation_list()
                .into_iter()
                .flat_map(|list| list.trait_adaptations())
                .filter_map(|adaptation| lower_adaptation(&adaptation))
                .collect(),
        })
        .collect()
}
```

`lower_adaptation` matches the `TraitAdaptation` AST alternation (check its variants in `generated.rs`: it should offer `TraitPrecedence` and `TraitAlias`). For a `TraitPrecedence`: `member` from `member_token()`, `trait_name` from `reference_name().map(|name| name.text())` **but only when a `::` separator exists** — in the unqualified form `reference_name()` returns the member itself (the accessor's documented behavior), so: `trait_name = precedence.reference_name().map(|n| n.text()).filter(|_| has_colon_colon(precedence.syntax()))` with `fn has_colon_colon(node: &SyntaxNode) -> bool { node.children_with_tokens().filter_map(|e| e.into_token()).any(|t| t.kind() == SyntaxKind::ColonColon) }`; `excluded` from `excluded_names()`. For a `TraitAlias`: same `trait_name` rule, `member` from `member_token()`, `visibility` mapped from `visibility_token()` kind, `alias` from `alias_token()` — with one guard: in the visibility-only form the alias accessor may return the member token itself or nothing; keep `alias` only when its token differs from the member token (compare text ranges). Adaptations whose member token is missing (error recovery) lower to `None` and are skipped.

The four class-like arms of `class_group` pass their `member_list()` to `trait_uses_of` (the accessor exists on all four generated types; functions/constants keep `Vec::new()` implicitly by never reaching it).

- [ ] **Step 4: Run the tests, adjust to parser reality, verify they pass**

Run: `cargo test --package celerrate_semantics members 2>&1 | tail -15`
Expected: PASS. If the visibility-only alias test fails on the `alias` guard, print the adaptation node's structure with a scratch `dbg!` run, fix the guard, remove the `dbg!`.

- [ ] **Step 5: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): capture trait uses and adaptations in the member tree"
```

---

### Task 6: The `member_tree` query and the invalidation-scope proof

The projection becomes a salsa query, and the plan's central invariants get their direct tests: a method body edit spares member consumers; a member signature edit re-runs member consumers but **never** re-runs `item_tree` consumers — including `source_symbol_table`, which is the structural settlement of the serial-rebuild debt.

**Files:**
- Modify: `crates/celerrate_semantics/src/queries.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (export `member_tree`)
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs`

**Interfaces:**
- Produces: `#[salsa::tracked(returns(ref))] pub fn member_tree(db: &dyn salsa::Database, file: SourceFile) -> MemberTree` — Tasks 7–10 consume it.

- [ ] **Step 1: Write the failing query test**

Append to the test module of `crates/celerrate_semantics/src/queries.rs`:

```rust
    #[test]
    fn the_member_tree_query_projects_a_file() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(3),
            b"<?php namespace App; class Service { public function run(): void {} }".to_vec(),
        );
        let tree = super::member_tree(&db, file);
        let class = tree.classes.first().unwrap();
        assert_eq!(class.name.as_deref(), Some("Service"));
        assert_eq!(class.namespace, "App");
        assert_eq!(
            class.members.first().map(|member| member.name.as_str()),
            Some("run"),
        );
    }
```

- [ ] **Step 2: Write the failing invalidation-scope tests**

Append to `crates/celerrate_semantics/tests/invalidation_scope.rs` (the file's harness — `TestDatabase::take_executed()`, `executions_of` — is already there; add `member_tree` and `MemberTree` to the `celerrate_semantics` import list):

```rust
/// A stand-in for the type-engine consumers of the member boundary:
/// any query that reads the member tree and nothing else syntactic.
#[salsa::tracked]
fn member_names(db: &dyn salsa::Database, file: SourceFile) -> Vec<String> {
    member_tree(db, file)
        .classes
        .iter()
        .flat_map(|class| class.members.iter().map(|member| member.name.clone()))
        .collect()
}

#[test]
fn a_method_body_edit_reaches_the_member_tree_and_stops_there() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php class A { public function f(): int { return 1; } }".to_vec(),
    );
    let _ = member_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php class A { public function f(): int { return 2; } }".to_vec());
    let _ = member_names(&db, file);

    let log = db.take_executed();
    assert_eq!(executions_of(&log, "member_tree"), 1, "{log:?}");
    assert_eq!(
        executions_of(&log, "member_names"),
        0,
        "an identical member tree must backdate, sparing member consumers: {log:?}",
    );
}

#[test]
fn a_member_signature_edit_never_reaches_item_tree_consumers() {
    // The structural settlement of the source_symbol_table debt: the
    // symbol table depends on item_tree only, and a member signature
    // edit leaves the top-level projection equal, so the global table
    // is never rebuilt — the audit's serial O(all symbols) loop no
    // longer fires on the type engine's canonical hot edit class.
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php class A { public function f(): int { return 1; } }".to_vec(),
    );
    let other = SourceFile::new(&db, FileId::new(1), b"<?php class B {}".to_vec());
    let files = AnalyzedFileSet::new(&db, vec![file, other]);
    let _ = member_names(&db, file);
    let _ = source_symbol_table(&db, files);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php class A { public function f(): string { return ''; } }".to_vec());
    let _ = member_names(&db, file);
    let _ = source_symbol_table(&db, files);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "member_tree"),
        1,
        "the signature changed, the member projection re-runs: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "member_names"),
        1,
        "member consumers see the new signature: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "the top-level projection re-runs over the new tree: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        0,
        "the item tree is equal and backdates: the global table never rebuilds: {log:?}",
    );
}

#[test]
fn a_docblock_prose_edit_reaches_member_consumers_by_design() {
    // The spec's accepted cost, observed at the query level: the raw
    // docblock is a member-tree field. The second-stage cutoff (the
    // parsed-annotation query) is plan 4's; until then the member tree
    // is the only stage.
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php class A { /** @return int */ public function f() {} }".to_vec(),
    );
    let _ = member_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php class A { /** @return int (documented) */ public function f() {} }".to_vec());
    let _ = member_names(&db, file);

    let log = db.take_executed();
    assert_eq!(executions_of(&log, "member_tree"), 1, "{log:?}");
    assert_eq!(executions_of(&log, "member_names"), 1, "{log:?}");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics member_tree invalidation_scope 2>&1 | tail -10`
Expected: FAIL to compile — the `member_tree` query does not exist.

- [ ] **Step 4: Implement the query**

In `crates/celerrate_semantics/src/queries.rs`, next to `item_tree`:

```rust
/// The member projection of one file: per class-like declaration, its
/// direct members with flags, signatures as unresolved names, and
/// docblock text. Range-free like the item tree, and a sibling of it
/// on purpose: a member edit changes this value without touching
/// `item_tree`, so top-level consumers — the global symbol table
/// first — are structurally spared. No artifact-cache consultation
/// yet: the typed-artifact classes are plan 9a.
#[salsa::tracked(returns(ref))]
pub fn member_tree(db: &dyn salsa::Database, file: SourceFile) -> MemberTree {
    MemberTree::from_root(file.file_id(db), &celerrate_db::parse(db, file).tree())
}
```

Add `use crate::members::MemberTree;` to the imports and `member_tree` to the `pub use queries::…` line of `lib.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics member_tree invalidation_scope 2>&1 | tail -10`
Expected: PASS, including the `source_symbol_table == 0` assertion — the debt proof.

- [ ] **Step 6: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): the member_tree query with its invalidation-scope proof"
```

---

### Task 7: Class-origin lookup and the file index

Linearization must go from a resolved ancestor **name** to the file and declaration that define it. `lookup_symbol` answers only "exists, of kind K" — this task adds the origin-carrying variant behind the same firewall, plus the `FileId → SourceFile` index the walk needs to fetch ancestor member trees.

**Files:**
- Modify: `crates/celerrate_semantics/src/lookup.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (export `analyzed_file_index`, `lookup_class_declaration`)

**Interfaces:**
- Consumes: `source_symbol_table`, `SymbolQuery`, `SymbolOrigin` (existing).
- Produces:

```rust
#[salsa::tracked(returns(ref))]
pub fn analyzed_file_index(db: &dyn salsa::Database, files: AnalyzedFileSet) -> Vec<(FileId, SourceFile)>; // sorted by FileId
#[salsa::tracked]
pub fn lookup_class_declaration(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    query: SymbolQuery<'_>,
) -> Option<(DeclarationKind, AstId)>; // source class-likes only; None for stubs and non-items
```

Task 8 consumes both. Callers turn the `AstId` into a `SourceFile` through `analyzed_file_index` (binary search on the sorted pairs).

- [ ] **Step 1: Write the failing tests**

Append to the test module of `crates/celerrate_semantics/src/lookup.rs` (reuse its `fixture`/`Fixture` helpers):

```rust
    use crate::lookup::{analyzed_file_index, lookup_class_declaration};

    fn class_declaration(
        fixture: &Fixture,
        written: &str,
    ) -> Option<(DeclarationKind, crate::AstId)> {
        let space = SymbolSpace::ClassLike;
        let query = SymbolQuery::new(&fixture.db, space, folded_symbol_key(space, written));
        lookup_class_declaration(&fixture.db, fixture.files, query)
    }

    #[test]
    fn a_source_class_answers_its_declaring_identity() {
        let fixture = fixture(&["<?php namespace App; class Service {}"]);
        let (kind, ast_id) = class_declaration(&fixture, "App\\Service").unwrap();
        assert_eq!(kind, DeclarationKind::Class);
        assert_eq!(ast_id.file, FileId::new(0));
    }

    #[test]
    fn a_stub_only_class_answers_none_here() {
        // The stub side has no member tree until plan 3; the walk
        // treats it as a stub boundary, which Task 8 records.
        let fixture = fixture(&["<?php"]);
        assert_eq!(class_declaration(&fixture, "Exception"), None);
    }

    #[test]
    fn the_file_index_maps_ids_to_handles_sorted() {
        let fixture = fixture(&["<?php class A {}", "<?php class B {}"]);
        let index = analyzed_file_index(&fixture.db, fixture.files);
        let ids: Vec<u32> = index.iter().map(|(id, _)| id.as_u32()).collect();
        assert_eq!(ids, vec![0, 1]);
    }
```

If `FileId` has no `as_u32()`, check `crates/celerrate_source` for the actual accessor (`index()`, `.0`, or `Debug`) and use that; do not add one.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics lookup 2>&1 | tail -10`
Expected: FAIL to compile.

- [ ] **Step 3: Implement both queries**

In `crates/celerrate_semantics/src/lookup.rs`:

```rust
use celerrate_db::SourceFile;
use celerrate_source::FileId;

use crate::ast_id::AstId;
use crate::index::SymbolOrigin;

/// The analyzed files by identifier, sorted: the bridge from an
/// `AstId` (which carries a `FileId`) back to the salsa handle whose
/// trees can be asked for. Depends on the file *set*, not on any
/// file's content, so content edits never re-run it.
#[salsa::tracked(returns(ref))]
pub fn analyzed_file_index(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
) -> Vec<(FileId, SourceFile)> {
    let mut index: Vec<(FileId, SourceFile)> = files
        .files(db)
        .iter()
        .map(|&file| (file.file_id(db), file))
        .collect();
    index.sort_by_key(|(id, _)| *id);
    index
}

/// The declaring identity of one source class-like: the same firewall
/// as `lookup_symbol`, answering the origin instead of the kind alone.
/// `None` for stub symbols (no source declaration), for `define()`
/// origins (not class-likes), and for unknown names.
#[salsa::tracked]
pub fn lookup_class_declaration<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    query: SymbolQuery<'db>,
) -> Option<(DeclarationKind, AstId)> {
    let entry = source_symbol_table(db, files).lookup(query.space(db), query.key(db))?;
    match entry.origin {
        SymbolOrigin::Item(ast_id) => Some((entry.kind, ast_id)),
        SymbolOrigin::Define(_) => None,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics lookup 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): origin-carrying class lookup and the analyzed-file index"
```

---

### Task 8: Inheritance linearization — the iterative walk

The per-class query resolving `extends`, `implements`, and trait `use` into a linearized member table: own members over trait members over inherited members, PHP's case rules per kind, an **iterative walk with a visited set** (never self-recursive — no salsa cycle by construction, the spec's mechanism), inheritance cycles detected and flagged, stub ancestors recorded as a boundary, and ancestry edges kept for plan 6's generic-argument threading. Adaptations and magic markers land in Task 9.

**Files:**
- Create: `crates/celerrate_semantics/src/linearize.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (add `mod linearize;`, export `AncestorEdge`, `AncestorRelation`, `ClassQuery`, `LinearizedClass`, `LinearizedMember`, `MemberOrigin`, `linearized_class`)

**Interfaces:**
- Consumes: `member_tree` (Task 6), `item_tree` (inheritance names live on its declarations), `lookup_class_declaration` + `analyzed_file_index` (Task 7), `resolve_candidates`/`UseTables` (existing), `lookup_symbol` (to classify unresolved-vs-stub ancestors), `folded_symbol_key`.
- Produces:

```rust
#[salsa::interned(debug)]
pub struct ClassQuery<'db> { #[returns(ref)] pub key: String } // folded ClassLike key
pub enum MemberOrigin { Own, Trait, Inherited }
pub enum AncestorRelation { Extends, Implements, UsesTrait }
pub struct AncestorEdge {
    pub relation: AncestorRelation,
    pub written: String,        // as written at the declaring site
    pub resolved: Option<String>, // folded key when it resolved to a source class
    pub owner: String,          // folded key of the class that declared the edge
}
pub struct LinearizedMember {
    pub key: String,            // folded member key (methods lowercased)
    pub member: Member,         // cloned payload
    pub owner: String,          // folded key of the declaring class
    pub origin: MemberOrigin,
}
pub struct LinearizedClass {
    pub members: Vec<LinearizedMember>, // sorted by (kind, key); first entry per key wins
    pub ancestry: Vec<AncestorEdge>,    // walk order
    pub stub_ancestors: Vec<String>,    // folded keys of ancestors that resolved to stubs only
    pub cyclic: bool,                   // an inheritance cycle was broken deterministically
}
#[salsa::tracked(returns(ref))]
pub fn linearized_class<'db>(
    db: &'db dyn salsa::Database,
    // The three handles travel separately, like lookup_symbol's.
    files: AnalyzedFileSet, stubs: StubIndexInput, configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> Option<LinearizedClass>; // None when the class itself is not a source class-like
```

Member-key folding: `fn folded_member_key(kind: MemberKind, name: &str) -> String` — methods `to_ascii_lowercase()`, everything else verbatim. `LinearizedClass.members` sorting and the first-wins rule realize PHP precedence because the walk pushes in precedence order and the sort is **stable** (`sort_by_key` is not stable across equal keys with different payloads — use `Vec::sort_by` with a full ordering on `(kind, key)` only via `sort_by(|a, b| (a.member.kind as u8, a.key.as_str()).cmp(&(b.member.kind as u8, b.key.as_str())))`, which `sort_by` performs stably in Rust's standard library). Lookup takes the first entry per `(kind, key)`.

- [ ] **Step 1: Write the failing tests**

In the new `crates/celerrate_semantics/src/linearize.rs`, a test module using the `lookup.rs` fixture pattern (copy the `Fixture`/`fixture` helpers — they are test-module-local there; duplicating twelve lines of fixture beats exporting test scaffolding):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    // Copy the Fixture / fixture helpers from lookup.rs's test module
    // verbatim (TestDatabase, AnalyzedFileSet, StubIndexInput with the
    // Exception/strlen stubs, ProjectConfiguration 8.1–8.5).

    use super::{ClassQuery, LinearizedClass, MemberOrigin, linearized_class};
    use crate::members::MemberKind;
    use crate::symbols::{SymbolSpace, folded_symbol_key};

    fn linearize(fixture: &Fixture, written: &str) -> Option<LinearizedClass> {
        let query = ClassQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, written),
        );
        linearized_class(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .clone()
    }

    fn member_owner(class: &LinearizedClass, kind: MemberKind, key: &str) -> Option<(String, MemberOrigin)> {
        class
            .members
            .iter()
            .find(|entry| entry.member.kind == kind && entry.key == key)
            .map(|entry| (entry.owner.clone(), entry.origin))
    }

    #[test]
    fn own_members_shadow_inherited_ones() {
        let fixture = fixture(&[
            "<?php class Base { public function hello() {} public function only() {} }",
            "<?php class Child extends Base { public function hello() {} }",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert_eq!(
            member_owner(&child, MemberKind::Method, "hello"),
            Some(("child".to_owned(), MemberOrigin::Own)),
        );
        assert_eq!(
            member_owner(&child, MemberKind::Method, "only"),
            Some(("base".to_owned(), MemberOrigin::Inherited)),
        );
        assert!(!child.cyclic);
    }

    #[test]
    fn trait_members_beat_inherited_and_lose_to_own() {
        let fixture = fixture(&[
            "<?php trait Greets { public function hello() {} public function bye() {} }",
            "<?php class Base { public function hello() {} public function bye() {} public function stays() {} }",
            "<?php class Child extends Base { use Greets; public function hello() {} }",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert_eq!(
            member_owner(&child, MemberKind::Method, "hello").unwrap().1,
            MemberOrigin::Own,
        );
        assert_eq!(
            member_owner(&child, MemberKind::Method, "bye"),
            Some(("greets".to_owned(), MemberOrigin::Trait)),
        );
        assert_eq!(
            member_owner(&child, MemberKind::Method, "stays").unwrap().1,
            MemberOrigin::Inherited,
        );
    }

    #[test]
    fn method_keys_fold_case_and_property_keys_do_not() {
        let fixture = fixture(&[
            "<?php class Base { public function CamelCase() {} public $Exact; }",
            "<?php class Child extends Base {}",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert!(member_owner(&child, MemberKind::Method, "camelcase").is_some());
        assert!(member_owner(&child, MemberKind::Property, "Exact").is_some());
        assert!(member_owner(&child, MemberKind::Property, "exact").is_none());
    }

    #[test]
    fn interface_constants_and_methods_inherit_through_extends_chains() {
        let fixture = fixture(&[
            "<?php interface Upper { const K = 1; public function f(); }",
            "<?php interface Lower extends Upper {}",
            "<?php class Impl implements Lower { public function f() {} }",
        ]);
        let implementation = linearize(&fixture, "Impl").unwrap();
        assert_eq!(
            member_owner(&implementation, MemberKind::ClassConstant, "K"),
            Some(("upper".to_owned(), MemberOrigin::Inherited)),
        );
        assert_eq!(
            member_owner(&implementation, MemberKind::Method, "f").unwrap().1,
            MemberOrigin::Own,
        );
    }

    #[test]
    fn imports_resolve_ancestor_names_at_the_declaring_site() {
        // The extends name resolves in Child's file with Child's
        // imports and namespace — not the asker's.
        let fixture = fixture(&[
            "<?php namespace Lib; class Base { public function inherited() {} }",
            "<?php namespace App; use Lib\\Base; class Child extends Base {}",
        ]);
        let child = linearize(&fixture, "App\\Child").unwrap();
        assert_eq!(
            member_owner(&child, MemberKind::Method, "inherited"),
            Some(("lib\\base".to_owned(), MemberOrigin::Inherited)),
        );
    }

    #[test]
    fn an_inheritance_cycle_is_broken_and_flagged() {
        let fixture = fixture(&[
            "<?php class A extends B { public function fromA() {} }",
            "<?php class B extends A { public function fromB() {} }",
        ]);
        let a = linearize(&fixture, "A").unwrap();
        assert!(a.cyclic);
        // The walk terminates and still linearizes what it saw once.
        assert!(member_owner(&a, MemberKind::Method, "froma").is_some());
        assert!(member_owner(&a, MemberKind::Method, "fromb").is_some());
        let self_cycle_fixture = fixture_one("<?php class Selfish extends Selfish {}");
        let selfish = linearize(&self_cycle_fixture, "Selfish").unwrap();
        assert!(selfish.cyclic);
    }

    #[test]
    fn a_stub_ancestor_is_a_recorded_boundary() {
        let fixture = fixture(&["<?php class AppException extends \\Exception {}"]);
        let class = linearize(&fixture, "AppException").unwrap();
        assert_eq!(class.stub_ancestors, vec!["exception".to_owned()]);
        assert!(!class.cyclic);
    }

    #[test]
    fn an_unresolvable_ancestor_leaves_an_unresolved_edge() {
        let fixture = fixture(&["<?php class Child extends Missing {}"]);
        let child = linearize(&fixture, "Child").unwrap();
        let edge = child.ancestry.first().unwrap();
        assert_eq!(edge.written, "Missing");
        assert_eq!(edge.resolved, None);
        assert!(child.stub_ancestors.is_empty());
    }

    #[test]
    fn a_non_class_key_answers_none() {
        let fixture = fixture(&["<?php function free() {}"]);
        assert!(linearize(&fixture, "free").is_none());
    }

    #[test]
    fn diamond_interfaces_keep_the_first_edge_deterministically() {
        let fixture = fixture(&[
            "<?php interface Left { const K = 1; }",
            "<?php interface Right { const K = 2; }",
            "<?php class Both implements Left, Right {}",
        ]);
        let both = linearize(&fixture, "Both").unwrap();
        // Edge order is declaration order: Left wins, always.
        assert_eq!(
            member_owner(&both, MemberKind::ClassConstant, "K"),
            Some(("left".to_owned(), MemberOrigin::Inherited)),
        );
    }
}
```

Add a `fixture_one(source: &str) -> Fixture` helper (a one-file `fixture`). Note the deliberate contracts: `owner` and `stub_ancestors` carry **folded keys**; edge order is declaration order; a cycle linearizes each participant once and flags `cyclic`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics linearize 2>&1 | tail -10`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the walk**

`crates/celerrate_semantics/src/linearize.rs` — the shape (write it in full; the helpers below are the complete logic):

```rust
//! Inheritance linearization: per class, the resolved member table.
//! Own members over trait members over inherited members, PHP's case
//! rules per kind. The walk is iterative with a visited set inside one
//! tracked query — it never demands its own kind recursively, so
//! `class A extends B; class B extends A` is a detected, flagged
//! condition, never a salsa cycle (spec section 2's mechanism).
//! Ancestry edges are kept for the type engine's generic-argument
//! threading (plan 6); stub ancestors are a recorded boundary until
//! the stub signature payload exists (plan 3).

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_source::FileId;
use celerrate_stubs::StubIndexInput;

use crate::items::DeclarationKind;
use crate::lookup::{
    SymbolQuery, analyzed_file_index, lookup_class_declaration, lookup_symbol,
};
use crate::members::{ClassMembers, Member, MemberKind};
use crate::queries::{item_tree, member_tree};
use crate::resolve::UseTables;
use crate::symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};
```

Key pieces:

1. **`folded_member_key`** (public, in this module): methods lowercase, others verbatim.

2. **The queue item**: `(folded_class_key, MemberOrigin-to-assign)`. The walk state: `visited: std::collections::HashSet<String>`, `members: Vec<LinearizedMember>`, `ancestry: Vec<AncestorEdge>`, `stub_ancestors: Vec<String>` (sorted+deduped at the end), `cyclic: bool`.

3. **Fetching one class**: fold the key → `lookup_class_declaration` → `None` means: try `lookup_symbol`; a stub `ClassLike` answer records a stub ancestor, anything else records nothing (the unresolved edge already carries `resolved: None`). A source hit whose kind is not class-like answers `None` for the query's own class and is skipped as an ancestor. From the `AstId`, binary-search `analyzed_file_index` for the `SourceFile`, then find the `ClassMembers` group in `member_tree(db, file)` by `ast_id` and the `Declaration` in `item_tree(db, file)` by `ast_id` (the declaration carries `extends`/`implements`/`trait_uses` and the namespace; anonymous classes have a group but no declaration — they linearize with no ancestors beyond their own group, fine for 1a).

4. **Resolving ancestor names at the declaring site**: `UseTables::for_namespace(item_tree(db, file), namespace)` + `resolve_candidates(written, SymbolSpace::ClassLike, namespace, &tables)` — take the first candidate that `lookup_class_declaration` or the stub side answers, exactly `resolve_name`'s order but keeping the origin. Wrap this as a module-private helper `resolve_ancestor(db, files, stubs, configuration, tree, namespace, written) -> AncestorAnswer { Source(kind, ast_id, folded_key) | Stub(folded_key) | Unresolved }`.

5. **The loop** (the heart — deterministic, cycle-safe):

```rust
let mut queue: std::collections::VecDeque<(String, MemberOrigin)> = VecDeque::new();
queue.push_back((class_key.clone(), MemberOrigin::Own));
while let Some((key, origin)) = queue.pop_front() {
    if !visited.insert(key.clone()) {
        cyclic = true;
        continue;
    }
    let Some(found) = fetch(db, files, &key) else { continue };
    // Push this class's members under `origin`
    // (Own for the root, Trait for trait targets, Inherited beyond).
    for member in &found.group.members {
        members.push(LinearizedMember {
            key: folded_member_key(member.kind, &member.name),
            member: member.clone(),
            owner: key.clone(),
            origin,
        });
    }
    // Edges, in declaration order: traits first (they beat parents),
    // then extends, then implements. Each edge resolves at this
    // class's site and enqueues.
    for (relation, written) in edges_of(&found.declaration) {
        let answer = resolve_ancestor(
            db, files, stubs, configuration,
            item_tree(db, found.file), &found.namespace, &written,
        );
        ancestry.push(AncestorEdge { relation, written, resolved: answer.folded_key(), owner: key.clone() });
        match answer {
            AncestorAnswer::Source { folded_key, .. } => {
                let next_origin = match (origin, relation) {
                    (MemberOrigin::Own, AncestorRelation::UsesTrait) => MemberOrigin::Trait,
                    _ => MemberOrigin::Inherited,
                };
                queue.push_back((folded_key, next_origin));
            }
            AncestorAnswer::Stub { folded_key } => stub_ancestors.push(folded_key),
            AncestorAnswer::Unresolved => {}
        }
    }
}
```

`edges_of` yields `(UsesTrait, name)` for every `trait_uses` entry, then `(Extends, name)` for `extends`, then `(Implements, name)` for `implements` — that order plus the stable sort realizes own > trait > parent > interfaces. Breadth-first with this edge order gives PHP's precedence for the depth-one case exactly and a deterministic (documented) order for deep mixed hierarchies; the doc comment states it: "precedence between two *transitive* sources of one member follows walk order, which is declaration order per level — deterministic, and refined only if the corpus proves PHP's exact C3-ish order matters in practice" (YAGNI: PHPStan does the same simplification).

6. **Finish**: stable-sort `members` by `(kind, key)` as specified in the Interfaces block, sort+dedup `stub_ancestors`, and return `Some(LinearizedClass { … })` — or `None` when the root key never fetched a source class-like.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics linearize 2>&1 | tail -15`
Expected: PASS. The diamond test and the cycle tests are the ones most likely to surface ordering bugs; if one fails, print the walk order, fix the queue discipline (never the test).

- [ ] **Step 5: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): linearize inheritance with an iterative, cycle-safe walk"
```

---

### Task 9: Trait adaptations and the suppression markers

`insteadof` and `as` reshape which trait members land in the table; the magic-method and dynamic-property markers give plan 8's unknown-members family its per-kind suppression facts. Both are linearization-time concerns: they need the resolved trait names and the full walk.

**Files:**
- Modify: `crates/celerrate_semantics/src/linearize.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (export `MagicMarkers`)

**Interfaces:**
- Consumes: `TraitUse`/`TraitAdaptation` (Task 5).
- Produces, added to `LinearizedClass`:

```rust
pub struct MagicMarkers {
    pub has_magic_get: bool,        // __get — suppresses unknown *properties*
    pub has_magic_set: bool,        // __set — idem (writes; plan 8 decides)
    pub has_magic_call: bool,       // __call — suppresses unknown *methods*
    pub has_magic_call_static: bool, // __callStatic — unknown *static methods*
    pub allows_dynamic_properties: bool, // #[AllowDynamicProperties], own or inherited
}
// LinearizedClass gains: pub magic: MagicMarkers
```

The markers are computed from the finished table (magic methods are members like any other, own or inherited or trait-provided) plus one attribute reading. `stdClass` needs no marker here: it is a stub, so a class extending it records the `stdclass` stub ancestor, and plan 8 reads `stub_ancestors` for it — note this in the struct's documentation.

- [ ] **Step 1: Write the failing tests**

Append to `linearize.rs` tests:

```rust
    #[test]
    fn insteadof_excludes_and_as_aliases_trait_members() {
        let fixture = fixture(&[
            "<?php trait B { public function hello() { return 'b'; } }",
            "<?php trait C { public function hello() { return 'c'; } }",
            "<?php class A { use B, C { B::hello insteadof C; C::hello as protected hi; } }",
        ]);
        let a = linearize(&fixture, "A").unwrap();
        // B::hello won; C::hello is excluded under its own name…
        assert_eq!(
            member_owner(&a, MemberKind::Method, "hello"),
            Some(("b".to_owned(), MemberOrigin::Trait)),
        );
        // …but re-enters under the alias, with the adapted visibility.
        let aliased = a
            .members
            .iter()
            .find(|entry| entry.key == "hi" && entry.member.kind == MemberKind::Method)
            .unwrap();
        assert_eq!(aliased.owner, "c");
        assert_eq!(
            aliased.member.flags.visibility,
            crate::members::Visibility::Protected,
        );
    }

    #[test]
    fn magic_methods_mark_the_class_own_or_inherited() {
        let fixture = fixture(&[
            "<?php class Base { public function __get($name) {} }",
            "<?php class Child extends Base { public function __call($name, $arguments) {} }",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert!(child.magic.has_magic_get);
        assert!(child.magic.has_magic_call);
        assert!(!child.magic.has_magic_set);
        assert!(!child.magic.has_magic_call_static);
    }

    #[test]
    fn the_allow_dynamic_properties_attribute_marks_the_class() {
        let fixture = fixture(&[
            "<?php #[AllowDynamicProperties] class Loose {}",
            "<?php class Child extends Loose {}",
        ]);
        assert!(linearize(&fixture, "Loose").unwrap().magic.allows_dynamic_properties);
        assert!(linearize(&fixture, "Child").unwrap().magic.allows_dynamic_properties);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics linearize 2>&1 | tail -10`
Expected: FAIL to compile (`magic` does not exist), then FAIL on adaptations.

- [ ] **Step 3: Implement adaptations and markers**

**Adaptations.** In the walk, when enqueuing a trait edge, carry the using class's `TraitUse` adaptations (they live on `found.group.trait_uses`; associate each adaptation with the resolved trait keys of its clause). When pushing a member whose `origin == MemberOrigin::Trait`:

- Skip it when a `Precedence` adaptation excludes it: the member key matches and the *providing* trait's folded key is in `excluded` (resolved through the same clause's name resolution; an unresolvable excluded name excludes nothing).
- For each `Alias` adaptation matching (member key, and trait key when qualified): push an **additional** entry whose `member.name`/key is the alias (when present) and whose `flags.visibility` is the adapted one (when present) — the original entry still pushes unless an `insteadof` removed it. A visibility-only alias (no new name) instead *replaces* the pushed entry's visibility.

Aliases and exclusions apply only to members provided by the traits of that clause — plain inherited members are never adapted; PHP's `as` on a method name matches the method key case-insensitively, like every method name.

**Markers.** After the sort, compute:

```rust
fn magic_markers(members: &[LinearizedMember], allows_dynamic: bool) -> MagicMarkers {
    let has = |name: &str| {
        members
            .iter()
            .any(|entry| entry.member.kind == MemberKind::Method && entry.key == name)
    };
    MagicMarkers {
        has_magic_get: has("__get"),
        has_magic_set: has("__set"),
        has_magic_call: has("__call"),
        has_magic_call_static: has("__callstatic"),
        allows_dynamic_properties: allows_dynamic,
    }
}
```

(`__callstatic`: method keys are already folded.) For `allows_dynamic_properties`: during the walk, when fetching each class, read its declaration node's attributes — this needs the attribute names in the member tree, not a syntax re-read: add a small field in Task 3's `ClassMembers` if you reach this step and it is absent — `pub attribute_names: Vec<String>` filled in `class_group` from `ast::ClassDeclaration::attribute_groups()` (and the other three kinds) via `group.attributes()` → `attribute.name().map(|name| name.text())`. The marker is true when any visited source class's `attribute_names` contains `AllowDynamicProperties` (compare the written name's last segment, case-insensitively — attribute names are class names). Update Task 3's struct and one test accordingly (add `attribute_names: Vec::new()` expectations where constructed); this is the one permitted retro-touch, and it stays inside this task's commit.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics 2>&1 | tail -10`
Expected: PASS, including every earlier module (the `ClassMembers` retro-touch compiles everywhere).

- [ ] **Step 5: Workspace gates and commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all`

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): apply trait adaptations and mark magic suppression facts"
```

---

### Task 10: The member-lookup firewall, the scope proof, and closure

The interned per-(class, member, kind) query — the firewall that keeps "adding a member invalidates only the files that looked it up" true — plus the invalidation-scope tests over linearization, and the plan's closing sweep.

**Files:**
- Create: `crates/celerrate_semantics/src/member_lookup.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (add `mod member_lookup;`, export `MemberQuery`, `MemberResolution`, `lookup_member`)
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: `linearized_class` (Tasks 8–9), `folded_member_key`.
- Produces:

```rust
#[salsa::interned(debug)]
pub struct MemberQuery<'db> {
    #[returns(ref)] pub class_key: String,  // folded ClassLike key
    pub kind: MemberKind,
    #[returns(ref)] pub member_key: String, // pre-folded with folded_member_key
}
pub struct MemberResolution {
    pub member: Member,
    pub owner: String,
    pub origin: MemberOrigin,
}
#[salsa::tracked]
pub fn lookup_member<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet, stubs: StubIndexInput, configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<MemberResolution>; // first entry per (kind, key): the precedence winner
```

This is plan 8's (checks) entry point; nothing in 1a consumes it beyond its own tests — it exists now because the firewall must be in place *before* consumers, not retrofitted under them.

- [ ] **Step 1: Write the failing tests**

In `member_lookup.rs` (fixture pattern again):

```rust
    #[test]
    fn a_member_resolves_through_the_linearized_table() {
        let fixture = fixture(&[
            "<?php class Base { public function hello() {} }",
            "<?php class Child extends Base {}",
        ]);
        let resolution = lookup(&fixture, "Child", MemberKind::Method, "HELLO").unwrap();
        assert_eq!(resolution.owner, "base");
        assert_eq!(resolution.origin, MemberOrigin::Inherited);
    }

    #[test]
    fn an_unknown_member_or_class_answers_none() {
        let fixture = fixture(&["<?php class A { public function f() {} }"]);
        assert!(lookup(&fixture, "A", MemberKind::Method, "missing").is_none());
        assert!(lookup(&fixture, "Ghost", MemberKind::Method, "f").is_none());
        // Kinds are distinct spaces: a method never answers a property.
        assert!(lookup(&fixture, "A", MemberKind::Property, "f").is_none());
    }
```

with a local `fn lookup(fixture, class_written, kind, member_written) -> Option<MemberResolution>` folding both keys (`folded_symbol_key` for the class, `folded_member_key` for the member — fold the member with the *queried* kind).

And the scope tests in `tests/invalidation_scope.rs`:

```rust
#[test]
fn adding_a_member_to_an_unrelated_class_spares_the_asker() {
    // The firewall: lookup_member re-runs when the linearized table
    // changes, but its answer for an untouched (class, member) pair is
    // equal, so consumers behind it backdate.
    let mut db = TestDatabase::default();
    let base = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php class Base { public function hello() {} }".to_vec(),
    );
    let unrelated = SourceFile::new(&db, FileId::new(1), b"<?php class Unrelated {}".to_vec());
    let files = AnalyzedFileSet::new(&db, vec![base, unrelated]);
    let stubs = test_stubs(&db);
    let configuration = test_configuration(&db);

    let query = MemberQuery::new(
        &db,
        folded_symbol_key(SymbolSpace::ClassLike, "Base"),
        MemberKind::Method,
        "hello".to_owned(),
    );
    let _ = lookup_member(&db, files, stubs, configuration, query);
    db.take_executed();

    unrelated
        .set_bytes(&mut db)
        .to(b"<?php class Unrelated { public function added() {} }".to_vec());
    let _ = lookup_member(&db, files, stubs, configuration, query);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "linearized_class"),
        0,
        "Base's linearization read nothing from Unrelated's file: {log:?}",
    );
}
```

(`test_stubs`/`test_configuration`: extract the two builder calls the file already repeats into helpers if they are not helpers yet — read the file first and follow what is there.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics member_lookup invalidation_scope 2>&1 | tail -10`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the firewall**

```rust
//! The per-member lookup: the firewall between the linearized tables
//! and their consumers, the same pattern as `lookup_symbol`. A member
//! added anywhere re-runs the affected linearization, but a lookup
//! whose answer did not change backdates, and the files that asked it
//! are spared.

#[salsa::tracked]
pub fn lookup_member<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<MemberResolution> {
    let class = ClassQuery::new(db, query.class_key(db).clone());
    let table = linearized_class(db, files, stubs, configuration, class).as_ref()?;
    let kind = query.kind(db);
    let key = query.member_key(db);
    table
        .members
        .iter()
        .find(|entry| entry.member.kind == kind && entry.key == *key)
        .map(|entry| MemberResolution {
            member: entry.member.clone(),
            owner: entry.owner.clone(),
            origin: entry.origin,
        })
}
```

(`find` on the sorted table returns the first entry per key — the precedence winner. A `partition_point` binary search is the obvious refinement; add it only if a benchmark ever asks.)

- [ ] **Step 4: Run all tests, then the closing sweep**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5 && cargo fmt --all && cargo deny check 2>&1 | tail -3`
Expected: all green.

Then the debt measurement record, which this plan owes the spec (section 9): the invalidation-scope test of Task 6 **is** the settlement evidence (member signature edits never rebuild the table). Append the closing entry to the audit trail:

Add to `.claude/superpowers/audits/2026-07-13-incremental-architecture-audit.md`, at the end of the `source_symbol_table` note's section, one dated paragraph:

```markdown
- 2026-07-14 (type-engine plan 1a) — settled structurally for the
  type engine's hot edit class: members live in a sibling projection
  (`member_tree`), so member and signature edits inside a class body
  never change `item_tree` values and the global table never rebuilds
  on them (pinned by
  `invalidation_scope.rs::a_member_signature_edit_never_reaches_item_tree_consumers`).
  The rebuild still fires on genuine top-level changes (new class,
  renamed function); that residue is unchanged from the audit and
  remains accepted, scale-bounded by top-level churn only.
```

- [ ] **Step 5: Final commit**

```bash
git add crates/celerrate_semantics .claude/superpowers/audits
git commit -m "✨ feat(semantics): the per-member lookup firewall closes plan 1a"
```

---

## Execution Notes

- Tasks are strictly ordered: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10. Task 2 can run in parallel with Task 1 (different crates); nothing else can.
- Every accessor named in this plan was verified against `crates/celerrate_syntax/src/ast/{generated,extensions}.rs` at plan-writing time; if one is missing at execution time, check `generated.rs` first — the grammar may name it slightly differently — and prefer adapting the call site over touching generated code.
- The incremental consistency harness (`tests/incremental_consistency.rs`) and the CLI cache tests must stay green after every task with **zero modifications**. A failure there is a traversal or determinism bug in this plan's code.
- What this plan deliberately does not do (do not "improve" it in): no artifact-cache class for `member_tree` (plan 9a), no docblock parsing (plan 4), no generic-argument extraction (plan 4b/6 — the `AncestorEdge` slots are the seam), no diagnostics (plan 8 mints identifiers through the registry), no LRU eviction (measured in plan 9b).


