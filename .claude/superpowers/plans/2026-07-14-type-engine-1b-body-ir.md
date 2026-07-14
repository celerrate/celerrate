# Type Engine 1b — Body IR Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lower every function and method body into a compact, range-free, `Eq`-comparable arena (the body IR) behind a per-body salsa query, with a range-carrying source-map sibling reconciled late, the spec's pinned sugar reductions (null-safe chains, first-class callables), and the redefined comment-only edit class (trivia no annotation reader consumes).

**Architecture:** The `ItemTree`/`AstIdMap` split, one level down. A per-body query (`body_ir`, keyed by an interned `BodyQuery` carrying the declaration's `AstId`) lowers the body block into two dense arenas (expressions, statements) holding no text offset whatsoever, so any edit above the body, and any ignorable-trivia or formatting-only edit inside it, produces an identical value that salsa backdates: body consumers (inference, plan 5) are structurally spared. A sibling query (`body_source_map`) re-runs the same walk and keeps the arena-index-to-`SyntaxNodePtr` mapping, free to change on every edit, consulted only at rendering time. Recognized annotation content (docblocks inside bodies, suppression-directive comments) is carried in the IR, so an edit to it invalidates body consumers while prose comments stay invisible. This is the rust-analyzer `hir-def` pattern, transposed (spec section 2).

**Tech Stack:** Rust workspace, salsa 0.27 (tracked and interned queries), the `celerrate_syntax` typed AST, plain assertions (no insta), TDD with `cargo test`.

**Spec:** `.claude/superpowers/specs/2026-07-14-type-engine-design.md` section 2 ("Bodies lower to a body IR") and section 11 item 2. Read it before starting.

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Option`/`Result`; test modules open with `#![allow(clippy::unwrap_used)]` (add `panic`, `indexing_slicing` only when used).
- TDD: every task writes its failing tests first, watches them fail, then implements minimally.
- Strict layering: all production changes in this plan live in `celerrate_semantics` (which already depends on `celerrate_syntax`, `celerrate_db`, `celerrate_source`, and salsa). No new dependencies, no upward edges.
- Error resilience: no input may crash the lowering. Every accessor miss lowers to a `Missing` value; error-recovery wreckage is tolerated, never a failure.
- Determinism: no wall clock, no randomness, no environment reads inside queries. Arena numbering is allocation-order (a pure function of the tree walk).
- Everything in English, full words (no abbreviated identifiers; standard acronyms such as IR are fine).
- Commits: gitmoji + Conventional Commits (`✅ test(semantics): …`, `✨ feat(semantics): …`, `♻️ refactor(semantics): …`). Never add Claude attribution of any kind.
- Run before every commit: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`; run `cargo fmt --all` and re-stage when it changes files.
- No em-dashes in any generated content (documentation, comments, messages).

## Scope decisions fixed by this plan

- **Bodies lowered:** `FunctionDeclaration` and `MethodDeclaration` blocks. Closure and arrow-function bodies lower **inline into their enclosing body's arena** (the rust-analyzer model): a closure is an expression, not a separate query key. **Property-hook bodies (PHP 8.4) are deferred**, recorded in the module documentation; they join when the corpus demands them (the measured corpus targets 8.2).
- **`Option` versus `Missing`:** `Option` fields encode *valid absence* (a short ternary's middle, `return;`, `$array[]`); the `Missing` variant encodes *wreckage* (a child error recovery could not produce). One rule, applied everywhere.
- **Faithful representation, deferred judgment:** the IR records what was written (`NamedReference { text }` for `true`, `self`, `PHP_EOL`; `MemberReference::Variable` after `::` as after `->`). Resolution and semantics belong to later plans.
- **Sugar reduced by the lowering table:** `elseif` chains nest as `else { if }`; a single-`Block` branch dissolves into its statements (so `if (c) x;`, `if (c) { x; }`, and `if (c): x; endif;` produce one identical IR); `list(...)` lowers exactly like `[...]`; heredocs lower as interpolated strings; parenthesized expressions are transparent **except** as null-safe chain boundaries (PHP semantics: parentheses stop short-circuiting); `?->` chains get one whole-chain wrapper; `foo(...)` lowers to a callable reference.
- **Declarations inside bodies** (named functions, class-likes, anonymous classes) lower to their `AstId` (the synthetic identity the spec gives them). Consequence, accepted and recorded by the spec: adding or removing a declaration earlier in the file renumbers these ids and re-derives the IR of bodies that reference them.
- **No artifact-cache consultation** in `body_ir`: the typed-artifact classes are plan 9a (same stance as `member_tree`).
- **`u32` arena indices** with a saturating overflow guard: unreachable behind the 4 GiB source cap, handled without panic anyway.

## The canonical data model

This is the complete, final shape (tasks grow it incrementally; this section is the authority for names and types). All types live in `crates/celerrate_semantics/src/body.rs` and derive `Debug, Clone, PartialEq, Eq, Hash` unless stated otherwise.

```rust
pub struct ExpressionId(u32);      // + Copy, PartialOrd, Ord; index() -> u32
pub struct StatementId(u32);       // + Copy, PartialOrd, Ord; index() -> u32

pub enum BodyExpression {
    Missing,
    Literal { text: String },                       // integers, floats, single-quoted strings
    Variable { name: String },                      // `$name`, sigil stripped
    DynamicVariable { target: ExpressionId },       // `$$x`, `${expr}`
    NamedReference { text: String },                // name in expression position; bare `static`
    Unary { operator: SyntaxKind, operand: ExpressionId },
    Postfix { operator: SyntaxKind, operand: ExpressionId },
    Binary { operator: SyntaxKind, lhs: ExpressionId, rhs: ExpressionId },
    Assignment { operator: SyntaxKind, by_reference: bool, target: ExpressionId, value: ExpressionId },
    Cast { operator: SyntaxKind, operand: ExpressionId },
    Ternary { condition: ExpressionId, middle: Option<ExpressionId>, alternative: ExpressionId },
    MemberAccess { receiver: ExpressionId, member: MemberReference, null_safe: bool },
    ScopedAccess { subject: ExpressionId, member: MemberReference },
    NullSafeChain { chain: ExpressionId },          // the whole-chain short-circuit wrapper
    Call { callee: ExpressionId, arguments: Vec<CallArgument> },
    CallableReference { callee: ExpressionId },     // first-class callable `foo(...)`
    New { class: ClassReference, arguments: Vec<CallArgument> },
    Index { subject: ExpressionId, index: Option<ExpressionId> },
    Array { entries: Vec<ArrayEntry> },             // `[...]`, `array(...)`, `list(...)`
    InterpolatedString { parts: Vec<StringPart> },  // `"..."`, heredocs, nowdocs
    ShellExec { parts: Vec<StringPart> },
    Isset { targets: Vec<ExpressionId> },
    Empty { target: ExpressionId },
    Eval { argument: ExpressionId },
    Exit { argument: Option<ExpressionId> },
    Print { operand: ExpressionId },
    Clone { operand: ExpressionId },
    Throw { operand: ExpressionId },
    Yield { key: Option<ExpressionId>, value: Option<ExpressionId>, delegated: bool },
    Include { operator: SyntaxKind, operand: ExpressionId },
    Match { subject: ExpressionId, arms: Vec<MatchCase> },
    Closure { parameters: Vec<ParameterSignature>, uses: Vec<ClosureUse>,
              return_type_text: Option<String>, is_static: bool, by_reference: bool,
              body: Vec<StatementId> },
    ArrowFunction { parameters: Vec<ParameterSignature>, return_type_text: Option<String>,
                    is_static: bool, by_reference: bool, body: ExpressionId },
}

pub enum MemberReference {
    Named { name: String },            // `->name`, `::name` (identifier or keyword)
    Variable { name: String },         // `->$x`, `::$x` (sigil stripped)
    Computed { expression: ExpressionId },  // `->{expr}`
    Missing,
}

pub enum ClassReference {
    Named { name: String },            // `new Foo`, `new self`, `new parent`
    StaticKeyword,                     // `new static`
    Anonymous { declaration: AstId },  // `new class { ... }`
    Dynamic { expression: ExpressionId },
    Missing,
}

pub struct CallArgument { pub label: Option<String>, pub spread: bool, pub value: ExpressionId }

pub enum ArrayEntry {
    Element { key: Option<ExpressionId>, value: ExpressionId, spread: bool, by_reference: bool },
    Hole,                              // `[, $second] = $pair;`
}

pub enum StringPart {
    Fragment { text: String },
    Simple { text: String },           // `$name`, `$name->prop`, `$name[0]` as written
    Interpolation { expression: ExpressionId },  // `{expr}`, `${expr}`
}

pub struct ClosureUse { pub name: String, pub by_reference: bool }

pub struct MatchCase { pub conditions: Vec<ExpressionId>, pub is_default: bool, pub body: ExpressionId }

pub enum BodyStatement {
    Missing,
    Expression { expression: ExpressionId },
    Block { statements: Vec<StatementId> },
    Return { value: Option<ExpressionId> },
    If { condition: ExpressionId, then_branch: Vec<StatementId>, else_branch: Vec<StatementId> },
    While { condition: ExpressionId, body: Vec<StatementId> },
    DoWhile { body: Vec<StatementId>, condition: ExpressionId },
    For { initializers: Vec<ExpressionId>, conditions: Vec<ExpressionId>,
          updates: Vec<ExpressionId>, body: Vec<StatementId> },
    Foreach { subject: ExpressionId, key: Option<ExpressionId>, value: ExpressionId,
              by_reference: bool, body: Vec<StatementId> },
    Switch { subject: ExpressionId, cases: Vec<SwitchArm> },
    Try { body: Vec<StatementId>, catches: Vec<CatchArm>, finally: Option<Vec<StatementId>> },
    Echo { values: Vec<ExpressionId> },
    Unset { targets: Vec<ExpressionId> },
    Global { targets: Vec<ExpressionId> },
    StaticVariables { variables: Vec<StaticVariableDeclaration> },
    Break { level: Option<ExpressionId> },
    Continue { level: Option<ExpressionId> },
    Goto { label: Option<String> },
    Label { name: Option<String> },
    Declare { statements: Vec<StatementId> },
    Declaration { declaration: AstId },   // nested named function, class-like, const, use
}

pub struct SwitchArm { pub condition: Option<ExpressionId>, pub statements: Vec<StatementId> }
pub struct CatchArm { pub types: Vec<String>, pub variable: Option<String>, pub statements: Vec<StatementId> }
pub struct StaticVariableDeclaration { pub name: String, pub initializer: Option<ExpressionId> }

pub struct BodyAnnotation { pub text: String, pub anchor: Option<StatementId> }

pub struct BodyIr {                    // Debug, Clone, PartialEq, Eq, Default (no Hash: Vec-heavy)
    pub expressions: Vec<BodyExpression>,
    pub statements: Vec<BodyStatement>,
    pub root: Vec<StatementId>,
    pub annotations: Vec<BodyAnnotation>,   // added by Task 6
}

pub struct BodySourceMap {             // Debug, Clone, PartialEq, Eq, Default
    pub(crate) expressions: Vec<SyntaxNodePtr>,   // parallel to BodyIr.expressions
    pub(crate) statements: Vec<SyntaxNodePtr>,    // parallel to BodyIr.statements
}

#[salsa::interned(debug)]
pub struct BodyQuery<'db> { pub ast_id: AstId }

#[salsa::tracked(returns(ref))]
pub fn body_ir<'db>(db: &'db dyn salsa::Database, file: SourceFile, body: BodyQuery<'db>) -> Option<BodyIr>;

#[salsa::tracked(returns(ref))]
pub fn body_source_map<'db>(db: &'db dyn salsa::Database, file: SourceFile, body: BodyQuery<'db>) -> Option<BodySourceMap>;

pub fn is_recognized_annotation(kind: SyntaxKind, text: &str) -> bool;   // Task 6
```

`ParameterSignature` is reused from `crate::members` (name, type_text, default_text, by_reference, variadic, is_promoted), keeping closure signatures byte-compatible with method signatures.

## File Structure

- Create: `crates/celerrate_semantics/src/body.rs` — the data model above, the two queries, `BodyQuery`, `is_recognized_annotation`.
- Create: `crates/celerrate_semantics/src/body_lowering.rs` — `pub(crate) fn lower_body(...) -> Option<(BodyIr, BodySourceMap)>` and the `Lowering` walk; most unit tests.
- Modify: `crates/celerrate_semantics/src/members.rs` — extract `pub(crate) fn parameter_signatures(...)` (Task 5, pure refactor plus reuse).
- Modify: `crates/celerrate_semantics/src/lib.rs` — module wiring and exports (grown per task).
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs` — the new edit classes (Task 7).
- Modify: `crates/celerrate_semantics/tests/incremental_consistency.rs` — body IR joins the harness (Task 7).

---

### Task 1: The arena, the per-body queries, and the minimal lowering

The plumbing end to end: dense ids, the two-arena `BodyIr`, the range-carrying `BodySourceMap`, the interned `BodyQuery`, both salsa queries, and a lowering that handles expression statements, returns, literals, variables, and named references (everything else lowers to `Missing` for now). After this task a body edit that only reformats whitespace already produces an identical IR.

**Files:**
- Create: `crates/celerrate_semantics/src/body.rs`
- Create: `crates/celerrate_semantics/src/body_lowering.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `crate::ast_id::{AstId, AstIdMap}`, `crate::queries::ast_id_map`, `celerrate_db::{SourceFile, parse}`, `celerrate_syntax::ast` accessors (`FunctionDeclaration::block`, `MethodDeclaration::block`, `Literal::value_token`, `VariableReference::name_token`, `NameExpression::{name, static_keyword_token}`, `ExpressionStatement::expression`, `ReturnStatement::expression`, `Block::statements`).
- Produces: everything in the canonical data model that Task 1 lists below; later tasks extend the two enums and the `Lowering` walk. `lower_body(declaration: &SyntaxNode) -> Option<(BodyIr, BodySourceMap)>` (signature extended in Task 2). Tests and later plans consume `body_ir(db, file, BodyQuery::new(db, ast_id))`.

- [ ] **Step 1: Write the failing unit tests for the minimal lowering**

Create `crates/celerrate_semantics/src/body_lowering.rs` containing only the test module for now (the module will not compile until Step 3 adds the production items; that is the failing state, driven by `lib.rs` wiring in Step 3):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_syntax::SyntaxKind;

    use crate::body::{BodyExpression, BodyIr, BodySourceMap, BodyStatement};

    /// Lowers the first function or method of `source`.
    fn lowered(source: &str) -> (BodyIr, BodySourceMap) {
        let parse = celerrate_syntax::parse(source);
        let root = parse.tree();
        let declaration = root
            .descendants()
            .find(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::FunctionDeclaration | SyntaxKind::MethodDeclaration,
                )
            })
            .unwrap();
        super::lower_body(&declaration).unwrap()
    }

    fn body(source: &str) -> BodyIr {
        lowered(source).0
    }

    fn root_statement(ir: &BodyIr, position: usize) -> &BodyStatement {
        ir.statement(ir.root[position]).unwrap()
    }

    fn root_expression(ir: &BodyIr, position: usize) -> &BodyExpression {
        let BodyStatement::Expression { expression } = root_statement(ir, position) else {
            panic!("expected an expression statement");
        };
        ir.expression(*expression).unwrap()
    }

    #[test]
    fn a_return_of_a_literal_lowers() {
        let ir = body("<?php function f() { return 1; }");
        assert_eq!(ir.root.len(), 1);
        let BodyStatement::Return { value: Some(value) } = root_statement(&ir, 0) else {
            panic!("expected a return with a value");
        };
        assert_eq!(
            ir.expression(*value),
            Some(&BodyExpression::Literal { text: "1".to_owned() }),
        );
    }

    #[test]
    fn a_bare_return_has_no_value() {
        let ir = body("<?php function f() { return; }");
        assert_eq!(
            root_statement(&ir, 0),
            &BodyStatement::Return { value: None },
        );
    }

    #[test]
    fn variables_lose_their_sigil_and_names_keep_their_spelling() {
        let ir = body("<?php function f() { $count; PHP_EOL; }");
        assert_eq!(
            root_expression(&ir, 0),
            &BodyExpression::Variable { name: "count".to_owned() },
        );
        assert_eq!(
            root_expression(&ir, 1),
            &BodyExpression::NamedReference { text: "PHP_EOL".to_owned() },
        );
    }

    #[test]
    fn a_method_body_lowers_and_a_bodyless_method_answers_none() {
        let ir = body("<?php class A { public function m() { return 1; } }");
        assert_eq!(ir.root.len(), 1);

        let parse = celerrate_syntax::parse("<?php interface I { public function m(); }");
        let method = parse
            .tree()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::MethodDeclaration)
            .unwrap();
        assert!(super::lower_body(&method).is_none());
    }

    #[test]
    fn a_non_body_declaration_answers_none() {
        let parse = celerrate_syntax::parse("<?php class A { public int $x = 0; }");
        let property = parse
            .tree()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PropertyDeclaration)
            .unwrap();
        assert!(super::lower_body(&property).is_none());
    }

    #[test]
    fn formatting_only_edits_produce_an_identical_value() {
        // The load-bearing claim of the whole plan, pinned from the
        // first task: no text offset ever enters the IR.
        let compact = body("<?php function f() { return 1; }");
        let spread_out = body("<?php function f()   {\n\n    return    1;\n}");
        assert_eq!(compact, spread_out);

        let different = body("<?php function f() { return 2; }");
        assert_ne!(compact, different);
    }

    #[test]
    fn identifiers_are_dense_and_reconcile_through_the_source_map() {
        let source = "<?php function f() { $x; return $x; }";
        let (ir, map) = lowered(source);
        assert_eq!(ir.expressions.len(), 2);
        assert_eq!(ir.statements.len(), 2);

        let parse = celerrate_syntax::parse(source);
        for (index, _) in ir.expressions.iter().enumerate() {
            let id = crate::body::ExpressionId::from_index(index).unwrap();
            let pointer = map.expression_pointer(id).unwrap();
            assert!(pointer.try_to_node(&parse.tree()).is_some());
        }
        for (index, _) in ir.statements.iter().enumerate() {
            let id = crate::body::StatementId::from_index(index).unwrap();
            let pointer = map.statement_pointer(id).unwrap();
            assert!(pointer.try_to_node(&parse.tree()).is_some());
        }
    }

    #[test]
    fn truncated_input_lowers_without_failure() {
        // The never-fail contract: error recovery's wreckage lowers
        // (to Missing forms) rather than failing the walk, and the
        // arenas stay parallel to their source map. Task 7's
        // adversarial batch pins arena integrity broadly.
        let (ir, map) = lowered("<?php function f() { $x = ");
        assert_eq!(ir.statements.len(), map.statements.len());
        assert_eq!(ir.expressions.len(), map.expressions.len());
    }
}
```

- [ ] **Step 2: Write the failing query-level tests**

These go at the bottom of `crates/celerrate_semantics/src/body.rs` (created in Step 3 with the production code above them; write the test module first mentally, it drives the shapes):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_source::FileId;
    use celerrate_syntax::SyntaxKind;

    use crate::ast_id::AstId;

    use super::{BodyQuery, body_ir, body_source_map};

    #[test]
    fn the_body_query_lowers_a_function_body() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function f() { return 1; }".to_vec(),
        );
        let body = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 0 });
        let ir = body_ir(&db, file, body).as_ref().unwrap();
        assert_eq!(ir.root.len(), 1);
    }

    #[test]
    fn a_method_is_addressed_by_its_member_index() {
        // Numbering: class = 0, method = 1 (the 1a contract).
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php class A { public function m() { return 1; } }".to_vec(),
        );
        let class = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 0 });
        assert!(body_ir(&db, file, class).is_none());
        let method = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 1 });
        assert!(body_ir(&db, file, method).is_some());
    }

    #[test]
    fn a_mismatched_file_or_unknown_index_answers_none() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function f() { return 1; }".to_vec(),
        );
        let wrong_file = BodyQuery::new(&db, AstId { file: FileId::new(9), index: 0 });
        assert!(body_ir(&db, file, wrong_file).is_none());
        let unknown = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 99 });
        assert!(body_ir(&db, file, unknown).is_none());
    }

    #[test]
    fn the_source_map_query_reconciles_an_expression() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function f() { return 1; }".to_vec(),
        );
        let body = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 0 });
        let ir = body_ir(&db, file, body).as_ref().unwrap();
        let map = body_source_map(&db, file, body).as_ref().unwrap();

        let super::BodyStatement::Return { value: Some(value) } =
            ir.statement(*ir.root.first().unwrap()).unwrap()
        else {
            panic!("expected a return");
        };
        let pointer = map.expression_pointer(*value).unwrap();
        assert_eq!(pointer.kind(), SyntaxKind::Literal);
    }
}
```

- [ ] **Step 3: Write the production code**

Create `crates/celerrate_semantics/src/body.rs`:

```rust
//! The body IR: the range-free, `Eq`-comparable lowering of one
//! function or method body, behind a per-body salsa query. The arenas
//! hold expressions and statements densely numbered; no text offset
//! ever enters the IR, so an edit above a body, and a formatting-only
//! or ignorable-trivia edit inside it, produces an identical value
//! that salsa backdates: body consumers are structurally spared.
//! Spans reconcile late through the sibling source-map query, the
//! `ItemTree`/`AstIdMap` split one level down.
//!
//! Deferred, recorded: property-hook bodies (PHP 8.4) are not lowered
//! yet; they join when the corpus demands them.

