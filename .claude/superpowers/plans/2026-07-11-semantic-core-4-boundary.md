# Semantic Core Part 4: Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `celerrate_semantics`: the stable declaration identity scheme
(`AstId`, `AstIdMap`), the range-free per-file `ItemTree`, both exposed as salsa
queries, and the invalidation-scope tests proving the early cutoff — the
invalidation boundary the parent spec fixed as a principle, in its
minimal-but-real form.

**Architecture:** Per the spec
`.claude/superpowers/specs/2026-07-11-semantic-core-design.md` (section 6).
Decisions fixed here:

- **Two queries with split volatility.** `ast_id_map(file)` carries one
  `SyntaxNodePtr` (kind plus range) per declaration node, so its value changes
  whenever ranges shift — it exists for late span reconciliation, and nothing
  long-lived depends on it. `item_tree(file)` carries **no ranges and no
  offsets anywhere**: a body edit, a comment edit, or a whitespace shift
  produces a value equal to the previous one, salsa backdates it, and no
  dependent re-runs. That equality is the early-cutoff mechanism, and the
  invalidation-scope tests observe it directly through salsa events.
- **One shared traversal defines "the declaration nodes".** A crate-private
  walk (`item_nodes`) produces the declaration nodes in preorder tree order
  with the enclosing namespace as walk state. `AstIdMap` numbers exactly that
  sequence; `ItemTree` projects exactly that sequence with the same positional
  indexes — agreement by construction, pinned by a test.
- **Traversal scope.** The walk descends into control-flow blocks and function
  bodies, so a declaration behind an `if (!function_exists(...))` guard is an
  item (the section 7 "declared anywhere counts as declared" stance, honored
  structurally). It never descends into a `MemberList`: members are not items
  in this sub-project, so class constants and declarations nested inside
  method bodies are invisible to the boundary (recorded as a narrowing, task
  9). Nameless declarations (anonymous classes, error-recovery wreckage) carry
  no top-level identity and are skipped.
- **Numbered kinds.** The eight declaration node kinds: `NamespaceDeclaration`,
  `UseDeclaration`, `ClassDeclaration`, `InterfaceDeclaration`,
  `TraitDeclaration`, `EnumDeclaration`, `FunctionDeclaration`,
  `ConstantDeclaration` (the class-likes and functions only when named).
  Namespace nodes are numbered but project no `ItemTree` entry of their own.
- **Namespaces are a field, not an item.** Each declaration and each import
  carries its enclosing namespace (`""` is global) — the `Eq`-stable encoding
  of the same information the spec lists. Statement-form `namespace Foo;`
  switches the walk state; brace-form `namespace Foo { ... }` scopes its block
  only (the model the stub compiler already validated).