use celerrate_db::SourceFile;
use celerrate_syntax::{SyntaxKind, SyntaxNodePtr};

use crate::ast_id::AstId;

/// The dense index of one expression in its body's arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExpressionId(u32);

impl ExpressionId {
    /// A dangling index used when the arena would exceed `u32`:
    /// unreachable behind the 4 GiB source cap, total anyway.
    pub(crate) const OVERFLOW: Self = Self(u32::MAX);

    pub(crate) fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    pub fn index(self) -> u32 {
        self.0
    }
}

/// The dense index of one statement in its body's arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StatementId(u32);

impl StatementId {
    /// See [`ExpressionId::OVERFLOW`].
    pub(crate) const OVERFLOW: Self = Self(u32::MAX);

    pub(crate) fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    pub fn index(self) -> u32 {
        self.0
    }
}

/// One expression, lowered. `Option` fields encode valid absence (a
/// short ternary's middle); [`BodyExpression::Missing`] encodes
/// wreckage (a child error recovery could not produce).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BodyExpression {
    Missing,
    /// An integer, float, or single-quoted string, as written.
    /// (`true`, `null`, `self`, and the magic constants parse as
    /// names and lower to [`BodyExpression::NamedReference`].)
    Literal { text: String },
    /// `$name`, the sigil stripped.
    Variable { name: String },
    /// A name in expression position (a constant fetch, a callee, a
    /// class reference), as written; also the bare `static` keyword.
    NamedReference { text: String },
}

/// One statement, lowered. Same absence rule as the expressions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BodyStatement {
    Missing,
    Expression { expression: ExpressionId },
    Return { value: Option<ExpressionId> },
}

/// The lowered body of one function or method: dense arenas, the
/// top-level statement list, no text offset anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyIr {
    pub expressions: Vec<BodyExpression>,
    pub statements: Vec<BodyStatement>,
    /// The body's top-level statements, in source order.
    pub root: Vec<StatementId>,
}

impl BodyIr {
    pub fn expression(&self, id: ExpressionId) -> Option<&BodyExpression> {
        self.expressions.get(id.0 as usize)
    }

    pub fn statement(&self, id: StatementId) -> Option<&BodyStatement> {
        self.statements.get(id.0 as usize)
    }
}

/// Arena indices back to nodes: the range-carrying sibling of
/// [`BodyIr`], free to change on every edit, consulted only at
/// rendering time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodySourceMap {
    pub(crate) expressions: Vec<SyntaxNodePtr>,
    pub(crate) statements: Vec<SyntaxNodePtr>,
}

impl BodySourceMap {
    /// The pointer of one lowered expression. A `Missing` expression
    /// and a synthetic node (a null-safe chain wrapper) point at the
    /// nearest enclosing written node.
    pub fn expression_pointer(&self, id: ExpressionId) -> Option<SyntaxNodePtr> {
        self.expressions.get(id.0 as usize).copied()
    }

    pub fn statement_pointer(&self, id: StatementId) -> Option<SyntaxNodePtr> {
        self.statements.get(id.0 as usize).copied()
    }
}

/// One body to lower: the declaration `AstId` of a function or method.
#[salsa::interned(debug)]
pub struct BodyQuery<'db> {
    pub ast_id: AstId,
}

/// The body IR of one declaration: `None` when the identity does not
/// name a function or method carrying a body in `file` (an abstract or
/// interface method, a property, a mismatched file). Range-free, so an
/// ignorable edit backdates and body consumers are spared. No
/// artifact-cache consultation yet: the typed-artifact classes are
/// plan 9a.
#[salsa::tracked(returns(ref))]
pub fn body_ir<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<BodyIr> {
    lowered_body(db, file, body).map(|(ir, _)| ir)
}

/// The source map of one body: the range-carrying sibling of
/// [`body_ir`], re-running the same walk. The duplicate walk is the
/// price of the split; the cutoff it buys is the point.
#[salsa::tracked(returns(ref))]
pub fn body_source_map<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<BodySourceMap> {
    lowered_body(db, file, body).map(|(_, map)| map)
}

fn lowered_body<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<(BodyIr, BodySourceMap)> {
    let ast_id = body.ast_id(db);
    if ast_id.file != file.file_id(db) {
        return None;
    }
    let map = crate::queries::ast_id_map(db, file);
    let pointer = map.pointer(ast_id.index)?;
    let root = celerrate_db::parse(db, file).tree();
    let node = pointer.try_to_node(&root)?;
    crate::body_lowering::lower_body(&node)
}
```

(The `SyntaxKind` import feeds the enums from Task 3 on; if the compiler flags it as unused in this task, keep the import out until Task 3 rather than allowing it.)

Add the production half of `crates/celerrate_semantics/src/body_lowering.rs` above its test module:

```rust
//! The lowering walk: one pass over a body's syntax turning statements
//! and expressions into the dense, range-free arenas of
//! [`crate::body::BodyIr`] and its range-carrying source-map sibling.
//! Every accessor miss lowers to `Missing`; error-recovery wreckage is
//! tolerated, never a failure.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};

use crate::body::{
    BodyExpression, BodyIr, BodySourceMap, BodyStatement, ExpressionId, StatementId,
};

/// Lowers one function or method declaration's body. `None` when the
/// node is not a function or method, or carries no body.
pub(crate) fn lower_body(declaration: &SyntaxNode) -> Option<(BodyIr, BodySourceMap)> {
    let block = match declaration.kind() {
        SyntaxKind::FunctionDeclaration => {
            ast::FunctionDeclaration::cast(declaration.clone())?.block()?
        }
        SyntaxKind::MethodDeclaration => {
            ast::MethodDeclaration::cast(declaration.clone())?.block()?
        }
        _ => return None,
    };
    let mut lowering = Lowering {
        ir: BodyIr::default(),
        source_map: BodySourceMap::default(),
    };
    let root = lowering.lower_statements(block.statements());
    lowering.ir.root = root;
    Some((lowering.ir, lowering.source_map))
}

struct Lowering {
    ir: BodyIr,
    source_map: BodySourceMap,
}

impl Lowering {
    fn allocate_expression(
        &mut self,
        expression: BodyExpression,
        node: &SyntaxNode,
    ) -> ExpressionId {
        let Some(id) = ExpressionId::from_index(self.ir.expressions.len()) else {
            return ExpressionId::OVERFLOW;
        };
        self.ir.expressions.push(expression);
        self.source_map.expressions.push(SyntaxNodePtr::new(node));
        id
    }

    fn allocate_statement(&mut self, statement: BodyStatement, node: &SyntaxNode) -> StatementId {
        let Some(id) = StatementId::from_index(self.ir.statements.len()) else {
            return StatementId::OVERFLOW;
        };
        self.ir.statements.push(statement);
        self.source_map.statements.push(SyntaxNodePtr::new(node));
        id
    }

    fn lower_statements(
        &mut self,
        statements: ast::AstChildren<ast::Statement>,
    ) -> Vec<StatementId> {
        statements
            .filter_map(|statement| self.lower_statement(&statement))
            .collect()
    }

    fn lower_statement(&mut self, statement: &ast::Statement) -> Option<StatementId> {
        let lowered = match statement {
            ast::Statement::ExpressionStatement(expression_statement) => {
                BodyStatement::Expression {
                    expression: self.lower_expression_or_missing(
                        expression_statement.expression(),
                        expression_statement.syntax(),
                    ),
                }
            }
            ast::Statement::ReturnStatement(return_statement) => BodyStatement::Return {
                value: return_statement
                    .expression()
                    .map(|expression| self.lower_expression(&expression)),
            },
            _ => BodyStatement::Missing,
        };
        Some(self.allocate_statement(lowered, statement.syntax()))
    }

    fn lower_expression(&mut self, expression: &ast::Expression) -> ExpressionId {
        self.lower_expression_kind(expression)
    }

    fn lower_expression_or_missing(
        &mut self,
        expression: Option<ast::Expression>,
        parent: &SyntaxNode,
    ) -> ExpressionId {
        match expression {
            Some(expression) => self.lower_expression(&expression),
            None => self.allocate_expression(BodyExpression::Missing, parent),
        }
    }