- **Use imports are group-expanded at lowering.** Each import carries its kind
  (class, function, constant — declaration-level token inherited, clause-level
  token overrides), the written absolute target (leading backslash trimmed;
  `use` targets are always absolute), and the alias (explicit `as` token, or
  the target's last segment). The per-file resolution *tables* are part 5;
  this part delivers their complete input.
- **Inheritance names are stored verbatim** (trivia-stripped, spelling and
  qualifiers preserved: `\Fully\Qualified`, `Relative\Name`, `namespace\Child`)
  as unresolved names. Sub-project 3 consumes them; they cost one field now.
- **`Name::text()` joins `celerrate_syntax`.** The trivia-stripped written
  text of a name is a syntax-crate concern; the stub compiler's private
  whitespace-filtering helper is replaced by it (DRY, and token-based
  stripping also handles comments inside names).
- **The consistency harness generalizes with a closure variant** in
  `celerrate_db::testing`, so `celerrate_semantics` replays edit sequences
  comparing item trees without any upward dependency from `celerrate_db`.
- **LRU eviction of syntax trees is deferred to part 8** (closure), where the
  memory economics are measured alongside the persistent cache. Part 4
  delivers the structural property that makes eviction possible: nothing above
  the boundary holds a red node — `AstIdMap` and `ItemTree` are plain
  `Send + Sync` values (recorded as a narrowing, task 9).

**Tech Stack:** Rust edition 2024, salsa 0.27 and rowan 0.16 (existing
workspace dependencies), existing crates `celerrate_source`,
`celerrate_syntax`, `celerrate_db` (its `testing` module provides the
instrumented `TestDatabase`). No new external dependencies anywhere.

## Global Constraints

- Zero panic, mechanically enforced: the workspace denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` is forbidden. Only
  test modules may `#[allow]` / `#![allow]` these lints. Use `.get()`,
  `u32::try_from(...).ok()`, `unwrap_or_default`, `try_to_node` (never
  `to_node`, which panics) in production code.
- Strict layering, DAG with no upward edges: `celerrate_semantics` depends on
  `celerrate_source`, `celerrate_syntax`, `celerrate_db`, and `salsa` — never
  the reverse. The harness generalization keeps `celerrate_db` free of any
  `celerrate_semantics` knowledge.
- Error resilience: malformed PHP lowers to whatever the error-resilient
  parser recovered; every accessor is `Option`-tolerant; no input crashes
  anything.
- Determinism: `ItemTree` and `AstIdMap` are pure functions of the parse; tree
  order everywhere; no wall-clock time, no randomness, no environment reads
  inside queries.
- TDD throughout: every step of behavior starts from a failing test.
- Everything in English, full words, no abbreviated names (standard acronyms
  fine; `AstId` follows the spec's own naming).
- Commits: gitmoji + Conventional Commits, repository-configured identity, no
  Claude attribution anywhere.
- Local commands that must stay green after every task:
  `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `cargo deny check`.

## File Structure

```
crates/celerrate_semantics/
  Cargo.toml                     crate manifest (task 2; salsa + db added task 6)
  src/lib.rs                     crate root and exports
  src/ast_id.rs                  AstId, AstIdMap (task 2)
  src/item_nodes.rs              the shared declaration-node traversal, crate-private (task 2)
  src/item_tree.rs               ItemTree, Declaration, UseImport, lowering (tasks 3-5)
  src/queries.rs                 ast_id_map and item_tree tracked queries (task 6)
  tests/invalidation_scope.rs    the early-cutoff proof per edit class (task 7)
  tests/incremental_consistency.rs  edit replay against from-scratch analysis (task 8)

Modified:
  crates/celerrate_syntax/src/ast/extensions.rs    Name::text() (task 1)
  crates/celerrate_syntax/tests/ast.rs             Name::text() test (task 1)
  crates/celerrate_stubs/src/compiler/extract.rs   delegate to Name::text() (task 1)
  crates/celerrate_db/src/testing.rs               closure-based harness variant (task 8)
  .claude/superpowers/specs/2026-07-11-semantic-core-design.md  narrowings (task 9)
```

---

### Task 1: The written text of a `Name`

The lowering needs the trivia-stripped written text of `Name` nodes
(inheritance names, use targets, namespace names). The stub compiler already
has a private whitespace-filtering version; the accessor belongs on the typed
AST, token-based (which also strips comments), and the compiler delegates.

**Files:**
- Modify: `crates/celerrate_syntax/src/ast/extensions.rs`
- Modify: `crates/celerrate_syntax/tests/ast.rs`
- Modify: `crates/celerrate_stubs/src/compiler/extract.rs`

**Interfaces:**
- Consumes: `tokens_of` (private helper already in `extensions.rs`),
  `ast::Name` (generated, no accessors of its own today).
- Produces: `impl Name { pub fn text(&self) -> String }` — the written name
  with trivia stripped, qualifiers preserved (`Foo\Bar`, `\Baz\Qux`,
  `namespace\Child`). Tasks 2-5 call it.

- [ ] **Step 1: Write the failing test**

Append to `crates/celerrate_syntax/tests/ast.rs` (imports are local to the
test function, so nothing collides with the file's existing imports):

```rust
#[test]
fn a_name_reads_back_as_its_written_text() {
    use celerrate_syntax::ast::{AstNode, Name};

    let parse = celerrate_syntax::parse("<?php use Foo\\Bar; new \\Baz\\Qux();");
    let names: Vec<String> = parse
        .tree()
        .descendants()
        .filter_map(Name::cast)
        .map(|name| name.text())
        .collect();
    assert_eq!(names, vec!["Foo\\Bar".to_owned(), "\\Baz\\Qux".to_owned()]);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_syntax --test ast a_name_reads_back -- --nocapture`
Expected: FAIL to compile with `no method named 'text' found` (the accessor
does not exist yet).

- [ ] **Step 3: Write the minimal implementation**

In `crates/celerrate_syntax/src/ast/extensions.rs`, add next to the other
`impl` blocks (after `impl ConstantElement`, for example):

```rust
impl Name {
    /// The written name with interior trivia stripped: every non-trivia
    /// token's text joined in order. Qualifiers are preserved
    /// (`Foo\Bar`, `\Baz\Qux`, `namespace\Child`).
    pub fn text(&self) -> String {
        tokens_of(self.syntax())
            .map(|token| token.text().to_owned())
            .collect()
    }
}
```

`Name` is already in the `use super::generated::{...}` import list of
`extensions.rs`; no import change is needed.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --package celerrate_syntax --test ast a_name_reads_back`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax/src/ast/extensions.rs crates/celerrate_syntax/tests/ast.rs
git commit -m "✨ feat(syntax): expose a name's trivia-stripped text"
```

- [ ] **Step 6: Delegate the stub compiler's helper**

In `crates/celerrate_stubs/src/compiler/extract.rs`, delete the private
helper:

```rust
/// The text of a `Name` node with any interior trivia stripped.
fn name_text(name: &ast::Name) -> String {
    name.syntax()
        .text()
        .to_string()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
```

and replace its three call sites:

- in `collect`: `.map(|name| name_text(&name))` becomes
  `.map(|name| name.text())`
- in `define_constant`: `let callee_name = name_text(&callee.name()?);`
  becomes `let callee_name = callee.name()?.text();`
- in `apply_attributes`: `let name = name_text(&name);` becomes
  `let name = name.text();`

- [ ] **Step 7: Run the stubs extraction tests to verify nothing regressed**

Run: `cargo test --package celerrate_stubs`
Expected: PASS (the extraction test suite pins the behavior)

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_stubs/src/compiler/extract.rs
git commit -m "♻️ refactor(stubs): read names through Name::text"
```

---

### Task 2: The crate, the traversal, and `AstIdMap`

**Files:**
- Create: `crates/celerrate_semantics/Cargo.toml`
- Create: `crates/celerrate_semantics/src/lib.rs`
- Create: `crates/celerrate_semantics/src/item_nodes.rs`
- Create: `crates/celerrate_semantics/src/ast_id.rs`

**Interfaces:**
- Consumes: `celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr}`,
  `celerrate_syntax::ast` (typed accessors: `name_token()` on the class-likes
  and functions, `NamespaceDeclaration::{name, block}`, `Name::text()` from
  task 1), `celerrate_source::FileId`.
- Produces:
  - `pub struct AstId { pub file: FileId, pub index: u32 }` — `Copy`, `Eq`,
    `Hash`, `Ord`.
  - `pub struct AstIdMap` with `pub fn from_root(root: &SyntaxNode) -> Self`,
    `pub fn pointer(&self, index: u32) -> Option<SyntaxNodePtr>`,
    `pub fn index_of(&self, node: &SyntaxNode) -> Option<u32>`,
    `pub fn len(&self) -> usize`, `pub fn is_empty(&self) -> bool`.
  - Crate-private: `item_nodes(root: &SyntaxNode) -> Vec<ItemNode>` where
    `ItemNode { node: SyntaxNode, namespace: String }` — tasks 3-5 iterate it.

- [ ] **Step 1: Scaffold the crate**

Create `crates/celerrate_semantics/Cargo.toml`:

```toml
[package]
name = "celerrate_semantics"
description = "Stable declaration identities and per-file item trees for the Celerrate toolchain"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_source = { path = "../celerrate_source" }
celerrate_syntax = { path = "../celerrate_syntax" }

[lints]
workspace = true
```

(`celerrate_db` and `salsa` join in task 6, when the queries need them.)

Create `crates/celerrate_semantics/src/lib.rs`:

```rust
//! Stable declaration identity and the per-file item tree: the
//! invalidation boundary of the analysis engine. [`AstIdMap`] numbers a
//! file's declaration nodes in tree order, so an [`AstId`] survives body
//! edits; the item tree (later modules) is the range-free,
//! `Eq`-comparable projection of one file's declarations that gives
//! salsa its early cutoff.

mod ast_id;
mod item_nodes;

pub use ast_id::{AstId, AstIdMap};
```

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_semantics/src/ast_id.rs` with the test module only
(the types come in step 4 — write the tests against the intended API):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_syntax::SyntaxKind;

    use super::AstIdMap;

    fn map_of(source: &str) -> (celerrate_syntax::Parse, AstIdMap) {
        let parse = celerrate_syntax::parse(source);
        let map = AstIdMap::from_root(&parse.tree());
        (parse, map)
    }

    fn kinds_of(map: &AstIdMap) -> Vec<SyntaxKind> {
        (0..u32::try_from(map.len()).unwrap())
            .filter_map(|index| map.pointer(index))
            .map(|pointer| pointer.kind())
            .collect()
    }

    #[test]
    fn declaration_nodes_are_numbered_in_tree_order() {
        let (_parse, map) = map_of(
            "<?php namespace N; use A; function first() {} class Second {} const THIRD = 1;",
        );
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::NamespaceDeclaration,
                SyntaxKind::UseDeclaration,
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::ClassDeclaration,
                SyntaxKind::ConstantDeclaration,
            ],
        );
    }

    #[test]
    fn guarded_declarations_are_numbered() {
        // The section 7 stance: a symbol declared behind a guard counts
        // as declared, so the walk descends into blocks and bodies.
        let (_parse, map) = map_of(
            "<?php\n\
             if (!function_exists('greet')) { function greet() {} }\n\
             function outer() { function inner() {} }\n",
        );
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::FunctionDeclaration,
            ],
        );
    }

    #[test]
    fn members_are_not_declaration_nodes() {
        // Class constants, methods, and anything inside a member list
        // are members, not items; the constant after the class is one.
        let (_parse, map) = map_of(
            "<?php class A { const B = 1; public function method() {} } const C = 1;",
        );
        assert_eq!(
            kinds_of(&map),
            vec![SyntaxKind::ClassDeclaration, SyntaxKind::ConstantDeclaration],
        );
    }

    #[test]
    fn nameless_declarations_are_not_numbered() {
        // Anonymous classes carry no top-level identity.
        let (_parse, map) = map_of(
            "<?php function wrapper() { return new class {}; } class Named {}",
        );
        assert_eq!(
            kinds_of(&map),
            vec![SyntaxKind::FunctionDeclaration, SyntaxKind::ClassDeclaration],
        );
    }

    #[test]
    fn a_pointer_reconciles_back_to_its_node() {
        let (parse, map) = map_of("<?php function greet() {}");
        let pointer = map.pointer(0).unwrap();
        let node = pointer.try_to_node(&parse.tree()).unwrap();
        assert_eq!(node.kind(), SyntaxKind::FunctionDeclaration);
        assert_eq!(map.index_of(&node), Some(0));
    }

    #[test]
    fn an_unknown_index_and_a_non_item_node_answer_none() {
        let (parse, map) = map_of("<?php function greet() {}");
        assert_eq!(map.pointer(99), None);
        assert_eq!(map.index_of(&parse.tree()), None);
        assert!(!map.is_empty());
    }

    #[test]
    fn a_body_edit_renumbers_nothing() {
        let (_before_parse, before) =
            map_of("<?php function a() { return 1; } class B {}");
        let (after_parse, after) =
            map_of("<?php function a() { return 1 + 1; } class B {}");
        // Class B moved (its range changed), but it kept its number.
        assert_eq!(
            before.pointer(1).unwrap().kind(),
            SyntaxKind::ClassDeclaration,
        );
        let class_node = after
            .pointer(1)
            .unwrap()
            .try_to_node(&after_parse.tree())
            .unwrap();
        assert_eq!(class_node.kind(), SyntaxKind::ClassDeclaration);
        assert_eq!(after.index_of(&class_node), Some(1));
    }

    #[test]
    fn an_empty_file_has_an_empty_map() {
        let (_parse, map) = map_of("");
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics`
Expected: FAIL to compile with `cannot find type 'AstIdMap'` (and the missing
`item_nodes` module).

- [ ] **Step 4: Write the implementation**

Create `crates/celerrate_semantics/src/item_nodes.rs`:

```rust
//! The shared traversal defining "the declaration nodes" of one file.
//! `AstIdMap` and the item tree both consume it, so their numbering
//! agrees by construction: preorder tree order, the enclosing namespace
//! tracked as walk state.
//!
//! The walk descends into control-flow blocks and function bodies (a
//! symbol declared behind an `if (!function_exists(...))` guard counts
//! as declared) but never into a `MemberList`: members are not items in
//! this sub-project, so class constants and declarations nested inside
//! method bodies are invisible here. Nameless declarations (anonymous
//! classes, error-recovery wreckage) carry no top-level identity and
//! are skipped.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

/// One declaration node with its enclosing namespace (`""` is global).
pub(crate) struct ItemNode {
    pub(crate) node: SyntaxNode,
    pub(crate) namespace: String,
}

/// Every declaration node of the file, in tree order.
pub(crate) fn item_nodes(root: &SyntaxNode) -> Vec<ItemNode> {
    let mut items = Vec::new();
    let mut namespace = String::new();
    collect(root, &mut namespace, &mut items);
    items
}

fn collect(node: &SyntaxNode, namespace: &mut String, items: &mut Vec<ItemNode>) {
    for child in node.children() {
        match child.kind() {
            // Members are not items: class constants and declarations
            // nested inside method bodies stay invisible.
            SyntaxKind::MemberList => {}
            SyntaxKind::NamespaceDeclaration => {
                items.push(ItemNode {
                    node: child.clone(),
                    namespace: namespace.clone(),
                });
                let Some(declaration) = ast::NamespaceDeclaration::cast(child.clone())
                else {
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
                        collect(block.syntax(), &mut inner, items);
                    }
                    // Statement form: the name applies to what follows.
                    None => *namespace = declared,
                }
            }
            _ => {
                if is_item(&child) {
                    items.push(ItemNode {
                        node: child.clone(),
                        namespace: namespace.clone(),
                    });
                }
                collect(&child, namespace, items);
            }
        }
    }
}

/// Whether one node is a declaration node. Class-likes and functions
/// must be named: anonymous classes and error-recovery wreckage carry
/// no top-level identity.
fn is_item(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::UseDeclaration | SyntaxKind::ConstantDeclaration => true,
        SyntaxKind::ClassDeclaration => ast::ClassDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::InterfaceDeclaration => ast::InterfaceDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::TraitDeclaration => ast::TraitDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::EnumDeclaration => ast::EnumDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::FunctionDeclaration => ast::FunctionDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        _ => false,
    }
}
```

Prepend to `crates/celerrate_semantics/src/ast_id.rs` (above the test module
written in step 2):

```rust
use celerrate_source::FileId;
use celerrate_syntax::{SyntaxNode, SyntaxNodePtr};

use crate::item_nodes::item_nodes;

/// The stable identity of one declaration: the file plus the
/// declaration's position in the file's tree-order numbering. A body
/// edit renumbers nothing, so an `AstId` survives everyday editing;
/// reconciliation back to the concrete node goes through [`AstIdMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AstId {
    pub file: FileId,
    pub index: u32,
}

/// Per file: the declaration nodes in tree order, each reachable again
/// through its [`SyntaxNodePtr`]. The map holds pointers (kind plus
/// range), never red nodes, so it is a plain `Send + Sync` value — and
/// its value changes whenever ranges shift, which is why nothing
/// long-lived may depend on it (consumers sit behind the item tree and
/// reconcile spans through this map as late as possible).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AstIdMap {
    pointers: Vec<SyntaxNodePtr>,
}

impl AstIdMap {
    /// Numbers the declaration nodes of one file's syntax tree.
    pub fn from_root(root: &SyntaxNode) -> Self {
        Self {
            pointers: item_nodes(root)
                .iter()
                .map(|item| SyntaxNodePtr::new(&item.node))
                .collect(),
        }
    }

    /// The pointer of the declaration numbered `index`.
    pub fn pointer(&self, index: u32) -> Option<SyntaxNodePtr> {
        self.pointers.get(index as usize).copied()
    }

    /// The number of `node`, when it is a declaration node of the tree
    /// this map was built from.
    pub fn index_of(&self, node: &SyntaxNode) -> Option<u32> {
        let pointer = SyntaxNodePtr::new(node);
        self.pointers
            .iter()
            .position(|candidate| *candidate == pointer)
            .and_then(|position| u32::try_from(position).ok())
    }

    pub fn len(&self) -> usize {
        self.pointers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pointers.is_empty()
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics`
Expected: PASS (8 tests)

- [ ] **Step 6: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: clean

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): add stable declaration numbering"
```

---

### Task 3: The `ItemTree` — declarations

**Files:**
- Create: `crates/celerrate_semantics/src/item_tree.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `item_nodes` / `ItemNode` (task 2), `AstId` (task 2), the typed
  AST accessors, `Name::text()` (task 1).
- Produces (later tasks and part 5 rely on these exact shapes):
  - `pub enum DeclarationKind { Class, Interface, Trait, Enum, Function, Constant }`
  - `pub struct Declaration { pub kind: DeclarationKind, pub name: String,
    pub namespace: String, pub ast_id: AstId, pub extends: Vec<String>,
    pub implements: Vec<String>, pub trait_uses: Vec<String> }`
  - `pub struct ItemTree { pub declarations: Vec<Declaration>,
    pub imports: Vec<UseImport> }` with
    `pub fn from_root(file: FileId, root: &SyntaxNode) -> Self`
  - `pub enum ImportKind { Class, Function, Constant }` and
    `pub struct UseImport { pub kind: ImportKind, pub target: String,
    pub alias: String, pub namespace: String, pub ast_id: AstId }` (declared
    now so the tree's shape is final; populated in task 5).
  - In this task, `extends`, `implements`, and `trait_uses` are always empty
    (task 4 fills them) and `imports` is always empty (task 5 fills it).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/item_tree.rs` with the test module
first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::FileId;

    use super::{DeclarationKind, ItemTree};
    use crate::ast_id::AstId;

    fn tree_of(source: &str) -> ItemTree {
        ItemTree::from_root(FileId::new(0), &celerrate_syntax::parse(source).tree())
    }

    fn declared(source: &str) -> Vec<(DeclarationKind, String, String)> {
        tree_of(source)
            .declarations
            .iter()
            .map(|declaration| {
                (
                    declaration.kind,
                    declaration.name.clone(),
                    declaration.namespace.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn every_declaration_kind_is_projected() {
        assert_eq!(
            declared(
                "<?php\n\
                 class Service {}\n\
                 interface Contract {}\n\
                 trait Helper {}\n\
                 enum Suit {}\n\
                 function greet() {}\n\
                 const LIMIT = 1;\n",
            ),
            vec![
                (DeclarationKind::Class, "Service".to_owned(), String::new()),
                (DeclarationKind::Interface, "Contract".to_owned(), String::new()),
                (DeclarationKind::Trait, "Helper".to_owned(), String::new()),
                (DeclarationKind::Enum, "Suit".to_owned(), String::new()),
                (DeclarationKind::Function, "greet".to_owned(), String::new()),
                (DeclarationKind::Constant, "LIMIT".to_owned(), String::new()),
            ],
        );
    }

    #[test]
    fn a_statement_form_namespace_scopes_everything_after_it() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace First;\n\
                 function one() {}\n\
                 namespace Second;\n\
                 function two() {}\n",
            ),
            vec![
                (DeclarationKind::Function, "one".to_owned(), "First".to_owned()),
                (DeclarationKind::Function, "two".to_owned(), "Second".to_owned()),
            ],
        );
    }

    #[test]
    fn brace_form_namespaces_scope_their_block_only() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace Ds { class Vector {} }\n\
                 namespace { function outside() {} }\n",
            ),
            vec![
                (DeclarationKind::Class, "Vector".to_owned(), "Ds".to_owned()),
                (DeclarationKind::Function, "outside".to_owned(), String::new()),
            ],
        );
    }

    #[test]
    fn guarded_and_nested_declarations_are_projected() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace App;\n\
                 if (!function_exists('greet')) { function greet() {} }\n\
                 function outer() { function inner() {} }\n",
            ),
            vec![
                (DeclarationKind::Function, "greet".to_owned(), "App".to_owned()),
                (DeclarationKind::Function, "outer".to_owned(), "App".to_owned()),
                (DeclarationKind::Function, "inner".to_owned(), "App".to_owned()),
            ],
        );
    }

    #[test]
    fn members_are_not_projected() {
        assert_eq!(
            declared(
                "<?php class A { const B = 1; public $property; public function method() {} }",
            ),
            vec![(DeclarationKind::Class, "A".to_owned(), String::new())],
        );
    }

    #[test]
    fn anonymous_classes_are_not_projected() {
        assert_eq!(
            declared("<?php $instance = new class {}; class Named {}"),
            vec![(DeclarationKind::Class, "Named".to_owned(), String::new())],
        );
    }

    #[test]
    fn a_grouped_constant_declaration_projects_one_entry_per_element() {
        let tree = tree_of("<?php const A = 1, B = 2;");
        let names: Vec<&str> = tree
            .declarations
            .iter()
            .map(|declaration| declaration.name.as_str())
            .collect();
        assert_eq!(names, vec!["A", "B"]);
        // Both elements share the declaration node's identity.
        assert_eq!(
            tree.declarations.first().unwrap().ast_id,
            tree.declarations.last().unwrap().ast_id,
        );
    }

    #[test]
    fn ast_ids_are_the_tree_order_positions_of_the_declaration_nodes() {
        // Numbering: namespace = 0, use = 1, class = 2.
        let tree = ItemTree::from_root(
            FileId::new(7),
            &celerrate_syntax::parse("<?php namespace N; use A; class B {}").tree(),
        );
        assert_eq!(
            tree.declarations.first().map(|declaration| declaration.ast_id),
            Some(AstId {
                file: FileId::new(7),
                index: 2,
            }),
        );
    }

    #[test]
    fn original_spelling_is_preserved() {
        // Case folding is the index's concern (part 5), never the tree's.
        assert_eq!(
            declared("<?php class MiXeDcAsE {}"),
            vec![(DeclarationKind::Class, "MiXeDcAsE".to_owned(), String::new())],
        );
    }

    #[test]
    fn malformed_input_projects_what_the_parser_recovered() {
        assert_eq!(
            declared("<?php class Broken { function ok() {}"),
            vec![(DeclarationKind::Class, "Broken".to_owned(), String::new())],
        );
    }

    #[test]
    fn a_body_edit_produces_an_identical_item_tree() {
        // The early-cutoff property, at the value level: no ranges, no
        // offsets, so bodies, comments, and whitespace never show up.
        let before = tree_of("<?php function greet() { return 1; } class After {}");
        let body_edit = tree_of("<?php function greet() { return 2; } class After {}");
        let comment_edit =
            tree_of("<?php // note\nfunction greet() { return 1; }   class After {}");
        assert_eq!(before, body_edit);
        assert_eq!(before, comment_edit);
    }

    #[test]
    fn empty_and_html_only_files_project_nothing() {
        assert_eq!(tree_of("").declarations, Vec::new());
        assert_eq!(tree_of("plain text, no PHP").declarations, Vec::new());
    }
}
```

And register the module: in `crates/celerrate_semantics/src/lib.rs`, after
`mod item_nodes;` add `mod item_tree;` and extend the exports:

```rust
mod ast_id;
mod item_nodes;
mod item_tree;