    fn lower_expression_kind(&mut self, expression: &ast::Expression) -> ExpressionId {
        let node = expression.syntax();
        let lowered = match expression {
            ast::Expression::Literal(literal) => match literal.value_token() {
                Some(token) => BodyExpression::Literal {
                    text: token.text().to_owned(),
                },
                None => BodyExpression::Missing,
            },
            ast::Expression::VariableReference(variable) => match variable.name_token() {
                Some(token) => BodyExpression::Variable {
                    name: token.text().trim_start_matches('$').to_owned(),
                },
                None => BodyExpression::Missing,
            },
            ast::Expression::NameExpression(name) => {
                let text = name.name().map(|name| name.text()).or_else(|| {
                    name.static_keyword_token()
                        .map(|token| token.text().to_owned())
                });
                match text {
                    Some(text) => BodyExpression::NamedReference { text },
                    None => BodyExpression::Missing,
                }
            }
            _ => BodyExpression::Missing,
        };
        self.allocate_expression(lowered, node)
    }
}
```

Wire both modules in `crates/celerrate_semantics/src/lib.rs`: add `mod body;` and `mod body_lowering;` to the module list (alphabetical, after `mod ast_id;`), and add the export:

```rust
pub use body::{
    BodyExpression, BodyIr, BodyQuery, BodySourceMap, BodyStatement, ExpressionId, StatementId,
    body_ir, body_source_map,
};
```

- [ ] **Step 4: Run the tests and verify they now pass**

Run: `cargo test --package celerrate_semantics body -- --nocapture`
Expected: all Task 1 tests PASS.

- [ ] **Step 5: Lint, format, full suite, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
git add crates/celerrate_semantics/src/body.rs crates/celerrate_semantics/src/body_lowering.rs crates/celerrate_semantics/src/lib.rs
git commit -m "✨ feat(semantics): lower bodies into a range-free arena behind per-body queries"
```

---

### Task 2: Every statement form lowers

All statement kinds gain real lowerings; `Missing` remains only for wreckage. The task also fixes the two statement-level sugar rows of the lowering table: `elseif` nests as `else { if }`, and a single-`Block` branch dissolves so the three `if` syntaxes produce one identical IR. Nested declarations lower to their `AstId`, which extends `lower_body`'s signature.

**Files:**
- Modify: `crates/celerrate_semantics/src/body.rs` (new `BodyStatement` variants and supporting structs)
- Modify: `crates/celerrate_semantics/src/body_lowering.rs`

**Interfaces:**
- Consumes: Task 1's `Lowering` walk; `crate::ast_id::{AstId, AstIdMap}` (`AstIdMap::index_of`); the statement accessors listed in the canonical model's sources (`IfStatement::{condition, statements, else_if_clauses, else_clause}`, `ForeachStatement::{subject, key, value, statements}`, `TryStatement::{block, catch_clauses, finally_clause}`, `SwitchStatement::{condition, switch_cases}`, `StaticStatement::static_variables`, `UnsetStatement::argument_list`, and so on).
- Produces: the complete `BodyStatement` enum plus `SwitchArm`, `CatchArm`, `StaticVariableDeclaration` exactly as in the canonical data model; `lower_body(file: FileId, map: &AstIdMap, declaration: &SyntaxNode) -> Option<(BodyIr, BodySourceMap)>` (the signature every later task and the queries use); `Lowering::lower_branch(statements: Vec<ast::Statement>) -> Vec<StatementId>`.

- [ ] **Step 1: Write the failing tests**

Add to the test module of `body_lowering.rs`:

```rust
    #[test]
    fn the_three_if_syntaxes_produce_one_identical_body() {
        // The dissolution rule: a single-Block branch dissolves into
        // its statements, so brace style is formatting, not code.
        let plain = body("<?php function f() { if ($c) return 1; }");
        let braced = body("<?php function f() { if ($c) { return 1; } }");
        let alternative = body("<?php function f() { if ($c): return 1; endif; }");
        assert_eq!(plain, braced);
        assert_eq!(braced, alternative);
    }

    #[test]
    fn elseif_chains_nest_as_else_if() {
        let ir = body(
            "<?php function f() { if ($a) { return 1; } elseif ($b) { return 2; } else { return 3; } }",
        );
        let BodyStatement::If { else_branch, .. } = root_statement(&ir, 0) else {
            panic!("expected an if");
        };
        assert_eq!(else_branch.len(), 1);
        let BodyStatement::If { then_branch, else_branch: innermost, .. } =
            ir.statement(else_branch[0]).unwrap()
        else {
            panic!("expected the nested elseif as an if");
        };
        assert_eq!(then_branch.len(), 1);
        assert_eq!(innermost.len(), 1);
        assert!(matches!(
            ir.statement(innermost[0]).unwrap(),
            BodyStatement::Return { value: Some(_) },
        ));
    }

    #[test]
    fn loops_lower_with_their_shapes() {
        let ir = body(
            "<?php function f() { while ($a) { $x; } do { $y; } while ($b); for ($i = 0; $i < 3; $i++) { $z; } }",
        );
        assert!(matches!(root_statement(&ir, 0), BodyStatement::While { body, .. } if body.len() == 1));
        assert!(matches!(root_statement(&ir, 1), BodyStatement::DoWhile { body, .. } if body.len() == 1));
        let BodyStatement::For { initializers, conditions, updates, body: for_body } =
            root_statement(&ir, 2)
        else {
            panic!("expected a for");
        };
        assert_eq!(
            (initializers.len(), conditions.len(), updates.len(), for_body.len()),
            (1, 1, 1, 1),
        );
    }

    #[test]
    fn foreach_lowers_key_value_and_by_reference() {
        let ir = body("<?php function f() { foreach ($items as $key => &$value) { $value; } }");
        let BodyStatement::Foreach { key, by_reference, body: foreach_body, .. } =
            root_statement(&ir, 0)
        else {
            panic!("expected a foreach");
        };
        assert!(key.is_some());
        assert!(*by_reference);
        assert_eq!(foreach_body.len(), 1);

        let ir = body("<?php function f() { foreach ($items as $value) {} }");
        let BodyStatement::Foreach { key, by_reference, .. } = root_statement(&ir, 0) else {
            panic!("expected a foreach");
        };
        assert!(key.is_none());
        assert!(!*by_reference);
    }

    #[test]
    fn switch_lowers_cases_and_default() {
        let ir = body(
            "<?php function f() { switch ($x) { case 1: return 1; default: return 2; } }",
        );
        let BodyStatement::Switch { cases, .. } = root_statement(&ir, 0) else {
            panic!("expected a switch");
        };
        assert_eq!(cases.len(), 2);
        assert!(cases[0].condition.is_some());
        assert!(cases[1].condition.is_none());
        assert_eq!(cases[1].statements.len(), 1);
    }

    #[test]
    fn try_catch_finally_lowers_types_and_variables() {
        let ir = body(
            "<?php function f() { try { $a; } catch (FooError | BarError $e) { $b; } catch (BazError) { $c; } finally { $d; } }",
        );
        let BodyStatement::Try { body: try_body, catches, finally } = root_statement(&ir, 0)
        else {
            panic!("expected a try");
        };
        assert_eq!(try_body.len(), 1);
        assert_eq!(catches.len(), 2);
        assert_eq!(catches[0].types, vec!["FooError".to_owned(), "BarError".to_owned()]);
        assert_eq!(catches[0].variable.as_deref(), Some("e"));
        assert_eq!(catches[1].variable, None);
        assert_eq!(finally.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn the_simple_statement_forms_lower() {
        let ir = body(
            "<?php function f() { echo 1, 2; unset($a, $b); global $g; static $s = 1; break 2; continue; goto end; end: ; declare(ticks=1) { $t; } }",
        );
        assert!(matches!(root_statement(&ir, 0), BodyStatement::Echo { values } if values.len() == 2));
        assert!(matches!(root_statement(&ir, 1), BodyStatement::Unset { targets } if targets.len() == 2));
        assert!(matches!(root_statement(&ir, 2), BodyStatement::Global { targets } if targets.len() == 1));
        let BodyStatement::StaticVariables { variables } = root_statement(&ir, 3) else {
            panic!("expected static variables");
        };
        assert_eq!(variables.len(), 1);
        assert_eq!(variables[0].name, "s");
        assert!(variables[0].initializer.is_some());
        assert!(matches!(root_statement(&ir, 4), BodyStatement::Break { level: Some(_) }));
        assert!(matches!(root_statement(&ir, 5), BodyStatement::Continue { level: None }));
        assert!(matches!(root_statement(&ir, 6), BodyStatement::Goto { label: Some(label) } if label == "end"));
        assert!(matches!(root_statement(&ir, 7), BodyStatement::Label { name: Some(name) } if name == "end"));
        assert!(matches!(root_statement(&ir, 8), BodyStatement::Declare { statements } if statements.len() == 1));
    }

    #[test]
    fn empty_statements_vanish_and_a_standalone_block_stays() {
        let ir = body("<?php function f() { ; $x; ; }");
        assert_eq!(ir.root.len(), 1);

        let ir = body("<?php function f() { { $x; } }");
        assert!(matches!(root_statement(&ir, 0), BodyStatement::Block { statements } if statements.len() == 1));
    }

    #[test]
    fn a_nested_declaration_lowers_to_its_identity() {
        // Numbering: outer function = 0, nested function = 1 (the
        // traversal descends into bodies; the 1a contract).
        let ir = body("<?php function f() { function nested() {} $x; }");
        let BodyStatement::Declaration { declaration } = root_statement(&ir, 0) else {
            panic!("expected a declaration statement");
        };
        assert_eq!(declaration.index, 1);
        assert_eq!(ir.root.len(), 2);
    }
```

The `lowered` helper changes with the new signature; replace its last line:

```rust
        let map = crate::ast_id::AstIdMap::from_root(&root);
        super::lower_body(celerrate_source::FileId::new(0), &map, &declaration).unwrap()
```