pub use ast_id::{AstId, AstIdMap};
pub use item_tree::{Declaration, DeclarationKind, ImportKind, ItemTree, UseImport};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics`
Expected: FAIL to compile with `cannot find type 'ItemTree'`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/celerrate_semantics/src/item_tree.rs` (above the tests):

```rust
//! The per-file item tree: the `Eq`-comparable, deterministically
//! ordered projection of one file's declarations. It carries no ranges
//! and no offsets — a body, comment, or whitespace edit produces an
//! identical value, salsa backdates it, and nothing downstream re-runs.
//! That equality is the invalidation boundary of the engine.

use celerrate_source::FileId;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::ast_id::AstId;
use crate::item_nodes::{ItemNode, item_nodes};

/// The kind of a declared symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationKind {
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Constant,
}

/// One declared symbol: original spelling, enclosing namespace (`""`
/// is global), stable identity, and the unresolved inheritance names
/// exactly as written (sub-project 3 consumes them; they cost one
/// field now).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Declaration {
    pub kind: DeclarationKind,
    pub name: String,
    pub namespace: String,
    pub ast_id: AstId,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub trait_uses: Vec<String>,
}

/// What one `use` clause imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportKind {
    Class,
    Function,
    Constant,
}

/// One expanded `use` import: group forms are flattened, the target is
/// the written absolute name (leading backslash trimmed), and the
/// alias is the explicit one or the target's last segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UseImport {
    pub kind: ImportKind,
    pub target: String,
    pub alias: String,
    pub namespace: String,
    pub ast_id: AstId,
}

/// The projection of one file's declarations and imports, in tree
/// order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemTree {
    pub declarations: Vec<Declaration>,
    pub imports: Vec<UseImport>,
}

impl ItemTree {
    /// Projects one file's syntax tree. Positions in the shared
    /// declaration-node traversal are the `AstId` indexes, so this
    /// numbering and [`crate::AstIdMap`]'s agree by construction.
    pub fn from_root(file: FileId, root: &SyntaxNode) -> Self {
        let mut tree = ItemTree::default();
        for (position, item) in item_nodes(root).into_iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                break;
            };
            lower(&item, AstId { file, index }, &mut tree);
        }
        tree
    }
}

/// The unresolved inheritance names of one class-like declaration.
struct Inheritance {
    extends: Vec<String>,
    implements: Vec<String>,
    trait_uses: Vec<String>,
}

impl Inheritance {
    const NONE: Inheritance = Inheritance {
        extends: Vec::new(),
        implements: Vec::new(),
        trait_uses: Vec::new(),
    };
}

fn lower(item: &ItemNode, ast_id: AstId, tree: &mut ItemTree) {
    match item.node.kind() {
        SyntaxKind::ClassDeclaration => {
            if let Some(declaration) = ast::ClassDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Class,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::InterfaceDeclaration => {
            if let Some(declaration) = ast::InterfaceDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Interface,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::TraitDeclaration => {
            if let Some(declaration) = ast::TraitDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Trait,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::EnumDeclaration => {
            if let Some(declaration) = ast::EnumDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Enum,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::FunctionDeclaration => {
            if let Some(declaration) = ast::FunctionDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Function,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::ConstantDeclaration => {
            if let Some(declaration) = ast::ConstantDeclaration::cast(item.node.clone()) {
                for element in declaration.constant_elements() {
                    push_declaration(
                        tree,
                        item,
                        ast_id,
                        DeclarationKind::Constant,
                        element.name_token(),
                        Inheritance::NONE,
                    );
                }
            }
        }
        // Use declarations expand into imports (a later task);
        // namespace declarations carry no projection of their own.
        _ => {}
    }
}

fn push_declaration(
    tree: &mut ItemTree,
    item: &ItemNode,
    ast_id: AstId,
    kind: DeclarationKind,
    name_token: Option<SyntaxToken>,
    inheritance: Inheritance,
) {
    let Some(name_token) = name_token else { return };
    tree.declarations.push(Declaration {
        kind,
        name: name_token.text().to_owned(),
        namespace: item.namespace.clone(),
        ast_id,
        extends: inheritance.extends,
        implements: inheritance.implements,
        trait_uses: inheritance.trait_uses,
    });
}
```