(and adjust the two `lower_body(&method)` / `lower_body(&property)` call sites in Task 1's tests the same way).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics body_lowering`
Expected: FAIL (missing variants, wrong `lower_body` arity).

- [ ] **Step 3: Implement**

In `body.rs`, replace the `BodyStatement` enum with the complete one from the canonical data model and add `SwitchArm`, `CatchArm`, `StaticVariableDeclaration` (all `#[derive(Debug, Clone, PartialEq, Eq, Hash)]`, fields exactly as specified). Extend the `lib.rs` export with `CatchArm, StaticVariableDeclaration, SwitchArm`.

In `body_lowering.rs`:

1. Extend the signature and the struct:

```rust
use celerrate_source::FileId;

use crate::ast_id::{AstId, AstIdMap};

pub(crate) fn lower_body(
    file: FileId,
    map: &AstIdMap,
    declaration: &SyntaxNode,
) -> Option<(BodyIr, BodySourceMap)> {
    // ... block extraction unchanged ...
    let mut lowering = Lowering {
        file,
        map,
        ir: BodyIr::default(),
        source_map: BodySourceMap::default(),
    };
    // ... rest unchanged ...
}

struct Lowering<'a> {
    file: FileId,
    map: &'a AstIdMap,
    ir: BodyIr,
    source_map: BodySourceMap,
}
```

Update the call in `body.rs::lowered_body` to `crate::body_lowering::lower_body(ast_id.file, map, &node)`.

2. Add the branch helper and the list helpers to `impl Lowering<'_>`:

```rust
    /// A branch position (an `if` arm, a loop body): one statement in
    /// the classic syntax, a list in the alternative syntax. A single
    /// `Block` dissolves into its statements, so brace style is
    /// formatting, not code.
    fn lower_branch(&mut self, statements: Vec<ast::Statement>) -> Vec<StatementId> {
        if statements.len() == 1
            && let Some(ast::Statement::Block(block)) = statements.first()
        {
            return self.lower_statements(block.statements());
        }
        statements
            .iter()
            .filter_map(|statement| self.lower_statement(statement))
            .collect()
    }

    /// The bare expressions of an argument list where the list is
    /// grouping syntax rather than a call (`unset`, `isset`, ...).
    fn lower_argument_expressions(&mut self, list: Option<ast::ArgumentList>) -> Vec<ExpressionId> {
        let arguments: Vec<ast::Argument> = list
            .into_iter()
            .flat_map(|list| list.arguments().collect::<Vec<_>>())
            .collect();
        arguments
            .iter()
            .map(|argument| self.lower_expression_or_missing(argument.expression(), argument.syntax()))
            .collect()
    }

    fn lower_for_list(&mut self, list: Option<ast::ForExpressionList>) -> Vec<ExpressionId> {
        let expressions: Vec<ast::Expression> = list
            .into_iter()
            .flat_map(|list| list.expressions().collect::<Vec<_>>())
            .collect();
        expressions
            .iter()
            .map(|expression| self.lower_expression(expression))
            .collect()
    }

    fn lower_block_statements(&mut self, block: Option<ast::Block>) -> Vec<StatementId> {
        match block {
            Some(block) => self.lower_statements(block.statements()),
            None => Vec::new(),
        }
    }
```

3. Replace the `_ => BodyStatement::Missing` fallback in `lower_statement` with the full arm set (the match becomes exhaustive over `ast::Statement`; no fallback remains):

```rust
            ast::Statement::Block(block) => BodyStatement::Block {
                statements: self.lower_statements(block.statements()),
            },
            ast::Statement::EmptyStatement(_) => return None,
            ast::Statement::EchoStatement(echo) => {
                let expressions: Vec<ast::Expression> = echo.expressions().collect();
                BodyStatement::Echo {
                    values: expressions
                        .iter()
                        .map(|expression| self.lower_expression(expression))
                        .collect(),
                }
            }
            ast::Statement::BreakStatement(break_statement) => BodyStatement::Break {
                level: break_statement
                    .expression()
                    .map(|expression| self.lower_expression(&expression)),
            },
            ast::Statement::ContinueStatement(continue_statement) => BodyStatement::Continue {
                level: continue_statement
                    .expression()
                    .map(|expression| self.lower_expression(&expression)),
            },
            ast::Statement::GlobalStatement(global) => {
                let expressions: Vec<ast::Expression> = global.expressions().collect();
                BodyStatement::Global {
                    targets: expressions
                        .iter()
                        .map(|expression| self.lower_expression(expression))
                        .collect(),
                }
            }
            ast::Statement::StaticStatement(static_statement) => {
                let declarations: Vec<ast::StaticVariable> =
                    static_statement.static_variables().collect();
                let mut variables = Vec::new();
                for declaration in &declarations {
                    let Some(name) = declaration.name_token() else {
                        continue;
                    };
                    variables.push(StaticVariableDeclaration {
                        name: name.text().trim_start_matches('$').to_owned(),
                        initializer: declaration
                            .expression()
                            .map(|expression| self.lower_expression(&expression)),
                    });
                }
                BodyStatement::StaticVariables { variables }
            }
            ast::Statement::UnsetStatement(unset) => BodyStatement::Unset {
                targets: self.lower_argument_expressions(unset.argument_list()),
            },
            ast::Statement::GotoStatement(goto) => BodyStatement::Goto {
                label: goto.label_token().map(|token| token.text().to_owned()),
            },
            ast::Statement::LabelStatement(label) => BodyStatement::Label {
                name: label.name_token().map(|token| token.text().to_owned()),
            },
            ast::Statement::IfStatement(if_statement) => self.lower_if(if_statement),
            ast::Statement::WhileStatement(while_statement) => BodyStatement::While {
                condition: self.lower_expression_or_missing(
                    while_statement.condition(),
                    while_statement.syntax(),
                ),
                body: self.lower_branch(while_statement.statements().collect()),
            },
            ast::Statement::DoWhileStatement(do_while) => BodyStatement::DoWhile {
                body: self.lower_branch(do_while.body().into_iter().collect()),
                condition: self
                    .lower_expression_or_missing(do_while.condition(), do_while.syntax()),
            },
            ast::Statement::ForStatement(for_statement) => BodyStatement::For {
                initializers: self.lower_for_list(for_statement.initializers()),
                conditions: self.lower_for_list(for_statement.condition()),
                updates: self.lower_for_list(for_statement.updates()),
                body: self.lower_branch(for_statement.statements().collect()),
            },
            ast::Statement::ForeachStatement(foreach) => {
                let by_reference = foreach
                    .syntax()
                    .children_with_tokens()
                    .filter_map(|element| element.into_token())
                    .any(|token| token.kind() == SyntaxKind::Ampersand);
                BodyStatement::Foreach {
                    subject: self
                        .lower_expression_or_missing(foreach.subject(), foreach.syntax()),
                    key: foreach.key().map(|key| self.lower_expression(&key)),
                    value: self.lower_expression_or_missing(foreach.value(), foreach.syntax()),
                    by_reference,
                    body: self.lower_branch(foreach.statements().collect()),
                }
            }
            ast::Statement::SwitchStatement(switch) => {
                let switch_cases: Vec<ast::SwitchCase> = switch.switch_cases().collect();
                let mut cases = Vec::new();
                for case in &switch_cases {
                    cases.push(SwitchArm {
                        condition: case
                            .condition()
                            .map(|condition| self.lower_expression(&condition)),
                        statements: self.lower_statements(case.statements()),
                    });
                }
                BodyStatement::Switch {
                    subject: self.lower_expression_or_missing(switch.condition(), switch.syntax()),
                    cases,
                }
            }
            ast::Statement::TryStatement(try_statement) => {
                let clauses: Vec<ast::CatchClause> = try_statement.catch_clauses().collect();
                let mut catches = Vec::new();
                for clause in &clauses {
                    catches.push(CatchArm {
                        types: clause.names().map(|name| name.text()).collect(),
                        variable: clause
                            .variable_reference()
                            .and_then(|variable| variable.name_token())
                            .map(|token| token.text().trim_start_matches('$').to_owned()),
                        statements: self.lower_block_statements(clause.block()),
                    });
                }
                BodyStatement::Try {
                    body: self.lower_block_statements(try_statement.block()),
                    catches,
                    finally: try_statement
                        .finally_clause()
                        .map(|clause| self.lower_block_statements(clause.block())),
                }
            }
            ast::Statement::DeclareStatement(declare) => BodyStatement::Declare {
                statements: self.lower_statements(declare.statements()),
            },
            ast::Statement::FunctionDeclaration(_)
            | ast::Statement::ConstantDeclaration(_)
            | ast::Statement::NamespaceDeclaration(_)
            | ast::Statement::UseDeclaration(_)
            | ast::Statement::ClassDeclaration(_)
            | ast::Statement::InterfaceDeclaration(_)
            | ast::Statement::TraitDeclaration(_)
            | ast::Statement::EnumDeclaration(_) => match self.map.index_of(statement.syntax()) {
                Some(index) => BodyStatement::Declaration {
                    declaration: AstId { file: self.file, index },
                },
                None => BodyStatement::Missing,
            },
```

4. Add the `if` lowering:

```rust
    fn lower_if(&mut self, if_statement: &ast::IfStatement) -> BodyStatement {
        let condition =
            self.lower_expression_or_missing(if_statement.condition(), if_statement.syntax());
        let then_branch = self.lower_branch(if_statement.statements().collect());
        let else_ifs: Vec<ast::ElseIfClause> = if_statement.else_if_clauses().collect();
        let else_branch = self.lower_else(&else_ifs, if_statement.else_clause().as_ref());
        BodyStatement::If { condition, then_branch, else_branch }
    }

    /// `elseif` is sugar for `else { if ... }`: each clause nests one
    /// synthetic `If` (anchored on the clause node), the `else` clause
    /// innermost.
    fn lower_else(
        &mut self,
        else_ifs: &[ast::ElseIfClause],
        else_clause: Option<&ast::ElseClause>,
    ) -> Vec<StatementId> {
        let Some((first, rest)) = else_ifs.split_first() else {
            return match else_clause {
                Some(clause) => self.lower_branch(clause.statements().collect()),
                None => Vec::new(),
            };
        };
        let condition = self.lower_expression_or_missing(first.condition(), first.syntax());
        let then_branch = self.lower_branch(first.statements().collect());
        let else_branch = self.lower_else(rest, else_clause);
        let nested = self.allocate_statement(
            BodyStatement::If { condition, then_branch, else_branch },
            first.syntax(),
        );
        vec![nested]
    }
```

Import `CatchArm, StaticVariableDeclaration, SwitchArm` from `crate::body` in `body_lowering.rs`.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test --package celerrate_semantics body`
Expected: PASS.

- [ ] **Step 5: Lint, format, full suite, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
git add -A crates/celerrate_semantics
git commit -m "✨ feat(semantics): lower every statement form into the body arena"
```

---

### Task 3: The operator and value expression families lower

Every expression kind except receivers, calls, `new`, indexes, and closures gains its real lowering: operators, casts, ternaries, arrays with destructuring holes, interpolated strings (heredocs included), and the grouping forms (`isset`, `match`, `yield`, ...). Parenthesized expressions become transparent.

**Files:**
- Modify: `crates/celerrate_semantics/src/body.rs` (new `BodyExpression` variants, `ArrayEntry`, `StringPart`, `MatchCase`)
- Modify: `crates/celerrate_semantics/src/body_lowering.rs`

**Interfaces:**
- Consumes: Task 2's walk; the expression accessors (`BinaryExpression::{lhs, operator_token, rhs}`, `AssignmentExpression::{target, operator_token, by_reference_token, value}`, `TernaryExpression::{condition, middle, third}`, `ArrayElement::{key, value, spread_token}`, `MatchArm::{conditions, is_default, body}`, `YieldExpression::{yield_from_token, key, value}`, `IncludeExpression::{operator_token, operand}`, the `StringInterpolation` enum, and so on).
- Produces: the `BodyExpression` variants `DynamicVariable`, `Unary`, `Postfix`, `Binary`, `Assignment`, `Cast`, `Ternary`, `Array`, `InterpolatedString`, `ShellExec`, `Isset`, `Empty`, `Eval`, `Exit`, `Print`, `Clone`, `Throw`, `Yield`, `Include`, `Match` plus `ArrayEntry`, `StringPart`, `MatchCase` exactly as in the canonical data model; the helpers `Lowering::{lower_array_entries, lower_string_parts, lower_first_argument_or_missing}` and the free function `token_kind_or_error`.

- [ ] **Step 1: Write the failing tests**

Add to the test module of `body_lowering.rs`:

```rust
    #[test]
    fn operators_are_distinguished_by_their_token_kind() {
        let ir = body("<?php function f() { $a + $b; $a . $b; $a instanceof Foo; $a ?? $b; }");
        assert!(matches!(root_expression(&ir, 0), BodyExpression::Binary { operator: SyntaxKind::Plus, .. }));
        assert!(matches!(root_expression(&ir, 1), BodyExpression::Binary { operator: SyntaxKind::Dot, .. }));
        assert!(matches!(root_expression(&ir, 2), BodyExpression::Binary { operator: SyntaxKind::InstanceOf, .. }));
        assert!(matches!(root_expression(&ir, 3), BodyExpression::Binary { operator: SyntaxKind::QuestionQuestion, .. }));
    }

    #[test]
    fn prefix_postfix_cast_and_assignment_lower() {
        let ir = body("<?php function f() { !$a; $a++; (int) $a; $a ??= $b; $a = &$b; }");
        assert!(matches!(root_expression(&ir, 0), BodyExpression::Unary { operator: SyntaxKind::Bang, .. }));
        assert!(matches!(root_expression(&ir, 1), BodyExpression::Postfix { operator: SyntaxKind::PlusPlus, .. }));
        assert!(matches!(root_expression(&ir, 2), BodyExpression::Cast { operator: SyntaxKind::IntCast, .. }));
        assert!(matches!(
            root_expression(&ir, 3),
            BodyExpression::Assignment { operator: SyntaxKind::QuestionQuestionEquals, by_reference: false, .. },
        ));
        assert!(matches!(
            root_expression(&ir, 4),
            BodyExpression::Assignment { operator: SyntaxKind::Equals, by_reference: true, .. },
        ));
    }

    #[test]
    fn the_short_ternary_has_no_middle() {
        let ir = body("<?php function f() { $a ? $b : $c; $a ?: $c; }");
        assert!(matches!(root_expression(&ir, 0), BodyExpression::Ternary { middle: Some(_), .. }));
        assert!(matches!(root_expression(&ir, 1), BodyExpression::Ternary { middle: None, .. }));
    }

    #[test]
    fn parentheses_are_transparent() {
        assert_eq!(
            body("<?php function f() { ($x); }"),
            body("<?php function f() { $x; }"),
        );
    }

    #[test]
    fn arrays_lower_keys_spreads_and_destructuring_holes() {
        let ir = body("<?php function f() { [1 => $a, ...$rest, &$b]; }");
        let BodyExpression::Array { entries } = root_expression(&ir, 0) else {
            panic!("expected an array");
        };
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], ArrayEntry::Element { key: Some(_), spread: false, .. }));
        assert!(matches!(&entries[1], ArrayEntry::Element { spread: true, .. }));
        assert!(matches!(&entries[2], ArrayEntry::Element { by_reference: true, .. }));

        let ir = body("<?php function f() { [, $second] = $pair; }");
        let BodyExpression::Assignment { target, .. } = root_expression(&ir, 0) else {
            panic!("expected an assignment");
        };
        let BodyExpression::Array { entries } = ir.expression(*target).unwrap() else {
            panic!("expected an array target");
        };
        assert!(matches!(&entries[0], ArrayEntry::Hole));
        assert!(matches!(&entries[1], ArrayEntry::Element { .. }));

        // A trailing comma is not a hole.
        let ir = body("<?php function f() { [$a, $b,]; }");
        let BodyExpression::Array { entries } = root_expression(&ir, 0) else {
            panic!("expected an array");
        };
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn list_destructuring_lowers_exactly_like_the_bracket_form() {
        assert_eq!(
            body("<?php function f() { list($a, $b) = $pair; }"),
            body("<?php function f() { [$a, $b] = $pair; }"),
        );
    }

    #[test]
    fn interpolated_strings_lower_fragments_and_interpolations() {
        let ir = body("<?php function f() { \"a {$x} b\"; }");
        let BodyExpression::InterpolatedString { parts } = root_expression(&ir, 0) else {
            panic!("expected an interpolated string");
        };
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], StringPart::Fragment { text } if text == "a "));
        assert!(matches!(&parts[1], StringPart::Interpolation { .. }));
        assert!(matches!(&parts[2], StringPart::Fragment { text } if text == " b"));
    }

    #[test]
    fn simple_interpolations_carry_their_written_text() {
        // The simple syntax reaches one property or index deep; the IR
        // records it as written and defers the semantics.
        let ir = body("<?php function f() { \"$user->name\"; }");
        let BodyExpression::InterpolatedString { parts } = root_expression(&ir, 0) else {
            panic!("expected an interpolated string");
        };
        assert!(matches!(&parts[0], StringPart::Simple { text } if text == "$user->name"));
    }

    #[test]
    fn heredocs_lower_as_interpolated_strings_with_the_label_erased() {
        let first = body("<?php function f() { <<<EOT\nhello $x\nEOT; }");
        let second = body("<?php function f() { <<<OTHER\nhello $x\nOTHER; }");
        // The label is formatting: two heredocs differing only in it
        // produce one identical IR.
        assert_eq!(first, second);
        let BodyExpression::InterpolatedString { .. } = root_expression(&first, 0) else {
            panic!("expected an interpolated string");
        };
    }

    #[test]
    fn the_grouping_forms_lower() {
        let ir = body(
            "<?php function f() { isset($a, $b); empty($c); print $d; clone $e; throw $f; require 'x.php'; `ls $dir`; }",
        );
        assert!(matches!(root_expression(&ir, 0), BodyExpression::Isset { targets } if targets.len() == 2));
        assert!(matches!(root_expression(&ir, 1), BodyExpression::Empty { .. }));
        assert!(matches!(root_expression(&ir, 2), BodyExpression::Print { .. }));
        assert!(matches!(root_expression(&ir, 3), BodyExpression::Clone { .. }));
        assert!(matches!(root_expression(&ir, 4), BodyExpression::Throw { .. }));
        assert!(matches!(root_expression(&ir, 5), BodyExpression::Include { operator: SyntaxKind::Require, .. }));
        assert!(matches!(root_expression(&ir, 6), BodyExpression::ShellExec { .. }));
    }

    #[test]
    fn exit_and_yield_encode_valid_absence() {
        let ir = body("<?php function f() { exit; exit(1); yield; yield $v; yield $k => $v; yield from $g; }");
        assert!(matches!(root_expression(&ir, 0), BodyExpression::Exit { argument: None }));
        assert!(matches!(root_expression(&ir, 1), BodyExpression::Exit { argument: Some(_) }));
        assert!(matches!(root_expression(&ir, 2), BodyExpression::Yield { key: None, value: None, delegated: false }));
        assert!(matches!(root_expression(&ir, 3), BodyExpression::Yield { key: None, value: Some(_), delegated: false }));
        assert!(matches!(root_expression(&ir, 4), BodyExpression::Yield { key: Some(_), value: Some(_), delegated: false }));
        assert!(matches!(root_expression(&ir, 5), BodyExpression::Yield { delegated: true, .. }));
    }

    #[test]
    fn match_lowers_arms_and_default() {
        let ir = body(
            "<?php function f() { match ($x) { 1, 2 => 'low', default => 'other' }; }",
        );
        let BodyExpression::Match { arms, .. } = root_expression(&ir, 0) else {
            panic!("expected a match");
        };
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].conditions.len(), 2);
        assert!(!arms[0].is_default);
        assert!(arms[1].is_default);
        assert!(arms[1].conditions.is_empty());
    }

    #[test]
    fn dynamic_variables_lower_their_target() {
        let ir = body("<?php function f() { $$name; }");
        let BodyExpression::DynamicVariable { target } = root_expression(&ir, 0) else {
            panic!("expected a dynamic variable");
        };
        assert!(matches!(ir.expression(*target).unwrap(), BodyExpression::Variable { .. }));
    }
```

Extend the test module's imports with `ArrayEntry`, `StringPart` from `crate::body`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics body_lowering`
Expected: FAIL (missing variants).

- [ ] **Step 3: Implement**

In `body.rs`, add the Task 3 variants to `BodyExpression` and the `ArrayEntry`, `StringPart`, `MatchCase` types exactly as in the canonical data model (all deriving `Debug, Clone, PartialEq, Eq, Hash`). `SyntaxKind` now enters the model (`use celerrate_syntax::{SyntaxKind, SyntaxNodePtr};`). Extend the `lib.rs` export with `ArrayEntry, MatchCase, StringPart`.

In `body_lowering.rs`, add the free helper and the arms. The helper:

```rust
/// The kind of a wreckage-tolerant operator token: `Error` when error
/// recovery produced none, keeping the lowering total and deterministic.
fn token_kind_or_error(token: Option<celerrate_syntax::SyntaxToken>) -> SyntaxKind {
    token.map_or(SyntaxKind::Error, |token| token.kind())
}
```

The new arms of `lower_expression_kind` (replacing that part of the fallback; the `_ =>` fallback stays until Task 5 closes the enum):

```rust
            ast::Expression::DynamicVariableExpression(dynamic) => BodyExpression::DynamicVariable {
                target: self.lower_expression_or_missing(dynamic.expression(), dynamic.syntax()),
            },
            ast::Expression::ParenthesizedExpression(parenthesized) => {
                // Transparent: `($x)` and `$x` are one IR. The null-safe
                // chain boundary this creates is handled in Task 4.
                return self.lower_expression_or_missing(
                    parenthesized.expression(),
                    parenthesized.syntax(),
                );
            }
            ast::Expression::PrefixExpression(prefix) => BodyExpression::Unary {
                operator: token_kind_or_error(prefix.operator_token()),
                operand: self.lower_expression_or_missing(prefix.operand(), prefix.syntax()),
            },
            ast::Expression::PostfixExpression(postfix) => BodyExpression::Postfix {
                operator: token_kind_or_error(postfix.operator_token()),
                operand: self.lower_expression_or_missing(postfix.operand(), postfix.syntax()),
            },
            ast::Expression::BinaryExpression(binary) => BodyExpression::Binary {
                operator: token_kind_or_error(binary.operator_token()),
                lhs: self.lower_expression_or_missing(binary.lhs(), binary.syntax()),
                rhs: self.lower_expression_or_missing(binary.rhs(), binary.syntax()),
            },
            ast::Expression::AssignmentExpression(assignment) => BodyExpression::Assignment {
                operator: token_kind_or_error(assignment.operator_token()),
                by_reference: assignment.by_reference_token().is_some(),
                target: self.lower_expression_or_missing(assignment.target(), assignment.syntax()),
                value: self.lower_expression_or_missing(assignment.value(), assignment.syntax()),
            },
            ast::Expression::CastExpression(cast) => BodyExpression::Cast {
                operator: token_kind_or_error(cast.operator_token()),
                operand: self.lower_expression_or_missing(cast.operand(), cast.syntax()),
            },
            ast::Expression::TernaryExpression(ternary) => BodyExpression::Ternary {
                condition: self.lower_expression_or_missing(ternary.condition(), ternary.syntax()),
                middle: ternary.middle().map(|middle| self.lower_expression(&middle)),
                alternative: self.lower_expression_or_missing(ternary.third(), ternary.syntax()),
            },
            ast::Expression::ArrayExpression(array) => BodyExpression::Array {
                entries: self.lower_array_entries(array.syntax()),
            },
            ast::Expression::ListExpression(list) => BodyExpression::Array {
                entries: self.lower_array_entries(list.syntax()),
            },
            ast::Expression::InterpolatedString(string) => BodyExpression::InterpolatedString {
                parts: self.lower_string_parts(string.syntax()),
            },
            ast::Expression::HeredocExpression(heredoc) => BodyExpression::InterpolatedString {
                parts: self.lower_string_parts(heredoc.syntax()),
            },
            ast::Expression::ShellExecExpression(shell) => BodyExpression::ShellExec {
                parts: self.lower_string_parts(shell.syntax()),
            },
            ast::Expression::IssetExpression(isset) => BodyExpression::Isset {
                targets: self.lower_argument_expressions(isset.argument_list()),
            },
            ast::Expression::EmptyExpression(empty) => BodyExpression::Empty {
                target: self.lower_first_argument_or_missing(empty.argument_list(), empty.syntax()),
            },
            ast::Expression::EvalExpression(eval) => BodyExpression::Eval {
                argument: self.lower_first_argument_or_missing(eval.argument_list(), eval.syntax()),
            },
            ast::Expression::ExitExpression(exit) => BodyExpression::Exit {
                argument: exit
                    .argument_list()
                    .and_then(|list| list.arguments().next())
                    .and_then(|argument| argument.expression())
                    .map(|expression| self.lower_expression(&expression)),
            },
            ast::Expression::PrintExpression(print) => BodyExpression::Print {
                operand: self.lower_expression_or_missing(print.operand(), print.syntax()),
            },
            ast::Expression::CloneExpression(clone) => {
                let operand = match clone.operand() {
                    Some(operand) => self.lower_expression(&operand),
                    None => self
                        .lower_first_argument_or_missing(clone.argument_list(), clone.syntax()),
                };
                BodyExpression::Clone { operand }
            }
            ast::Expression::ThrowExpression(throw) => BodyExpression::Throw {
                operand: self.lower_expression_or_missing(throw.operand(), throw.syntax()),
            },
            ast::Expression::YieldExpression(yield_expression) => BodyExpression::Yield {
                key: yield_expression.key().map(|key| self.lower_expression(&key)),
                value: yield_expression
                    .value()
                    .map(|value| self.lower_expression(&value)),
                delegated: yield_expression.yield_from_token().is_some(),
            },
            ast::Expression::IncludeExpression(include) => BodyExpression::Include {
                operator: token_kind_or_error(include.operator_token()),
                operand: self.lower_expression_or_missing(include.operand(), include.syntax()),
            },
            ast::Expression::MatchExpression(match_expression) => {
                let match_arms: Vec<ast::MatchArm> = match_expression.match_arms().collect();
                let mut arms = Vec::new();
                for arm in &match_arms {
                    let conditions: Vec<ast::Expression> = arm.conditions().collect();
                    arms.push(MatchCase {
                        conditions: conditions
                            .iter()
                            .map(|condition| self.lower_expression(condition))
                            .collect(),
                        is_default: arm.is_default(),
                        body: self.lower_expression_or_missing(arm.body(), arm.syntax()),
                    });
                }
                BodyExpression::Match {
                    subject: self.lower_expression_or_missing(
                        match_expression.subject(),
                        match_expression.syntax(),
                    ),
                    arms,
                }
            }
```

And the three helpers on `impl Lowering<'_>`:

```rust
    fn lower_first_argument_or_missing(
        &mut self,
        list: Option<ast::ArgumentList>,
        parent: &SyntaxNode,
    ) -> ExpressionId {
        let expression = list
            .and_then(|list| list.arguments().next())
            .and_then(|argument| argument.expression());
        self.lower_expression_or_missing(expression, parent)
    }

    /// Array and list entries in written order, empty destructuring
    /// slots kept as holes: a comma with no element since the previous
    /// separator marks one (`[, $second]`), a trailing comma does not.
    fn lower_array_entries(&mut self, node: &SyntaxNode) -> Vec<ArrayEntry> {
        let mut entries = Vec::new();
        let mut element_since_separator = false;
        for element in node.children_with_tokens() {
            match element {
                celerrate_syntax::SyntaxElement::Node(child) => {
                    let Some(array_element) = ast::ArrayElement::cast(child) else {
                        continue;
                    };
                    let by_reference = array_element
                        .syntax()
                        .children_with_tokens()
                        .filter_map(|element| element.into_token())
                        .any(|token| token.kind() == SyntaxKind::Ampersand);
                    entries.push(ArrayEntry::Element {
                        key: array_element.key().map(|key| self.lower_expression(&key)),
                        value: self.lower_expression_or_missing(
                            array_element.value(),
                            array_element.syntax(),
                        ),
                        spread: array_element.spread_token().is_some(),
                        by_reference,
                    });
                    element_since_separator = true;
                }
                celerrate_syntax::SyntaxElement::Token(token)
                    if token.kind() == SyntaxKind::Comma =>
                {
                    if !element_since_separator {
                        entries.push(ArrayEntry::Hole);
                    }
                    element_since_separator = false;
                }
                _ => {}
            }
        }
        entries
    }

    fn lower_string_parts(&mut self, node: &SyntaxNode) -> Vec<StringPart> {
        let mut parts = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                celerrate_syntax::SyntaxElement::Token(token)
                    if token.kind() == SyntaxKind::StringFragment =>
                {
                    parts.push(StringPart::Fragment {
                        text: token.text().to_owned(),
                    });
                }
                celerrate_syntax::SyntaxElement::Node(child) => {
                    let Some(interpolation) = ast::StringInterpolation::cast(child) else {
                        continue;
                    };
                    parts.push(match &interpolation {
                        ast::StringInterpolation::SimpleInterpolation(simple) => {
                            StringPart::Simple {
                                text: simple.syntax().text().to_string(),
                            }
                        }
                        ast::StringInterpolation::BraceInterpolation(brace) => {
                            StringPart::Interpolation {
                                expression: self
                                    .lower_expression_or_missing(brace.expression(), brace.syntax()),
                            }
                        }
                        ast::StringInterpolation::DollarBraceInterpolation(brace) => {
                            StringPart::Interpolation {
                                expression: self
                                    .lower_expression_or_missing(brace.expression(), brace.syntax()),
                            }
                        }
                    });
                }
                _ => {}
            }
        }
        parts
    }
```

Import `ArrayEntry, MatchCase, StringPart` from `crate::body` in `body_lowering.rs`.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test --package celerrate_semantics body`
Expected: PASS. If `the_grouping_forms_lower` fails on the shell-exec or heredoc fixtures because a fragment boundary differs from the assumption, adjust the asserted fragment texts to the parser's actual output (assert the shape, keep the pinned part kinds).

- [ ] **Step 5: Lint, format, full suite, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
git add -A crates/celerrate_semantics
git commit -m "✨ feat(semantics): lower the operator and value expression families"
```

---

### Task 4: Receivers, calls, `new`, and the pinned sugar reductions

The dereference family lowers: member and scoped access, calls with labeled and spread arguments, indexes, and `new` with all five class-reference shapes. On top of it, the two lowering-table rows the spec pins by name: `foo(...)` lowers to `CallableReference`, and any dereference chain containing a `?->` receives exactly one whole-chain `NullSafeChain` wrapper, with parentheses as chain boundaries (PHP semantics: a parenthesized prefix stops the short-circuit).

**Files:**
- Modify: `crates/celerrate_semantics/src/body.rs`
- Modify: `crates/celerrate_semantics/src/body_lowering.rs`

**Interfaces:**
- Consumes: Tasks 1-3; `MemberAccessExpression::{subject, operator_token, member_name}` (`Arrow` versus `NullsafeArrow`), `ScopedAccessExpression::{subject, member_name}`, `MemberName::{name_token, expression}`, `CallExpression::{callee, argument_list}`, `ArgumentList::{arguments, ellipsis_token}`, `Argument::{label_token, spread_token, expression}`, `IndexExpression::{subject, index}`, `NewExpression::{static_keyword_token, name, expression, class_declaration, argument_list}`, `ClassDeclaration::argument_list`, `AstIdMap::index_of`.
- Produces: the `BodyExpression` variants `MemberAccess`, `ScopedAccess`, `NullSafeChain`, `Call`, `CallableReference`, `New`, `Index` plus `MemberReference`, `ClassReference`, `CallArgument` exactly as in the canonical data model; `Lowering::{lower_chain_link, lower_link_or_missing, lower_member_reference, lower_call_arguments}` and the free function `chain_contains_null_safe(&ast::Expression) -> bool`.

- [ ] **Step 1: Write the failing tests**

Add to the test module of `body_lowering.rs` (extend imports with `CallArgument, ClassReference, MemberReference` as used):

```rust
    fn null_safe_chain_count(ir: &BodyIr) -> usize {
        ir.expressions
            .iter()
            .filter(|expression| matches!(expression, BodyExpression::NullSafeChain { .. }))
            .count()
    }

    #[test]
    fn a_method_call_on_this_lowers_as_call_over_member_access() {
        let ir = body("<?php function f() { $this->run(1, label: 2, ...$rest); }");
        let BodyExpression::Call { callee, arguments } = root_expression(&ir, 0) else {
            panic!("expected a call");
        };
        let BodyExpression::MemberAccess { receiver, member, null_safe } =
            ir.expression(*callee).unwrap()
        else {
            panic!("expected a member access callee");
        };
        assert!(!*null_safe);
        assert_eq!(member, &MemberReference::Named { name: "run".to_owned() });
        assert_eq!(
            ir.expression(*receiver),
            Some(&BodyExpression::Variable { name: "this".to_owned() }),
        );
        assert_eq!(arguments.len(), 3);
        assert_eq!(arguments[0].label, None);
        assert_eq!(arguments[1].label.as_deref(), Some("label"));
        assert!(arguments[2].spread);
    }

    #[test]
    fn member_references_distinguish_named_variable_and_computed() {
        let ir = body("<?php function f() { $a->name; $a->$dynamic; $a->{$computed}; Foo::$property; }");
        let expect_member = |position: usize| match root_expression(&ir, position) {
            BodyExpression::MemberAccess { member, .. } => member.clone(),
            BodyExpression::ScopedAccess { member, .. } => member.clone(),
            other => panic!("expected an access, got {other:?}"),
        };
        assert_eq!(expect_member(0), MemberReference::Named { name: "name".to_owned() });
        assert_eq!(expect_member(1), MemberReference::Variable { name: "dynamic".to_owned() });
        assert!(matches!(expect_member(2), MemberReference::Computed { .. }));
        assert_eq!(expect_member(3), MemberReference::Variable { name: "property".to_owned() });
    }

    #[test]
    fn scoped_access_and_class_constants_lower() {
        let ir = body("<?php function f() { Foo::bar(); static::create(); Foo::class; }");
        let BodyExpression::Call { callee, .. } = root_expression(&ir, 0) else {
            panic!("expected a call");
        };
        assert!(matches!(ir.expression(*callee).unwrap(), BodyExpression::ScopedAccess { .. }));
        let BodyExpression::Call { callee, .. } = root_expression(&ir, 1) else {
            panic!("expected a call");
        };
        let BodyExpression::ScopedAccess { subject, .. } = ir.expression(*callee).unwrap() else {
            panic!("expected a scoped access");
        };
        assert_eq!(
            ir.expression(*subject),
            Some(&BodyExpression::NamedReference { text: "static".to_owned() }),
        );
        assert!(matches!(
            root_expression(&ir, 2),
            BodyExpression::ScopedAccess { member: MemberReference::Named { name }, .. } if name == "class",
        ));
    }

    #[test]
    fn indexes_lower_and_the_push_form_has_no_index() {
        let ir = body("<?php function f() { $a[0]; $a[] = 1; }");
        assert!(matches!(root_expression(&ir, 0), BodyExpression::Index { index: Some(_), .. }));
        let BodyExpression::Assignment { target, .. } = root_expression(&ir, 1) else {
            panic!("expected an assignment");
        };
        assert!(matches!(
            ir.expression(*target).unwrap(),
            BodyExpression::Index { index: None, .. },
        ));
    }

    #[test]
    fn new_lowers_all_class_reference_shapes() {
        let ir = body(
            "<?php function f() { new Foo(1); new self; new static; new $factory; new class(2) {}; }",
        );
        let class_of = |position: usize| match root_expression(&ir, position) {
            BodyExpression::New { class, .. } => class.clone(),
            other => panic!("expected new, got {other:?}"),
        };
        assert_eq!(class_of(0), ClassReference::Named { name: "Foo".to_owned() });
        assert_eq!(class_of(1), ClassReference::Named { name: "self".to_owned() });
        assert_eq!(class_of(2), ClassReference::StaticKeyword);
        assert!(matches!(class_of(3), ClassReference::Dynamic { .. }));
        // The anonymous class is the second numbered declaration of the
        // file (function = 0, anonymous class = 1), and its constructor
        // arguments travel on the New expression.
        let BodyExpression::New { class: ClassReference::Anonymous { declaration }, arguments } =
            root_expression(&ir, 4)
        else {
            panic!("expected an anonymous new");
        };
        assert_eq!(declaration.index, 1);
        assert_eq!(arguments.len(), 1);
    }

    #[test]
    fn a_first_class_callable_lowers_to_a_callable_reference() {
        let ir = body("<?php function f() { strlen(...); $obj->m(...); Foo::bar(...); foo(...$args); }");
        assert!(matches!(root_expression(&ir, 0), BodyExpression::CallableReference { .. }));
        assert!(matches!(root_expression(&ir, 1), BodyExpression::CallableReference { .. }));
        assert!(matches!(root_expression(&ir, 2), BodyExpression::CallableReference { .. }));
        // Spreading an argument is a call, not a callable reference.
        assert!(matches!(
            root_expression(&ir, 3),
            BodyExpression::Call { arguments, .. } if arguments.len() == 1 && arguments[0].spread,
        ));
    }

    #[test]
    fn a_null_safe_chain_gets_exactly_one_wrapper_at_its_top() {
        let ir = body("<?php function f() { $a?->b->c()['d']; }");
        assert_eq!(null_safe_chain_count(&ir), 1);
        // The wrapper is the outermost expression of the statement.
        let BodyStatement::Expression { expression } = root_statement(&ir, 0) else {
            panic!("expected an expression statement");
        };
        let BodyExpression::NullSafeChain { chain } = ir.expression(*expression).unwrap() else {
            panic!("expected the wrapper at the top");
        };
        // Inside, the null-safe link carries its flag.
        let mut current = *chain;
        let null_safe_flag = loop {
            match ir.expression(current).unwrap() {
                BodyExpression::Index { subject, .. } => current = *subject,
                BodyExpression::Call { callee, .. } => current = *callee,
                BodyExpression::MemberAccess { receiver, null_safe, .. } => {
                    if *null_safe {
                        break true;
                    }
                    current = *receiver;
                }
                _ => break false,
            }
        };
        assert!(null_safe_flag);
    }

    #[test]
    fn a_plain_chain_gets_no_wrapper() {
        let ir = body("<?php function f() { $a->b->c(); }");
        assert_eq!(null_safe_chain_count(&ir), 0);
    }

    #[test]
    fn parentheses_bound_the_short_circuit() {
        // PHP semantics: `($a?->b)->c` does not short-circuit `->c`;
        // the wrapper closes at the parenthesis, and the outer access
        // sees a possibly-null receiver (exactly what the nullability
        // family must report).
        let ir = body("<?php function f() { ($a?->b)->c; }");
        assert_eq!(null_safe_chain_count(&ir), 1);
        let BodyStatement::Expression { expression } = root_statement(&ir, 0) else {
            panic!("expected an expression statement");
        };
        let BodyExpression::MemberAccess { receiver, null_safe, .. } =
            ir.expression(*expression).unwrap()
        else {
            panic!("expected the outer access at the top");
        };
        assert!(!*null_safe);
        assert!(matches!(
            ir.expression(*receiver).unwrap(),
            BodyExpression::NullSafeChain { .. },
        ));
    }

    #[test]
    fn independent_chains_wrap_independently() {
        let ir = body("<?php function f() { $a?->b[$c?->d]; }");
        assert_eq!(null_safe_chain_count(&ir), 2);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics body_lowering`
Expected: FAIL.

- [ ] **Step 3: Implement**

In `body.rs`, add the variants `MemberAccess`, `ScopedAccess`, `NullSafeChain`, `Call`, `CallableReference`, `New`, `Index` and the types `MemberReference`, `ClassReference`, `CallArgument` exactly as in the canonical data model. Extend the `lib.rs` export with `CallArgument, ClassReference, MemberReference`.

In `body_lowering.rs`:

1. The chain wrapper replaces the body of `lower_expression`:

```rust
    fn lower_expression(&mut self, expression: &ast::Expression) -> ExpressionId {
        let id = self.lower_chain_link(expression);
        if chain_contains_null_safe(expression) {
            return self.allocate_expression(
                BodyExpression::NullSafeChain { chain: id },
                expression.syntax(),
            );
        }
        id
    }
```

2. The chain walk. The four dereference kinds recurse into their continuation child through `lower_chain_link` (staying inside the current chain); every other child position goes through `lower_expression` (opening a new chain scope). Everything else falls through to `lower_expression_kind`:

```rust
    fn lower_chain_link(&mut self, expression: &ast::Expression) -> ExpressionId {
        match expression {
            ast::Expression::MemberAccessExpression(access) => {
                let receiver = self.lower_link_or_missing(access.subject(), access.syntax());
                let null_safe = access
                    .operator_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::NullsafeArrow);
                let member = self.lower_member_reference(access.member_name());
                self.allocate_expression(
                    BodyExpression::MemberAccess { receiver, member, null_safe },
                    access.syntax(),
                )
            }
            ast::Expression::ScopedAccessExpression(scoped) => {
                let subject = self.lower_link_or_missing(scoped.subject(), scoped.syntax());
                let member = self.lower_member_reference(scoped.member_name());
                self.allocate_expression(
                    BodyExpression::ScopedAccess { subject, member },
                    scoped.syntax(),
                )
            }
            ast::Expression::CallExpression(call) => {
                let callee = self.lower_link_or_missing(call.callee(), call.syntax());
                let is_first_class = call.argument_list().is_some_and(|list| {
                    list.ellipsis_token().is_some() && list.arguments().next().is_none()
                });
                let lowered = if is_first_class {
                    BodyExpression::CallableReference { callee }
                } else {
                    BodyExpression::Call {
                        callee,
                        arguments: self.lower_call_arguments(call.argument_list()),
                    }
                };
                self.allocate_expression(lowered, call.syntax())
            }
            ast::Expression::IndexExpression(index) => {
                let subject = self.lower_link_or_missing(index.subject(), index.syntax());
                let lowered = BodyExpression::Index {
                    subject,
                    index: index
                        .index()
                        .map(|expression| self.lower_expression(&expression)),
                };
                self.allocate_expression(lowered, index.syntax())
            }
            other => self.lower_expression_kind(other),
        }
    }

    fn lower_link_or_missing(
        &mut self,
        expression: Option<ast::Expression>,
        parent: &SyntaxNode,
    ) -> ExpressionId {
        match expression {
            Some(expression) => self.lower_chain_link(&expression),
            None => self.allocate_expression(BodyExpression::Missing, parent),
        }
    }
```

3. The chain inspection, a free function next to `token_kind_or_error`:

```rust
/// Whether the dereference chain rooted at `expression` contains a
/// `?->` link. Only the four chain kinds are descended; a parenthesized
/// prefix is not part of the chain (PHP semantics: parentheses stop the
/// short-circuit), so it answers `false` here and wraps on its own when
/// its interior is lowered.
fn chain_contains_null_safe(expression: &ast::Expression) -> bool {
    let mut current = expression.clone();
    loop {
        let subject = match &current {
            ast::Expression::MemberAccessExpression(access) => {
                if access
                    .operator_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::NullsafeArrow)
                {
                    return true;
                }
                access.subject()
            }
            ast::Expression::CallExpression(call) => call.callee(),
            ast::Expression::IndexExpression(index) => index.subject(),
            ast::Expression::ScopedAccessExpression(scoped) => scoped.subject(),
            _ => return false,
        };
        match subject {
            Some(subject) => current = subject,
            None => return false,
        }
    }
}
```

4. The member reference and call-argument helpers on `impl Lowering<'_>`:

```rust
    fn lower_member_reference(&mut self, member: Option<ast::MemberName>) -> MemberReference {
        let Some(member) = member else {
            return MemberReference::Missing;
        };
        if let Some(token) = member.name_token() {
            return MemberReference::Named {
                name: token.text().to_owned(),
            };
        }
        let braced = member
            .syntax()
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .any(|token| token.kind() == SyntaxKind::OpenBrace);
        match member.expression() {
            Some(ast::Expression::VariableReference(variable)) if !braced => {
                match variable.name_token() {
                    Some(token) => MemberReference::Variable {
                        name: token.text().trim_start_matches('$').to_owned(),
                    },
                    None => MemberReference::Missing,
                }
            }
            Some(expression) => MemberReference::Computed {
                expression: self.lower_expression(&expression),
            },
            None => MemberReference::Missing,
        }
    }

    fn lower_call_arguments(&mut self, list: Option<ast::ArgumentList>) -> Vec<CallArgument> {
        let arguments: Vec<ast::Argument> = list
            .into_iter()
            .flat_map(|list| list.arguments().collect::<Vec<_>>())
            .collect();
        let mut lowered = Vec::new();
        for argument in &arguments {
            lowered.push(CallArgument {
                label: argument.label_token().map(|token| token.text().to_owned()),
                spread: argument.spread_token().is_some(),
                value: self.lower_expression_or_missing(argument.expression(), argument.syntax()),
            });
        }
        lowered
    }
```

5. The `new` arm in `lower_expression_kind`:

```rust
            ast::Expression::NewExpression(new) => {
                let (class, arguments) = if let Some(declaration) = new.class_declaration() {
                    let class = match self.map.index_of(declaration.syntax()) {
                        Some(index) => ClassReference::Anonymous {
                            declaration: AstId { file: self.file, index },
                        },
                        None => ClassReference::Missing,
                    };
                    // An anonymous class carries its own constructor
                    // arguments inside the declaration node.
                    (class, self.lower_call_arguments(declaration.argument_list()))
                } else {
                    let class = if new.static_keyword_token().is_some() {
                        ClassReference::StaticKeyword
                    } else if let Some(name) = new.name() {
                        ClassReference::Named { name: name.text() }
                    } else if let Some(expression) = new.expression() {
                        ClassReference::Dynamic {
                            expression: self.lower_expression(&expression),
                        }
                    } else {
                        ClassReference::Missing
                    };
                    (class, self.lower_call_arguments(new.argument_list()))
                };
                BodyExpression::New { class, arguments }
            }
```

Import `CallArgument, ClassReference, MemberReference` from `crate::body`.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test --package celerrate_semantics body`
Expected: PASS.

- [ ] **Step 5: Lint, format, full suite, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
git add -A crates/celerrate_semantics
git commit -m "✨ feat(semantics): lower receivers and calls with the pinned sugar reductions"
```

---

### Task 5: Closures and arrow functions lower into the enclosing arena

Closure bodies are expressions of their enclosing body (the rust-analyzer model): no separate query key, dense ids in the same arenas. Signatures reuse `ParameterSignature`, extracted from `members.rs` into a shared helper. This task closes the `BodyExpression` match: the `_ =>` fallback disappears and the match becomes exhaustive, so a future grammar addition forces an explicit decision here.

**Files:**
- Modify: `crates/celerrate_semantics/src/members.rs` (extract `parameter_signatures`)
- Modify: `crates/celerrate_semantics/src/body.rs`
- Modify: `crates/celerrate_semantics/src/body_lowering.rs`

**Interfaces:**
- Consumes: `ClosureExpression::{static_keyword_token, by_reference_token, parameter_list, closure_use_clause, return_type, block}`, `ArrowFunctionExpression::{static_keyword_token, by_reference_token, parameter_list, return_type, body}`, `ClosureUseClause::variable_references`, `ast::type_text`.
- Produces: `pub(crate) fn parameter_signatures(list: Option<ast::ParameterList>) -> Vec<ParameterSignature>` in `members.rs` (also consumed by `method_signature` there); the `BodyExpression::Closure` and `BodyExpression::ArrowFunction` variants and `ClosureUse` exactly as in the canonical data model; the free function `closure_uses(Option<ast::ClosureUseClause>) -> Vec<ClosureUse>`.

- [ ] **Step 1: Write the failing tests**

Add to the test module of `body_lowering.rs` (extend imports with `ClosureUse`):

```rust
    #[test]
    fn a_closure_lowers_signature_uses_and_body_inline() {
        let ir = body(
            "<?php function f() { $g = static function (int $a, &$b) use ($captured, &$shared): void { return $a; }; }",
        );
        let BodyExpression::Assignment { value, .. } = root_expression(&ir, 0) else {
            panic!("expected an assignment");
        };
        let BodyExpression::Closure {
            parameters, uses, return_type_text, is_static, by_reference, body: closure_body,
        } = ir.expression(*value).unwrap()
        else {
            panic!("expected a closure");
        };
        assert!(*is_static);
        assert!(!*by_reference);
        assert_eq!(return_type_text.as_deref(), Some("void"));
        assert_eq!(parameters.len(), 2);
        assert_eq!(parameters[0].name, "a");
        assert_eq!(parameters[0].type_text.as_deref(), Some("int"));
        assert!(parameters[1].by_reference);
        assert_eq!(
            uses,
            &vec![
                ClosureUse { name: "captured".to_owned(), by_reference: false },
                ClosureUse { name: "shared".to_owned(), by_reference: true },
            ],
        );
        // The closure body's statements live in the same arena.
        assert_eq!(closure_body.len(), 1);
        assert!(matches!(
            ir.statement(closure_body[0]).unwrap(),
            BodyStatement::Return { value: Some(_) },
        ));
    }

    #[test]
    fn an_arrow_function_lowers_its_expression_body() {
        let ir = body("<?php function f() { $g = fn (int $x): int => $x + 1; }");
        let BodyExpression::Assignment { value, .. } = root_expression(&ir, 0) else {
            panic!("expected an assignment");
        };
        let BodyExpression::ArrowFunction { parameters, return_type_text, body: arrow_body, .. } =
            ir.expression(*value).unwrap()
        else {
            panic!("expected an arrow function");
        };
        assert_eq!(parameters.len(), 1);
        assert_eq!(return_type_text.as_deref(), Some("int"));
        assert!(matches!(
            ir.expression(*arrow_body).unwrap(),
            BodyExpression::Binary { operator: SyntaxKind::Plus, .. },
        ));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics body_lowering`
Expected: FAIL.

- [ ] **Step 3: Extract the parameter helper (refactor under green)**

In `members.rs`, lift the parameter mapping out of `method_signature` into a crate-visible function, and rewrite `method_signature` on top of it (behavior identical; the existing member tests stay green):

```rust
/// The written signatures of a parameter list: shared between method
/// signatures and the body IR's closures, so both stay byte-compatible.
pub(crate) fn parameter_signatures(
    list: Option<ast::ParameterList>,
) -> Vec<ParameterSignature> {
    list.into_iter()
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
        .collect()
}

fn method_signature(method: &ast::MethodDeclaration) -> MemberSignature {
    MemberSignature {
        parameters: parameter_signatures(method.parameter_list()),
        type_text: method.return_type().map(|ty| ast::type_text(&ty)),
        default_text: None,
        by_reference: method.by_reference_token().is_some(),
    }
}
```

Run: `cargo test --package celerrate_semantics members`
Expected: PASS (pure refactor).

- [ ] **Step 4: Implement the closure lowering**

In `body.rs`, add the `Closure` and `ArrowFunction` variants and `ClosureUse` exactly as in the canonical data model (`use crate::members::ParameterSignature;`). Extend the `lib.rs` export with `ClosureUse`.

In `body_lowering.rs`, add the two arms (and delete the `_ => BodyExpression::Missing` fallback: the match over `ast::Expression` is now exhaustive):

```rust
            ast::Expression::ClosureExpression(closure) => BodyExpression::Closure {
                parameters: crate::members::parameter_signatures(closure.parameter_list()),
                uses: closure_uses(closure.closure_use_clause()),
                return_type_text: closure.return_type().map(|ty| ast::type_text(&ty)),
                is_static: closure.static_keyword_token().is_some(),
                by_reference: closure.by_reference_token().is_some(),
                body: self.lower_block_statements(closure.block()),
            },
            ast::Expression::ArrowFunctionExpression(arrow) => BodyExpression::ArrowFunction {
                parameters: crate::members::parameter_signatures(arrow.parameter_list()),
                return_type_text: arrow.return_type().map(|ty| ast::type_text(&ty)),
                is_static: arrow.static_keyword_token().is_some(),
                by_reference: arrow.by_reference_token().is_some(),
                body: self.lower_expression_or_missing(arrow.body(), arrow.syntax()),
            },
```

And the free helper:

```rust
/// The captures of a `use (...)` clause, in written order. The `&`
/// sits between commas as a bare token, so a pending flag associates
/// it with the following variable.
fn closure_uses(clause: Option<ast::ClosureUseClause>) -> Vec<ClosureUse> {
    let Some(clause) = clause else {
        return Vec::new();
    };
    let mut uses = Vec::new();
    let mut by_reference = false;
    for element in clause.syntax().children_with_tokens() {
        match element {
            celerrate_syntax::SyntaxElement::Token(token)
                if token.kind() == SyntaxKind::Ampersand =>
            {
                by_reference = true;
            }
            celerrate_syntax::SyntaxElement::Node(node) => {
                if let Some(variable) = ast::VariableReference::cast(node)
                    && let Some(name) = variable.name_token()
                {
                    uses.push(ClosureUse {
                        name: name.text().trim_start_matches('$').to_owned(),
                        by_reference,
                    });
                }
                by_reference = false;
            }
            _ => {}
        }
    }
    uses
}
```

Import `ClosureUse` from `crate::body`.

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test --package celerrate_semantics body`
Expected: PASS.

- [ ] **Step 6: Lint, format, full suite, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
git add -A crates/celerrate_semantics
git commit -m "✨ feat(semantics): lower closures and arrow functions into the body arena"
```

---

### Task 6: Recognized annotations and the redefined comment-only edit class

The IR's content becomes "code plus recognized annotation content" (spec section 2): docblocks inside bodies (any tag reader may consume them: inline `@var`, assertion tags) and line or block comments carrying a suppression directive are carried verbatim in the IR, anchored to the following statement. Everything else stays trivia: the redefined comment-only edit class is exactly the complement. Nested declaration subtrees are skipped (their bodies are other lowerings' scopes).