Note: `SyntaxToken` may be flagged as unused until task 5; it is used by
`push_declaration`'s signature already, so the import is live now.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics`
Expected: PASS (task 2's 8 tests + this task's 12)

- [ ] **Step 5: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: clean

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): project declarations into an item tree"
```

---

### Task 4: Unresolved inheritance names

**Files:**
- Modify: `crates/celerrate_semantics/src/item_tree.rs`

**Interfaces:**
- Consumes: `ExtendsClause::names()`, `ImplementsClause::names()`,
  `MemberList::member_declarations()`, `MemberDeclaration::TraitUseClause`,
  `TraitUseClause::names()`, `Name::text()`.
- Produces: `Declaration::{extends, implements, trait_uses}` populated with
  the written names, spelling and qualifiers preserved. Part 5's resolution
  and sub-project 3's linearization consume them.

- [ ] **Step 1: Write the failing tests**

Append to the test module of `crates/celerrate_semantics/src/item_tree.rs`:

```rust
    fn only_declaration(source: &str) -> super::Declaration {
        let tree = tree_of(source);
        assert_eq!(tree.declarations.len(), 1, "expected one declaration");
        tree.declarations.into_iter().next().unwrap()
    }

    #[test]
    fn a_class_carries_its_unresolved_inheritance_names() {
        let class = only_declaration(
            "<?php namespace App;\n\
             class Service extends \\Core\\Base implements Contract, \\Psr\\Log\\LoggerAwareInterface {\n\
                 use Concerns\\Loggable;\n\
                 use \\Shared\\Serializable;\n\
             }\n",
        );
        assert_eq!(class.extends, vec!["\\Core\\Base".to_owned()]);
        assert_eq!(
            class.implements,
            vec![
                "Contract".to_owned(),
                "\\Psr\\Log\\LoggerAwareInterface".to_owned(),
            ],
        );
        assert_eq!(
            class.trait_uses,
            vec![
                "Concerns\\Loggable".to_owned(),
                "\\Shared\\Serializable".to_owned(),
            ],
        );
    }

    #[test]
    fn an_interface_extends_many_parents() {
        let interface =
            only_declaration("<?php interface Both extends First, Second\\Third {}");
        assert_eq!(
            interface.extends,
            vec!["First".to_owned(), "Second\\Third".to_owned()],
        );
        assert_eq!(interface.implements, Vec::<String>::new());
    }

    #[test]
    fn an_enum_carries_its_implements_names() {
        let declaration =
            only_declaration("<?php enum Suit: string implements HasColor { use Colored; }");
        assert_eq!(declaration.implements, vec!["HasColor".to_owned()]);
        assert_eq!(declaration.trait_uses, vec!["Colored".to_owned()]);
    }

    #[test]
    fn a_grouped_trait_use_lists_every_name() {
        let class = only_declaration("<?php class Mixed { use A, B\\C; }");
        assert_eq!(
            class.trait_uses,
            vec!["A".to_owned(), "B\\C".to_owned()],
        );
    }

    #[test]
    fn functions_and_constants_carry_no_inheritance() {
        let function = only_declaration("<?php function greet() {}");
        assert_eq!(function.extends, Vec::<String>::new());
        assert_eq!(function.implements, Vec::<String>::new());
        assert_eq!(function.trait_uses, Vec::<String>::new());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics a_class_carries`
Expected: FAIL — `extends` is empty (`Inheritance::NONE` everywhere).

- [ ] **Step 3: Write the implementation**

In `crates/celerrate_semantics/src/item_tree.rs`, add two helpers next to
`Inheritance`:

```rust
/// The written names of one clause, in source order.
fn names_of(names: Option<ast::AstChildren<ast::Name>>) -> Vec<String> {
    names
        .into_iter()
        .flatten()
        .map(|name| name.text())
        .collect()
}

/// The trait names a class-like uses, read from its member list. The
/// traversal never descends into member lists; this projection field
/// is the one place the tree looks inside one, because the spec names
/// trait `use` among the inheritance names.
fn trait_use_names(member_list: Option<ast::MemberList>) -> Vec<String> {
    member_list
        .into_iter()
        .flat_map(|list| list.member_declarations())
        .filter_map(|member| match member {
            ast::MemberDeclaration::TraitUseClause(clause) => Some(clause),
            _ => None,
        })
        .flat_map(|clause| clause.names())
        .map(|name| name.text())
        .collect()
}
```

All four class-likes share the same accessor names (`extends_clause`,
`implements_clause`, `member_list` — the grammar carries them on every
class-like, so reading them uniformly is also the error-tolerant choice), and
the generated types cannot share a trait, so a small macro keeps the four
arms from diverging. Add next to the helpers:

```rust
/// The unresolved inheritance names of one class-like declaration. The
/// four generated class-like types share accessor names but no trait;
/// this macro reads them uniformly.
macro_rules! inheritance_of {
    ($declaration:expr) => {
        Inheritance {
            extends: names_of($declaration.extends_clause().map(|clause| clause.names())),
            implements: names_of(
                $declaration.implements_clause().map(|clause| clause.names()),
            ),
            trait_uses: trait_use_names($declaration.member_list()),
        }
    };
}
```

Then replace `Inheritance::NONE` in the four class-like arms of `lower` with
`inheritance_of!(declaration)`, so they read:

```rust
        SyntaxKind::ClassDeclaration => {
            if let Some(declaration) = ast::ClassDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Class,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
        SyntaxKind::InterfaceDeclaration => {
            if let Some(declaration) = ast::InterfaceDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Interface,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
        SyntaxKind::TraitDeclaration => {
            if let Some(declaration) = ast::TraitDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Trait,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
        SyntaxKind::EnumDeclaration => {
            if let Some(declaration) = ast::EnumDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Enum,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
```

(`FunctionDeclaration` and `ConstantDeclaration` keep `Inheritance::NONE`,
so the constant stays. The macro must be defined above `lower` in the file —
declarative macros resolve textually.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics`
Expected: PASS

- [ ] **Step 5: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: clean

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): carry unresolved inheritance names"
```

---

### Task 5: Use imports

**Files:**
- Modify: `crates/celerrate_semantics/src/item_tree.rs`

**Interfaces:**
- Consumes: `UseDeclaration::{import_type_token, use_clauses}`,
  `UseClause::{import_type_token, name, use_group, alias_token}`,
  `UseGroup::use_clauses`, `Name::text()`.
- Produces: `ItemTree::imports` populated: group forms flattened, kind
  resolved (declaration-level inherited, clause-level overrides), target
  absolute (leading backslash trimmed), alias explicit or derived. Part 5
  builds the per-namespace resolution tables from exactly this.

- [ ] **Step 1: Write the failing tests**

Append to the test module of `crates/celerrate_semantics/src/item_tree.rs`:

```rust
    use super::{ImportKind, UseImport};

    fn imports_of(source: &str) -> Vec<UseImport> {
        tree_of(source).imports
    }

    fn targets_and_aliases(source: &str) -> Vec<(ImportKind, String, String)> {
        imports_of(source)
            .into_iter()
            .map(|import| (import.kind, import.target, import.alias))
            .collect()
    }

    #[test]
    fn a_simple_use_imports_a_class_with_its_last_segment_as_alias() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Bar".to_owned())],
        );
    }

    #[test]
    fn a_leading_backslash_is_trimmed_from_the_target() {
        // Use targets are always absolute; the written backslash adds
        // nothing.
        assert_eq!(
            targets_and_aliases("<?php use \\Foo\\Bar;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Bar".to_owned())],
        );
    }

    #[test]
    fn an_explicit_alias_wins() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar as Baz;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Baz".to_owned())],
        );
    }

    #[test]
    fn function_and_const_declarations_set_the_import_kind() {
        assert_eq!(
            targets_and_aliases("<?php use function Foo\\greet; use const Foo\\LIMIT;"),
            vec![
                (ImportKind::Function, "Foo\\greet".to_owned(), "greet".to_owned()),
                (ImportKind::Constant, "Foo\\LIMIT".to_owned(), "LIMIT".to_owned()),
            ],
        );
    }

    #[test]
    fn a_group_expands_with_the_shared_prefix() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar\\{Baz, Qux\\Deep as D};"),
            vec![
                (ImportKind::Class, "Foo\\Bar\\Baz".to_owned(), "Baz".to_owned()),
                (ImportKind::Class, "Foo\\Bar\\Qux\\Deep".to_owned(), "D".to_owned()),
            ],
        );
    }

    #[test]
    fn a_mixed_group_overrides_the_kind_per_clause() {
        assert_eq!(
            targets_and_aliases(
                "<?php use Foo\\{function greet, const LIMIT, Service};",
            ),
            vec![
                (ImportKind::Function, "Foo\\greet".to_owned(), "greet".to_owned()),
                (ImportKind::Constant, "Foo\\LIMIT".to_owned(), "LIMIT".to_owned()),
                (ImportKind::Class, "Foo\\Service".to_owned(), "Service".to_owned()),
            ],
        );
    }

    #[test]
    fn comma_separated_clauses_each_import() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\A, Foo\\B;"),
            vec![
                (ImportKind::Class, "Foo\\A".to_owned(), "A".to_owned()),
                (ImportKind::Class, "Foo\\B".to_owned(), "B".to_owned()),
            ],
        );
    }

    #[test]
    fn imports_carry_their_enclosing_namespace_and_identity() {
        let tree = tree_of("<?php namespace App; use Lib\\Helper;");
        let import = tree.imports.first().unwrap();
        assert_eq!(import.namespace, "App");
        // Numbering: namespace = 0, use declaration = 1.
        assert_eq!(import.ast_id.index, 1);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics a_simple_use`
Expected: FAIL — `imports` is empty (the `UseDeclaration` arm does nothing).

- [ ] **Step 3: Write the implementation**

In `crates/celerrate_semantics/src/item_tree.rs`, replace the fall-through
arm of `lower` so `UseDeclaration` expands (the final `_ => {}` stays for
namespace declarations):

```rust
        SyntaxKind::UseDeclaration => {
            if let Some(declaration) = ast::UseDeclaration::cast(item.node.clone()) {
                let inherited = import_kind_of(declaration.import_type_token())
                    .unwrap_or(ImportKind::Class);
                for clause in declaration.use_clauses() {
                    expand_use_clause(&clause, inherited, "", item, ast_id, tree);
                }
            }
        }
```

and add the helpers:

```rust
/// The import kind named by a `function` / `const` token, when present.
fn import_kind_of(token: Option<SyntaxToken>) -> Option<ImportKind> {
    match token?.kind() {
        SyntaxKind::Function => Some(ImportKind::Function),
        SyntaxKind::Const => Some(ImportKind::Constant),
        _ => None,
    }
}

/// Expands one `use` clause: a plain clause becomes one import, a
/// group form recurses with the accumulated prefix. Wreckage without a
/// usable target expands to nothing.
fn expand_use_clause(
    clause: &ast::UseClause,
    inherited: ImportKind,
    prefix: &str,
    item: &ItemNode,
    ast_id: AstId,
    tree: &mut ItemTree,
) {
    let kind = import_kind_of(clause.import_type_token()).unwrap_or(inherited);
    let written = clause
        .name()
        .map(|name| name.text())
        .unwrap_or_default();
    let target = join_qualified(prefix, written.trim_start_matches('\\'));
    if let Some(group) = clause.use_group() {
        for inner in group.use_clauses() {
            expand_use_clause(&inner, kind, &target, item, ast_id, tree);
        }
        return;
    }
    if target.is_empty() {
        return;
    }
    let alias = clause
        .alias_token()
        .map(|token| token.text().to_owned())
        .unwrap_or_else(|| last_segment(&target).to_owned());
    tree.imports.push(UseImport {
        kind,
        target,
        alias,
        namespace: item.namespace.clone(),
        ast_id,
    });
}

fn join_qualified(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else if name.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}\\{name}")
    }
}