Recorded conservatism: a prose-only docblock inside a body still invalidates that body's IR; the second-stage cutoff at the parsed-annotation level is plan 4a's business (spec section 5), and body-inline docblocks are rare and usually annotation-bearing.

**Files:**
- Modify: `crates/celerrate_semantics/src/body.rs`
- Modify: `crates/celerrate_semantics/src/body_lowering.rs`

**Interfaces:**
- Consumes: comment trivia kinds (`DocComment`, `LineComment`, `BlockComment`), the source map's statement pointers (ranges are consumed during lowering and never stored).
- Produces: `pub struct BodyAnnotation { pub text: String, pub anchor: Option<StatementId> }`, the `annotations: Vec<BodyAnnotation>` field on `BodyIr`, and `pub fn is_recognized_annotation(kind: SyntaxKind, text: &str) -> bool` (the pinned predicate later plans and tests share).

- [ ] **Step 1: Write the failing tests**

Add to the test module of `body_lowering.rs` (extend imports with `BodyAnnotation` if asserted directly):

```rust
    #[test]
    fn prose_comments_are_invisible_to_the_body() {
        // The redefined comment-only edit class: trivia no annotation
        // reader consumes never changes a body IR.
        let first = body("<?php function f() { // a prose note\n $x; /* more prose */ }");
        let second = body("<?php function f() { // an edited note\n $x; /* other prose */ }");
        assert_eq!(first, second);
        assert!(first.annotations.is_empty());
    }

    #[test]
    fn an_inline_var_docblock_is_carried_and_anchored() {
        let ir = body("<?php function f() { $a; /** @var User $u */ $u; }");
        assert_eq!(ir.annotations.len(), 1);
        assert_eq!(ir.annotations[0].text, "/** @var User $u */");
        // Anchored to the statement that follows it: `$u;`, the second
        // root statement.
        assert_eq!(ir.annotations[0].anchor, Some(ir.root[1]));

        let edited = body("<?php function f() { $a; /** @var Admin $u */ $u; }");
        assert_ne!(ir, edited);
    }

    #[test]
    fn suppression_directives_are_carried_prose_line_comments_are_not() {
        let ir = body(
            "<?php function f() { // @phpstan-ignore-next-line\n $x->y; # @psalm-suppress PossiblyNullReference\n $z; }",
        );
        assert_eq!(ir.annotations.len(), 2);
        assert!(ir.annotations[0].text.contains("@phpstan-ignore"));
        assert!(ir.annotations[1].text.contains("@psalm-suppress"));
    }

    #[test]
    fn a_trailing_comment_anchors_to_nothing() {
        let ir = body("<?php function f() { $x; /** @var int $x */ }");
        assert_eq!(ir.annotations.len(), 1);
        assert_eq!(ir.annotations[0].anchor, None);
    }

    #[test]
    fn nested_declaration_bodies_keep_their_own_annotations() {
        // The comment belongs to the anonymous class's method body,
        // not to the enclosing function's.
        let outer = body(
            "<?php function f() { $o = new class { function m() { /** @var A $a */ $a; } }; }",
        );
        assert!(outer.annotations.is_empty());

        // A closure's body belongs to this lowering, so its
        // annotations are carried here.
        let with_closure = body("<?php function f() { $g = function () { /** @var A $a */ $a; }; }");
        assert_eq!(with_closure.annotations.len(), 1);
    }
```