fn last_segment(target: &str) -> &str {
    target.rsplit('\\').next().unwrap_or(target)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics`
Expected: PASS

- [ ] **Step 5: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: clean

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): expand use imports in the item tree"
```

---

### Task 6: The salsa queries

**Files:**
- Modify: `crates/celerrate_semantics/Cargo.toml`
- Create: `crates/celerrate_semantics/src/queries.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `celerrate_db::{SourceFile, parse}`,
  `celerrate_db::testing::TestDatabase` (tests only), `salsa`.
- Produces (part 5 and the tests of tasks 7-8 consume these):
  - `pub fn ast_id_map(db: &dyn salsa::Database, file: SourceFile) -> &AstIdMap`
    (a `#[salsa::tracked(returns(ref))]` query)
  - `pub fn item_tree(db: &dyn salsa::Database, file: SourceFile) -> &ItemTree`
    (a `#[salsa::tracked(returns(ref))]` query)

- [ ] **Step 1: Add the dependencies**

In `crates/celerrate_semantics/Cargo.toml`, extend `[dependencies]`:

```toml
[dependencies]
celerrate_db = { path = "../celerrate_db" }
celerrate_source = { path = "../celerrate_source" }
celerrate_syntax = { path = "../celerrate_syntax" }
salsa = { workspace = true }
```

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_semantics/src/queries.rs` with the test module
first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_source::FileId;
    use celerrate_syntax::SyntaxKind;
    use salsa::Setter;

    use super::{ast_id_map, item_tree};

    #[test]
    fn the_item_tree_query_projects_a_file() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(3),
            b"<?php namespace App; class Service {}".to_vec(),
        );
        let tree = item_tree(&db, file);
        let declaration = tree.declarations.first().unwrap();
        assert_eq!(declaration.name, "Service");
        assert_eq!(declaration.namespace, "App");
        assert_eq!(declaration.ast_id.file, FileId::new(3));
    }

    #[test]
    fn the_map_and_the_tree_number_declarations_identically() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php namespace N; use A; class B extends C {} function d() {} const E = 1;"
                .to_vec(),
        );
        let tree = item_tree(&db, file);
        let map = ast_id_map(&db, file);
        let root = celerrate_db::parse(&db, file).tree();
        assert!(!tree.declarations.is_empty());
        for declaration in &tree.declarations {
            let node = map
                .pointer(declaration.ast_id.index)
                .and_then(|pointer| pointer.try_to_node(&root));
            assert!(
                node.is_some(),
                "declaration {declaration:?} must reconcile through the map",
            );
        }
        for import in &tree.imports {
            let kind = map
                .pointer(import.ast_id.index)
                .and_then(|pointer| pointer.try_to_node(&root))
                .map(|node| node.kind());
            assert_eq!(kind, Some(SyntaxKind::UseDeclaration));
        }
    }

    #[test]
    fn editing_bytes_reprojects() {
        let mut db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 1);
        file.set_bytes(&mut db)
            .to(b"<?php class A {} class B {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 2);
    }

    #[test]
    fn an_undecodable_file_projects_an_empty_tree() {
        // Oversized or undecodable inputs parse as empty in
        // celerrate_db; the projection follows without failing.
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"\xFF\xFE<?php".to_vec());
        let _ = item_tree(&db, file);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics`
Expected: FAIL to compile with `cannot find function 'item_tree'` (the module
is not registered yet; register it in step 4).

- [ ] **Step 4: Write the implementation**

Prepend to `crates/celerrate_semantics/src/queries.rs`:

```rust
//! The boundary as salsa queries. Two per-file queries with split
//! volatility: the numbering carries ranges and re-runs whenever they
//! shift; the item tree carries none, so a body edit backdates and
//! everything downstream is spared. Query definitions live here, in
//! their domain crate; the concrete database is assembled at the
//! composition root.

use celerrate_db::SourceFile;

use crate::ast_id::AstIdMap;
use crate::item_tree::ItemTree;

/// The declaration numbering of one file. The value changes whenever
/// ranges shift: consume [`item_tree`] instead, and reconcile spans
/// through this map as late as possible.
#[salsa::tracked(returns(ref))]
pub fn ast_id_map(db: &dyn salsa::Database, file: SourceFile) -> AstIdMap {
    AstIdMap::from_root(&celerrate_db::parse(db, file).tree())
}

/// The item tree of one file: range-free, so a body edit produces an
/// equal value and salsa backdates it — the early-cutoff boundary
/// every cross-file consumer sits behind.
#[salsa::tracked(returns(ref))]
pub fn item_tree(db: &dyn salsa::Database, file: SourceFile) -> ItemTree {
    ItemTree::from_root(file.file_id(db), &celerrate_db::parse(db, file).tree())
}
```

In `crates/celerrate_semantics/src/lib.rs`, register and export:

```rust
mod ast_id;
mod item_nodes;
mod item_tree;
mod queries;

pub use ast_id::{AstId, AstIdMap};
pub use item_tree::{Declaration, DeclarationKind, ImportKind, ItemTree, UseImport};
pub use queries::{ast_id_map, item_tree};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics`
Expected: PASS

- [ ] **Step 6: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: clean (no new external dependencies, but `deny` confirms it)

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): expose the boundary as salsa queries"
```

---

### Task 7: Invalidation-scope tests — the early-cutoff proof

This is the deliverable the part is named for: salsa event instrumentation
asserting, per canonical edit class, exactly which queries re-executed. A
test-local tracked query (`declared_names`) stands in for part 5's per-name
consumers.

**Files:**
- Test: `crates/celerrate_semantics/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: `celerrate_db::testing::TestDatabase` (its `take_executed`
  drains the `WillExecute` log), `celerrate_semantics::{ast_id_map, item_tree}`,
  `salsa::Setter`.