And a predicate test in the test module of `body.rs`:

```rust
    #[test]
    fn the_recognized_annotation_predicate_is_pinned() {
        use celerrate_syntax::SyntaxKind;

        use super::is_recognized_annotation;

        assert!(is_recognized_annotation(SyntaxKind::DocComment, "/** anything */"));
        assert!(is_recognized_annotation(SyntaxKind::LineComment, "// @phpstan-ignore-line"));
        assert!(is_recognized_annotation(SyntaxKind::LineComment, "# @psalm-suppress Foo"));
        assert!(is_recognized_annotation(SyntaxKind::BlockComment, "/* @phpstan-ignore */"));
        assert!(!is_recognized_annotation(SyntaxKind::LineComment, "// prose"));
        assert!(!is_recognized_annotation(SyntaxKind::BlockComment, "/* prose */"));
        assert!(!is_recognized_annotation(SyntaxKind::Whitespace, "@phpstan-ignore"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics body`
Expected: FAIL (`annotations` field missing).

- [ ] **Step 3: Implement**

In `body.rs`, add to the model:

```rust
/// One recognized annotation-bearing comment inside a body: content a
/// type-engine reader consumes, carried in the IR so an edit to it
/// invalidates body consumers while prose trivia stays invisible.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BodyAnnotation {
    /// The comment text, verbatim.
    pub text: String,
    /// The first lowered statement starting after the comment ends;
    /// `None` when the comment trails every statement of the body.
    pub anchor: Option<StatementId>,
}

/// Whether one comment token is recognized annotation content: every
/// docblock (inline `@var`, assertion tags, anything a tag reader may
/// consume), plus line and block comments carrying a suppression
/// directive. The redefined comment-only edit class is exactly the
/// complement: trivia this predicate rejects never changes a body IR.
pub fn is_recognized_annotation(kind: SyntaxKind, text: &str) -> bool {
    match kind {
        SyntaxKind::DocComment => true,
        SyntaxKind::LineComment | SyntaxKind::BlockComment => {
            text.contains("@phpstan-ignore") || text.contains("@psalm-suppress")
        }
        _ => false,
    }
}
```

Add `pub annotations: Vec<BodyAnnotation>,` to `BodyIr` (keep `Default`). Extend the `lib.rs` export with `BodyAnnotation, is_recognized_annotation`.

In `body_lowering.rs`, call the collection from `lower_body` after the root statements are lowered:

```rust
    let root = lowering.lower_statements(block.statements());
    lowering.ir.root = root;
    lowering.collect_annotations(&block);
```

And implement it (plus its free helper):

```rust
    /// Collects the recognized comments of the body, in document
    /// order, each anchored to the first lowered statement that starts
    /// after it. Ranges are consumed here and never stored: the IR
    /// keeps text and arena ids only.
    fn collect_annotations(&mut self, block: &ast::Block) {
        let mut comments = Vec::new();
        collect_recognized_comments(block.syntax(), &mut comments);
        for token in comments {
            let end = token.text_range().end();
            let anchor = self
                .source_map
                .statements
                .iter()
                .enumerate()
                .filter(|(_, pointer)| pointer.text_range().start() >= end)
                .min_by_key(|(_, pointer)| pointer.text_range().start())
                .and_then(|(index, _)| StatementId::from_index(index));
            self.ir.annotations.push(BodyAnnotation {
                text: token.text().to_owned(),
                anchor,
            });
        }
    }
```

```rust
/// The recognized comment tokens under `node`, nested declaration
/// subtrees skipped: their bodies are other lowerings' scopes.
/// Closures are not declarations, so their bodies stay in scope.
fn collect_recognized_comments(
    node: &SyntaxNode,
    comments: &mut Vec<celerrate_syntax::SyntaxToken>,
) {
    for element in node.children_with_tokens() {
        match element {
            celerrate_syntax::SyntaxElement::Node(child) => {
                if !matches!(
                    child.kind(),
                    SyntaxKind::ClassDeclaration
                        | SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::TraitDeclaration
                        | SyntaxKind::EnumDeclaration
                        | SyntaxKind::FunctionDeclaration,
                ) {
                    collect_recognized_comments(&child, comments);
                }
            }
            celerrate_syntax::SyntaxElement::Token(token) => {
                if crate::body::is_recognized_annotation(token.kind(), token.text()) {
                    comments.push(token);
                }
            }
        }
    }
}
```

Import `BodyAnnotation` from `crate::body` in `body_lowering.rs`.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test --package celerrate_semantics body`
Expected: PASS.

- [ ] **Step 5: Lint, format, full suite, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
git add -A crates/celerrate_semantics
git commit -m "✨ feat(semantics): carry recognized annotation content in the body IR"
```

---

### Task 7: The invalidation contract, the harness extension, and closure

The point of the whole plan, proven the only way it can be: execution logs. The new edit classes join `tests/invalidation_scope.rs` (a body edit re-runs only that body's consumers; a prose-comment, formatting, signature, or member-docblock edit re-runs none), the body IR joins the incremental-consistency harness, an adversarial batch pins error resilience and arena integrity, and the crate documentation catches up.

**Files:**
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs`
- Modify: `crates/celerrate_semantics/tests/incremental_consistency.rs`
- Modify: `crates/celerrate_semantics/src/body_lowering.rs` (the adversarial batch test)
- Modify: `crates/celerrate_semantics/src/lib.rs` (crate documentation)

**Interfaces:**
- Consumes: everything Tasks 1-6 produced, through the public exports (`BodyQuery`, `body_ir`, `AstId`, the model types); `celerrate_db::testing::{TestDatabase, assert_incremental_consistency_with}`.
- Produces: no new production API; the pinned contract.

- [ ] **Step 1: Write the invalidation-scope tests (failing only if the contract is broken; they must pass immediately, which is itself the verification)**

Add to `crates/celerrate_semantics/tests/invalidation_scope.rs` (extend the existing imports with `BodyQuery, body_ir` from `celerrate_semantics` and `AstId`; `AstId` is already exported):

```rust
/// A stand-in for plan 5's inference: any query that reads one body's
/// IR and nothing else syntactic. If the IR backdates, this must never
/// re-run.
#[salsa::tracked]
fn body_statement_count<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> usize {
    body_ir(db, file, body)
        .as_ref()
        .map_or(0, |ir| ir.statements.len())
}

#[test]
fn a_body_edit_reruns_only_that_bodys_consumers() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function f() { return 1; } function g() { return 2; }".to_vec(),
    );
    let f = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 0 });
    let g = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 1 });
    let _ = body_statement_count(&db, file, f);
    let _ = body_statement_count(&db, file, g);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function f() { return 9; } function g() { return 2; }".to_vec());
    let _ = body_statement_count(&db, file, f);
    let _ = body_statement_count(&db, file, g);
    let log = db.take_executed();

    // Both lowerings re-run (they read the parse), g's IR backdates:
    // only f's consumer re-executes.
    assert_eq!(executions_of(&log, "body_ir"), 2);
    assert_eq!(executions_of(&log, "body_statement_count"), 1);
}