- Produces: nothing new — pins engine behavior.

- [ ] **Step 1: Write the tests**

Create `crates/celerrate_semantics/tests/invalidation_scope.rs`:

```rust
//! Invalidation-scope tests for the boundary: after each canonical
//! edit class, assert exactly which queries re-executed. The
//! consistency harness verifies the result; these tests verify how
//! little work produced it — the direct proof of the item tree's
//! early cutoff, which no correctness test can observe.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::SourceFile;
use celerrate_db::testing::TestDatabase;
use celerrate_semantics::{ast_id_map, item_tree};
use celerrate_source::FileId;
use salsa::Setter;

/// A stand-in for part 5's consumers: any query that reads the item
/// tree and nothing else syntactic. If the tree backdates, this must
/// never re-run.
#[salsa::tracked]
fn declared_names(db: &dyn salsa::Database, file: SourceFile) -> Vec<String> {
    item_tree(db, file)
        .declarations
        .iter()
        .map(|declaration| declaration.name.clone())
        .collect()
}

fn executions_of(log: &[String], query: &str) -> usize {
    let prefix = format!("{query}(");
    log.iter()
        .filter(|entry| entry.contains(prefix.as_str()))
        .count()
}

#[test]
fn a_body_edit_reaches_the_item_tree_and_stops_there() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function greet() { return 1; }".to_vec(),
    );
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function greet() { return 2; }".to_vec());
    let _ = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "parse"),
        1,
        "the edited file reparses: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "the projection re-runs over the new tree: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "declared_names"),
        0,
        "an identical item tree must backdate, sparing every consumer: {log:?}",
    );
}

#[test]
fn a_comment_only_edit_spares_every_consumer() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function greet() { return 1; }".to_vec(),
    );
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php // a note\nfunction greet() { return 1; }".to_vec());
    let _ = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(executions_of(&log, "item_tree"), 1, "{log:?}");
    assert_eq!(executions_of(&log, "declared_names"), 0, "{log:?}");
}

#[test]
fn a_whitespace_shift_renumbers_without_reprojecting_consumers() {
    // The split-volatility design, observed directly: ranges shifted,
    // so the numbering's value changes — but the item tree is
    // range-free, equal, backdated.
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function greet() {}".to_vec(),
    );
    let _ = declared_names(&db, file);
    let _ = ast_id_map(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php\n\nfunction greet() {}".to_vec());
    let _ = declared_names(&db, file);
    let _ = ast_id_map(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "ast_id_map"),
        1,
        "ranges shifted, the numbering re-runs: {log:?}",
    );
    assert_eq!(executions_of(&log, "item_tree"), 1, "{log:?}");
    assert_eq!(
        executions_of(&log, "declared_names"),
        0,
        "a range-free item tree must backdate: {log:?}",
    );
}

#[test]
fn a_signature_edit_reaches_the_consumers() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function greet() { return 1; }".to_vec(),
    );
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function hello() { return 1; }".to_vec());
    let names = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "declared_names"),
        1,
        "a renamed declaration must reach the consumers: {log:?}",
    );
    assert_eq!(names, vec!["hello".to_owned()]);
}

#[test]
fn adding_a_declaration_reaches_the_consumers() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
    let _ = declared_names(&db, file);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php class A {} class B {}".to_vec());
    let names = declared_names(&db, file);

    let log = db.take_executed();
    assert_eq!(executions_of(&log, "declared_names"), 1, "{log:?}");
    assert_eq!(names, vec!["A".to_owned(), "B".to_owned()]);
}

#[test]
fn editing_one_file_reprojects_only_that_file() {
    let mut db = TestDatabase::default();
    let edited = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
    let untouched = SourceFile::new(&db, FileId::new(1), b"<?php class B {}".to_vec());
    let _ = declared_names(&db, edited);
    let _ = declared_names(&db, untouched);
    db.take_executed();

    edited
        .set_bytes(&mut db)
        .to(b"<?php class A {} class C {}".to_vec());
    let _ = declared_names(&db, edited);
    let _ = declared_names(&db, untouched);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "only the edited file reprojects: {log:?}",
    );
    assert_eq!(executions_of(&log, "declared_names"), 1, "{log:?}");
}

#[test]
fn a_new_file_does_not_reanalyze_existing_files() {
    let db = TestDatabase::default();
    let existing = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
    let _ = declared_names(&db, existing);
    db.take_executed();

    let added = SourceFile::new(&db, FileId::new(1), b"<?php class B {}".to_vec());
    let _ = declared_names(&db, added);
    let _ = declared_names(&db, existing);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "item_tree"),
        1,
        "only the new file lowers: {log:?}",
    );
    assert_eq!(executions_of(&log, "declared_names"), 1, "{log:?}");
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --package celerrate_semantics --test invalidation_scope`
Expected: PASS. These tests pin already-implemented behavior; if any fails,
the engine has a real early-cutoff defect — investigate the failing edit
class (most likely a range or offset leaked into `ItemTree`), do not weaken
the assertion.

- [ ] **Step 3: Run the workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_semantics/tests/invalidation_scope.rs
git commit -m "✅ test(semantics): pin the early cutoff per edit class"
```

---

### Task 8: The incremental-consistency harness grows the boundary

The part 1 harness replays edits and compares against a from-scratch
database, but it lives in `celerrate_db` and cannot know about item trees
(no upward dependencies). Generalize it with a closure variant, keep the
existing entry point delegating to it, and give `celerrate_semantics` its
own replay suite.

**Files:**
- Modify: `crates/celerrate_db/src/testing.rs`
- Test: `crates/celerrate_semantics/tests/incremental_consistency.rs`
- (Unchanged but exercised: `crates/celerrate_db/tests/incremental_consistency.rs`)

**Interfaces:**
- Consumes: the existing `assert_incremental_consistency(initial, edits)`
  and `TestDatabase`.
- Produces:
  `pub fn assert_incremental_consistency_with(initial: &[&[u8]], edits: &[(usize, &[u8])], assert_file_matches: &dyn Fn(&TestDatabase, SourceFile, &TestDatabase, SourceFile, usize))`
  — the closure receives (incremental database, its file handle, from-scratch
  database, its fresh file handle, file index) for every file after every
  edit. Part 5's index-level replays will reuse it too.

- [ ] **Step 1: Write the failing test**

Create `crates/celerrate_semantics/tests/incremental_consistency.rs`:

```rust
//! The incremental correctness harness, grown to the boundary: edit
//! sequences replayed over one incremental database, with the item
//! tree and the numbering asserted identical to a from-scratch
//! analysis after every edit.

#![allow(clippy::unwrap_used)]