#[test]
fn a_prose_comment_edit_inside_a_body_spares_body_consumers() {
    // The redefined comment-only edit class, observed end to end.
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function f() { // draft\n return 1; }".to_vec(),
    );
    let f = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 0 });
    let _ = body_statement_count(&db, file, f);
    db.take_executed();

    file.set_bytes(&mut db)
        .to(b"<?php function f() { // final\n return 1; }".to_vec());
    let _ = body_statement_count(&db, file, f);
    let log = db.take_executed();

    assert_eq!(executions_of(&log, "body_ir"), 1);
    assert_eq!(executions_of(&log, "body_statement_count"), 0);
}

#[test]
fn an_annotation_edit_inside_a_body_reruns_body_consumers() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php function f() { /** @var User $u */ $u; // @phpstan-ignore-next-line\n $x; }".to_vec(),
    );
    let f = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 0 });
    let _ = body_statement_count(&db, file, f);
    db.take_executed();

    // An inline @var edit is a code edit for the type engine.
    file.set_bytes(&mut db)
        .to(b"<?php function f() { /** @var Admin $u */ $u; // @phpstan-ignore-next-line\n $x; }".to_vec());
    let _ = body_statement_count(&db, file, f);
    assert_eq!(executions_of(&db.take_executed(), "body_statement_count"), 1);

    // So is a suppression-directive edit.
    file.set_bytes(&mut db)
        .to(b"<?php function f() { /** @var Admin $u */ $u; // @phpstan-ignore-line\n $x; }".to_vec());
    let _ = body_statement_count(&db, file, f);
    assert_eq!(executions_of(&db.take_executed(), "body_statement_count"), 1);
}

#[test]
fn signature_formatting_and_member_docblock_edits_spare_body_consumers() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php class A { /** doc */ public function m(int $a) { if ($a) return 1; } }".to_vec(),
    );
    let m = BodyQuery::new(&db, AstId { file: FileId::new(0), index: 1 });
    let _ = body_statement_count(&db, file, m);
    db.take_executed();

    // A signature edit: parameters live in the member tree, not here.
    file.set_bytes(&mut db).to(
        b"<?php class A { /** doc */ public function m(int $a, int $b = 0) { if ($a) return 1; } }".to_vec(),
    );
    let _ = body_statement_count(&db, file, m);
    assert_eq!(executions_of(&db.take_executed(), "body_statement_count"), 0);

    // A member-docblock edit: the docblock belongs to the member tree.
    file.set_bytes(&mut db).to(
        b"<?php class A { /** other doc */ public function m(int $a, int $b = 0) { if ($a) return 1; } }".to_vec(),
    );
    let _ = body_statement_count(&db, file, m);
    assert_eq!(executions_of(&db.take_executed(), "body_statement_count"), 0);

    // A brace-style edit: the dissolution rule makes it formatting.
    file.set_bytes(&mut db).to(
        b"<?php class A { /** other doc */ public function m(int $a, int $b = 0) { if ($a) { return 1; } } }".to_vec(),
    );
    let _ = body_statement_count(&db, file, m);
    assert_eq!(executions_of(&db.take_executed(), "body_statement_count"), 0);
}
```

Run: `cargo test --package celerrate_semantics --test invalidation_scope`
Expected: PASS. Any failure here is a real cutoff defect from Tasks 1-6: debug it there (the likeliest culprits are an offset leaking into the IR or a nondeterministic collection order), never weaken the assertion.

- [ ] **Step 2: Extend the incremental-consistency harness**

Add to `crates/celerrate_semantics/tests/incremental_consistency.rs` (extend imports with `BodyQuery, body_ir` and `celerrate_semantics::AstId`):

```rust
/// Body IR consistency: every numbered declaration's lowering (bodied
/// or not) must be byte-identical to a from-scratch database's.
fn assert_body_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])]) {
    assert_incremental_consistency_with(
        initial,
        edits,
        &|incremental, file, from_scratch, fresh_file, index| {
            let count = ast_id_map(incremental, file).len();
            assert_eq!(
                count,
                ast_id_map(from_scratch, fresh_file).len(),
                "numbering diverged for file {index}",
            );
            for declaration in 0..u32::try_from(count).unwrap() {
                let body = BodyQuery::new(
                    incremental,
                    AstId { file: file.file_id(incremental), index: declaration },
                );
                let fresh_body = BodyQuery::new(
                    from_scratch,
                    AstId { file: fresh_file.file_id(from_scratch), index: declaration },
                );
                assert_eq!(
                    body_ir(incremental, file, body),
                    body_ir(from_scratch, fresh_file, fresh_body),
                    "body IR diverged for file {index} declaration {declaration}",
                );
            }
        },
    );
}

#[test]
fn body_lowerings_replay_consistently() {
    assert_body_consistency(
        &[b"<?php class A { public function m() { return $this->x?->y(); } } function f() { $g = fn () => 1; }"],
        &[
            (0, b"<?php class A { public function m() { return $this->x?->y(); } } function f() { $g = fn () => 2; }"),
            (0, b"<?php class A { public function m() { /** @var Y $y */ return ($this->x?->y)(); } } function f() { $g = fn () => 2; }"),
            (0, b"<?php class B {} class A { public function m() { return new class { function n() { return 1; } }; } } function f() {}"),
            (0, b"<?php function f() { if (true) { foreach ([1, ...$r] as $k => &$v) { yield $k => $v; } } }"),
            (0, b"<?php function f() { match ($x) { 1, 2 => strlen(...), default => [, $b] = $p } ; }"),
            (0, b"<?php function f() { $x = "),
        ],
    );
}
```

(`SourceFile::file_id` takes the database; the harness closure receives both databases, so both file identities are read from their own database.)

Run: `cargo test --package celerrate_semantics --test incremental_consistency`
Expected: PASS.

- [ ] **Step 3: The adversarial batch pins resilience and arena integrity**

Add to the test module of `body_lowering.rs`:

```rust
    /// Every id stored anywhere in the IR must land inside its arena:
    /// the dense-arena integrity invariant, checked exhaustively.
    fn assert_well_formed(ir: &BodyIr) {
        let expression = |id: &crate::body::ExpressionId| {
            assert!(ir.expression(*id).is_some(), "dangling expression id");
        };
        let optional = |id: &Option<crate::body::ExpressionId>| {
            if let Some(id) = id {
                expression(id);
            }
        };
        let statements = |ids: &[crate::body::StatementId]| {
            for id in ids {
                assert!(ir.statement(*id).is_some(), "dangling statement id");
            }
        };
        statements(&ir.root);
        for annotation in &ir.annotations {
            if let Some(anchor) = annotation.anchor {
                assert!(ir.statement(anchor).is_some(), "dangling annotation anchor");
            }
        }
        for statement in &ir.statements {
            match statement {
                BodyStatement::Missing
                | BodyStatement::Goto { .. }
                | BodyStatement::Label { .. }
                | BodyStatement::Declaration { .. } => {}
                BodyStatement::Expression { expression: id } => expression(id),
                BodyStatement::Block { statements: ids }
                | BodyStatement::Declare { statements: ids } => statements(ids),
                BodyStatement::Return { value } => optional(value),
                BodyStatement::If { condition, then_branch, else_branch } => {
                    expression(condition);
                    statements(then_branch);
                    statements(else_branch);
                }
                BodyStatement::While { condition, body } => {
                    expression(condition);
                    statements(body);
                }
                BodyStatement::DoWhile { body, condition } => {
                    statements(body);
                    expression(condition);
                }
                BodyStatement::For { initializers, conditions, updates, body } => {
                    initializers.iter().for_each(&expression);
                    conditions.iter().for_each(&expression);
                    updates.iter().for_each(&expression);
                    statements(body);
                }
                BodyStatement::Foreach { subject, key, value, body, .. } => {
                    expression(subject);
                    optional(key);
                    expression(value);
                    statements(body);
                }
                BodyStatement::Switch { subject, cases } => {
                    expression(subject);
                    for case in cases {
                        optional(&case.condition);
                        statements(&case.statements);
                    }
                }
                BodyStatement::Try { body: try_body, catches, finally } => {
                    statements(try_body);
                    for catch in catches {
                        statements(&catch.statements);
                    }
                    if let Some(finally) = finally {
                        statements(finally);
                    }
                }
                BodyStatement::Echo { values: ids }
                | BodyStatement::Unset { targets: ids }
                | BodyStatement::Global { targets: ids } => ids.iter().for_each(&expression),
                BodyStatement::StaticVariables { variables } => {
                    for variable in variables {
                        optional(&variable.initializer);
                    }
                }
                BodyStatement::Break { level } | BodyStatement::Continue { level } => {
                    optional(level);
                }
            }
        }
        for lowered in &ir.expressions {
            match lowered {
                BodyExpression::Missing
                | BodyExpression::Literal { .. }
                | BodyExpression::Variable { .. }
                | BodyExpression::NamedReference { .. } => {}
                BodyExpression::DynamicVariable { target: id }
                | BodyExpression::Unary { operand: id, .. }
                | BodyExpression::Postfix { operand: id, .. }
                | BodyExpression::Cast { operand: id, .. }
                | BodyExpression::NullSafeChain { chain: id }
                | BodyExpression::CallableReference { callee: id }
                | BodyExpression::Empty { target: id }
                | BodyExpression::Eval { argument: id }
                | BodyExpression::Print { operand: id }
                | BodyExpression::Clone { operand: id }
                | BodyExpression::Throw { operand: id }
                | BodyExpression::Include { operand: id, .. }
                | BodyExpression::ArrowFunction { body: id, .. } => expression(id),
                BodyExpression::Binary { lhs, rhs, .. } => {
                    expression(lhs);
                    expression(rhs);
                }
                BodyExpression::Assignment { target, value, .. } => {
                    expression(target);
                    expression(value);
                }
                BodyExpression::Ternary { condition, middle, alternative } => {
                    expression(condition);
                    optional(middle);
                    expression(alternative);
                }
                BodyExpression::MemberAccess { receiver, member, .. } => {
                    expression(receiver);
                    if let crate::body::MemberReference::Computed { expression: id } = member {
                        expression(id);
                    }
                }
                BodyExpression::ScopedAccess { subject, member } => {
                    expression(subject);
                    if let crate::body::MemberReference::Computed { expression: id } = member {
                        expression(id);
                    }
                }
                BodyExpression::Call { callee, arguments } => {
                    expression(callee);
                    for argument in arguments {
                        expression(&argument.value);
                    }
                }
                BodyExpression::New { class, arguments } => {
                    if let crate::body::ClassReference::Dynamic { expression: id } = class {
                        expression(id);
                    }
                    for argument in arguments {
                        expression(&argument.value);
                    }
                }
                BodyExpression::Index { subject, index } => {
                    expression(subject);
                    optional(index);
                }
                BodyExpression::Array { entries } => {
                    for entry in entries {
                        if let ArrayEntry::Element { key, value, .. } = entry {
                            optional(key);
                            expression(value);
                        }
                    }
                }
                BodyExpression::InterpolatedString { parts }
                | BodyExpression::ShellExec { parts } => {
                    for part in parts {
                        if let StringPart::Interpolation { expression: id } = part {
                            expression(id);
                        }
                    }
                }
                BodyExpression::Isset { targets } => targets.iter().for_each(&expression),
                BodyExpression::Exit { argument } => optional(argument),
                BodyExpression::Yield { key, value, .. } => {
                    optional(key);
                    optional(value);
                }
                BodyExpression::Match { subject, arms } => {
                    expression(subject);
                    for arm in arms {
                        arm.conditions.iter().for_each(&expression);
                        expression(&arm.body);
                    }
                }
                BodyExpression::Closure { body, .. } => statements(body),
            }
        }
    }

    #[test]
    fn adversarial_inputs_lower_without_failure_and_stay_well_formed() {
        let sources = [
            "<?php function f() { if ( } ",
            "<?php function f() { $a?-> ",
            "<?php function f() { foo(, 1, label:, ...$x ",
            "<?php function f() { match ($x) { 1 => , => 2 } }",
            "<?php function f() { \"unterminated $x ",
            "<?php function f() { [, , &, ...] = $p; }",
            "<?php function f() { new class { function } ; }",
            "<?php function f() { fn () => fn () => function () { yield; }; }",
            "<?php function f() { try { } catch () { } finally }",
            "<?php function f() { $$ ; ${ } ; ->x ; ::y ; }",
        ];
        for source in sources {
            let parse = celerrate_syntax::parse(source);
            let root = parse.tree();
            let map = crate::ast_id::AstIdMap::from_root(&root);
            for node in root.descendants() {
                if let Some((ir, _)) =
                    super::lower_body(celerrate_source::FileId::new(0), &map, &node)
                {
                    assert_well_formed(&ir);
                }
            }
        }
    }
```

Run: `cargo test --package celerrate_semantics body_lowering`
Expected: PASS.

- [ ] **Step 4: Documentation catch-up**

Extend the crate documentation in `crates/celerrate_semantics/src/lib.rs`: after the sentence about the item tree, add:

```rust
//! One level down, the body IR (`body_ir`) lowers each function or
//! method body into a range-free arena behind the same split: spans
//! reconcile late through `body_source_map`, and only code plus
//! recognized annotation content invalidates body consumers.
```

Verify all exports read cleanly (one `pub use body::{...}` block, alphabetical).

- [ ] **Step 5: Final verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
git add -A
git commit -m "✅ test(semantics): pin the body-level invalidation contract"
```

Cross-check against the spec before closing (the task is not done until each answer is yes):
- Range-free arena, densely numbered, per-body query? (section 2)
- Separate source-map query, late reconciliation? (section 2)
- `?->` whole-chain short-circuit structure and first-class callables in the lowering table? (section 2)
- Comment-only edit class redefined as trivia no annotation reader consumes, with the recognized set carried in the IR? (section 2)
- Invalidation-scope tests over the new edit classes? (section 10, item 2)
- Property-hook deferral recorded in the module documentation? (this plan's scope decision)

---

## Execution notes

- Tasks are strictly ordered: 2 needs 1's walk, 4 needs 3's operand lowering, 5 closes the enum, 6 needs the full walk, 7 needs everything.
- If an AST accessor behaves differently from a test's assumption (fragment boundaries, wreckage shapes), adjust the asserted *values* to the parser's actual output while keeping the asserted *structure*; if a structural assumption fails (an accessor missing entirely), stop and re-check the grammar in `crates/celerrate_syntax/php.ungram` before changing the model.
- Never weaken an invalidation-scope assertion to make it pass: those tests are the deliverable.