use celerrate_db::testing::assert_incremental_consistency_with;
use celerrate_semantics::{ast_id_map, item_tree};

fn assert_semantic_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])]) {
    assert_incremental_consistency_with(
        initial,
        edits,
        &|incremental, file, from_scratch, fresh_file, index| {
            assert_eq!(
                item_tree(incremental, file),
                item_tree(from_scratch, fresh_file),
                "item tree diverged for file {index}",
            );
            assert_eq!(
                ast_id_map(incremental, file),
                ast_id_map(from_scratch, fresh_file),
                "declaration numbering diverged for file {index}",
            );
        },
    );
}

#[test]
fn body_signature_and_namespace_edits_replay_consistently() {
    assert_semantic_consistency(
        &[b"<?php namespace App; use Lib\\Helper; class Service { public function run() { return 1; } }"],
        &[
            (0, b"<?php namespace App; use Lib\\Helper; class Service { public function run() { return 2; } }"),
            (0, b"<?php namespace App; use Lib\\Helper; class Renamed { public function run() { return 2; } }"),
            (0, b"<?php namespace Core; class Renamed extends Base {}"),
        ],
    );
}

#[test]
fn declaration_churn_replays_consistently() {
    assert_semantic_consistency(
        &[b"<?php function keep() {}"],
        &[
            (0, b"<?php function keep() {} function added() {}"),
            (0, b"<?php if (!function_exists('keep')) { function keep() {} }"),
            (0, b"<?php use Foo\\{Bar, function baz}; const A = 1, B = 2;"),
            (0, b"<?php"),
        ],
    );
}

#[test]
fn malformed_intermediate_states_replay_consistently() {
    // Mid-typing states: the boundary must stay consistent over
    // whatever the error-resilient parser recovers.
    assert_semantic_consistency(
        &[b"<?php class Complete {}"],
        &[
            (0, b"<?php class Broken {"),
            (0, b"<?php class Broken { use "),
            (0, b"<?php class Fixed { use Helper; }"),
        ],
    );
}

#[test]
fn multiple_files_replay_independently() {
    assert_semantic_consistency(
        &[
            b"<?php namespace A; class One {}",
            b"<?php namespace B; class Two {}",
        ],
        &[
            (0, b"<?php namespace A; class One { public function m() {} }"),
            (1, b"<?php namespace B; class Two {} class Three {}"),
            (0, b"<?php namespace A;"),
        ],
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_semantics --test incremental_consistency`
Expected: FAIL to compile with
`cannot find function 'assert_incremental_consistency_with'`.

- [ ] **Step 3: Generalize the harness**

In `crates/celerrate_db/src/testing.rs`, replace the two functions
`assert_incremental_consistency` and `assert_matches_from_scratch` with:

```rust
/// Replays an edit sequence against one incremental database and, after
/// every edit, asserts each file's analysis is byte-for-byte identical
/// to a from-scratch database built on the current state.
///
/// `initial` provides the starting bytes of file 0, 1, 2, ...; each
/// edit is `(file index, new bytes)`. Panics (test-style assertions)
/// on any divergence or out-of-range file index.
pub fn assert_incremental_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])]) {
    assert_incremental_consistency_with(
        initial,
        edits,
        &|incremental, file, from_scratch, fresh_file, index| {
            assert_eq!(
                parse(incremental, file).tree().text().to_string(),
                parse(from_scratch, fresh_file).tree().text().to_string(),
                "tree text diverged for file {index}",
            );
            assert_eq!(
                file_diagnostics(incremental, file),
                file_diagnostics(from_scratch, fresh_file),
                "diagnostics diverged for file {index}",
            );
        },
    );
}

/// The closure form of the replay: upper layers (the item tree today,
/// the symbol index in part 5) extend the same harness with their own
/// per-file comparison, without any dependency from this crate upward.
/// The closure receives the incremental database and its file handle,
/// the from-scratch database and its fresh handle, and the file index;
/// it runs for every file after the initial state and after every edit.
pub fn assert_incremental_consistency_with(
    initial: &[&[u8]],
    edits: &[(usize, &[u8])],
    assert_file_matches: &dyn Fn(&TestDatabase, SourceFile, &TestDatabase, SourceFile, usize),
) {
    let mut incremental = TestDatabase::default();
    let mut current: Vec<Vec<u8>> = initial.iter().map(|bytes| bytes.to_vec()).collect();
    let files: Vec<SourceFile> = current
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            SourceFile::new(&incremental, FileId::new(index as u32), bytes.clone())
        })
        .collect();

    assert_matches_from_scratch(&incremental, &files, &current, assert_file_matches);
    for &(file_index, new_bytes) in edits {
        assert!(
            file_index < files.len(),
            "edit targets unknown file index {file_index}",
        );
        let (Some(slot), Some(file)) = (current.get_mut(file_index), files.get(file_index)) else {
            // Unreachable: guarded by the assertion above.
            return;
        };
        *slot = new_bytes.to_vec();
        file.set_bytes(&mut incremental).to(new_bytes.to_vec());
        assert_matches_from_scratch(&incremental, &files, &current, assert_file_matches);
    }
}

fn assert_matches_from_scratch(
    incremental: &TestDatabase,
    files: &[SourceFile],
    current: &[Vec<u8>],
    assert_file_matches: &dyn Fn(&TestDatabase, SourceFile, &TestDatabase, SourceFile, usize),
) {
    let from_scratch = TestDatabase::default();
    for (index, (file, bytes)) in files.iter().zip(current).enumerate() {
        let fresh_file = SourceFile::new(&from_scratch, FileId::new(index as u32), bytes.clone());
        assert_file_matches(incremental, *file, &from_scratch, fresh_file, index);
    }
}
```

(The module's existing imports already cover everything used here.)

- [ ] **Step 4: Run the db harness tests to verify the refactor holds**

Run: `cargo test --package celerrate_db`
Expected: PASS (the existing `incremental_consistency` suite pins the
delegating entry point)

- [ ] **Step 5: Commit the refactor**

```bash
git add crates/celerrate_db/src/testing.rs
git commit -m "♻️ refactor(db): open the harness to upper-layer checks"
```

- [ ] **Step 6: Run the semantics replay tests to verify they pass**

Run: `cargo test --package celerrate_semantics --test incremental_consistency`
Expected: PASS

- [ ] **Step 7: Run the workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: clean

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_semantics/tests/incremental_consistency.rs
git commit -m "✅ test(semantics): replay edits against from-scratch runs"
```

---

### Task 9: Record the narrowings in the design spec

The repository's established pattern: decisions narrowed during a part's
implementation are recorded back into the umbrella design, so the spec stays
the source of truth.

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-11-semantic-core-design.md`

- [ ] **Step 1: Append the narrowings to section 6**

In section 6 ("The invalidation boundary and name resolution"), after the
paragraph on the minimal `ItemTree` (the one ending "Syntax trees themselves
are LRU-evicted and reparsed on demand."), insert:

```markdown
Deliberate narrowings, recorded here after implementation review. The
traversal that defines "the declaration nodes" (shared by the `AstIdMap`
and the `ItemTree`, so their numbering agrees by construction) descends
into control-flow blocks and function bodies — a declaration behind an
`if (!function_exists(...))` guard is an item, the section 7 stance
honored structurally — but never into a member list: class constants
and declarations nested inside method bodies are invisible to the
boundary, and nameless declarations (anonymous classes, error-recovery
wreckage) are skipped until the type engine gives them meaning.
Namespaces are carried as a field on each declaration and import rather
than as standalone items: the same information, in the `Eq`-stable
encoding the early cutoff wants. LRU eviction of syntax trees is
deferred to part 8, where the memory economics are measured alongside
the persistent cache; part 4 delivers the structural property that
makes eviction safe (no layer above the boundary holds a syntax node —
the `AstIdMap` and the `ItemTree` are plain `Send + Sync` values).
```

- [ ] **Step 2: Verify the workspace is fully green**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all clean — this is the part's closing verification.

- [ ] **Step 3: Commit**

```bash
git add .claude/superpowers/specs/2026-07-11-semantic-core-design.md
git commit -m "📝 docs(specs): record the boundary narrowings"
```
