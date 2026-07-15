# Type Engine 5 — Inference Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The demand-driven inference engine: one per-body query
producing the expression type table, locals by assignment propagation,
the complete narrowing floor, inferred returns, the interprocedural
fixpoint under the join-ascent discipline (budget below salsa's panic
cap, deterministic widening to `mixed`), and the typed edge-count
instrument. Design source:
`.claude/superpowers/specs/2026-07-14-type-engine-design.md`, sections 6
(the whole section), 2 (the body IR contract inference consumes), 9
(the instrument), 10 (harnesses 2 and 3), and 11 item 8 (this plan).

**Architecture:** Inference lives in `celerrate_types` (the parent
spec's layer for "inference and the type system"), consuming the body
IR from `celerrate_semantics` — it never touches a syntax tree, so LRU
eviction safety is structural. Two queries: `inferred_body_types`, the
per-body flow walk producing an `Eq`-comparable `InferredBody`
(expression types, the joined return, pure interprocedural edge
counts), and `inferred_function_return`, the small resident projection
that is the fixpoint's currency — the **first salsa cycle recovery in
the workspace** (`cycle_fn`/`cycle_initial`), with monotone ascent
forced structurally by joining each iterate with the previous
approximation. Narrowing is environment-to-environment transformation
(`branch_environments`), which makes `!`/`&&`/`||` distribution
compositional instead of an algebra of refinement records. Call typing
follows the precedence **dynamic provider claim, then declared return,
then inferred return**, counting each edge class as pure data in the
query result.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), salsa 0.27.2
(interned `TypeId`, tracked queries, `cycle_fn`/`cycle_initial`
fixpoint recovery, `salsa::Cancelled`), the existing body IR
(`celerrate_semantics::body`), the declared-type layer of plan 3.

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
  No indexing: `.get()`, `.first()`, iterators, `.split_once()`.
  The fixpoint budget exists precisely because salsa panics at
  `MAX_ITERATIONS = 200` iterations
  (`salsa-0.27.2/src/cycle.rs`) — reaching that panic is a
  zero-panic breach.
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **Inference never touches a syntax tree** (design section 2): the
  walker consumes `BodyIr` and the member/declared queries only. No
  `celerrate_db::parse` call anywhere in the new modules, no
  `TextRange` in any inference result.
- **Determinism**: no wall clock, no randomness, no environment reads
  inside queries. Iteration over environments is `BTreeMap` order;
  canonical type ordering is the lattice's structural order, never
  interner handles. Edge counts are **pure data in the query result**,
  never process-global counters mutated inside a query (the workspace
  counter convention: atomics live at the orchestration layer only).
- **Conservative silence, never a guess**: `mixed` is the answer to
  everything inference cannot know (unresolved callees, dynamic
  receivers, iteration typing before plan 6). A wrong precise type is
  a future false positive; `mixed` is silence.
- **Every narrowing form is covered by tests before it can influence a
  published diagnostic** (design section 6): each form lands with its
  own test in the same task.
- **No diagnostics ship from this plan**: no new `CEL####` identifier,
  no rendering change, no CLI surface change. Plan 8 consumes the type
  tables.
- **Strict layering**: one new dependency edge, DAG-legal and
  downward: `celerrate_types` → `celerrate_syntax` (the body IR's
  operator vocabulary is `SyntaxKind`; consuming `BodyExpression`
  requires it). The bridge and the one-dependency rule are untouched;
  `cargo xtask dependency-shape` stays green.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.

## Fixed decisions (the header the tasks implement)

1. **Two queries, one result struct.** `inferred_body_types(db, files,
   stubs, configuration, file, body)` is the flow walk over one body:
   `#[salsa::tracked(returns(ref))]`, answering
   `Option<InferredBody>` (`None` when the identity carries no body,
   mirroring `body_ir`). `InferredBody { expression_types:
   Vec<TypeId>, return_type: TypeId, edge_counts:
   InterproceduralEdgeCounts }` derives `Eq`: a body edit that leaves
   every inferred result identical backdates, and dependents are
   spared. `inferred_function_return(db, files, stubs, configuration,
   query: FunctionQuery)` is the projection of `return_type` — small,
   resident, the fixpoint's currency. The input quartet `(files,
   stubs, configuration)` threads through both, the workspace's
   resolution-context convention.

2. **The fixpoint discipline** (design section 6, verbatim
   requirements). `inferred_function_return` opts into salsa cycle
   recovery: `#[salsa::tracked(cycle_fn = return_cycle_recover,
   cycle_initial = return_cycle_initial)]`. The initial value is
   `TypeId::never` (the lattice bottom). The recovery function is a
   thin shim over a pure, unit-testable helper `ascend(db, iteration,
   last_provisional, computed)`: it **joins the computed iterate with
   the previous approximation** (`widening::join` — monotone ascent
   forced structurally, so oscillation between two values is
   impossible and every entry point converges to the same fixpoint),
   and past `FIXPOINT_ITERATION_BUDGET` (32, pinned by a test to stay
   below salsa's 200 panic cap) an unconverged value widens
   deterministically to `mixed` — the bailout, never the panic, never
   an error. Termination inside the budget comes from the lattice's
   own caps (`UNION_ARITY_CAP`, `STRUCTURAL_DEPTH_CAP`, literal
   widening under `join`), which are deterministic and entry-point
   independent by construction.

3. **Call typing precedence: provider claim, then declared return,
   then inferred return.** A dynamic type provider that claims the
   resolved callee (`SymbolClaim::Function`/`Method`) answers first;
   its contribution is **widened at the consumption boundary**
   (`widening::capped_child` — a plugin never controls termination).
   Failing a claim or a `None` answer, the declared return applies
   when one is actually declared; failing that, the inferred return
   (free functions only in this plan). Each taken edge increments one
   of `declared_return_edges` / `inferred_return_edges` /
   `provider_edges` in `InterproceduralEdgeCounts` — the design's
   residual instrument ("how many results depend on *inferred*
   returns"), shipped as pure data from this plan on.

4. **The declared-present gate is `value_trust != Trust::NativeOnly ||
   !value_type.is_mixed(db)`.** `DeclaredSignature` does not record
   whether a native return text existed; an untyped return resolves to
   `(mixed, NativeOnly)`. Consequence, accepted and recorded: an
   explicit native `: mixed` return is indistinguishable and falls
   through to inference — sound (every inference result is a subtype
   of `mixed`) and immaterial to results; it changes only invalidation
   shape (callers of such a function depend on its body).

5. **The plan-5/plan-6 boundary.** Method bodies are inferred with
   `$this` and `self::`/`static::` receivers typed as **the defining
   class** (no late-static-binding placeholders in inference results
   yet; plan 6 re-keys trait methods, carries the symbolic
   placeholders, and substitutes at call sites — replacing decision 6
   below). Method calls take provider or declared returns, never
   method-inferred returns (the `FunctionQuery` fixpoint covers free
   functions only). `foreach` values and keys are `mixed` (iteration
   typing is plan 6). Template variables in declared returns flow
   through unsubstituted (call-site solving is plan 6). `parent::`
   resolves through the linearized ancestry's first `Extends` edge.

6. **Placeholder substitution at call sites, minimal form.** A
   declared return containing `SelfPlaceholder` or `StaticPlaceholder`
   (the `@return $this` fluent idiom, everywhere in Symfony builders)
   substitutes **the receiver's class type**, at the top level and one
   level into unions; `ParentPlaceholder` answers `mixed`. This is
   scaffolding plan 6 replaces with the forwarding model; without it
   every fluent chain types as a placeholder and the tables are
   useless to plan 8.

7. **Narrowing is environment transformation.**
   `branch_environments(condition, env) -> (true_env, false_env)`
   types the condition (side effects included: `if (($x = f())
   instanceof Foo)` binds before narrowing) and answers the two
   refined environments. Composition is structural recursion: `!`
   swaps, `&&` chains the true side and `join_any`s the false sides,
   `||` is the dual. The floor forms all reduce to four pure leaf
   transformations in `narrowing.rs`: `narrow_to`, `remove_type`,
   `remove_falsy`, `keep_falsy` (each taking the quartet where the
   subtype judgment is needed), plus `TypeId::without_null` from the
   lattice.

8. **Narrowing subjects** are `NarrowingSubject::Local { name }`,
   `ThisProperty { name }` (`$this->prop`), and `StaticProperty {
   name }` (`self::$prop` / `static::$prop` on the defining class),
   per the design's stable-base rule. `subject_of` also sees through
   `Assignment` to its target, so an assign-and-test condition narrows
   the assigned subject.

9. **The environment discipline.** `Environment` is a
   `BTreeMap<NarrowingSubject, TypeId>` plus a `reachable` flag. A
   subject absent from the map reads as its **wide type** (`mixed`
   for locals; the declared property type for property subjects), so
   absence is always silence. `join` is pointwise over the key union
   with the absent side contributing the wide type; an unreachable
   side contributes nothing (which is exactly how early returns narrow
   the code after an `if`). Loops run an **inner fixpoint** with
   `LOOP_ITERATION_BUDGET` (4) join-ascent passes, then one final
   recording pass from the converged environment; exhaustion widens
   the still-changing bindings to `mixed` — the interprocedural
   discipline in miniature, deterministic by the same argument.
   `break`/`continue`/`goto` mark the current path unreachable
   (conservative: their bindings are dropped, and dropped means
   `mixed`, which is silence). A `Label` statement clears the
   environment (any `goto` may land there).

10. **The conservative kill rule** (design section 6, verbatim
    motivation): any call expression (`Call`, `New`), closure or arrow
    function creation, `yield` (the body suspends), `eval`, `include`,
    and shell-exec **kill all property-subject bindings**. Passing a
    variable to a by-reference parameter rebinds it to the parameter's
    declared type (the general write-back rule; the stdlib provider
    refines further in plan 7) and thereby invalidates its narrowing.
    A by-reference closure `use` degrades the captured local to
    `mixed` on both sides of the closure boundary; a by-reference
    assignment (`$b = &$a`) degrades both sides to `mixed` (aliased
    locals are unknowable without alias analysis). A call to `extract`
    drops every `Local` binding.

11. **Union and intersection receivers** (member reads and method
    calls): a union receiver answers the **join over its non-null
    constituents'** member answers; any constituent that is `mixed`,
    `object`, or an unresolvable class makes the whole read `mixed`
    (silence). An intersection receiver answers the join over the
    intersectands that resolve the member. A `mixed` or dynamic
    receiver is silent (`mixed`). No diagnostic is at stake here (plan
    8), only table precision.

12. **Anonymous classes type as `mixed`** in this plan: `new class
    { }` has a synthetic `AstId` identity but no folded symbol key, so
    member resolution cannot address it. Recorded debt toward plan 6/8
    (the member boundary already projects their members; only the
    expression-to-key path is missing).

13. **Typed counters are pure data; the printed line lands with the
    first consumer.** The workspace rule is that counters never live
    inside queries (`CacheStatistics` atomics are incremented at the
    CLI orchestration layer only). Nothing in the CLI demands
    inference until plan 8, so plan 5 ships the instrument as
    `InferredBody.edge_counts` and plan 8/9a extend the
    `CELERRATE_CACHE_STATS` rendering when the orchestration layer
    first aggregates it. Deviation from a literal reading of design
    section 9 ("grows typed counters" at plan 5), recorded here: the
    instrument exists and is tested from this plan; the printed line
    would render constant zeros until plan 8.

14. **The LRU lever is documented, not pulled.** Salsa 0.27 supports
    `lru = N` on tracked functions and `inferred_body_types` (with
    `body_ir`) is the design's named candidate, but no `lru` exists
    anywhere in the workspace yet, the interaction with provisional
    cycle memos is unmeasured, and plan 9b owns the peak-memory number
    against its budget. The query's rustdoc names the lever and its
    owner; setting a capacity here would be a number invented without
    the measurement.

15. **Cancellation-mid-fixpoint is pinned with a blocking test
    provider.** A test-only dynamic type provider rendezvouses on
    barriers inside a self-recursive fixpoint; the main thread
    triggers a pending write, confirms the cancellation flag by
    catching `salsa::Cancelled` on its own handle, releases the
    provider, and asserts the worker unwound with `Cancelled` and that
    a fresh demand answers the post-edit fixpoint — no provisional
    value served. (`salsa::Cancelled::catch` is the public API; the
    CLI's `analysis.rs` guard is the in-tree precedent.)

16. **Display-based assertions follow the canonical rendering.**
    Tests assert inferred types through `TypeId::display` strings
    (class names as folded keys — the plan-2 rendering debt,
    unchanged). Union rendering follows the structural order (rank,
    then name). The expected strings in this plan are written against
    that convention; if a display assertion fails only on constituent
    order or on the exact spelling of a form (a literal's quoting, a
    range's brackets), fix the expectation to `display.rs`'s actual
    rendering, never the code — the asserted *type* is the contract,
    the string is the probe.

17. **Assertion-tag scope.** `MemberAnnotations.assertions`
    (`ParsedAssertion { subject, asserted, polarity, negated }`,
    carried since plan 3 "for plan 5's narrowing") applies at call
    sites: subjects of the form `$name` map through the declared
    parameter list to the argument's narrowing subject; subjects of
    the form `$this->name` map to the caller's `ThisProperty` subject.
    Any other subject shape is ignored (recorded). `Always` applies
    after the call; `IfTrue`/`IfFalse` apply when the call is a
    condition. The real tag grammar is the bridge's (plan 4b); the
    tests here inject a fake `TypeSyntax`, proving the layer against
    the trait.

## File structure

```
crates/celerrate_types/src/inference.rs        NEW: InferredBody, edge counts, body_owner, the two queries, the fixpoint
crates/celerrate_types/src/flow.rs             NEW: FlowContext, Environment, the walker, branch_environments
crates/celerrate_types/src/narrowing.rs        NEW: NarrowingSubject, subject_of, the four leaf transformations, the is_* table
crates/celerrate_types/src/operators.rs        NEW: literal/operator/cast/index typing, pure TypeId functions
crates/celerrate_types/src/lib.rs              MODIFY: modules + re-exports
crates/celerrate_types/tests/fixpoint.rs       NEW: determinism, entry-point independence, budget, cancellation
crates/celerrate_types/tests/invalidation_scope.rs  MODIFY: the new edit-class probes
```

`flow.rs` will be the largest module (the walker); it stays one file
because the walker's statement and expression arms change together —
the `declared.rs` precedent. `narrowing.rs` and `operators.rs` are
pure functions over `TypeId`, testable without a walker.

---
### Task 1: The inference vocabulary and the per-body query skeleton

`InferredBody`, the edge-count instrument, the `body_owner` context
resolution, and `inferred_body_types` answering an all-`mixed` table
sized to the arena — the surface every later task fills in. The
`celerrate_syntax` dependency edge lands here too.

**Files:**
- Create: `crates/celerrate_types/src/inference.rs`
- Modify: `crates/celerrate_types/src/lib.rs`
- Modify: `crates/celerrate_types/Cargo.toml`

**Interfaces:**
- Consumes: `celerrate_semantics::{AstId, BodyQuery, ExpressionId,
  FreeFunction, Member, MemberKind, MemberTree, body_ir, member_tree,
  fully_qualified_name, folded_symbol_key, SymbolSpace}`,
  `celerrate_db::{AnalyzedFileSet, SourceFile}`,
  `celerrate_project::ProjectConfiguration`,
  `celerrate_stubs::StubIndexInput`, `crate::representation::TypeId`.
- Produces: `InterproceduralEdgeCounts { declared_return_edges: u32,
  inferred_return_edges: u32, provider_edges: u32 }`,
  `InferredBody<'db> { expression_types: Vec<TypeId<'db>>,
  return_type: TypeId<'db>, edge_counts: InterproceduralEdgeCounts }`
  with `expression_type(&self, id: ExpressionId) ->
  Option<TypeId<'db>>`, `#[salsa::tracked(returns(ref))] pub fn
  inferred_body_types<'db>(db, files: AnalyzedFileSet, stubs:
  StubIndexInput, configuration: ProjectConfiguration, file:
  SourceFile, body: BodyQuery<'db>) -> Option<InferredBody<'db>>`, and
  `pub(crate) enum BodyOwner { Function(FreeFunction), Method {
  class_key: Option<String>, namespace: String, member: Member } }`
  with the tracked per-body projection
  `#[salsa::tracked(returns(ref))] pub(crate) fn body_owner<'db>(db,
  file: SourceFile, body: BodyQuery<'db>) -> Option<BodyOwner>`.
  Every later task consumes these; Task 10 adds
  `inferred_function_return` beside them.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_types/src/inference.rs` with the test module
first (the module body comes in Step 3). The fixture is the
`declared.rs` test pattern with the file handles kept (recorded debt:
no shared test-support module exists).

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{AstId, BodyQuery, body_ir};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{BodyOwner, body_owner, inferred_body_types};

    struct Fixture {
        db: TestDatabase,
        handles: Vec<SourceFile>,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    fn fixture(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles.clone());
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Fixture {
            db,
            handles,
            files,
            stubs,
            configuration,
        }
    }

    /// The body of the declaration numbered `index` in file 0.
    fn body_query(fixture: &Fixture, index: u32) -> BodyQuery<'_> {
        BodyQuery::new(
            &fixture.db,
            AstId {
                file: FileId::new(0),
                index,
            },
        )
    }

    #[test]
    fn the_query_answers_a_table_sized_to_the_body_arena() {
        let fixture = fixture(&["<?php function f() { return 1 + 2; }"]);
        let file = fixture.handles[0];
        let body = body_query(&fixture, 0);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        let ir = body_ir(&fixture.db, file, body).as_ref().unwrap();
        assert_eq!(inferred.expression_types.len(), ir.expressions.len());
        assert!(!ir.expressions.is_empty(), "the fixture has expressions");
    }

    #[test]
    fn a_non_body_identity_answers_none() {
        let fixture = fixture(&["<?php class A { public function m() {} }"]);
        let file = fixture.handles[0];
        // Numbering: class = 0 (no body), method = 1 (the 1a contract).
        let class = body_query(&fixture, 0);
        assert!(
            inferred_body_types(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                file,
                class,
            )
            .is_none()
        );
    }

    #[test]
    fn body_owner_resolves_free_functions_and_methods() {
        let fixture = fixture(&[
            "<?php namespace App; function f() {} class A { public function m() {} }",
        ]);
        let file = fixture.handles[0];
        let function = body_owner(&fixture.db, file, body_query(&fixture, 0))
            .clone()
            .unwrap();
        let BodyOwner::Function(free_function) = function else {
            panic!("expected a free function owner");
        };
        assert_eq!(free_function.name, "f");
        assert_eq!(free_function.namespace, "App");

        let method = body_owner(&fixture.db, file, body_query(&fixture, 2))
            .clone()
            .unwrap();
        let BodyOwner::Method {
            class_key, member, ..
        } = method
        else {
            panic!("expected a method owner");
        };
        assert_eq!(class_key.as_deref(), Some("app\\a"));
        assert_eq!(member.name, "m");
    }

    #[test]
    fn an_anonymous_class_method_owner_has_no_key() {
        let fixture = fixture(&[
            "<?php function wrapper() { return new class { public function m() {} }; }",
        ]);
        let file = fixture.handles[0];
        // Numbering: wrapper = 0, anonymous class = 1, method = 2.
        let owner = body_owner(&fixture.db, file, body_query(&fixture, 2))
            .clone()
            .unwrap();
        let BodyOwner::Method { class_key, .. } = owner else {
            panic!("expected a method owner");
        };
        assert!(class_key.is_none());
    }
}
```

Register the module in `crates/celerrate_types/src/lib.rs`: add
`mod inference;` (alphabetical, between `mod dynamic_type_provider;`
and `mod judgments;`) and the re-export block (alphabetical among the
`pub use` items):

```rust
pub use inference::{InferredBody, InterproceduralEdgeCounts, inferred_body_types};
```

Add the dependency in `crates/celerrate_types/Cargo.toml`
(alphabetical among the path dependencies):

```toml
celerrate_syntax = { path = "../celerrate_syntax" }
```

(No code uses it until Task 2; adding it with the crate skeleton keeps
the manifest change reviewed once.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -5`
Expected: FAIL to compile — `InferredBody`, `inferred_body_types`,
`BodyOwner`, `body_owner` are not defined.

- [ ] **Step 3: Implement the skeleton**

Fill the module body above the test module in
`crates/celerrate_types/src/inference.rs`:

```rust
//! Demand-driven inference: the per-body flow walk and (Task 10) the
//! interprocedural fixpoint. One query per body produces the full
//! expression type table plus the joined return type; nothing here
//! ever touches a syntax tree — the walker consumes the range-free
//! body IR, so LRU eviction of parse trees is structurally safe.
//!
//! Memory lever, named for plan 9b (design section 6): the full
//! expression type tables produced by [`inferred_body_types`] are the
//! LRU candidates (`salsa` supports `lru = N` on tracked functions),
//! while inferred returns stay resident — small, hot, and the
//! fixpoint's currency. No capacity is set here: plan 9b owns the
//! peak-memory measurement against its budget and pulls the lever
//! with a number.

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    AstId, BodyQuery, ExpressionId, FreeFunction, Member, MemberKind, SymbolSpace, body_ir,
    folded_symbol_key, fully_qualified_name, member_tree,
};
use celerrate_stubs::StubIndexInput;

use crate::representation::TypeId;

/// The interprocedural edge classes one body's inference took, as
/// pure data: the design's residual instrument ("how many results
/// depend on *inferred* returns"), aggregated by the first
/// orchestration-layer consumer (plan 8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, salsa::Update)]
pub struct InterproceduralEdgeCounts {
    /// Call results taken from a declared (native or annotated) return.
    pub declared_return_edges: u32,
    /// Call results taken from another body's inferred return.
    pub inferred_return_edges: u32,
    /// Call results taken from a dynamic type provider's claim.
    pub provider_edges: u32,
}

/// The inference result of one body: a type per arena expression, the
/// joined return type, and the edge-count instrument. `Eq`-comparable
/// on purpose: a body edit that leaves every inferred result identical
/// backdates, and dependents are spared.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct InferredBody<'db> {
    pub expression_types: Vec<TypeId<'db>>,
    pub return_type: TypeId<'db>,
    pub edge_counts: InterproceduralEdgeCounts,
}

impl<'db> InferredBody<'db> {
    pub fn expression_type(&self, id: ExpressionId) -> Option<TypeId<'db>> {
        self.expression_types.get(id.index() as usize).copied()
    }
}

/// The declaration a body belongs to: a free function, or a method of
/// a class-like (whose folded key is `None` for an anonymous class —
/// decision 12: no folded symbol key exists to resolve members
/// against). `Eq` so the tracked projection backdates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyOwner {
    Function(FreeFunction),
    Method {
        class_key: Option<String>,
        namespace: String,
        member: Member,
    },
}

/// Resolves the owning declaration of one body through the member
/// projection. `None` when the identity names no function or method
/// of `file`. A tracked query on purpose, and load-bearing for the
/// invalidation story: `member_tree` changes whenever *any* member of
/// the file changes, but this per-body projection backdates for every
/// body whose own declaration did not — so editing one signature
/// re-infers that member's body and no other (the design's harness-2
/// contract, pinned in Task 12).
#[salsa::tracked(returns(ref))]
pub(crate) fn body_owner<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<BodyOwner> {
    let ast_id = body.ast_id(db);
    let tree = member_tree(db, file);
    if let Some(function) = tree
        .functions
        .iter()
        .find(|function| function.ast_id == ast_id)
    {
        return Some(BodyOwner::Function(function.clone()));
    }
    for class in &tree.classes {
        let Some(member) = class.members.iter().find(|member| member.ast_id == ast_id) else {
            continue;
        };
        if member.kind != MemberKind::Method {
            return None;
        }
        let class_key = class.name.as_deref().map(|name| {
            folded_symbol_key(
                SymbolSpace::ClassLike,
                &fully_qualified_name(&class.namespace, name),
            )
        });
        return Some(BodyOwner::Method {
            class_key,
            namespace: class.namespace.clone(),
            member: member.clone(),
        });
    }
    None
}

/// The inference of one body: `None` when the identity carries no
/// body in `file` (mirroring `body_ir`). Task 3 replaces the
/// all-`mixed` table with the flow walk.
#[salsa::tracked(returns(ref))]
pub fn inferred_body_types<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<InferredBody<'db>> {
    let _ = (files, stubs, configuration);
    let ir = body_ir(db, file, body).as_ref()?;
    let mixed = TypeId::mixed(db);
    Some(InferredBody {
        expression_types: vec![mixed; ir.expressions.len()],
        return_type: mixed,
        edge_counts: InterproceduralEdgeCounts::default(),
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types inference` — PASS (4 tests).
Then: `cargo test --workspace 2>&1 | tail -3` — PASS;
`cargo clippy --workspace --all-targets -- -D warnings`;
`cargo fmt --all`; `cargo xtask dependency-shape`.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/inference.rs crates/celerrate_types/src/lib.rs crates/celerrate_types/Cargo.toml
git commit -m "✨ feat(types): the inference vocabulary and the per-body query skeleton"
```

---

### Task 2: Expression atoms and operators

`operators.rs`: pure `TypeId -> TypeId` typing rules for literals,
named atoms, casts, unary/postfix/binary operators, and index reads —
testable without a walker, consumed by Task 3's walker. The rules are
deliberately fold-free (no constant arithmetic): general result types,
with one exception — unary minus on an integer literal, because
negative literals feed `match`/`===` narrowing.

**Files:**
- Create: `crates/celerrate_types/src/operators.rs`
- Modify: `crates/celerrate_types/src/lib.rs`

**Interfaces:**
- Consumes: `celerrate_syntax::SyntaxKind` (the operator vocabulary
  the body IR carries), `crate::representation::TypeId`,
  `crate::widening::join`.
- Produces (all `pub(crate)`, all in `operators.rs`):
  `literal_type(db, text: &str) -> TypeId`,
  `named_reference_type(db, text: &str) -> Option<TypeId>` (`true`,
  `false`, `null`; `None` means "an ordinary constant fetch" — the
  caller answers `mixed`), `cast_type(db, operator: SyntaxKind,
  operand: TypeId) -> TypeId`, `unary_type(db, operator: SyntaxKind,
  operand: TypeId) -> TypeId`, `postfix_type(db, operand: TypeId) ->
  TypeId`, `binary_type(db, operator: SyntaxKind, lhs: TypeId, rhs:
  TypeId) -> TypeId`, `index_type(db, subject: TypeId, index:
  Option<TypeId>) -> TypeId`. Task 3's walker consumes every one of
  these.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_types/src/operators.rs` with the test module
first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_syntax::SyntaxKind;

    use super::*;
    use crate::representation::{ShapeField, ShapeKey, TypeId};

    #[test]
    fn literals_type_as_their_literal_forms() {
        let db = TestDatabase::default();
        assert_eq!(literal_type(&db, "42"), TypeId::int_literal(&db, 42));
        assert_eq!(literal_type(&db, "1_000"), TypeId::int_literal(&db, 1000));
        assert_eq!(literal_type(&db, "0x10"), TypeId::int_literal(&db, 16));
        assert_eq!(literal_type(&db, "0o17"), TypeId::int_literal(&db, 15));
        assert_eq!(literal_type(&db, "017"), TypeId::int_literal(&db, 15));
        assert_eq!(literal_type(&db, "0b101"), TypeId::int_literal(&db, 5));
        assert_eq!(literal_type(&db, "1.5"), TypeId::float_literal(&db, 1.5));
        assert_eq!(literal_type(&db, "1e3"), TypeId::float_literal(&db, 1000.0));
        assert_eq!(
            literal_type(&db, r"'it\''"),
            TypeId::string_literal(&db, "it'"),
            "escaped quote unescapes",
        );
        assert_eq!(
            literal_type(&db, r"'a\\b'"),
            TypeId::string_literal(&db, r"a\b"),
        );
        // An integer literal PHP overflows to float types as float.
        assert_eq!(
            literal_type(&db, "99999999999999999999"),
            TypeId::float(&db),
        );
    }

    #[test]
    fn named_atoms_type_and_constants_stay_unknown() {
        let db = TestDatabase::default();
        assert_eq!(
            named_reference_type(&db, "true"),
            Some(TypeId::bool_literal(&db, true)),
        );
        assert_eq!(
            named_reference_type(&db, "FALSE"),
            Some(TypeId::bool_literal(&db, false)),
        );
        assert_eq!(named_reference_type(&db, "Null"), Some(TypeId::null(&db)));
        assert_eq!(named_reference_type(&db, "PHP_EOL"), None);
    }

    #[test]
    fn casts_type_by_their_operator() {
        let db = TestDatabase::default();
        let mixed = TypeId::mixed(&db);
        assert_eq!(cast_type(&db, SyntaxKind::IntCast, mixed), TypeId::int(&db));
        assert_eq!(cast_type(&db, SyntaxKind::BoolCast, mixed), TypeId::bool(&db));
        assert_eq!(
            cast_type(&db, SyntaxKind::FloatCast, mixed),
            TypeId::float(&db),
        );
        assert_eq!(
            cast_type(&db, SyntaxKind::StringCast, mixed),
            TypeId::string(&db),
        );
        assert_eq!(
            cast_type(&db, SyntaxKind::BinaryCast, mixed),
            TypeId::string(&db),
        );
        assert_eq!(
            cast_type(&db, SyntaxKind::ObjectCast, mixed),
            TypeId::object(&db),
        );
        // (array) on an array keeps it; on anything else, general array.
        let list = TypeId::list(&db, TypeId::int(&db));
        assert_eq!(cast_type(&db, SyntaxKind::ArrayCast, list), list);
        assert_eq!(
            cast_type(&db, SyntaxKind::ArrayCast, mixed),
            TypeId::array(
                &db,
                TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
                TypeId::mixed(&db),
            ),
        );
    }

    #[test]
    fn unary_and_postfix_operators_type() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let float = TypeId::float(&db);
        let mixed = TypeId::mixed(&db);
        let int_or_float = TypeId::union(&db, [int, float]);
        assert_eq!(unary_type(&db, SyntaxKind::Bang, mixed), TypeId::bool(&db));
        assert_eq!(unary_type(&db, SyntaxKind::Tilde, mixed), int);
        assert_eq!(unary_type(&db, SyntaxKind::Minus, int), int);
        assert_eq!(unary_type(&db, SyntaxKind::Minus, float), float);
        assert_eq!(unary_type(&db, SyntaxKind::Minus, mixed), int_or_float);
        assert_eq!(
            unary_type(&db, SyntaxKind::Minus, TypeId::int_literal(&db, 1)),
            TypeId::int_literal(&db, -1),
            "negative literals feed narrowing",
        );
        // `@$x` keeps the operand's type; `&$x` too.
        assert_eq!(unary_type(&db, SyntaxKind::At, float), float);
        assert_eq!(postfix_type(&db, int), int);
        assert_eq!(postfix_type(&db, mixed), int_or_float);
    }

    #[test]
    fn binary_operators_type_by_the_table() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let float = TypeId::float(&db);
        let string = TypeId::string(&db);
        let mixed = TypeId::mixed(&db);
        let bool_type = TypeId::bool(&db);
        let int_or_float = TypeId::union(&db, [int, float]);

        assert_eq!(binary_type(&db, SyntaxKind::Plus, int, int), int);
        assert_eq!(binary_type(&db, SyntaxKind::Plus, int, float), float);
        assert_eq!(binary_type(&db, SyntaxKind::Star, mixed, int), int_or_float);
        assert_eq!(binary_type(&db, SyntaxKind::Slash, int, int), int_or_float);
        assert_eq!(binary_type(&db, SyntaxKind::Slash, float, int), float);
        assert_eq!(binary_type(&db, SyntaxKind::Percent, mixed, mixed), int);
        assert_eq!(
            binary_type(&db, SyntaxKind::StarStar, int, int),
            int_or_float,
            "exponentiation overflows to float",
        );
        // `+` on two arrays is the array-union operator.
        let left = TypeId::list(&db, int);
        let right = TypeId::list(&db, string);
        assert_eq!(
            binary_type(&db, SyntaxKind::Plus, left, right),
            crate::widening::join(&db, left, right),
        );
        assert_eq!(binary_type(&db, SyntaxKind::Dot, mixed, mixed), string);
        assert_eq!(
            binary_type(&db, SyntaxKind::EqualsEqualsEquals, mixed, mixed),
            bool_type,
        );
        assert_eq!(binary_type(&db, SyntaxKind::Less, mixed, mixed), bool_type);
        assert_eq!(
            binary_type(&db, SyntaxKind::Spaceship, mixed, mixed),
            TypeId::int_range(&db, Some(-1), Some(1)),
        );
        assert_eq!(
            binary_type(&db, SyntaxKind::AmpersandAmpersand, mixed, mixed),
            bool_type,
        );
        assert_eq!(binary_type(&db, SyntaxKind::And, mixed, mixed), bool_type);
        assert_eq!(
            binary_type(&db, SyntaxKind::InstanceOf, mixed, mixed),
            bool_type,
        );
        assert_eq!(
            binary_type(&db, SyntaxKind::Ampersand, int, int),
            int,
            "bitwise on integers",
        );
        assert_eq!(binary_type(&db, SyntaxKind::LessLess, mixed, mixed), int);
        // The walker owns `??`; the fallback here is the null-stripped join.
        let nullable_int = TypeId::union(&db, [int, TypeId::null(&db)]);
        assert_eq!(
            binary_type(&db, SyntaxKind::QuestionQuestion, nullable_int, string),
            TypeId::union(&db, [int, string]),
        );
        // Anything unknown answers mixed (the pipe operator, for one).
        assert_eq!(
            binary_type(&db, SyntaxKind::PipeGreater, mixed, mixed),
            mixed,
        );
    }

    #[test]
    fn index_reads_type_by_the_subject() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        let mixed = TypeId::mixed(&db);
        let list = TypeId::list(&db, string);
        assert_eq!(index_type(&db, list, Some(int)), string);
        assert_eq!(index_type(&db, string, Some(int)), string);
        assert_eq!(index_type(&db, mixed, Some(int)), mixed);
        // A shape with a known literal key answers the field.
        let shape = TypeId::shape(
            &db,
            vec![ShapeField {
                key: ShapeKey::String("id".to_owned()),
                optional: false,
                value: int,
            }],
        );
        assert_eq!(
            index_type(&db, shape, Some(TypeId::string_literal(&db, "id"))),
            int,
        );
        assert_eq!(index_type(&db, shape, Some(string)), int, "join of fields");
    }
}
```

Register the module in `crates/celerrate_types/src/lib.rs`: add
`mod operators;` (alphabetical, between `mod ordering;` and
`mod representation;`). Nothing is re-exported: the walker is the only
consumer.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types operators 2>&1 | tail -5`
Expected: FAIL to compile — none of the functions exist.

- [ ] **Step 3: Implement the typing rules**

Fill the module body above the test module:

```rust
//! Pure typing rules for expression atoms and operators: `TypeId` in,
//! `TypeId` out, no environment, no queries beyond the interner. The
//! rules are fold-free (no constant arithmetic — a folded value would
//! be a precision promise plan 8 has to keep); the one exception is
//! unary minus on an integer literal, because negative literals feed
//! `match` and `===` narrowing. `mixed` is the answer to every form
//! the table does not know: silence, never a guess.

use celerrate_syntax::SyntaxKind;

use crate::representation::TypeId;
use crate::widening::join;

/// `int|float`, the numeric fallback of the arithmetic rules.
fn int_or_float<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    TypeId::union(db, [TypeId::int(db), TypeId::float(db)])
}

fn is_int<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.int_bounds(db).is_some()
}

fn is_float<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of == TypeId::float(db) || of.float_literal_value(db).is_some()
}

fn is_array<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.array_value(db).is_some() || of.shape_fields(db).is_some()
}

/// An integer, float, or single-quoted string literal, as written
/// (the body IR's `Literal { text }` contract). An integer form that
/// overflows `i64` types as `float`, PHP's own overflow rule; an
/// unparseable form answers the general scalar.
pub(crate) fn literal_type<'db>(db: &'db dyn salsa::Database, text: &str) -> TypeId<'db> {
    if let Some(quoted) = text.strip_prefix('\'') {
        let inner = quoted.strip_suffix('\'').unwrap_or(quoted);
        let mut value = String::with_capacity(inner.len());
        let mut characters = inner.chars();
        while let Some(character) = characters.next() {
            if character == '\\' {
                match characters.next() {
                    Some('\'') => value.push('\''),
                    Some('\\') => value.push('\\'),
                    Some(other) => {
                        value.push('\\');
                        value.push(other);
                    }
                    None => value.push('\\'),
                }
            } else {
                value.push(character);
            }
        }
        return TypeId::string_literal(db, &value);
    }
    let digits: String = text.chars().filter(|&character| character != '_').collect();
    let float_like = digits.contains('.')
        || ((digits.contains('e') || digits.contains('E'))
            && !digits.starts_with("0x")
            && !digits.starts_with("0X"));
    if float_like {
        return match digits.parse::<f64>() {
            Ok(value) => TypeId::float_literal(db, value),
            Err(_) => TypeId::float(db),
        };
    }
    let parsed = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16)
    } else if let Some(binary) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        i64::from_str_radix(binary, 2)
    } else if let Some(octal) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        i64::from_str_radix(octal, 8)
    } else if digits.len() > 1 && digits.starts_with('0') {
        i64::from_str_radix(digits.get(1..).unwrap_or(""), 8)
    } else {
        digits.parse::<i64>()
    };
    match parsed {
        Ok(value) => TypeId::int_literal(db, value),
        // Overflow (PHP widens to float) and any unparseable residue.
        Err(_) if digits.chars().next().is_some_and(|c| c.is_ascii_digit()) => TypeId::float(db),
        Err(_) => TypeId::int(db),
    }
}

/// `true`, `false`, `null` parse as names (the body IR contract);
/// anything else is an ordinary constant fetch the caller types
/// `mixed`.
pub(crate) fn named_reference_type<'db>(
    db: &'db dyn salsa::Database,
    text: &str,
) -> Option<TypeId<'db>> {
    match text.to_ascii_lowercase().as_str() {
        "true" => Some(TypeId::bool_literal(db, true)),
        "false" => Some(TypeId::bool_literal(db, false)),
        "null" => Some(TypeId::null(db)),
        _ => None,
    }
}

pub(crate) fn cast_type<'db>(
    db: &'db dyn salsa::Database,
    operator: SyntaxKind,
    operand: TypeId<'db>,
) -> TypeId<'db> {
    match operator {
        SyntaxKind::IntCast => TypeId::int(db),
        SyntaxKind::BoolCast => TypeId::bool(db),
        SyntaxKind::FloatCast => TypeId::float(db),
        SyntaxKind::StringCast | SyntaxKind::BinaryCast => TypeId::string(db),
        SyntaxKind::ObjectCast => TypeId::object(db),
        SyntaxKind::ArrayCast => {
            if is_array(db, operand) {
                operand
            } else {
                TypeId::array(
                    db,
                    TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
                    TypeId::mixed(db),
                )
            }
        }
        _ => TypeId::mixed(db),
    }
}

pub(crate) fn unary_type<'db>(
    db: &'db dyn salsa::Database,
    operator: SyntaxKind,
    operand: TypeId<'db>,
) -> TypeId<'db> {
    match operator {
        SyntaxKind::Bang => TypeId::bool(db),
        SyntaxKind::Tilde => TypeId::int(db),
        SyntaxKind::Minus | SyntaxKind::Plus => {
            if operator == SyntaxKind::Minus
                && let Some(value) = operand.int_literal_value(db)
            {
                return TypeId::int_literal(db, value.saturating_neg());
            }
            numeric_preserving(db, operand)
        }
        // `@$x` (error suppression) and `&$x` keep the operand's type.
        SyntaxKind::At | SyntaxKind::Ampersand => operand,
        _ => TypeId::mixed(db),
    }
}

/// `$x++` / `$x--` read the operand before mutation.
pub(crate) fn postfix_type<'db>(
    db: &'db dyn salsa::Database,
    operand: TypeId<'db>,
) -> TypeId<'db> {
    numeric_preserving(db, operand)
}

/// int stays int, float stays float, everything else is `int|float`.
fn numeric_preserving<'db>(db: &'db dyn salsa::Database, operand: TypeId<'db>) -> TypeId<'db> {
    if is_int(db, operand) {
        TypeId::int(db)
    } else if is_float(db, operand) {
        TypeId::float(db)
    } else {
        int_or_float(db)
    }
}

/// Both operands decidedly int (never float)?
fn arithmetic<'db>(
    db: &'db dyn salsa::Database,
    lhs: TypeId<'db>,
    rhs: TypeId<'db>,
) -> TypeId<'db> {
    if is_int(db, lhs) && is_int(db, rhs) {
        TypeId::int(db)
    } else if is_float(db, lhs) || is_float(db, rhs) {
        TypeId::float(db)
    } else {
        int_or_float(db)
    }
}

pub(crate) fn binary_type<'db>(
    db: &'db dyn salsa::Database,
    operator: SyntaxKind,
    lhs: TypeId<'db>,
    rhs: TypeId<'db>,
) -> TypeId<'db> {
    match operator {
        SyntaxKind::Plus if is_array(db, lhs) && is_array(db, rhs) => join(db, lhs, rhs),
        SyntaxKind::Plus | SyntaxKind::Minus | SyntaxKind::Star => arithmetic(db, lhs, rhs),
        SyntaxKind::Slash => {
            if is_float(db, lhs) || is_float(db, rhs) {
                TypeId::float(db)
            } else {
                int_or_float(db)
            }
        }
        // `**` overflows to float even on two ints.
        SyntaxKind::StarStar => {
            if is_float(db, lhs) || is_float(db, rhs) {
                TypeId::float(db)
            } else {
                int_or_float(db)
            }
        }
        SyntaxKind::Percent
        | SyntaxKind::Ampersand
        | SyntaxKind::Pipe
        | SyntaxKind::Caret
        | SyntaxKind::LessLess
        | SyntaxKind::GreaterGreater => TypeId::int(db),
        SyntaxKind::Dot => TypeId::string(db),
        SyntaxKind::EqualsEquals
        | SyntaxKind::BangEquals
        | SyntaxKind::EqualsEqualsEquals
        | SyntaxKind::BangEqualsEquals
        | SyntaxKind::Less
        | SyntaxKind::Greater
        | SyntaxKind::LessEquals
        | SyntaxKind::GreaterEquals
        | SyntaxKind::AmpersandAmpersand
        | SyntaxKind::PipePipe
        | SyntaxKind::And
        | SyntaxKind::Or
        | SyntaxKind::Xor
        | SyntaxKind::InstanceOf => TypeId::bool(db),
        SyntaxKind::Spaceship => TypeId::int_range(db, Some(-1), Some(1)),
        // The walker owns `??` (environment-sensitive); this fallback
        // serves operand positions the walker does not special-case.
        SyntaxKind::QuestionQuestion => join(db, lhs.without_null(db), rhs),
        _ => TypeId::mixed(db),
    }
}

/// An index read: shapes answer their field (or the field join when
/// the key is not a known literal), arrays their value, strings a
/// string; everything else is silence.
pub(crate) fn index_type<'db>(
    db: &'db dyn salsa::Database,
    subject: TypeId<'db>,
    index: Option<TypeId<'db>>,
) -> TypeId<'db> {
    if let Some(fields) = subject.shape_fields(db) {
        let literal_key = index.and_then(|key| {
            key.int_literal_value(db)
                .map(crate::representation::ShapeKey::Integer)
                .or_else(|| {
                    key.string_literal_value(db)
                        .map(crate::representation::ShapeKey::String)
                })
        });
        if let Some(wanted) = literal_key
            && let Some(field) = fields.iter().find(|field| field.key == wanted)
        {
            return field.value;
        }
        return fields
            .iter()
            .map(|field| field.value)
            .reduce(|left, right| join(db, left, right))
            .unwrap_or_else(|| TypeId::mixed(db));
    }
    if let Some(value) = subject.array_value(db) {
        return value;
    }
    if subject == TypeId::string(db) || subject.string_literal_value(db).is_some() {
        return TypeId::string(db);
    }
    TypeId::mixed(db)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types operators` — PASS (6 tests).
Then the full local gate (workspace tests, clippy, fmt).

Note: `is_int` uses `int_bounds` (`Some` for `int`, ranges, and
literals); if a general `bool` sneaks through any helper, that is a
bug in the table, not in the lattice.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/operators.rs crates/celerrate_types/src/lib.rs
git commit -m "✨ feat(types): expression atoms and operators type by the pure table"
```

---
### Task 3: Locals flow by assignment propagation

`flow.rs` (the walker: environment threading, assignment propagation,
branch joins, the loop discipline, divergence tracking) and
`narrowing.rs` (the `NarrowingSubject` vocabulary only — the
transformations are Task 4). `inferred_body_types` starts running the
walk. After this task, returns, locals, branches, loops, try/catch,
and destructuring all type; conditions do not narrow yet.

**Files:**
- Create: `crates/celerrate_types/src/flow.rs`
- Create: `crates/celerrate_types/src/narrowing.rs`
- Modify: `crates/celerrate_types/src/inference.rs`
- Modify: `crates/celerrate_types/src/lib.rs`

**Interfaces:**
- Consumes: Task 1's `InferredBody`/`InterproceduralEdgeCounts`/
  `body_owner`, Task 2's `operators::*`, `celerrate_semantics::
  {ArrayEntry, BodyExpression, BodyIr, BodyStatement, ExpressionId,
  MemberReference, StatementId, UseTables, item_tree}`,
  `crate::declared::{NameSite, qualified_class_name}` (crate-private,
  same crate), `crate::{DeclaredSignature, FunctionQuery, MemberQuery,
  declared_function_signature, declared_member_signature}`,
  `crate::widening::join`.
- Produces: `narrowing.rs`: `pub(crate) enum NarrowingSubject { Local
  { name: String }, ThisProperty { name: String }, StaticProperty {
  name: String } }` (derives `Debug, Clone, PartialEq, Eq, PartialOrd,
  Ord`) and `pub(crate) fn subject_of(ir: &BodyIr, expression:
  ExpressionId) -> Option<NarrowingSubject>`. `flow.rs`:
  `pub(crate) struct FlowContext<'db, 'body> { db, files, stubs,
  configuration, ir, namespace: String, tables: UseTables,
  owner_class_key: Option<String>, method_is_static: bool, parameters:
  Vec<(String, TypeId<'db>)> }`, `pub(crate) struct FlowResult<'db> {
  expression_types: Vec<TypeId<'db>>, return_type: TypeId<'db>,
  edge_counts: InterproceduralEdgeCounts }`, `pub(crate) fn
  walk_body<'db>(context: &FlowContext<'db, '_>) -> FlowResult<'db>`,
  `pub(crate) struct Environment<'db>` with `new / bind / binding /
  remove / clear / mark_unreachable / is_reachable / join / join_any /
  widened_where_changed`, and `pub(crate) const
  LOOP_ITERATION_BUDGET: u32 = 4`. Tasks 4-11 all extend the
  `Walker` in this file.

- [ ] **Step 1: Write the failing tests**

Add to the test module of `crates/celerrate_types/src/inference.rs`
(the fixture from Task 1 is already there). A shared probe:

```rust
    /// The display of the inferred return of declaration `index` in
    /// file 0 — the assertion shape most flow tests use (decision 16).
    fn return_display(fixture: &Fixture, index: u32) -> String {
        let file = fixture.handles[0];
        let body = body_query(fixture, index);
        inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap()
        .return_type
        .display(&fixture.db)
    }

    #[test]
    fn a_literal_return_types_the_body() {
        let fixture = fixture(&["<?php function f() { return 1; }"]);
        assert_eq!(return_display(&fixture, 0), "1");
    }

    #[test]
    fn assignment_propagates_to_a_later_read() {
        let fixture = fixture(&["<?php function f() { $x = 'a'; return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "'a'");
    }

    #[test]
    fn parameters_seed_from_their_declared_types() {
        let fixture = fixture(&["<?php function f(int $x) { return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "int");
    }

    #[test]
    fn a_variadic_parameter_seeds_as_a_list() {
        let fixture = fixture(&["<?php function f(int ...$x) { return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "list<int>");
    }

    #[test]
    fn branches_join_and_one_sided_assignment_is_silence() {
        let fixture = fixture(&[
            "<?php
            function two(bool $c) { if ($c) { $x = 1; } else { $x = 2; } return $x; }
            function one(bool $c) { if ($c) { $y = 1; } return $y; }",
        ]);
        assert_eq!(return_display(&fixture, 0), "1|2");
        // Assigned on one path only: the absent side reads mixed.
        assert_eq!(return_display(&fixture, 1), "mixed");
    }

    #[test]
    fn a_reachable_fall_through_joins_null_and_a_throwing_body_is_never() {
        let fixture = fixture(&[
            "<?php
            function maybe(bool $c) { if ($c) { return 1; } }
            function raises() { throw new \\RuntimeException('boom'); }",
        ]);
        assert_eq!(return_display(&fixture, 0), "1|null");
        assert_eq!(return_display(&fixture, 1), "never");
    }

    #[test]
    fn a_yielding_body_returns_a_generator() {
        let fixture = fixture(&["<?php function f() { yield 1; }"]);
        assert_eq!(return_display(&fixture, 0), "generator");
    }

    #[test]
    fn a_loop_joins_its_passes_and_terminates_deterministically() {
        let fixture = fixture(&[
            "<?php
            function joins(bool $c) { $x = 1; while ($c) { $x = 'a'; } return $x; }
            function grows(bool $c) { $x = 1; while ($c) { $x = [$x]; } return $x; }",
        ]);
        assert_eq!(return_display(&fixture, 0), "1|'a'");
        // The growing case must terminate (budget + caps) and be
        // reproducible; the exact widened form is not the contract.
        let first = return_display(&fixture, 1);
        let again = fixture(&[
            "<?php
            function joins(bool $c) { $x = 1; while ($c) { $x = 'a'; } return $x; }
            function grows(bool $c) { $x = 1; while ($c) { $x = [$x]; } return $x; }",
        ]);
        assert_eq!(first, return_display(&again, 1));
    }

    #[test]
    fn unset_forgets_and_a_catch_variable_types() {
        let fixture = fixture(&[
            "<?php
            function forgets() { $x = 1; unset($x); return $x; }
            function catches() { try { return 1; } catch (\\RuntimeException $e) { return $e; } }",
        ]);
        assert_eq!(return_display(&fixture, 0), "mixed");
        assert_eq!(return_display(&fixture, 1), "1|runtimeexception");
    }

    #[test]
    fn destructuring_binds_element_types() {
        let fixture = fixture(&["<?php function f() { [$a, $b] = [1, 'x']; return $a; }"]);
        assert_eq!(return_display(&fixture, 0), "1");
    }

    #[test]
    fn methods_seed_their_declared_parameters_too() {
        let fixture = fixture(&[
            "<?php class A { public function m(string $s) { return $s; } }",
        ]);
        // Numbering: class = 0, method = 1.
        assert_eq!(return_display(&fixture, 1), "string");
    }
```

Also update the Task 1 test
`the_query_answers_a_table_sized_to_the_body_arena`: keep the size
assertion, and strengthen it — the walk now types the literals:

```rust
        // `1 + 2` types as int now that the walk runs.
        let super::InferredBody { expression_types, .. } = inferred;
        assert!(
            expression_types
                .iter()
                .any(|of| *of == crate::TypeId::int(&fixture.db)),
            "the sum typed as int",
        );
```

(`TypeId` is already re-exported at the crate root.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -8`
Expected: the new tests FAIL — every return currently answers `mixed`
(`a_literal_return_types_the_body` asserts `"1"`, gets `"mixed"`).

- [ ] **Step 3: Create the subject vocabulary**

Create `crates/celerrate_types/src/narrowing.rs`:

```rust
//! Narrowing subjects (design section 6: locals, and property fetches
//! on a stable base — `$this->prop`, `self::$prop`) and, from Task 4
//! on, the pure leaf transformations the condition forms reduce to.

use celerrate_semantics::{BodyExpression, BodyIr, ExpressionId, MemberReference};

/// One narrowable subject. `Ord` because the environment is a
/// `BTreeMap`: deterministic iteration is a determinism invariant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NarrowingSubject {
    Local { name: String },
    /// `$this->name` — never through `?->` (a null-safe fetch is not
    /// a stable base).
    ThisProperty { name: String },
    /// `self::$name` / `static::$name` on the defining class.
    StaticProperty { name: String },
}

/// The narrowing subject of one expression, seeing through
/// `Assignment` to its target so an assign-and-test condition
/// (`if (($x = f()) instanceof Foo)`) narrows the assigned subject.
pub(crate) fn subject_of(ir: &BodyIr, expression: ExpressionId) -> Option<NarrowingSubject> {
    match ir.expression(expression)? {
        BodyExpression::Variable { name } if name != "this" => Some(NarrowingSubject::Local {
            name: name.clone(),
        }),
        BodyExpression::Assignment { target, .. } => subject_of(ir, *target),
        BodyExpression::MemberAccess {
            receiver,
            member: MemberReference::Named { name },
            null_safe: false,
        } => match ir.expression(*receiver)? {
            BodyExpression::Variable { name: receiver_name } if receiver_name == "this" => {
                Some(NarrowingSubject::ThisProperty { name: name.clone() })
            }
            _ => None,
        },
        BodyExpression::ScopedAccess {
            subject,
            member: MemberReference::Variable { name },
        } => match ir.expression(*subject)? {
            BodyExpression::NamedReference { text } => {
                let folded = text.to_ascii_lowercase();
                (folded == "self" || folded == "static")
                    .then(|| NarrowingSubject::StaticProperty { name: name.clone() })
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::SourceFile;
    use celerrate_semantics::{AstId, BodyQuery, BodyStatement, body_ir};
    use celerrate_source::FileId;

    use super::{NarrowingSubject, subject_of};

    /// Lowers one function body and answers the IR plus the first
    /// top-level expression-statement's expression.
    fn first_expression(source: &str) -> (celerrate_semantics::BodyIr, celerrate_semantics::ExpressionId) {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        let body = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 0,
            },
        );
        let ir = body_ir(&db, file, body).as_ref().unwrap().clone();
        let Some(BodyStatement::Expression { expression }) =
            ir.root.first().and_then(|&id| ir.statement(id)).cloned()
        else {
            panic!("expected an expression statement");
        };
        (ir, expression)
    }

    #[test]
    fn subjects_extract_from_their_stable_shapes() {
        let (ir, expression) = first_expression("<?php function f() { $x; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::Local { name: "x".to_owned() }),
        );

        let (ir, expression) = first_expression("<?php function f() { $this->prop; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::ThisProperty { name: "prop".to_owned() }),
        );

        let (ir, expression) = first_expression("<?php function f() { self::$prop; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::StaticProperty { name: "prop".to_owned() }),
        );

        // Assignment sees through to its target.
        let (ir, expression) = first_expression("<?php function f() { $x = 1; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::Local { name: "x".to_owned() }),
        );

        // `$this` itself, `?->` fetches, and computed members are not
        // stable bases.
        let (ir, expression) = first_expression("<?php function f() { $this; }");
        assert_eq!(subject_of(&ir, expression), None);
        let (ir, expression) = first_expression("<?php function f() { $a?->prop; }");
        assert_eq!(subject_of(&ir, expression), None);
    }
}
```

Note on the `?->` case: the lowered chain is wrapped in
`NullSafeChain`, whose subject is `None` by the catch-all arm; the
inner `MemberAccess` carries `null_safe: true` and is rejected
explicitly. Both routes answer `None`.

- [ ] **Step 4: Implement the walker**

Create `crates/celerrate_types/src/flow.rs`:

```rust
//! The flow walk of one body: expression typing with an environment
//! of narrowing subjects threaded through statements. Branches join,
//! divergence (return, throw, break, goto) marks the path
//! unreachable, and loops run an inner join-ascent fixpoint with a
//! budget — the interprocedural discipline in miniature (design
//! section 6). Absence is silence: a subject missing from the
//! environment reads as its wide type, `mixed` for locals.

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    ArrayEntry, BodyExpression, BodyIr, BodyStatement, ExpressionId, StatementId, StringPart,
    UseTables,
};
use celerrate_stubs::StubIndexInput;
use celerrate_syntax::SyntaxKind;

use crate::inference::InterproceduralEdgeCounts;
use crate::narrowing::{NarrowingSubject, subject_of};
use crate::operators;
use crate::representation::TypeId;
use crate::widening::join;

/// Join-ascent passes a loop may take before the still-moving
/// bindings widen to `mixed` — the deterministic bailout.
pub(crate) const LOOP_ITERATION_BUDGET: u32 = 4;

/// Everything one body's walk needs, resolved once by
/// `inferred_body_types`.
pub(crate) struct FlowContext<'db, 'body> {
    pub db: &'db dyn salsa::Database,
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
    pub ir: &'body BodyIr,
    pub namespace: String,
    pub tables: UseTables,
    /// The defining class's folded key; `None` for free functions and
    /// anonymous-class methods (decision 12).
    pub owner_class_key: Option<String>,
    pub method_is_static: bool,
    /// Parameter names with their seeded (declared) types.
    pub parameters: Vec<(String, TypeId<'db>)>,
}

pub(crate) struct FlowResult<'db> {
    pub expression_types: Vec<TypeId<'db>>,
    pub return_type: TypeId<'db>,
    pub edge_counts: InterproceduralEdgeCounts,
}

/// The abstract state at one program point. `reachable` is the
/// divergence flag: joins ignore an unreachable side, which is
/// exactly how an early return narrows the code after an `if`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Environment<'db> {
    bindings: BTreeMap<NarrowingSubject, TypeId<'db>>,
    reachable: bool,
}

impl<'db> Environment<'db> {
    pub(crate) fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
            reachable: true,
        }
    }

    pub(crate) fn bind(&mut self, subject: NarrowingSubject, of: TypeId<'db>) {
        self.bindings.insert(subject, of);
    }

    pub(crate) fn binding(&self, subject: &NarrowingSubject) -> Option<TypeId<'db>> {
        self.bindings.get(subject).copied()
    }

    pub(crate) fn remove(&mut self, subject: &NarrowingSubject) {
        self.bindings.remove(subject);
    }

    /// Forget everything (a `goto` label: any jump may land here).
    pub(crate) fn clear(&mut self) {
        self.bindings.clear();
        self.reachable = true;
    }

    pub(crate) fn mark_unreachable(&mut self) {
        self.reachable = false;
    }

    pub(crate) fn is_reachable(&self) -> bool {
        self.reachable
    }

    /// The control-flow join: an unreachable side contributes
    /// nothing; two reachable sides join pointwise, an absent side
    /// contributing `mixed` (absence is silence).
    pub(crate) fn join(db: &'db dyn salsa::Database, left: &Self, right: &Self) -> Self {
        if !left.reachable {
            return right.clone();
        }
        if !right.reachable {
            return left.clone();
        }
        Self::join_any(db, left, right)
    }

    /// The pointwise join regardless of reachability — the
    /// exception-edge combinator (a partially executed `try` body) and
    /// the loop accumulator. Reachable when either side is.
    pub(crate) fn join_any(db: &'db dyn salsa::Database, left: &Self, right: &Self) -> Self {
        let mut bindings = BTreeMap::new();
        let mixed = TypeId::mixed(db);
        for subject in left.bindings.keys().chain(right.bindings.keys()) {
            if bindings.contains_key(subject) {
                continue;
            }
            let a = left.binding(subject).unwrap_or(mixed);
            let b = right.binding(subject).unwrap_or(mixed);
            bindings.insert(subject.clone(), join(db, a, b));
        }
        Self {
            bindings,
            reachable: left.reachable || right.reachable,
        }
    }

    /// The loop-budget bailout: every binding that still differs from
    /// `wider` widens to `mixed`, deterministically.
    pub(crate) fn widened_where_changed(
        &self,
        db: &'db dyn salsa::Database,
        wider: &Self,
    ) -> Self {
        let mut result = self.clone();
        for (subject, value) in &wider.bindings {
            if self.binding(subject) != Some(*value) {
                result.bindings.insert(subject.clone(), TypeId::mixed(db));
            }
        }
        result
    }
}

pub(crate) struct Walker<'db, 'body, 'context> {
    context: &'context FlowContext<'db, 'body>,
    types: Vec<TypeId<'db>>,
    returns: Vec<TypeId<'db>>,
    saw_yield: bool,
    edge_counts: InterproceduralEdgeCounts,
}

/// Walks one body from its seeded parameter environment.
pub(crate) fn walk_body<'db>(context: &FlowContext<'db, '_>) -> FlowResult<'db> {
    let db = context.db;
    let mut walker = Walker {
        context,
        types: vec![TypeId::mixed(db); context.ir.expressions.len()],
        returns: Vec::new(),
        saw_yield: false,
        edge_counts: InterproceduralEdgeCounts::default(),
    };
    let mut environment = Environment::new();
    for (name, seeded) in &context.parameters {
        environment.bind(NarrowingSubject::Local { name: name.clone() }, *seeded);
    }
    walker.statements(&context.ir.root.clone(), &mut environment);
    let return_type = if walker.saw_yield {
        TypeId::class(db, "Generator", vec![])
    } else {
        let explicit = walker
            .returns
            .iter()
            .copied()
            .reduce(|left, right| join(db, left, right));
        match (explicit, environment.is_reachable()) {
            // A reachable end of body returns null implicitly.
            (Some(joined), true) => join(db, joined, TypeId::null(db)),
            (None, true) => TypeId::null(db),
            (Some(joined), false) => joined,
            // No return statement ever reached: the body never
            // returns normally.
            (None, false) => TypeId::never(db),
        }
    };
    FlowResult {
        expression_types: walker.types,
        return_type,
        edge_counts: walker.edge_counts,
    }
}

impl<'db> Walker<'db, '_, '_> {
    fn db(&self) -> &'db dyn salsa::Database {
        self.context.db
    }

    fn record(&mut self, id: ExpressionId, of: TypeId<'db>) -> TypeId<'db> {
        if let Some(slot) = self.types.get_mut(id.index() as usize) {
            *slot = of;
        }
        of
    }

    fn recorded(&self, id: ExpressionId) -> TypeId<'db> {
        self.types
            .get(id.index() as usize)
            .copied()
            .unwrap_or_else(|| TypeId::mixed(self.db()))
    }

    /// A written class name resolved at the body's declaring site.
    fn class_type_of_written(&self, written: &str) -> TypeId<'db> {
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables: &self.context.tables,
        };
        TypeId::class(
            self.db(),
            &crate::declared::qualified_class_name(&site, written),
            vec![],
        )
    }

    /// The current type of a subject: its binding, or the wide type —
    /// `mixed` in this task (Task 8 widens property subjects to their
    /// declared type).
    fn subject_type(&self, environment: &Environment<'db>, subject: &NarrowingSubject) -> TypeId<'db> {
        environment
            .binding(subject)
            .unwrap_or_else(|| TypeId::mixed(self.db()))
    }

    fn statements(&mut self, list: &[StatementId], environment: &mut Environment<'db>) {
        for &statement in list {
            if !environment.is_reachable() {
                // Dead code still gets its expressions typed (the
                // table covers the whole arena), against a throwaway
                // empty environment — reachable locally so nested
                // joins behave, discarded so it cannot resurrect the
                // real path.
                let mut scratch = Environment::new();
                self.statement(statement, &mut scratch);
                continue;
            }
            self.statement(statement, environment);
        }
    }

    fn statement(&mut self, id: StatementId, environment: &mut Environment<'db>) {
        let db = self.db();
        let Some(statement) = self.context.ir.statement(id).cloned() else {
            return;
        };
        match statement {
            BodyStatement::Missing | BodyStatement::Declaration { .. } => {}
            BodyStatement::Block { statements } => self.statements(&statements, environment),
            BodyStatement::Expression { expression } => {
                self.expression(expression, environment);
            }
            BodyStatement::Return { value } => {
                let returned = match value {
                    Some(value) => self.expression(value, environment),
                    None => TypeId::null(db),
                };
                self.returns.push(returned);
                environment.mark_unreachable();
            }
            BodyStatement::Echo { values } => {
                for value in values {
                    self.expression(value, environment);
                }
            }
            BodyStatement::Break { level } | BodyStatement::Continue { level } => {
                if let Some(level) = level {
                    self.expression(level, environment);
                }
                // Conservative: the path's bindings are dropped, and
                // dropped reads as mixed — silence (decision 9).
                environment.mark_unreachable();
            }
            BodyStatement::Global { targets } => {
                for target in targets {
                    self.expression(target, environment);
                    if let Some(subject) = subject_of(self.context.ir, target) {
                        environment.bind(subject, TypeId::mixed(db));
                    }
                }
            }
            BodyStatement::StaticVariables { variables } => {
                for variable in variables {
                    if let Some(initializer) = variable.initializer {
                        self.expression(initializer, environment);
                    }
                    // A static local persists across calls: mixed.
                    environment.bind(
                        NarrowingSubject::Local {
                            name: variable.name.clone(),
                        },
                        TypeId::mixed(db),
                    );
                }
            }
            BodyStatement::Unset { targets } => {
                for target in targets {
                    self.expression(target, environment);
                    if let Some(subject) = subject_of(self.context.ir, target) {
                        environment.remove(&subject);
                    }
                }
            }
            BodyStatement::Goto { .. } => environment.mark_unreachable(),
            BodyStatement::Label { .. } => environment.clear(),
            BodyStatement::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expression(condition, environment);
                let mut when_true = environment.clone();
                let mut when_false = environment.clone();
                self.statements(&then_branch, &mut when_true);
                self.statements(&else_branch, &mut when_false);
                *environment = Environment::join(db, &when_true, &when_false);
            }
            BodyStatement::While { condition, body } => {
                self.looped(environment, |walker, env| {
                    walker.expression(condition, env);
                    let exit = env.clone();
                    walker.statements(&body, env);
                    exit
                });
            }
            BodyStatement::DoWhile { body, condition } => {
                self.looped(environment, |walker, env| {
                    walker.statements(&body, env);
                    walker.expression(condition, env);
                    env.clone()
                });
            }
            BodyStatement::For {
                initializers,
                conditions,
                updates,
                body,
            } => {
                for initializer in &initializers {
                    self.expression(*initializer, environment);
                }
                self.looped(environment, |walker, env| {
                    for condition in &conditions {
                        walker.expression(*condition, env);
                    }
                    let exit = env.clone();
                    walker.statements(&body, env);
                    for update in &updates {
                        walker.expression(*update, env);
                    }
                    exit
                });
            }
            BodyStatement::Foreach {
                subject,
                key,
                value,
                by_reference: _,
                body,
            } => {
                self.expression(subject, environment);
                self.looped(environment, |walker, env| {
                    // Iteration typing is plan 6: key and value are
                    // mixed, honestly.
                    if let Some(key) = key
                        && let Some(subject) = subject_of(walker.context.ir, key)
                    {
                        walker.expression(key, env);
                        env.bind(subject, TypeId::mixed(walker.db()));
                    }
                    walker.expression(value, env);
                    if let Some(subject) = subject_of(walker.context.ir, value) {
                        env.bind(subject, TypeId::mixed(walker.db()));
                    }
                    walker.statements(&body, env);
                    env.clone()
                });
            }
            BodyStatement::Switch { subject, cases } => {
                self.expression(subject, environment);
                let pre = environment.clone();
                let mut fall_in: Option<Environment<'db>> = None;
                let mut exits: Vec<Environment<'db>> = Vec::new();
                let mut has_default = false;
                for case in &cases {
                    if let Some(condition) = case.condition {
                        // Conditions evaluate against a scratch clone:
                        // their side effects on later cases are
                        // conservatively dropped.
                        let mut scratch = pre.clone();
                        self.expression(condition, &mut scratch);
                    } else {
                        has_default = true;
                    }
                    let mut entry = match fall_in.take() {
                        Some(previous) => Environment::join_any(db, &previous, &pre),
                        None => pre.clone(),
                    };
                    self.statements(&case.statements, &mut entry);
                    if entry.is_reachable() {
                        fall_in = Some(entry.clone());
                    }
                    exits.push(entry);
                }
                let mut post = exits
                    .into_iter()
                    .reduce(|left, right| Environment::join(db, &left, &right))
                    .unwrap_or_else(|| pre.clone());
                if !has_default {
                    post = Environment::join(db, &post, &pre);
                }
                *environment = post;
            }
            BodyStatement::Try {
                body,
                catches,
                finally,
            } => {
                let entry = environment.clone();
                let mut after_try = environment.clone();
                self.statements(&body, &mut after_try);
                // A catch entry sees any prefix of the try body:
                // pointwise join regardless of reachability.
                let catch_entry = Environment::join_any(db, &entry, &after_try);
                let mut exits = vec![after_try];
                for catch in &catches {
                    let mut arm = catch_entry.clone();
                    let caught = TypeId::union(
                        db,
                        catch
                            .types
                            .iter()
                            .map(|written| self.class_type_of_written(written)),
                    );
                    if let Some(variable) = &catch.variable {
                        arm.bind(
                            NarrowingSubject::Local {
                                name: variable.clone(),
                            },
                            caught,
                        );
                    }
                    self.statements(&catch.statements, &mut arm);
                    exits.push(arm);
                }
                let mut post = exits
                    .into_iter()
                    .reduce(|left, right| Environment::join(db, &left, &right))
                    .unwrap_or(entry);
                if let Some(finally) = finally {
                    self.statements(&finally, &mut post);
                }
                *environment = post;
            }
            BodyStatement::Declare { statements } => self.statements(&statements, environment),
        }
    }

    /// The loop discipline (decision 9): join-ascent passes to a
    /// fixpoint under `LOOP_ITERATION_BUDGET`, deterministic widening
    /// to `mixed` on exhaustion, then one final recording pass from
    /// the fixed environment. The pass closure answers the loop's
    /// *exit* environment (a `while`'s condition-false state); the
    /// final pass's exit becomes the post-loop environment.
    fn looped<F>(&mut self, environment: &mut Environment<'db>, mut pass: F)
    where
        F: FnMut(&mut Self, &mut Environment<'db>) -> Environment<'db>,
    {
        let db = self.db();
        let mut current = environment.clone();
        let mut budget = LOOP_ITERATION_BUDGET;
        loop {
            let mut attempt = current.clone();
            let _ = pass(self, &mut attempt);
            let joined = Environment::join_any(db, &current, &attempt);
            if joined == current {
                break;
            }
            if budget == 0 {
                current = current.widened_where_changed(db, &joined);
                break;
            }
            budget -= 1;
            current = joined;
        }
        let mut fixed = current;
        let exit = pass(self, &mut fixed);
        *environment = exit;
    }

    fn expression(&mut self, id: ExpressionId, environment: &mut Environment<'db>) -> TypeId<'db> {
        let db = self.db();
        let Some(expression) = self.context.ir.expression(id).cloned() else {
            return self.record(id, TypeId::mixed(db));
        };
        let of = match expression {
            BodyExpression::Missing => TypeId::mixed(db),
            BodyExpression::Literal { text } => operators::literal_type(db, &text),
            BodyExpression::Variable { name } => {
                if name == "this" {
                    // Task 6 types `$this` as the defining class.
                    TypeId::mixed(db)
                } else {
                    self.subject_type(environment, &NarrowingSubject::Local { name })
                }
            }
            BodyExpression::NamedReference { text } => {
                operators::named_reference_type(db, &text).unwrap_or_else(|| TypeId::mixed(db))
            }
            BodyExpression::DynamicVariable { target } => {
                self.expression(target, environment);
                TypeId::mixed(db)
            }
            BodyExpression::Unary { operator, operand } => {
                let operand_type = self.expression(operand, environment);
                operators::unary_type(db, operator, operand_type)
            }
            BodyExpression::Postfix { operand, .. } => {
                let operand_type = self.expression(operand, environment);
                operators::postfix_type(db, operand_type)
            }
            BodyExpression::Binary { operator, lhs, rhs } => {
                let lhs_type = self.expression(lhs, environment);
                let rhs_type = self.expression(rhs, environment);
                operators::binary_type(db, operator, lhs_type, rhs_type)
            }
            BodyExpression::Assignment {
                operator,
                by_reference,
                target,
                value,
            } => {
                self.expression(target, environment);
                let value_type = self.expression(value, environment);
                self.assignment(operator, by_reference, target, value, value_type, environment)
            }
            BodyExpression::Cast { operator, operand } => {
                let operand_type = self.expression(operand, environment);
                operators::cast_type(db, operator, operand_type)
            }
            BodyExpression::Ternary {
                condition,
                middle,
                alternative,
            } => {
                self.expression(condition, environment);
                let mut when_true = environment.clone();
                let mut when_false = environment.clone();
                let middle_type = match middle {
                    Some(middle) => self.expression(middle, &mut when_true),
                    // The short ternary answers the condition's value
                    // when it is truthy.
                    None => self.recorded(condition),
                };
                let alternative_type = self.expression(alternative, &mut when_false);
                *environment = Environment::join(db, &when_true, &when_false);
                join(db, middle_type, alternative_type)
            }
            BodyExpression::Array { entries } => self.array_literal(&entries, environment),
            BodyExpression::InterpolatedString { parts } => {
                self.string_parts(&parts, environment);
                TypeId::string(db)
            }
            BodyExpression::ShellExec { parts } => {
                self.string_parts(&parts, environment);
                TypeId::union(
                    db,
                    [
                        TypeId::string(db),
                        TypeId::bool_literal(db, false),
                        TypeId::null(db),
                    ],
                )
            }
            BodyExpression::Isset { targets } => {
                for target in targets {
                    self.expression(target, environment);
                }
                TypeId::bool(db)
            }
            BodyExpression::Empty { target } => {
                self.expression(target, environment);
                TypeId::bool(db)
            }
            BodyExpression::Eval { argument } => {
                self.expression(argument, environment);
                // eval can rewrite every local: forget them all.
                let locals: Vec<NarrowingSubject> = environment
                    .bindings
                    .keys()
                    .filter(|subject| matches!(subject, NarrowingSubject::Local { .. }))
                    .cloned()
                    .collect();
                for subject in locals {
                    environment.remove(&subject);
                }
                TypeId::mixed(db)
            }
            BodyExpression::Exit { argument } => {
                if let Some(argument) = argument {
                    self.expression(argument, environment);
                }
                environment.mark_unreachable();
                TypeId::never(db)
            }
            BodyExpression::Print { operand } => {
                self.expression(operand, environment);
                TypeId::int_literal(db, 1)
            }
            BodyExpression::Clone { operand } => self.expression(operand, environment),
            BodyExpression::Throw { operand } => {
                self.expression(operand, environment);
                environment.mark_unreachable();
                TypeId::never(db)
            }
            BodyExpression::Yield { key, value, .. } => {
                self.saw_yield = true;
                if let Some(key) = key {
                    self.expression(key, environment);
                }
                if let Some(value) = value {
                    self.expression(value, environment);
                }
                TypeId::mixed(db)
            }
            BodyExpression::Include { operand, .. } => {
                self.expression(operand, environment);
                TypeId::mixed(db)
            }
            BodyExpression::Match { subject, arms } => {
                self.expression(subject, environment);
                let mut result: Option<TypeId<'db>> = None;
                let mut exits: Vec<Environment<'db>> = Vec::new();
                for arm in &arms {
                    let mut arm_env = environment.clone();
                    for condition in &arm.conditions {
                        self.expression(*condition, &mut arm_env);
                    }
                    let body_type = self.expression(arm.body, &mut arm_env);
                    result = Some(match result {
                        Some(previous) => join(db, previous, body_type),
                        None => body_type,
                    });
                    exits.push(arm_env);
                }
                // An unmatched subject throws: only arm exits join.
                if let Some(post) = exits
                    .into_iter()
                    .reduce(|left, right| Environment::join(db, &left, &right))
                {
                    *environment = post;
                }
                result.unwrap_or_else(|| TypeId::never(db))
            }
            BodyExpression::MemberAccess { receiver, member, .. } => {
                self.expression(receiver, environment);
                if let celerrate_semantics::MemberReference::Computed { expression } = member {
                    self.expression(expression, environment);
                }
                // Task 6 resolves member reads.
                TypeId::mixed(db)
            }
            BodyExpression::ScopedAccess { subject, member } => {
                self.expression(subject, environment);
                if let celerrate_semantics::MemberReference::Computed { expression } = member {
                    self.expression(expression, environment);
                }
                TypeId::mixed(db)
            }
            BodyExpression::NullSafeChain { chain } => {
                // Task 7 implements the whole-chain rule.
                self.expression(chain, environment)
            }
            BodyExpression::Call { callee, arguments } => {
                self.expression(callee, environment);
                for argument in &arguments {
                    self.expression(argument.value, environment);
                }
                // Tasks 6 and 9 resolve call results.
                TypeId::mixed(db)
            }
            BodyExpression::CallableReference { callee } => {
                self.expression(callee, environment);
                TypeId::mixed(db)
            }
            BodyExpression::New { class, arguments } => {
                if let celerrate_semantics::ClassReference::Dynamic { expression } = &class {
                    self.expression(*expression, environment);
                }
                for argument in &arguments {
                    self.expression(argument.value, environment);
                }
                TypeId::mixed(db)
            }
            BodyExpression::Index { subject, index } => {
                let subject_type = self.expression(subject, environment);
                let index_type = index.map(|index| self.expression(index, environment));
                operators::index_type(db, subject_type, index_type)
            }
            BodyExpression::Closure { body, .. } => {
                // Task 9 types closures and seeds captures; the inner
                // body is walked now so the table covers its arena.
                let mut inner = Environment::new();
                self.statements_nested(&body, &mut inner);
                TypeId::mixed(db)
            }
            BodyExpression::ArrowFunction { body, .. } => {
                let mut inner = environment.clone();
                let _ = self.expression_nested(body, &mut inner);
                TypeId::mixed(db)
            }
        };
        self.record(id, of)
    }

    /// Walks a nested body (closure) without contributing its
    /// `return` statements to the enclosing body's return type.
    fn statements_nested(&mut self, list: &[StatementId], environment: &mut Environment<'db>) {
        let saved_returns = std::mem::take(&mut self.returns);
        let saved_yield = self.saw_yield;
        self.statements(list, environment);
        self.returns = saved_returns;
        self.saw_yield = saved_yield;
    }

    fn expression_nested(
        &mut self,
        id: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let saved_returns = std::mem::take(&mut self.returns);
        let saved_yield = self.saw_yield;
        let of = self.expression(id, environment);
        self.returns = saved_returns;
        self.saw_yield = saved_yield;
        of
    }

    fn string_parts(&mut self, parts: &[StringPart], environment: &mut Environment<'db>) {
        for part in parts {
            if let StringPart::Interpolation { expression } = part {
                self.expression(*expression, environment);
            }
        }
    }

    /// An array literal: a shape when every entry has a statically
    /// known key (or none — positional) and no spread; otherwise the
    /// general array of the joined keys and values.
    fn array_literal(
        &mut self,
        entries: &[ArrayEntry],
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        let mut fields: Vec<crate::representation::ShapeField<'db>> = Vec::new();
        let mut next_index: i64 = 0;
        let mut shape_holds = true;
        let mut joined_key: Option<TypeId<'db>> = None;
        let mut joined_value: Option<TypeId<'db>> = None;
        let mut is_list = true;
        for entry in entries {
            let ArrayEntry::Element {
                key,
                value,
                spread,
                by_reference: _,
            } = entry
            else {
                continue; // a destructuring hole never appears in a literal read
            };
            let key_type = key.map(|key| self.expression(key, environment));
            let value_type = self.expression(*value, environment);
            if *spread {
                shape_holds = false;
                is_list = false;
                let spread_key = value_type
                    .array_key(db)
                    .unwrap_or_else(|| TypeId::union(db, [TypeId::int(db), TypeId::string(db)]));
                let spread_value = value_type
                    .array_value(db)
                    .unwrap_or_else(|| TypeId::mixed(db));
                joined_key = Some(joined_key.map_or(spread_key, |k| join(db, k, spread_key)));
                joined_value =
                    Some(joined_value.map_or(spread_value, |v| join(db, v, spread_value)));
                continue;
            }
            let shape_key = match key_type {
                None => {
                    let index = next_index;
                    next_index += 1;
                    Some(crate::representation::ShapeKey::Integer(index))
                }
                Some(of) => {
                    is_list = false;
                    of.int_literal_value(db)
                        .map(crate::representation::ShapeKey::Integer)
                        .or_else(|| {
                            of.string_literal_value(db)
                                .map(crate::representation::ShapeKey::String)
                        })
                }
            };
            match shape_key {
                Some(shape_key) if shape_holds => fields.push(crate::representation::ShapeField {
                    key: shape_key,
                    optional: false,
                    value: value_type,
                }),
                _ => shape_holds = false,
            }
            let this_key = key_type.unwrap_or_else(|| TypeId::int(db));
            joined_key = Some(joined_key.map_or(this_key, |k| join(db, k, this_key)));
            joined_value = Some(joined_value.map_or(value_type, |v| join(db, v, value_type)));
        }
        if shape_holds && !fields.is_empty() {
            return TypeId::shape(db, fields);
        }
        match (joined_key, joined_value) {
            (Some(key), Some(value)) => {
                if is_list {
                    TypeId::non_empty_list(db, value)
                } else {
                    TypeId::non_empty_array(db, key, value)
                }
            }
            // The empty literal.
            _ => TypeId::shape(db, vec![]),
        }
    }

    /// One assignment: propagate to the target's subject, updating
    /// array bases on index writes and destructuring element-wise.
    /// Answers the expression's own type (the assigned value; the
    /// computed value for compound forms).
    fn assignment(
        &mut self,
        operator: SyntaxKind,
        by_reference: bool,
        target: ExpressionId,
        _value: ExpressionId,
        value_type: TypeId<'db>,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        if by_reference {
            // `$b = &$a`: aliased locals are unknowable without alias
            // analysis — both sides degrade to mixed (decision 10).
            if let Some(subject) = subject_of(self.context.ir, target) {
                environment.bind(subject, TypeId::mixed(db));
            }
            if let Some(subject) = subject_of(self.context.ir, _value) {
                environment.bind(subject, TypeId::mixed(db));
            }
            return TypeId::mixed(db);
        }
        let assigned = match compound_base(operator) {
            Some(base) => {
                let current = self.recorded(target);
                operators::binary_type(db, base, current, value_type)
            }
            None => value_type,
        };
        self.assign_target(target, assigned, environment);
        assigned
    }

    fn assign_target(
        &mut self,
        target: ExpressionId,
        value_type: TypeId<'db>,
        environment: &mut Environment<'db>,
    ) {
        let db = self.db();
        match self.context.ir.expression(target).cloned() {
            // Destructuring: `[$a, $b] = ...`, `['k' => $v] = ...`.
            Some(BodyExpression::Array { entries }) => {
                let mut next_index: i64 = 0;
                for entry in &entries {
                    let ArrayEntry::Element { key, value, .. } = entry else {
                        next_index += 1;
                        continue;
                    };
                    let key_type = match key {
                        Some(key) => Some(self.recorded(*key)),
                        None => {
                            let index = next_index;
                            next_index += 1;
                            Some(TypeId::int_literal(db, index))
                        }
                    };
                    let element = operators::index_type(db, value_type, key_type);
                    self.assign_target(*value, element, environment);
                }
            }
            // An index write rebinds the base array.
            Some(BodyExpression::Index { subject, index }) => {
                if let Some(base) = subject_of(self.context.ir, subject) {
                    let current = environment.binding(&base);
                    let key_type = index.map(|index| self.recorded(index));
                    let updated = updated_array(db, current, key_type, value_type);
                    environment.bind(base, updated);
                }
            }
            _ => {
                if let Some(subject) = subject_of(self.context.ir, target) {
                    environment.bind(subject, value_type);
                }
            }
        }
    }
}

/// `$a op= $b` reduces to `op`; `None` for plain `=` (and for `??=`,
/// which Task 5 handles in the walker).
fn compound_base(operator: SyntaxKind) -> Option<SyntaxKind> {
    Some(match operator {
        SyntaxKind::PlusEquals => SyntaxKind::Plus,
        SyntaxKind::MinusEquals => SyntaxKind::Minus,
        SyntaxKind::StarEquals => SyntaxKind::Star,
        SyntaxKind::SlashEquals => SyntaxKind::Slash,
        SyntaxKind::DotEquals => SyntaxKind::Dot,
        SyntaxKind::PercentEquals => SyntaxKind::Percent,
        SyntaxKind::StarStarEquals => SyntaxKind::StarStar,
        SyntaxKind::AmpersandEquals => SyntaxKind::Ampersand,
        SyntaxKind::PipeEquals => SyntaxKind::Pipe,
        SyntaxKind::CaretEquals => SyntaxKind::Caret,
        SyntaxKind::LessLessEquals => SyntaxKind::LessLess,
        SyntaxKind::GreaterGreaterEquals => SyntaxKind::GreaterGreater,
        _ => return None,
    })
}

/// The new type of an array base after `$a[k] = v`: a shape upserts
/// the field when the key is a known literal, an array joins key and
/// value, anything else becomes an array from this write.
fn updated_array<'db>(
    db: &'db dyn salsa::Database,
    current: Option<TypeId<'db>>,
    key_type: Option<TypeId<'db>>,
    value_type: TypeId<'db>,
) -> TypeId<'db> {
    use crate::representation::{ShapeField, ShapeKey};
    let literal_key = key_type.and_then(|key| {
        key.int_literal_value(db)
            .map(ShapeKey::Integer)
            .or_else(|| key.string_literal_value(db).map(ShapeKey::String))
    });
    if let Some(current) = current {
        if let Some(mut fields) = current.shape_fields(db) {
            match (&literal_key, key_type) {
                (Some(wanted), _) => {
                    fields.retain(|field| field.key != *wanted);
                    fields.push(ShapeField {
                        key: wanted.clone(),
                        optional: false,
                        value: value_type,
                    });
                    return TypeId::shape(db, fields);
                }
                (None, None) => {
                    // `$a[] = v`: the next free integer key.
                    let next = fields
                        .iter()
                        .filter_map(|field| match &field.key {
                            ShapeKey::Integer(index) => Some(*index + 1),
                            ShapeKey::String(_) => None,
                        })
                        .max()
                        .unwrap_or(0);
                    fields.push(ShapeField {
                        key: ShapeKey::Integer(next),
                        optional: false,
                        value: value_type,
                    });
                    return TypeId::shape(db, fields);
                }
                (None, Some(_)) => {
                    // A dynamic key on a shape: degrade to the array
                    // of the joined parts.
                    let (key, value) = shape_join(db, &fields);
                    let key_join = key_type.map_or(key, |of| join(db, key, of));
                    return TypeId::array(db, key_join, join(db, value, value_type));
                }
            }
        }
        if let (Some(key), Some(value)) = (current.array_key(db), current.array_value(db)) {
            let pushed_key = key_type.unwrap_or_else(|| TypeId::int(db));
            return TypeId::non_empty_array(
                db,
                join(db, key, pushed_key),
                join(db, value, value_type),
            );
        }
    }
    // Anything else (absent, mixed, scalar): the write makes it an
    // array from here on.
    match (literal_key, key_type) {
        (Some(key), _) => TypeId::shape(
            db,
            vec![ShapeField {
                key,
                optional: false,
                value: value_type,
            }],
        ),
        (None, None) => TypeId::non_empty_list(db, value_type),
        (None, Some(key)) => TypeId::non_empty_array(db, key, value_type),
    }
}

fn shape_join<'db>(
    db: &'db dyn salsa::Database,
    fields: &[crate::representation::ShapeField<'db>],
) -> (TypeId<'db>, TypeId<'db>) {
    use crate::representation::ShapeKey;
    let mut key: Option<TypeId<'db>> = None;
    let mut value: Option<TypeId<'db>> = None;
    for field in fields {
        let field_key = match &field.key {
            ShapeKey::Integer(index) => TypeId::int_literal(db, *index),
            ShapeKey::String(text) => TypeId::string_literal(db, text),
        };
        key = Some(key.map_or(field_key, |k| join(db, k, field_key)));
        value = Some(value.map_or(field.value, |v| join(db, v, field.value)));
    }
    (
        key.unwrap_or_else(|| TypeId::union(db, [TypeId::int(db), TypeId::string(db)])),
        value.unwrap_or_else(|| TypeId::mixed(db)),
    )
}
```

One access rule the code above relies on: `Environment.bindings` is
read directly inside `expression` for the `Eval` arm — keep the field
`pub(crate)` visibility *inside the module* by leaving `Walker` and
`Environment` in the same file (they are), or add a
`locals(&self) -> impl Iterator` helper if the borrow checker
complains; either shape is fine, the behavior is the contract.

- [ ] **Step 5: Wire the walk into the query**

In `crates/celerrate_types/src/inference.rs`, replace the all-`mixed`
body of `inferred_body_types` and add the seeding helper. New imports:
`celerrate_semantics::{MemberSignature, UseTables, item_tree,
folded_member_key}`, `crate::declared::{DeclaredSignature,
FunctionQuery, declared_function_signature, declared_member_signature}`,
`celerrate_semantics::MemberQuery`, `crate::flow::{FlowContext,
walk_body}`.

```rust
#[salsa::tracked(returns(ref))]
pub fn inferred_body_types<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<InferredBody<'db>> {
    let ir = body_ir(db, file, body).as_ref()?;
    let owner = body_owner(db, file, body);
    let (namespace, owner_class_key, method_is_static, parameters) = match owner {
        Some(BodyOwner::Function(function)) => {
            let key = folded_symbol_key(
                SymbolSpace::Function,
                &fully_qualified_name(&function.namespace, &function.name),
            );
            let declared = declared_function_signature(
                db,
                files,
                stubs,
                configuration,
                crate::declared::FunctionQuery::new(db, key),
            );
            (
                function.namespace.clone(),
                None,
                false,
                seeded_parameters(db, declared.as_ref(), &function.signature),
            )
        }
        Some(BodyOwner::Method {
            class_key,
            namespace,
            member,
        }) => {
            let declared = class_key.as_ref().and_then(|key| {
                declared_member_signature(
                    db,
                    files,
                    stubs,
                    configuration,
                    celerrate_semantics::MemberQuery::new(
                        db,
                        key.clone(),
                        MemberKind::Method,
                        celerrate_semantics::folded_member_key(MemberKind::Method, &member.name),
                    ),
                )
            });
            (
                namespace.clone(),
                class_key.clone(),
                member.flags.is_static,
                seeded_parameters(db, declared.as_ref(), &member.signature),
            )
        }
        None => (String::new(), None, false, Vec::new()),
    };
    let tables = celerrate_semantics::UseTables::for_namespace(
        celerrate_semantics::item_tree(db, file),
        &namespace,
    );
    let context = FlowContext {
        db,
        files,
        stubs,
        configuration,
        ir,
        namespace,
        tables,
        owner_class_key,
        method_is_static,
        parameters,
    };
    let result = walk_body(&context);
    Some(InferredBody {
        expression_types: result.expression_types,
        return_type: result.return_type,
        edge_counts: result.edge_counts,
    })
}

/// Parameter names paired with their seeded types: the declared
/// parameter type (the plan-3 layer, annotation-refined) or `mixed`,
/// a variadic parameter collecting into a list of it.
fn seeded_parameters<'db>(
    db: &'db dyn salsa::Database,
    declared: Option<&crate::declared::DeclaredSignature<'db>>,
    signature: &celerrate_semantics::MemberSignature,
) -> Vec<(String, TypeId<'db>)> {
    signature
        .parameters
        .iter()
        .map(|parameter| {
            let declared_type = declared
                .and_then(|signature| {
                    signature
                        .parameters
                        .iter()
                        .find(|candidate| candidate.name == parameter.name)
                })
                .and_then(|candidate| candidate.parameter_type)
                .unwrap_or_else(|| TypeId::mixed(db));
            let seeded = if parameter.variadic {
                TypeId::list(db, declared_type)
            } else {
                declared_type
            };
            (parameter.name.clone(), seeded)
        })
        .collect()
}
```

Register the modules in `crates/celerrate_types/src/lib.rs`: add
`mod flow;` and `mod narrowing;` to the module list (alphabetical).
Nothing new is re-exported.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS
(the Task 1-3 inference tests, the narrowing subject test, and every
pre-existing test). Then the full local gate.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/inference.rs crates/celerrate_types/src/lib.rs
git commit -m "✨ feat(types): locals flow by assignment propagation under the loop discipline"
```

---

### Task 4: The narrowing floor's condition forms

The four pure leaf transformations, the `is_*` table, and
`branch_environments` — condition-to-environments transformation with
structural `!`/`&&`/`||` distribution. Forms landing here:
`instanceof` (and negation), `===`/`!==` against narrowing literals
(`null`, `true`/`false`, int/string literals, enum cases), the `is_*`
family, `isset()`/`empty()`, truthiness, and boolean composition.
`if`, `while`, ternary, and the logical operators start narrowing.

**Files:**
- Modify: `crates/celerrate_types/src/narrowing.rs`
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: Task 3's `Environment`/`Walker`/`subject_of`,
  `crate::judgments::{Proof, subtype_of}`, `TypeId::without_null`.
- Produces (`narrowing.rs`, all `pub(crate)`): `narrow_to(db, files,
  stubs, configuration, current: TypeId, target: TypeId) -> TypeId`,
  `remove_type(db, files, stubs, configuration, current, target) ->
  TypeId`, `remove_falsy(db, current) -> TypeId`, `keep_falsy(db,
  current) -> TypeId`, `type_check_target(db, folded_callee: &str) ->
  Option<TypeId>`, `is_narrowing_literal(db, of: TypeId) -> bool`.
  (`flow.rs`): `fn branch_environments(&mut self, condition:
  ExpressionId, environment: &mut Environment<'db>) ->
  (Environment<'db>, Environment<'db>)` on `Walker`. Tasks 5, 7, 8,
  and 11 build on these.

- [ ] **Step 1: Write the failing tests**

Unit tests for the leaf transformations, in
`crates/celerrate_types/src/narrowing.rs`'s test module (extend it;
the quartet fixture is the `declared.rs` pattern — add the same
`fixture` helper used in `inference.rs`, sources
`&["<?php interface Countable {} class Foo implements Countable {} class Bar {}"]`):

```rust
    #[test]
    fn narrow_to_distributes_over_unions_and_intersects_class_pairs() {
        let fixture = fixture(&[
            "<?php interface Liftable {} class Foo implements Liftable {} class Bar {}",
        ]);
        let db = &fixture.db;
        let foo = crate::TypeId::class(db, "Foo", vec![]);
        let bar = crate::TypeId::class(db, "Bar", vec![]);
        let liftable = crate::TypeId::class(db, "Liftable", vec![]);
        let null = crate::TypeId::null(db);
        let mixed = crate::TypeId::mixed(db);
        let narrow = |current, target| {
            super::narrow_to(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                current,
                target,
            )
        };
        // mixed narrows to the target outright.
        assert_eq!(narrow(mixed, foo), foo);
        // A union keeps the holding constituents and drops null.
        assert_eq!(narrow(crate::TypeId::union(db, [foo, null]), foo), foo);
        // A subtype constituent narrows to itself, not the supertype.
        assert_eq!(narrow(crate::TypeId::union(db, [foo, bar]), liftable), foo);
        // Two unrelated concrete classes cannot both hold: the
        // possibly-implementing pair intersects instead of dropping.
        assert_eq!(
            narrow(bar, liftable),
            crate::TypeId::intersection(db, [bar, liftable]),
        );
        // A scalar can never be an instance: never.
        assert_eq!(narrow(crate::TypeId::int(db), foo), crate::TypeId::never(db));
        // A mixed target narrows nothing.
        assert_eq!(narrow(foo, mixed), foo);
    }

    #[test]
    fn remove_type_drops_proven_constituents_only() {
        let fixture = fixture(&["<?php class Foo {} class Bar {}"]);
        let db = &fixture.db;
        let foo = crate::TypeId::class(db, "Foo", vec![]);
        let bar = crate::TypeId::class(db, "Bar", vec![]);
        let null = crate::TypeId::null(db);
        let remove = |current, target| {
            super::remove_type(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                current,
                target,
            )
        };
        assert_eq!(remove(crate::TypeId::union(db, [foo, null]), null), foo);
        assert_eq!(
            remove(crate::TypeId::union(db, [foo, bar]), foo),
            bar,
        );
        // Removing the whole of a non-union leaves never.
        assert_eq!(remove(foo, foo), crate::TypeId::never(db));
        // mixed cannot be subtracted from.
        assert_eq!(remove(crate::TypeId::mixed(db), foo), crate::TypeId::mixed(db));
    }

    #[test]
    fn falsy_filters_split_the_scalar_families() {
        let fixture = fixture(&["<?php"]);
        let db = &fixture.db;
        let nullable_false = crate::TypeId::union(
            db,
            [
                crate::TypeId::int(db),
                crate::TypeId::bool_literal(db, false),
                crate::TypeId::null(db),
            ],
        );
        assert_eq!(
            super::remove_falsy(db, nullable_false),
            crate::TypeId::int(db),
        );
        // General bool minus false is true.
        assert_eq!(
            super::remove_falsy(db, crate::TypeId::bool(db)),
            crate::TypeId::bool_literal(db, true),
        );
        // keep_falsy is the dual: int keeps exactly 0.
        assert_eq!(
            super::keep_falsy(db, crate::TypeId::int(db)),
            crate::TypeId::int_literal(db, 0),
        );
        assert_eq!(
            super::keep_falsy(db, crate::TypeId::bool(db)),
            crate::TypeId::bool_literal(db, false),
        );
    }

    #[test]
    fn the_type_check_table_answers_the_common_family() {
        let db = TestDatabase::default();
        assert_eq!(
            super::type_check_target(&db, "is_string"),
            Some(crate::TypeId::string(&db)),
        );
        assert_eq!(
            super::type_check_target(&db, "is_int"),
            Some(crate::TypeId::int(&db)),
        );
        assert_eq!(
            super::type_check_target(&db, "is_integer"),
            Some(crate::TypeId::int(&db)),
        );
        assert_eq!(
            super::type_check_target(&db, "is_null"),
            Some(crate::TypeId::null(&db)),
        );
        assert!(super::type_check_target(&db, "is_object").is_some());
        assert!(super::type_check_target(&db, "is_numeric").is_some());
        assert!(super::type_check_target(&db, "strlen").is_none());
    }
```

End-to-end narrowing tests in `inference.rs`'s test module — the
forms, each through `return_display`:

```rust
    #[test]
    fn instanceof_narrows_both_branches() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function f(mixed $x) { if ($x instanceof Foo) { return $x; } return 1; }
            function negated(mixed $x) { if (!($x instanceof Foo)) { return 1; } return $x; }",
        ]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|foo");
    }

    #[test]
    fn strict_null_comparisons_narrow() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function f(?Foo $x) { if ($x === null) { return 1; } return $x; }
            function g(?Foo $x) { if ($x !== null) { return $x; } return 1; }",
        ]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|foo");
    }

    #[test]
    fn false_comparisons_narrow_the_strpos_idiom() {
        let fixture = fixture(&[
            "<?php function f(int|false $position) {
                if ($position === false) { return 'missing'; }
                return $position;
            }",
        ]);
        assert_eq!(return_display(&fixture, 0), "int|'missing'");
    }

    #[test]
    fn the_is_family_narrows() {
        let fixture = fixture(&[
            "<?php function f(mixed $x) { if (is_string($x)) { return $x; } return 1; }",
        ]);
        assert_eq!(return_display(&fixture, 0), "1|string");
    }

    #[test]
    fn boolean_composition_distributes() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function both(mixed $x) {
                if ($x instanceof Foo && is_string($x)) { return 1; }
                return 2;
            }
            function either(?Foo $x) {
                if ($x === null || $x instanceof Foo) { return 1; }
                return $x;
            }",
        ]);
        // `either`'s fall-through sees the union minus both
        // alternatives — never — so the function's return joins to
        // exactly the then-branch's literal. Without `||`
        // distribution the answer would be "null|1|foo".
        assert_eq!(return_display(&fixture, 2), "1");
        // `both` must compose without crashing or mis-joining.
        let _ = return_display(&fixture, 1);
    }

    #[test]
    fn early_returns_narrow_the_rest_of_the_body() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function f(?Foo $x) {
                if ($x === null) { return 1; }
                return $x;
            }",
        ]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
    }

    #[test]
    fn isset_and_empty_narrow_their_targets() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function set(?Foo $x) { if (isset($x)) { return $x; } return 1; }
            function filled(string|null $x) { if (!empty($x)) { return $x; } return 1; }",
        ]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|string");
    }

    #[test]
    fn truthiness_narrows_and_a_while_condition_narrows_its_body() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function truthy(?Foo $x) { if ($x) { return $x; } return 1; }
            function looped(?Foo $x) { while ($x !== null) { return $x; } return 1; }",
        ]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|foo");
    }

    #[test]
    fn an_assign_and_test_condition_narrows_the_assigned_subject() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function f(?Foo $source) {
                if (($x = $source) !== null) { return $x; }
                return 1;
            }",
        ]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types 2>&1 | tail -8`
Expected: the `narrowing` unit tests FAIL to compile (the
transformations do not exist); the end-to-end tests FAIL on `mixed`
where narrowed types are expected.

- [ ] **Step 3: Implement the leaf transformations**

Add to `crates/celerrate_types/src/narrowing.rs` (above the test
module). New imports: `celerrate_db::AnalyzedFileSet`,
`celerrate_project::ProjectConfiguration`,
`celerrate_stubs::StubIndexInput`, `crate::judgments::{Proof,
subtype_of}`, `crate::representation::TypeId`.

```rust
/// The values `===`-comparison can narrow by: exactly the forms whose
/// value set is one canonical point (or, for enum cases, one case).
pub(crate) fn is_narrowing_literal<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.is_null(db)
        || of.bool_literal_value(db).is_some()
        || of.int_literal_value(db).is_some()
        || of.float_literal_value(db).is_some()
        || of.string_literal_value(db).is_some()
        || of.enum_case_parts(db).is_some()
}

/// Is this constituent class-like enough that an unproven instanceof
/// keeps it as an intersection rather than dropping it?
fn class_like<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.class_name(db).is_some()
        || of == TypeId::object(db)
        || !of.intersectands(db).is_empty()
        || of.template_bound(db).is_some()
}

/// Positive narrowing: the subject is known to be `target`.
/// Distributes over unions; a `mixed` subject becomes the target; a
/// `mixed` target narrows nothing. Per constituent: a proven subtype
/// keeps itself (precision), a proven supertype narrows to the
/// target, an undecided class-like pair intersects (two instanceofs
/// produce `Foo&Countable`), and a refuted non-class pair drops.
pub(crate) fn narrow_to<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    current: TypeId<'db>,
    target: TypeId<'db>,
) -> TypeId<'db> {
    if target.is_mixed(db) {
        return current;
    }
    if current.is_mixed(db) {
        return target;
    }
    let constituents = constituents_of(db, current);
    let narrowed = constituents.into_iter().filter_map(|constituent| {
        if subtype_of(db, files, stubs, configuration, constituent, target) == Proof::Holds {
            return Some(constituent);
        }
        if subtype_of(db, files, stubs, configuration, target, constituent) == Proof::Holds {
            return Some(target);
        }
        if class_like(db, constituent) && class_like(db, target) {
            return Some(TypeId::intersection(db, [constituent, target]));
        }
        None
    });
    TypeId::union(db, narrowed)
}

/// Negative narrowing: the subject is known not to be `target`.
/// Drops the constituents proven subtypes of the target; everything
/// undecided stays (conservative). `mixed` cannot be subtracted from.
pub(crate) fn remove_type<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    current: TypeId<'db>,
    target: TypeId<'db>,
) -> TypeId<'db> {
    if current.is_mixed(db) || target.is_mixed(db) {
        return current;
    }
    let constituents = constituents_of(db, current);
    let kept = constituents.into_iter().filter(|&constituent| {
        subtype_of(db, files, stubs, configuration, constituent, target) != Proof::Holds
    });
    TypeId::union(db, kept)
}

/// One level of union constituents (a non-union answers itself).
fn constituents_of<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> Vec<TypeId<'db>> {
    let parts = of.constituents(db);
    if parts.is_empty() { vec![of] } else { parts }
}

/// Truthiness, positive side: drop null and the literal falsy
/// scalars; a general bool tightens to `true`. Everything the rule
/// cannot decide stays — silence, never a guess.
pub(crate) fn remove_falsy<'db>(db: &'db dyn salsa::Database, current: TypeId<'db>) -> TypeId<'db> {
    let kept = constituents_of(db, current)
        .into_iter()
        .filter_map(|constituent| {
            if constituent.is_null(db) {
                return None;
            }
            if constituent == TypeId::bool(db) {
                return Some(TypeId::bool_literal(db, true));
            }
            match constituent.bool_literal_value(db) {
                Some(false) => return None,
                Some(true) | None => {}
            }
            if constituent.int_literal_value(db) == Some(0) {
                return None;
            }
            if constituent.float_literal_value(db) == Some(0.0) {
                return None;
            }
            if let Some(text) = constituent.string_literal_value(db)
                && (text.is_empty() || text == "0")
            {
                return None;
            }
            Some(constituent)
        });
    TypeId::union(db, kept)
}

/// Truthiness, negative side: keep only what can be falsy, tightened
/// to its falsy form where one exists (int to 0, bool to false).
/// `mixed` and general strings/arrays stay themselves.
pub(crate) fn keep_falsy<'db>(db: &'db dyn salsa::Database, current: TypeId<'db>) -> TypeId<'db> {
    let kept = constituents_of(db, current)
        .into_iter()
        .filter_map(|constituent| {
            if constituent.is_null(db) || constituent.is_mixed(db) {
                return Some(constituent);
            }
            if constituent == TypeId::bool(db) {
                return Some(TypeId::bool_literal(db, false));
            }
            if let Some(value) = constituent.bool_literal_value(db) {
                return (!value).then_some(constituent);
            }
            if constituent == TypeId::int(db) {
                return Some(TypeId::int_literal(db, 0));
            }
            if let Some(value) = constituent.int_literal_value(db) {
                return (value == 0).then_some(constituent);
            }
            if constituent == TypeId::float(db) {
                return Some(TypeId::float_literal(db, 0.0));
            }
            if let Some(value) = constituent.float_literal_value(db) {
                return (value == 0.0).then_some(constituent);
            }
            if constituent == TypeId::string(db) {
                return Some(TypeId::union(
                    db,
                    [
                        TypeId::string_literal(db, ""),
                        TypeId::string_literal(db, "0"),
                    ],
                ));
            }
            if let Some(text) = constituent.string_literal_value(db) {
                return (text.is_empty() || text == "0").then_some(constituent);
            }
            // Objects and known classes are always truthy; arrays and
            // everything else undecided stay (empty arrays are falsy).
            if constituent.class_name(db).is_some() || constituent == TypeId::object(db) {
                return None;
            }
            Some(constituent)
        });
    TypeId::union(db, kept)
}

/// The `is_*` family: the callee's folded global name to the type its
/// truth asserts. Unlisted names answer `None` — no facts.
pub(crate) fn type_check_target<'db>(
    db: &'db dyn salsa::Database,
    folded_callee: &str,
) -> Option<TypeId<'db>> {
    Some(match folded_callee {
        "is_string" => TypeId::string(db),
        "is_int" | "is_integer" | "is_long" => TypeId::int(db),
        "is_float" | "is_double" => TypeId::float(db),
        "is_bool" => TypeId::bool(db),
        "is_null" => TypeId::null(db),
        "is_object" => TypeId::object(db),
        "is_resource" => TypeId::resource(db),
        "is_array" => TypeId::array(
            db,
            TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
            TypeId::mixed(db),
        ),
        "is_iterable" => TypeId::iterable(db, TypeId::mixed(db), TypeId::mixed(db)),
        "is_scalar" => TypeId::union(
            db,
            [
                TypeId::int(db),
                TypeId::float(db),
                TypeId::string(db),
                TypeId::bool(db),
            ],
        ),
        "is_numeric" => TypeId::union(
            db,
            [TypeId::int(db), TypeId::float(db), TypeId::numeric_string(db)],
        ),
        "is_countable" => TypeId::union(
            db,
            [
                TypeId::array(
                    db,
                    TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
                    TypeId::mixed(db),
                ),
                TypeId::class(db, "Countable", vec![]),
            ],
        ),
        _ => return None,
    })
}
```

- [ ] **Step 4: Implement `branch_environments` and wire the walker**

In `crates/celerrate_types/src/flow.rs`, three changes.

**(a)** Rename the big `expression` match to `expression_value` —
the `fn` name only: every `self.expression(...)` call inside its arms
stays as written, so operands keep routing through the new wrapper
(a nested condition form in value position must still narrow). Then
add the routing wrapper — condition-shaped forms go through the
branch machinery so their operands narrow and their environments
join, everything else through the value path:

```rust
    fn expression(&mut self, id: ExpressionId, environment: &mut Environment<'db>) -> TypeId<'db> {
        let db = self.db();
        let condition_shaped = matches!(
            self.context.ir.expression(id),
            Some(BodyExpression::Unary {
                operator: SyntaxKind::Bang,
                ..
            }) | Some(BodyExpression::Binary {
                operator: SyntaxKind::AmpersandAmpersand
                    | SyntaxKind::PipePipe
                    | SyntaxKind::And
                    | SyntaxKind::Or
                    | SyntaxKind::InstanceOf
                    | SyntaxKind::EqualsEqualsEquals
                    | SyntaxKind::BangEqualsEquals,
                ..
            })
        );
        if condition_shaped {
            let (when_true, when_false) = self.branch_environments(id, environment);
            *environment = Environment::join_any(db, &when_true, &when_false);
            return self.recorded(id);
        }
        self.expression_value(id, environment)
    }
```

**(b)** Add `branch_environments` and its helpers:

```rust
    /// Types `condition` (side effects included) and answers the two
    /// environments its truth and falsity establish. Composition is
    /// structural: `!` swaps, `&&` chains the true side and joins the
    /// false sides, `||` is the dual (design section 6: negation and
    /// boolean composition are in the floor — early returns are
    /// vacuous without them).
    fn branch_environments(
        &mut self,
        condition: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> (Environment<'db>, Environment<'db>) {
        let db = self.db();
        let ir = self.context.ir;
        match ir.expression(condition).cloned() {
            Some(BodyExpression::Unary {
                operator: SyntaxKind::Bang,
                operand,
            }) => {
                let (when_true, when_false) = self.branch_environments(operand, environment);
                self.record(condition, TypeId::bool(db));
                (when_false, when_true)
            }
            Some(BodyExpression::Binary {
                operator: SyntaxKind::AmpersandAmpersand | SyntaxKind::And,
                lhs,
                rhs,
            }) => {
                let (mut lhs_true, lhs_false) = self.branch_environments(lhs, environment);
                let (rhs_true, rhs_false) = self.branch_environments(rhs, &mut lhs_true);
                self.record(condition, TypeId::bool(db));
                (rhs_true, Environment::join_any(db, &lhs_false, &rhs_false))
            }
            Some(BodyExpression::Binary {
                operator: SyntaxKind::PipePipe | SyntaxKind::Or,
                lhs,
                rhs,
            }) => {
                let (lhs_true, mut lhs_false) = self.branch_environments(lhs, environment);
                let (rhs_true, rhs_false) = self.branch_environments(rhs, &mut lhs_false);
                self.record(condition, TypeId::bool(db));
                (Environment::join_any(db, &lhs_true, &rhs_true), rhs_false)
            }
            Some(BodyExpression::Binary {
                operator: SyntaxKind::InstanceOf,
                lhs,
                rhs,
            }) => {
                let subject_type = self.expression(lhs, environment);
                let target = self.instanceof_target(rhs, environment);
                self.record(condition, TypeId::bool(db));
                let subject = subject_of(ir, lhs);
                self.split_on_subject(environment, subject, subject_type, |walker, current| {
                    walker.narrowed_to(current, target)
                }, |walker, current| {
                    walker.removed_type(current, target)
                })
            }
            Some(BodyExpression::Binary {
                operator: operator @ (SyntaxKind::EqualsEqualsEquals | SyntaxKind::BangEqualsEquals),
                lhs,
                rhs,
            }) => {
                let lhs_type = self.expression(lhs, environment);
                let rhs_type = self.expression(rhs, environment);
                self.record(condition, TypeId::bool(db));
                let sides = if crate::narrowing::is_narrowing_literal(db, rhs_type) {
                    Some((lhs, lhs_type, rhs_type))
                } else if crate::narrowing::is_narrowing_literal(db, lhs_type) {
                    Some((rhs, rhs_type, lhs_type))
                } else {
                    None
                };
                let Some((subject_expression, subject_type, literal)) = sides else {
                    return (environment.clone(), environment.clone());
                };
                let subject = subject_of(ir, subject_expression);
                let (equal, unequal) = (
                    self.narrowed_to(subject_type, literal),
                    self.removed_type(subject_type, literal),
                );
                let mut when_true = environment.clone();
                let mut when_false = environment.clone();
                if let Some(subject) = subject {
                    if operator == SyntaxKind::EqualsEqualsEquals {
                        when_true.bind(subject.clone(), equal);
                        when_false.bind(subject, unequal);
                    } else {
                        when_true.bind(subject.clone(), unequal);
                        when_false.bind(subject, equal);
                    }
                }
                (when_true, when_false)
            }
            Some(BodyExpression::Isset { targets }) => {
                let mut when_true = environment.clone();
                for &target in &targets {
                    self.expression(target, environment);
                    if let Some(subject) = subject_of(ir, target) {
                        let current = self.subject_type(environment, &subject);
                        when_true.bind(subject, current.without_null(db));
                    }
                }
                self.record(condition, TypeId::bool(db));
                // isset false can mean unset, not only null: no facts.
                (when_true, environment.clone())
            }
            Some(BodyExpression::Empty { target }) => {
                self.expression(target, environment);
                self.record(condition, TypeId::bool(db));
                let subject = subject_of(ir, target);
                let current = subject
                    .as_ref()
                    .map(|subject| self.subject_type(environment, subject))
                    .unwrap_or_else(|| TypeId::mixed(db));
                self.split_on_subject(
                    environment,
                    subject,
                    current,
                    |walker, current| crate::narrowing::keep_falsy(walker.db(), current),
                    |walker, current| crate::narrowing::remove_falsy(walker.db(), current),
                )
            }
            // The default: type it, then facts — an is_* call's table
            // facts, or truthiness on the condition's own subject (a
            // bare variable, an assign-and-test).
            _ => {
                self.expression_value(condition, environment);
                if let Some((subject, target)) = self.type_check_facts(condition) {
                    let current = self.subject_type(environment, &subject);
                    let mut when_true = environment.clone();
                    let mut when_false = environment.clone();
                    when_true.bind(subject.clone(), self.narrowed_to(current, target));
                    when_false.bind(subject, self.removed_type(current, target));
                    return (when_true, when_false);
                }
                let subject = subject_of(ir, condition);
                let current = subject
                    .as_ref()
                    .map(|subject| self.subject_type(environment, subject))
                    .unwrap_or_else(|| TypeId::mixed(db));
                self.split_on_subject(
                    environment,
                    subject,
                    current,
                    |walker, current| crate::narrowing::remove_falsy(walker.db(), current),
                    |walker, current| crate::narrowing::keep_falsy(walker.db(), current),
                )
            }
        }
    }

    /// Clone-and-bind: the two environments a decided subject fact
    /// produces. No subject, no facts — two clones.
    fn split_on_subject(
        &mut self,
        environment: &Environment<'db>,
        subject: Option<NarrowingSubject>,
        current: TypeId<'db>,
        when_true: impl FnOnce(&mut Self, TypeId<'db>) -> TypeId<'db>,
        when_false: impl FnOnce(&mut Self, TypeId<'db>) -> TypeId<'db>,
    ) -> (Environment<'db>, Environment<'db>) {
        let mut true_env = environment.clone();
        let mut false_env = environment.clone();
        if let Some(subject) = subject {
            let positive = when_true(self, current);
            let negative = when_false(self, current);
            true_env.bind(subject.clone(), positive);
            false_env.bind(subject, negative);
        }
        (true_env, false_env)
    }

    fn narrowed_to(&self, current: TypeId<'db>, target: TypeId<'db>) -> TypeId<'db> {
        crate::narrowing::narrow_to(
            self.db(),
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            current,
            target,
        )
    }

    fn removed_type(&self, current: TypeId<'db>, target: TypeId<'db>) -> TypeId<'db> {
        crate::narrowing::remove_type(
            self.db(),
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            current,
            target,
        )
    }

    /// The instanceof right-hand side: a written class name resolves
    /// at the declaring site; an expression whose type is
    /// `class-string<T>` or a class contributes that class; anything
    /// else narrows nothing (`mixed`).
    fn instanceof_target(
        &mut self,
        rhs: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        match self.context.ir.expression(rhs).cloned() {
            Some(BodyExpression::NamedReference { text }) => {
                if operators::named_reference_type(db, &text).is_some() {
                    // `true`/`false`/`null` are not class names.
                    TypeId::mixed(db)
                } else {
                    self.record(rhs, TypeId::mixed(db));
                    self.class_type_of_written(&text)
                }
            }
            _ => {
                let of = self.expression(rhs, environment);
                if let Some(argument) = of.class_string_argument(db).flatten() {
                    argument
                } else if of.class_name(db).is_some() {
                    of
                } else {
                    TypeId::mixed(db)
                }
            }
        }
    }

    /// `is_string($x)`-shaped calls: the callee is an unqualified or
    /// root-qualified name in the table, the first argument is a
    /// subject, no spread. (PHP resolves unqualified function names
    /// through the namespace with a global fallback; the `is_*` names
    /// are never redeclared in practice — and if one is, the wrong
    /// narrowing is over-narrowing on working code, which the plan-8
    /// corpus gate would catch. Recorded stance.)
    fn type_check_facts(
        &mut self,
        condition: ExpressionId,
    ) -> Option<(NarrowingSubject, TypeId<'db>)> {
        let db = self.db();
        let ir = self.context.ir;
        let Some(BodyExpression::Call { callee, arguments }) = ir.expression(condition) else {
            return None;
        };
        let Some(BodyExpression::NamedReference { text }) = ir.expression(*callee) else {
            return None;
        };
        let name = text.strip_prefix('\\').unwrap_or(text);
        if name.contains('\\') {
            return None;
        }
        let target = crate::narrowing::type_check_target(db, &name.to_ascii_lowercase())?;
        let first = arguments.first().filter(|argument| !argument.spread)?;
        let subject = subject_of(ir, first.value)?;
        Some((subject, target))
    }
```

**(c)** Rewire the branch consumers. Replace the Task 3 `If` arm:

```rust
            BodyStatement::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let (mut when_true, mut when_false) =
                    self.branch_environments(condition, environment);
                self.statements(&then_branch, &mut when_true);
                self.statements(&else_branch, &mut when_false);
                *environment = Environment::join(db, &when_true, &when_false);
            }
```

Replace the `While` arm's pass closure so the body walks under the
condition's truth and the loop exits under its falsity:

```rust
            BodyStatement::While { condition, body } => {
                self.looped(environment, |walker, env| {
                    let (mut when_true, when_false) =
                        walker.branch_environments(condition, env);
                    walker.statements(&body, &mut when_true);
                    *env = when_true;
                    when_false
                });
            }
```

Replace the `Ternary` arm's two clones with the branch environments:

```rust
            BodyExpression::Ternary {
                condition,
                middle,
                alternative,
            } => {
                let (mut when_true, mut when_false) =
                    self.branch_environments(condition, environment);
                let middle_type = match middle {
                    Some(middle) => self.expression(middle, &mut when_true),
                    None => crate::narrowing::remove_falsy(db, self.recorded(condition)),
                };
                let alternative_type = self.expression(alternative, &mut when_false);
                *environment = Environment::join(db, &when_true, &when_false);
                join(db, middle_type, alternative_type)
            }
```

(The short ternary's value tightens too: a truthy condition value
cannot be falsy.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS.
Then the full local gate. If a display assertion fails on rendering
only, apply decision 16.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/flow.rs crates/celerrate_types/src/inference.rs
git commit -m "✨ feat(types): the narrowing floor's condition forms"
```

---
### Task 5: `match`, `switch`, coalescing, and `assert()`

The remaining statement-shaped narrowing forms: `match` arms
(including the `match(true)` idiom and default-arm subtraction),
`switch` with strict-safe cases, `??` and `??=` (which drop null from
their left operand), and the `assert()` special form.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: Task 4's `branch_environments`, `narrowed_to`,
  `removed_type`, `is_narrowing_literal`, `subject_of`,
  `TypeId::without_null`, `crate::judgments::{Proof, subtype_of}`.
- Produces: the rewritten `Match` arm, the narrowing `Switch` arm, the
  `QuestionQuestion` arm in `expression_value`, the
  `QuestionQuestionEquals` branch in `assignment`, and the `assert`
  pre-check in the `Call` arm (which Tasks 6 and 9 must keep as the
  first check when they rewrite that arm).

- [ ] **Step 1: Write the failing tests**

Add to `crates/celerrate_types/src/inference.rs`'s test module:

```rust
    #[test]
    fn match_arms_narrow_their_subject_and_the_default_subtracts() {
        let fixture = fixture(&[
            "<?php function f(int|string $x) {
                return match ($x) { 1, 2 => $x, default => 'other' };
            }",
        ]);
        // Arm: 1|2. Default: the literals are not subtractable from
        // the general int, so int|string stays — joined with the arm.
        assert_eq!(return_display(&fixture, 0), "1|2|'other'");
    }

    #[test]
    fn the_match_true_idiom_narrows_by_arm_condition() {
        let fixture = fixture(&[
            "<?php function f(mixed $x) {
                return match (true) { is_string($x) => $x, default => 1 };
            }",
        ]);
        assert_eq!(return_display(&fixture, 0), "1|string");
    }

    #[test]
    fn switch_narrows_strict_safe_cases() {
        let fixture = fixture(&[
            "<?php function f(int $x) {
                switch ($x) { case 1: return $x; }
                return 2;
            }",
        ]);
        assert_eq!(return_display(&fixture, 0), "1|2");
    }

    #[test]
    fn coalescing_drops_null_from_its_left_operand() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function coalesce(?string $x) { return $x ?? 'd'; }
            function keeps(?Foo $x) { return $x ?? null; }
            function assigns(?int $x) { $x ??= 0; return $x; }",
        ]);
        // join(string, 'd') absorbs the literal.
        assert_eq!(return_display(&fixture, 1), "string");
        assert_eq!(return_display(&fixture, 2), "null|foo");
        assert_eq!(return_display(&fixture, 3), "int");
    }

    #[test]
    fn assert_narrows_the_rest_of_the_body() {
        let fixture = fixture(&[
            "<?php class Foo {}
            function f(mixed $x) { assert($x instanceof Foo); return $x; }",
        ]);
        assert_eq!(return_display(&fixture, 1), "foo");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -8`
Expected: FAIL — match/switch/coalesce answers are unnarrowed joins,
`assert` narrows nothing.

- [ ] **Step 3: Implement the forms**

Four changes in `crates/celerrate_types/src/flow.rs`.

**(a)** Replace the `Match` arm of `expression_value`:

```rust
            BodyExpression::Match { subject, arms } => {
                let subject_type = self.expression(subject, environment);
                // `match (true)` — the subject is the literal `true`.
                let is_match_true = matches!(
                    self.context.ir.expression(subject),
                    Some(BodyExpression::NamedReference { text })
                        if text.eq_ignore_ascii_case("true")
                );
                let match_subject = subject_of(self.context.ir, subject);
                let mut result: Option<TypeId<'db>> = None;
                let mut exits: Vec<Environment<'db>> = Vec::new();
                let mut seen_condition_types: Vec<TypeId<'db>> = Vec::new();
                for arm in &arms {
                    let mut arm_env = environment.clone();
                    if is_match_true {
                        // Each condition is itself a condition; the
                        // arm runs when any of them held.
                        let mut condition_envs: Vec<Environment<'db>> = Vec::new();
                        for condition in &arm.conditions {
                            let mut base = environment.clone();
                            let (when_true, _) = self.branch_environments(*condition, &mut base);
                            condition_envs.push(when_true);
                        }
                        if let Some(joined) = condition_envs
                            .into_iter()
                            .reduce(|left, right| Environment::join_any(db, &left, &right))
                        {
                            arm_env = joined;
                        }
                    } else {
                        let mut literals: Vec<TypeId<'db>> = Vec::new();
                        let mut all_literal = !arm.conditions.is_empty();
                        for condition in &arm.conditions {
                            let condition_type = self.expression(*condition, &mut arm_env);
                            if crate::narrowing::is_narrowing_literal(db, condition_type) {
                                literals.push(condition_type);
                            } else {
                                all_literal = false;
                            }
                        }
                        seen_condition_types.extend(literals.iter().copied());
                        if arm.is_default {
                            // The default arm subtracts every literal
                            // condition seen across the arms.
                            if let Some(match_subject) = &match_subject {
                                let mut current = self.subject_type(&arm_env, match_subject);
                                for literal in &seen_condition_types {
                                    current = self.removed_type(current, *literal);
                                }
                                arm_env.bind(match_subject.clone(), current);
                            }
                        } else if all_literal
                            && let Some(match_subject) = &match_subject
                        {
                            let current = self.subject_type(&arm_env, match_subject);
                            let target = TypeId::union(db, literals.iter().copied());
                            arm_env.bind(match_subject.clone(), self.narrowed_to(current, target));
                        }
                    }
                    let body_type = self.expression(arm.body, &mut arm_env);
                    result = Some(match result {
                        Some(previous) => join(db, previous, body_type),
                        None => body_type,
                    });
                    exits.push(arm_env);
                }
                let _ = subject_type;
                if let Some(post) = exits
                    .into_iter()
                    .reduce(|left, right| Environment::join(db, &left, &right))
                {
                    *environment = post;
                }
                result.unwrap_or_else(|| TypeId::never(db))
            }
```

One ordering caveat the code embodies: `seen_condition_types` is
collected in arm order, so a `default` arm written before a literal
arm subtracts only what precedes it — PHP evaluates arms in order, so
this is the semantics, not an approximation.

**(b)** In the `Switch` arm, replace the per-case entry computation.
A case narrows when its condition types as an int or string literal
**and** the subject is entirely that scalar family (loose equality
coincides with strict there — the design's "strict cases"):

```rust
                for case in &cases {
                    let mut case_fact: Option<(NarrowingSubject, TypeId<'db>)> = None;
                    if let Some(condition) = case.condition {
                        let mut scratch = pre.clone();
                        let condition_type = self.expression(condition, &mut scratch);
                        let literal_family = if condition_type.int_literal_value(db).is_some() {
                            Some(TypeId::int(db))
                        } else if condition_type.string_literal_value(db).is_some() {
                            Some(TypeId::string(db))
                        } else {
                            None
                        };
                        if let (Some(family), Some(switch_subject)) =
                            (literal_family, subject_of(self.context.ir, subject))
                        {
                            let current = self.subject_type(&pre, &switch_subject);
                            let strict_safe = crate::judgments::subtype_of(
                                db,
                                self.context.files,
                                self.context.stubs,
                                self.context.configuration,
                                current,
                                family,
                            ) == crate::judgments::Proof::Holds;
                            if strict_safe {
                                case_fact = Some((
                                    switch_subject,
                                    self.narrowed_to(current, condition_type),
                                ));
                            }
                        }
                    } else {
                        has_default = true;
                    }
                    let mut narrowed_pre = pre.clone();
                    if let Some((switch_subject, narrowed)) = case_fact {
                        narrowed_pre.bind(switch_subject, narrowed);
                    }
                    let mut entry = match fall_in.take() {
                        Some(previous) => Environment::join_any(db, &previous, &narrowed_pre),
                        None => narrowed_pre,
                    };
                    self.statements(&case.statements, &mut entry);
                    if entry.is_reachable() {
                        fall_in = Some(entry.clone());
                    }
                    exits.push(entry);
                }
```

**(c)** Add the `??` arm to `expression_value`, **before** the generic
`Binary` arm:

```rust
            BodyExpression::Binary {
                operator: SyntaxKind::QuestionQuestion,
                lhs,
                rhs,
            } => {
                let lhs_type = self.expression(lhs, environment);
                let subject = subject_of(self.context.ir, lhs);
                // The right operand evaluates only when the left was
                // null; the result path re-joins both sides.
                let mut when_null = environment.clone();
                let mut when_present = environment.clone();
                if let Some(subject) = &subject {
                    when_null.bind(subject.clone(), TypeId::null(db));
                    let current = self.subject_type(environment, subject);
                    when_present.bind(subject.clone(), current.without_null(db));
                }
                let rhs_type = self.expression(rhs, &mut when_null);
                *environment = Environment::join_any(db, &when_present, &when_null);
                join(db, lhs_type.without_null(db), rhs_type)
            }
```

(`TypeId::union` drops `never`, so an always-null left operand
contributes nothing to the join.)

And the `??=` branch at the top of `assignment` (before the
`by_reference` check is fine too; order between them is immaterial —
`??=` has no by-reference form):

```rust
        if operator == SyntaxKind::QuestionQuestionEquals {
            let current = self.recorded(target);
            let assigned = join(db, current.without_null(db), value_type);
            self.assign_target(target, assigned, environment);
            return assigned;
        }
```

(The value operand was already walked unconditionally by the
`Assignment` arm — its environment effects apply on both paths, a
recorded conservative approximation.)

**(d)** In the `Call` arm of `expression_value`, add the `assert`
pre-check before the existing generic walk (Tasks 6 and 9 keep this
check first when they rewrite the arm):

```rust
            BodyExpression::Call { callee, arguments } => {
                if let Some(BodyExpression::NamedReference { text }) =
                    self.context.ir.expression(callee)
                {
                    let name = text.strip_prefix('\\').unwrap_or(text.as_str());
                    if name.eq_ignore_ascii_case("assert")
                        && let Some(first) = arguments.first().filter(|argument| !argument.spread)
                    {
                        let value = first.value;
                        self.record(callee, TypeId::mixed(db));
                        let (when_true, _) = self.branch_environments(value, environment);
                        *environment = when_true;
                        for argument in arguments.iter().skip(1) {
                            self.expression(argument.value, environment);
                        }
                        return self.record(id, TypeId::bool(db));
                    }
                }
                // ... the existing generic walk stays below ...
            }
```

(The early `return self.record(id, ...)` short-circuits the arm's
normal `of` flow — `id` is `expression_value`'s own parameter, and
`record` both stores and answers the type, so the shape matches every
other early exit.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS.
Full local gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/inference.rs
git commit -m "✨ feat(types): match, switch, coalescing, and assert narrowing"
```

---

### Task 6: Member reads and calls through declared signatures

`$this` types as the defining class; property, class-constant, and
enum-case reads, method and static calls resolve through
`declared_member_signature`; `new` types; `self::`/`parent::`/
`static::` resolve against the defining class; placeholder
substitution at call sites (decision 6); union/intersection receiver
stances (decision 11). Declared-return edges start counting.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: `celerrate_semantics::{AncestorRelation, ClassQuery,
  ClassReference, MemberKind, MemberQuery, MemberReference,
  folded_member_key, linearized_class}`,
  `crate::declared::{DeclaredSignature, Trust,
  declared_member_signature}`.
- Produces (on `Walker`): `fn this_type(&self) -> TypeId<'db>`,
  `fn receiver_parts(&self, of: TypeId<'db>) -> Option<Vec<String>>`
  (class-like keys, `None` = opaque),
  `member_value_type(&self, keys: &[String], kind: MemberKind,
  name: &str) -> Option<TypeId<'db>>`, `method_signatures(&self, keys:
  &[String], name: &str) -> Vec<DeclaredSignature<'db>>`,
  `declared_present(&self, signature: &DeclaredSignature<'db>) ->
  bool`, `substitute_receiver(&self, of: TypeId<'db>, receiver:
  TypeId<'db>) -> TypeId<'db>`, `scoped_subject(&mut self, subject:
  ExpressionId, environment) -> (TypeId<'db>, Option<Vec<String>>)`,
  and `parent_class_key(&self) -> Option<String>`. Tasks 7-11 build on
  all of these.

- [ ] **Step 1: Write the failing tests**

Add to `crates/celerrate_types/src/inference.rs`'s test module:

```rust
    #[test]
    fn this_and_its_property_reads_type_from_the_declaration() {
        let fixture = fixture(&[
            "<?php class A {
                public ?string $s = null;
                public function own() { return $this; }
                public function read() { return $this->s; }
            }",
        ]);
        // Numbering: class 0, property 1, own 2, read 3.
        assert_eq!(return_display(&fixture, 2), "a");
        assert_eq!(return_display(&fixture, 3), "null|string");
    }

    #[test]
    fn method_calls_take_declared_returns_and_count_the_edge() {
        let fixture = fixture(&[
            "<?php class A { public function name(): string { return 'a'; } }
            function f(A $a) { return $a->name(); }",
        ]);
        assert_eq!(return_display(&fixture, 2), "string");
        let file = fixture.handles[0];
        let body = body_query(&fixture, 2);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.declared_return_edges, 1);
    }

    #[test]
    fn fluent_static_returns_substitute_the_receiver() {
        let fixture = fixture(&[
            "<?php class Builder { public function with(): static { return $this; } }
            function f(Builder $b) { return $b->with(); }",
        ]);
        assert_eq!(return_display(&fixture, 2), "builder");
    }

    #[test]
    fn static_calls_and_scoped_reads_resolve() {
        let fixture = fixture(&[
            "<?php class K {
                const int N = 1;
                public static function make(): float { return 1.0; }
            }
            function call() { return K::make(); }
            function constant() { return K::N; }
            function name() { return K::class; }",
        ]);
        assert_eq!(return_display(&fixture, 3), "float");
        assert_eq!(return_display(&fixture, 4), "int");
        assert_eq!(return_display(&fixture, 5), "class-string<k>");
    }

    #[test]
    fn an_enum_case_read_types_as_the_case() {
        let fixture = fixture(&["<?php enum E { case A; } function f() { return E::A; }"]);
        let file = fixture.handles[0];
        let body = body_query(&fixture, 2);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        assert_eq!(
            inferred.return_type.enum_case_parts(&fixture.db),
            Some(("e".to_owned(), "A".to_owned())),
        );
    }

    #[test]
    fn union_receivers_join_and_opaque_receivers_stay_silent() {
        let fixture = fixture(&[
            "<?php class A { public function n(): int { return 1; } }
            class B { public function n(): string { return 'b'; } }
            function joined(A|B $x) { return $x->n(); }
            function nullable(?A $x) { return $x->n(); }
            function opaque(mixed $x) { return $x->n(); }",
        ]);
        assert_eq!(return_display(&fixture, 4), "int|string");
        // The null constituent is the nullability family's business
        // (plan 8); the read types from the non-null part.
        assert_eq!(return_display(&fixture, 5), "int");
        assert_eq!(return_display(&fixture, 6), "mixed");
    }

    #[test]
    fn parent_and_self_resolve_against_the_defining_class() {
        let fixture = fixture(&[
            "<?php class Base { public function root(): int { return 1; } }
            class Child extends Base {
                public function up() { return parent::root(); }
                public function own() { return self::class; }
            }",
        ]);
        // Numbering: Base 0, root 1, Child 2, up 3, own 4.
        assert_eq!(return_display(&fixture, 3), "int");
        assert_eq!(return_display(&fixture, 4), "class-string<child>");
    }

    #[test]
    fn new_types_as_the_class_and_anonymous_stays_mixed() {
        let fixture = fixture(&[
            "<?php class A {}
            function named() { return new A(); }
            function anonymous() { return new class {}; }",
        ]);
        assert_eq!(return_display(&fixture, 1), "a");
        assert_eq!(return_display(&fixture, 2), "mixed");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -8`
Expected: FAIL — the reads and calls all answer `mixed`.

- [ ] **Step 3: Implement the resolution helpers**

Add to `crates/celerrate_types/src/flow.rs` (inside `impl Walker`).
New imports: `celerrate_semantics::{AncestorRelation, ClassQuery,
ClassReference, MemberKind, MemberQuery, MemberReference,
folded_member_key, linearized_class}`,
`crate::declared::{DeclaredSignature, Trust, declared_member_signature}`.

```rust
    /// `$this`: the defining class in a non-static method; `mixed`
    /// (silence) everywhere else. Plan 6 replaces this with the
    /// symbolic late-static-binding placeholder (decision 5).
    fn this_type(&self) -> TypeId<'db> {
        match (&self.context.owner_class_key, self.context.method_is_static) {
            (Some(key), false) => TypeId::class(self.db(), key, vec![]),
            _ => TypeId::mixed(self.db()),
        }
    }

    /// The class-like keys a receiver type addresses; `None` when any
    /// part is opaque (mixed, object, a scalar, an unresolvable
    /// shape) — the silent stance of decision 11. Union constituents
    /// must all resolve (null skipped: the nullability family's
    /// business); intersection contributes every resolving part.
    fn receiver_parts(&self, of: TypeId<'db>) -> Option<Vec<String>> {
        let db = self.db();
        if of == TypeId::self_placeholder(db) || of == TypeId::static_placeholder(db) {
            return self.context.owner_class_key.clone().map(|key| vec![key]);
        }
        if let Some(name) = of.class_name(db) {
            return Some(vec![name]);
        }
        if let Some((enum_name, _)) = of.enum_case_parts(db) {
            return Some(vec![enum_name]);
        }
        let constituents = of.constituents(db);
        if !constituents.is_empty() {
            let mut keys = Vec::new();
            for part in constituents {
                if part.is_null(db) {
                    continue;
                }
                keys.extend(self.receiver_parts(part)?);
            }
            return (!keys.is_empty()).then_some(keys);
        }
        let intersectands = of.intersectands(db);
        if !intersectands.is_empty() {
            let mut keys = Vec::new();
            for part in intersectands {
                if let Some(mut resolved) = self.receiver_parts(part) {
                    keys.append(&mut resolved);
                }
            }
            return (!keys.is_empty()).then_some(keys);
        }
        if let Some(bound) = of.template_bound(db) {
            return self.receiver_parts(bound);
        }
        None
    }

    /// A member's declared value across the receiver keys: the join
    /// of the keys that answer; `None` when none does.
    fn member_value_type(
        &self,
        keys: &[String],
        kind: MemberKind,
        name: &str,
    ) -> Option<TypeId<'db>> {
        let db = self.db();
        keys.iter()
            .filter_map(|key| {
                declared_member_signature(
                    db,
                    self.context.files,
                    self.context.stubs,
                    self.context.configuration,
                    MemberQuery::new(db, key.clone(), kind, folded_member_key(kind, name)),
                )
                .map(|signature| signature.value_type)
            })
            .reduce(|left, right| join(db, left, right))
    }

    /// The declared method signatures across the receiver keys, in
    /// key order (empty when none resolves).
    fn method_signatures(&self, keys: &[String], name: &str) -> Vec<DeclaredSignature<'db>> {
        let db = self.db();
        keys.iter()
            .filter_map(|key| {
                declared_member_signature(
                    db,
                    self.context.files,
                    self.context.stubs,
                    self.context.configuration,
                    MemberQuery::new(
                        db,
                        key.clone(),
                        MemberKind::Method,
                        folded_member_key(MemberKind::Method, name),
                    ),
                )
            })
            .collect()
    }

    /// The declared-present gate (decision 4).
    fn declared_present(&self, signature: &DeclaredSignature<'db>) -> bool {
        signature.value_trust != Trust::NativeOnly || !signature.value_type.is_mixed(self.db())
    }

    /// Decision 6: `self`/`static` placeholders in a declared return
    /// substitute the receiver's type, top level and one level into
    /// unions; `parent` answers mixed. Plan 6 replaces this with the
    /// forwarding model.
    fn substitute_receiver(&self, of: TypeId<'db>, receiver: TypeId<'db>) -> TypeId<'db> {
        let db = self.db();
        if of == TypeId::self_placeholder(db) || of == TypeId::static_placeholder(db) {
            return receiver;
        }
        if of == TypeId::parent_placeholder(db) {
            return TypeId::mixed(db);
        }
        let constituents = of.constituents(db);
        if !constituents.is_empty() {
            return TypeId::union(
                db,
                constituents
                    .into_iter()
                    .map(|part| self.substitute_receiver(part, receiver)),
            );
        }
        of
    }

    /// The first `extends` edge of the defining class's ancestry.
    fn parent_class_key(&self) -> Option<String> {
        let db = self.db();
        let owner = self.context.owner_class_key.as_ref()?;
        let linearized = linearized_class(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            ClassQuery::new(db, owner.clone()),
        )
        .as_ref()?;
        linearized
            .ancestry
            .iter()
            .find(|edge| edge.relation == AncestorRelation::Extends)
            .and_then(|edge| edge.resolved.clone())
    }

    /// A `Foo::`/`self::`/`static::`/`parent::` subject: its type for
    /// substitution and the keys it addresses. An expression subject
    /// (an object, a `class-string`) resolves through its type.
    fn scoped_subject(
        &mut self,
        subject: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> (TypeId<'db>, Option<Vec<String>>) {
        let db = self.db();
        match self.context.ir.expression(subject).cloned() {
            Some(BodyExpression::NamedReference { text }) => {
                let folded = text.to_ascii_lowercase();
                let keys = match folded.as_str() {
                    "self" | "static" => self.context.owner_class_key.clone().map(|key| vec![key]),
                    "parent" => self.parent_class_key().map(|key| vec![key]),
                    _ => {
                        let class = self.class_type_of_written(&text);
                        self.record(subject, TypeId::mixed(db));
                        let keys = class.class_name(db).map(|name| vec![name]);
                        let of = class;
                        return (of, keys);
                    }
                };
                self.record(subject, TypeId::mixed(db));
                let of = keys
                    .as_ref()
                    .and_then(|keys| keys.first())
                    .map(|key| TypeId::class(db, key, vec![]))
                    .unwrap_or_else(|| TypeId::mixed(db));
                (of, keys)
            }
            _ => {
                let of = self.expression(subject, environment);
                let through_class_string = of.class_string_argument(db).flatten();
                let effective = through_class_string.unwrap_or(of);
                (effective, self.receiver_parts(effective))
            }
        }
    }
```

- [ ] **Step 4: Rewire the reading and calling arms**

Still in `flow.rs`, replace four `expression_value` arms.

**(a)** The `Variable` arm's `"this"` case: replace `TypeId::mixed(db)`
with `self.this_type()`.

**(b)** The `MemberAccess` arm (property reads; the method-call case
is the `Call` arm's, which checks its callee shape first):

```rust
            BodyExpression::MemberAccess {
                receiver,
                member,
                null_safe,
            } => {
                let receiver_type = self.expression(receiver, environment);
                // Task 7 refines the null_safe receiver; until then
                // resolve through the non-null part unconditionally —
                // identical member answers either way.
                let resolving = receiver_type.without_null(db);
                let _ = null_safe;
                match member {
                    MemberReference::Named { name } => self
                        .receiver_parts(resolving)
                        .and_then(|keys| {
                            self.member_value_type(&keys, MemberKind::Property, &name)
                        })
                        .unwrap_or_else(|| TypeId::mixed(db)),
                    MemberReference::Computed { expression } => {
                        self.expression(expression, environment);
                        TypeId::mixed(db)
                    }
                    MemberReference::Variable { .. } | MemberReference::Missing => {
                        TypeId::mixed(db)
                    }
                }
            }
```

**(c)** The `ScopedAccess` arm:

```rust
            BodyExpression::ScopedAccess { subject, member } => {
                let (subject_type, keys) = self.scoped_subject(subject, environment);
                let _ = subject_type;
                match member {
                    MemberReference::Named { name } => {
                        if name.eq_ignore_ascii_case("class") {
                            // `Foo::class`, `self::class`, `static::class`.
                            let argument = keys
                                .as_ref()
                                .and_then(|keys| keys.first())
                                .map(|key| TypeId::class(db, key, vec![]));
                            TypeId::class_string(db, argument)
                        } else {
                            keys.as_ref()
                                .and_then(|keys| {
                                    self.member_value_type(
                                        keys,
                                        MemberKind::ClassConstant,
                                        &name,
                                    )
                                    .or_else(|| {
                                        self.member_value_type(keys, MemberKind::EnumCase, &name)
                                    })
                                })
                                .unwrap_or_else(|| TypeId::mixed(db))
                        }
                    }
                    // `Foo::$prop`: a static property is a Property.
                    MemberReference::Variable { name } => keys
                        .as_ref()
                        .and_then(|keys| {
                            self.member_value_type(keys, MemberKind::Property, &name)
                        })
                        .unwrap_or_else(|| TypeId::mixed(db)),
                    MemberReference::Computed { expression } => {
                        self.expression(expression, environment);
                        TypeId::mixed(db)
                    }
                    MemberReference::Missing => TypeId::mixed(db),
                }
            }
```

**(d)** The `Call` arm: after Task 5's `assert` pre-check, route by
callee shape. Method and static calls resolve here; everything else
keeps the Task 3 behavior until Task 9:

```rust
            BodyExpression::Call { callee, arguments } => {
                // (Task 5's `assert` pre-check stays here, first.)
                match self.context.ir.expression(callee).cloned() {
                    Some(BodyExpression::MemberAccess {
                        receiver,
                        member: MemberReference::Named { name },
                        null_safe,
                    }) => {
                        let receiver_type = self.expression(receiver, environment);
                        let resolving = receiver_type.without_null(db);
                        let _ = null_safe;
                        self.record(callee, TypeId::mixed(db));
                        // Task 9's provider tier reads the argument
                        // types; until then they type for the table.
                        let _argument_types = self.typed_arguments(&arguments, environment);
                        self.method_call_result(resolving, &name)
                    }
                    Some(BodyExpression::ScopedAccess {
                        subject,
                        member: MemberReference::Named { name },
                    }) => {
                        let (subject_type, keys) = self.scoped_subject(subject, environment);
                        self.record(callee, TypeId::mixed(db));
                        let _argument_types = self.typed_arguments(&arguments, environment);
                        match keys {
                            Some(keys) => {
                                let receiver = subject_type;
                                self.method_call_result_for_keys(&keys, receiver, &name)
                            }
                            None => TypeId::mixed(db),
                        }
                    }
                    _ => {
                        // Task 9: named function calls and callable
                        // values. Until then: walk and stay silent.
                        self.expression(callee, environment);
                        self.typed_arguments(&arguments, environment);
                        TypeId::mixed(db)
                    }
                }
            }
```

With the three call helpers:

```rust
    fn typed_arguments(
        &mut self,
        arguments: &[celerrate_semantics::CallArgument],
        environment: &mut Environment<'db>,
    ) -> Vec<TypeId<'db>> {
        arguments
            .iter()
            .map(|argument| self.expression(argument.value, environment))
            .collect()
    }

    /// An instance method call's result on a resolved receiver type.
    fn method_call_result(&mut self, receiver: TypeId<'db>, name: &str) -> TypeId<'db> {
        match self.receiver_parts(receiver) {
            Some(keys) => self.method_call_result_for_keys(&keys, receiver, name),
            None => TypeId::mixed(self.db()),
        }
    }

    /// The declared-return path (decision 3's middle tier): a
    /// declared return per resolving key, joined; the gate failing on
    /// any key answers mixed for that key (method-inferred returns
    /// are plan 6). Placeholders substitute the receiver (decision 6).
    fn method_call_result_for_keys(
        &mut self,
        keys: &[String],
        receiver: TypeId<'db>,
        name: &str,
    ) -> TypeId<'db> {
        let db = self.db();
        let signatures = self.method_signatures(keys, name);
        if signatures.is_empty() {
            return TypeId::mixed(db);
        }
        let mut result: Option<TypeId<'db>> = None;
        let mut any_declared = false;
        for signature in &signatures {
            let value = if self.declared_present(signature) {
                any_declared = true;
                self.substitute_receiver(signature.value_type, receiver)
            } else {
                TypeId::mixed(db)
            };
            result = Some(match result {
                Some(previous) => join(db, previous, value),
                None => value,
            });
        }
        if any_declared {
            self.edge_counts.declared_return_edges += 1;
        }
        result.unwrap_or_else(|| TypeId::mixed(db))
    }
```

**(e)** The `New` arm:

```rust
            BodyExpression::New { class, arguments } => {
                let of = match &class {
                    ClassReference::Named { name } => self.class_type_of_written(name),
                    ClassReference::StaticKeyword => self.this_type(),
                    ClassReference::Dynamic { expression } => {
                        let dynamic = self.expression(*expression, environment);
                        dynamic
                            .class_string_argument(db)
                            .flatten()
                            .or_else(|| dynamic.class_name(db).map(|name| {
                                TypeId::class(db, &name, dynamic.class_arguments(db))
                            }))
                            .unwrap_or_else(|| TypeId::mixed(db))
                    }
                    // Decision 12: no folded key exists yet.
                    ClassReference::Anonymous { .. } | ClassReference::Missing => {
                        TypeId::mixed(db)
                    }
                };
                for argument in &arguments {
                    self.expression(argument.value, environment);
                }
                of
            }
```

(In the `Dynamic` case `new $instance` clones the instance's class —
`class_name`+`class_arguments` rebuild it; a plain `TypeId::class`
receiver would round-trip identically, and the rebuild keeps generic
arguments when plan 6 starts producing them.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS.
Full local gate. (`static_calls_and_scoped_reads_resolve` uses a PHP
8.3 typed constant, inside the fixture range 8.1-8.5; if the typed
constant's availability gate interferes in the test corpus, use
`const N = 1` with a `@var`-free assertion on `int_bounds` instead —
the declared layer's constant handling is plan 3's contract, not
this plan's.)

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/inference.rs
git commit -m "✨ feat(types): member reads and calls resolve through declared signatures"
```

---

### Task 7: The null-safe chain rule

`?->` with the design's whole-chain semantics: inside the
short-circuited suffix the receiver is the non-null type; only the
final chain result re-acquires `|null`. The body IR already wraps
every chain containing `?->` exactly once in `NullSafeChain`.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: the `NullSafeChain` wrapper contract
  (`crates/celerrate_semantics/src/body.rs:221-224`), Task 6's
  receiver machinery, `crate::judgments::{Nullability, nullability}`.
- Produces: the `null_safe_reacquires: bool` walker field and the
  rewritten `NullSafeChain` arm; the `null_safe` handling inside the
  `MemberAccess` arm and the method-call callee path. Task 8's
  env-first property reads sit above this.

- [ ] **Step 1: Write the failing tests**

Add to `crates/celerrate_types/src/inference.rs`'s test module:

```rust
    #[test]
    fn a_null_safe_chain_reacquires_null_once_at_the_end() {
        let fixture = fixture(&[
            "<?php class B { public function c(): int { return 1; } }
            class A { public function b(): B { return new B(); } }
            function f(?A $a) { return $a?->b()->c(); }",
        ]);
        // One null receiver short-circuits the whole chain: the inner
        // ->c() sees B (never B|null), the chain result is int|null.
        assert_eq!(return_display(&fixture, 4), "null|int");
    }

    #[test]
    fn a_narrowed_receiver_reacquires_nothing() {
        let fixture = fixture(&[
            "<?php class B {}
            class A { public function b(): B { return new B(); } }
            function f(?A $a) {
                if ($a === null) { return 1; }
                return $a?->b();
            }",
        ]);
        assert_eq!(return_display(&fixture, 3), "1|b");
    }

    #[test]
    fn every_null_safe_link_strips_before_resolving() {
        let fixture = fixture(&[
            "<?php class B { public function c(): int { return 1; } }
            class A { public function b(): ?B { return null; } }
            function f(?A $a) { return $a?->b()?->c(); }",
        ]);
        assert_eq!(return_display(&fixture, 4), "null|int");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -6`
Expected: FAIL — chains currently type without the re-acquired null
(`"int"` where `"null|int"` is expected).

- [ ] **Step 3: Implement the rule**

In `crates/celerrate_types/src/flow.rs`:

**(a)** Add the walker field (initialize `false` in `walk_body`):

```rust
    /// Set while typing inside a `NullSafeChain` when a `?->` link's
    /// receiver was possibly null: the wrapper re-acquires `|null`
    /// once, at the end (the design's whole-chain rule).
    null_safe_reacquires: bool,
```

**(b)** Replace the `NullSafeChain` arm:

```rust
            BodyExpression::NullSafeChain { chain } => {
                let saved = std::mem::replace(&mut self.null_safe_reacquires, false);
                let chain_type = self.expression(chain, environment);
                let reacquires = std::mem::replace(&mut self.null_safe_reacquires, saved);
                if reacquires {
                    join(db, chain_type, TypeId::null(db))
                } else {
                    chain_type
                }
            }
```

**(c)** In the `MemberAccess` arm (Task 6's) and the method-call
callee path of the `Call` arm, replace the `let resolving =
receiver_type.without_null(db); let _ = null_safe;` pair with the
real rule:

```rust
                let resolving = if null_safe {
                    if crate::judgments::nullability(db, receiver_type)
                        != crate::judgments::Nullability::NeverNull
                    {
                        self.null_safe_reacquires = true;
                    }
                    receiver_type.without_null(db)
                } else {
                    receiver_type
                };
```

(The plain `->` case resolves on the unstripped type: `member_value_type`
and `method_call_result` already skip null constituents through
`receiver_parts` — reporting the possibly-null dereference is the
nullability family's job in plan 8, and typing the member is ours.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS.
Full local gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/inference.rs
git commit -m "✨ feat(types): the null-safe chain re-acquires null once at its end"
```

---
### Task 8: Property narrowing under the conservative kill rule

Property subjects go live: `$this->prop` and `self::$prop` reads
consult the environment first and fall back to their declared type;
assignments to them bind; and the conservative invalidation rule kills
every property binding on calls, closure creation, `yield`, and the
rest of decision 10's list. By-reference effects land too: the
write-back rule, and the closure-`use (&$x)` degradation.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: Tasks 4-7 (`subject_of` property variants,
  `member_value_type`, the call arms).
- Produces (on `Walker`): `fn kill_property_bindings(&mut self,
  environment: &mut Environment<'db>)`, `fn apply_by_reference(&mut
  self, parameters: &[crate::declared::DeclaredParameter<'db>],
  arguments: &[celerrate_semantics::CallArgument], environment: &mut
  Environment<'db>)`, and the widened `subject_type` (property
  subjects fall back to their declared type). On `Environment`:
  `fn subjects(&self) -> Vec<NarrowingSubject>` (a keys snapshot for
  the kill and eval sweeps). Task 9 reuses `apply_by_reference` for
  function calls; Task 11's `Always` assertions apply after these
  kills.

- [ ] **Step 1: Write the failing tests**

Add to `crates/celerrate_types/src/inference.rs`'s test module:

```rust
    #[test]
    fn the_lazy_getter_narrows_its_property() {
        let fixture = fixture(&[
            "<?php class Service {}
            class Locator {
                private ?Service $service = null;
                public function get(): object {
                    if ($this->service === null) {
                        $this->service = new Service();
                    }
                    return $this->service;
                }
            }",
        ]);
        // Numbering: Service 0, Locator 1, property 2, get 3.
        assert_eq!(return_display(&fixture, 3), "service");
    }

    #[test]
    fn a_method_call_kills_property_narrowings() {
        let fixture = fixture(&[
            "<?php class Service {}
            class Holder {
                private ?Service $service = null;
                private function log(): void {}
                public function get() {
                    if ($this->service === null) { return 1; }
                    $this->log();
                    return $this->service;
                }
            }",
        ]);
        // The call re-widens the property to its declared type.
        assert_eq!(return_display(&fixture, 4), "null|1|service");
    }

    #[test]
    fn by_reference_arguments_take_the_write_back_type() {
        let fixture = fixture(&[
            "<?php class W { public function fill(array &$out): void {} }
            function f(W $w) {
                $x = null;
                $w->fill($x);
                return $x;
            }",
        ]);
        assert_eq!(return_display(&fixture, 2), "array<int|string, mixed>");
    }

    #[test]
    fn a_by_reference_closure_use_degrades_the_local() {
        let fixture = fixture(&[
            "<?php function f() {
                $x = 'a';
                $g = function () use (&$x) {};
                return $x;
            }",
        ]);
        assert_eq!(return_display(&fixture, 0), "mixed");
    }

    #[test]
    fn extract_forgets_every_local() {
        let fixture = fixture(&[
            "<?php function f() { $x = 1; extract(['x' => 'a']); return $x; }",
        ]);
        assert_eq!(return_display(&fixture, 0), "mixed");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -8`
Expected: the lazy getter answers `"null|service"` (no env read), the
kill test answers `"1|service"` (no kill), the write-back answers
`"null"`, the closure test `"'a'"`, extract `"1"`.

- [ ] **Step 3: Implement the property environment**

In `crates/celerrate_types/src/flow.rs`:

**(a)** Add the keys snapshot on `Environment`:

```rust
    pub(crate) fn subjects(&self) -> Vec<NarrowingSubject> {
        self.bindings.keys().cloned().collect()
    }
```

(Rewrite the `Eval` arm's local sweep through it too — the direct
field walk was Task 3 scaffolding.)

**(b)** Widen `subject_type` so property subjects fall back to their
declared type — the "wide type" of decision 9:

```rust
    fn subject_type(
        &self,
        environment: &Environment<'db>,
        subject: &NarrowingSubject,
    ) -> TypeId<'db> {
        if let Some(bound) = environment.binding(subject) {
            return bound;
        }
        let db = self.db();
        match subject {
            NarrowingSubject::Local { .. } => TypeId::mixed(db),
            NarrowingSubject::ThisProperty { name }
            | NarrowingSubject::StaticProperty { name } => self
                .context
                .owner_class_key
                .as_ref()
                .and_then(|key| {
                    self.member_value_type(
                        std::slice::from_ref(key),
                        MemberKind::Property,
                        name,
                    )
                })
                .unwrap_or_else(|| TypeId::mixed(db)),
        }
    }
```

**(c)** Environment-first property reads. At the top of the
`MemberAccess` and `ScopedAccess` arms of `expression_value` (before
the receiver is typed), add:

```rust
                if let Some(subject) = subject_of(self.context.ir, id)
                    && let Some(bound) = environment.binding(&subject)
                {
                    // A narrowed (or assigned) stable-base property:
                    // the environment wins over the declaration.
                    // The receiver is still typed for the table.
                    ...
                }
```

Concretely for `MemberAccess`: type the receiver first (the table
covers it), then check the binding and return it if present, else the
Task 6 resolution. For `ScopedAccess` (`self::$prop`): same shape.
The `id` here is the arm's own expression id — pass it into
`expression_value` (it already is the `id` parameter).

**(d)** The kill rule:

```rust
    /// Decision 10: any call, instantiation, closure creation,
    /// `yield`, `eval`, `include`, or shell-exec may run arbitrary
    /// code that rewrites object state: every property binding dies.
    /// Over-killing is the conservative direction — a dropped binding
    /// reads as the declared type.
    fn kill_property_bindings(&mut self, environment: &mut Environment<'db>) {
        for subject in environment.subjects() {
            if !matches!(subject, NarrowingSubject::Local { .. }) {
                environment.remove(&subject);
            }
        }
    }
```

Call it at the end of these `expression_value` arms, after their own
typing work: `Call` (all callee shapes), `New`, `Closure`,
`ArrowFunction`, `Yield`, `Eval`, `Include`, `ShellExec`. Order
matters and is load-bearing: the *receiver and arguments were already
typed* under the pre-call environment — `$this->foo->bar()` on a
narrowed `$this->foo` dereferences before the kill, which is exactly
the design's evaluation-order reading.

**(e)** The by-reference write-back:

```rust
    /// The by-reference rules (design section 6): an argument bound
    /// to a by-reference parameter takes the parameter's declared
    /// type after the call (the general write-back; plan 7's stdlib
    /// provider refines `$matches`), which also invalidates its
    /// narrowing. Named labels resolve by name; a spread ends the
    /// positional mapping (conservative).
    fn apply_by_reference(
        &mut self,
        parameters: &[crate::declared::DeclaredParameter<'db>],
        arguments: &[celerrate_semantics::CallArgument],
        environment: &mut Environment<'db>,
    ) {
        let db = self.db();
        for (index, argument) in arguments.iter().enumerate() {
            if argument.spread {
                break;
            }
            let parameter = match &argument.label {
                Some(label) => parameters.iter().find(|parameter| parameter.name == *label),
                None => parameters.get(index).or_else(|| {
                    parameters.last().filter(|parameter| parameter.variadic)
                }),
            };
            let Some(parameter) = parameter else {
                continue;
            };
            if !parameter.by_reference {
                continue;
            }
            if let Some(subject) = subject_of(self.context.ir, argument.value) {
                environment.bind(
                    subject,
                    parameter.parameter_type.unwrap_or_else(|| TypeId::mixed(db)),
                );
            }
        }
    }
```

Wire it into the method-call and static-call paths of the `Call` arm:
after computing the result, when exactly one receiver key resolved a
signature, `self.apply_by_reference(&signature.parameters, &arguments,
environment);` (a union receiver's differing signatures write back
nothing — conservative, recorded). This requires
`method_call_result_for_keys` to hand back the single resolved
signature; change its return to the pair
`(TypeId<'db>, Option<DeclaredSignature<'db>>)` where the signature is
`Some` only when exactly one key resolved, and adjust the two call
sites. Task 9 wires the same helper into function calls; Task 11
reads the same single-signature channel for assertions.

**(f)** Closure `use` by-reference degradation, in the `Closure` arm
(still typing `mixed` until Task 9 — the capture effects land now):

```rust
                for capture in &uses {
                    if capture.by_reference {
                        environment.bind(
                            NarrowingSubject::Local {
                                name: capture.name.clone(),
                            },
                            TypeId::mixed(db),
                        );
                    }
                }
```

**(g)** The `extract` sweep, in the `Call` arm's named-callee path
(Task 9 restructures that path; land the check where the Task 6 code
falls through to "walk and stay silent" — it must survive Task 9's
rewrite): when the callee is an unqualified or root-qualified
`extract`, drop every `Local` binding after typing the arguments:

```rust
                if name.eq_ignore_ascii_case("extract") {
                    for subject in environment.subjects() {
                        if matches!(subject, NarrowingSubject::Local { .. }) {
                            environment.remove(&subject);
                        }
                    }
                }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS.
Full local gate. Check the Task 4-7 tests still pass in particular:
the kill rule must not regress local-subject narrowing (locals
survive every kill).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/inference.rs
git commit -m "✨ feat(types): property narrowing under the conservative kill rule"
```

---

### Task 9: Function calls, callables, and provider dispatch

Named function calls resolve (namespaced candidate first, global
fallback — mirroring the reference checks), dynamic type providers
answer their claims first (widened at the boundary), closures and
arrow functions type as callable signatures with captures seeded,
first-class callables project declared signatures, and `$callable()`
invokes through `callable_return`. The inferred-return fallback stays
`mixed` until Task 10 wires the fixpoint.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: `celerrate_semantics::{SymbolQuery, SymbolSpace,
  lookup_function_declaration, resolve_candidates, stub_symbol_table}`,
  `crate::declared::{FunctionQuery, declared_function_signature,
  lower_written_text, NameSite}`,
  `crate::dynamic_type_provider::{DynamicTypeProviderRegistry,
  Invocation, SymbolClaim}`, `crate::widening::capped_child`, Task 8's
  `apply_by_reference` and kill rule.
- Produces (on `Walker`): `fn resolved_function_key(&self, written:
  &str) -> (String, bool /* exists in source */)`,
  `fn provider_return(&mut self, claim: SymbolClaim, receiver_type:
  Option<TypeId<'db>>, argument_types: &[TypeId<'db>]) ->
  Option<TypeId<'db>>`, `fn function_call_result(&mut self, key:
  &str, source_exists: bool) -> TypeId<'db>` (Task 10 replaces its
  inferred fallback), `fn closure_type(&mut self, parameters:
  &[celerrate_semantics::ParameterSignature], return_type_text:
  Option<&str>, returns: Vec<TypeId<'db>>, saw_yield: bool,
  end_reachable: bool) -> TypeId<'db>`, and `fn nested_returns(&mut
  self, ...)` replacing Task 3's `statements_nested`/
  `expression_nested`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/celerrate_types/src/inference.rs`'s test module:

```rust
    #[test]
    fn function_calls_take_declared_returns_and_resolve_through_the_namespace() {
        let fixture = fixture(&[
            "<?php namespace App;
            function g(): string { return 'x'; }
            function f() { return g(); }",
        ]);
        assert_eq!(return_display(&fixture, 1), "string");
        let file = fixture.handles[0];
        let body = body_query(&fixture, 1);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.declared_return_edges, 1);
    }

    #[test]
    fn a_dynamic_provider_claim_answers_first_and_counts() {
        let fixture = fixture(&[
            "<?php function maker(): string { return 'x'; }
            function f() { return maker(); }",
        ]);
        register_fake_provider(&fixture);
        assert_eq!(return_display(&fixture, 1), "int");
        let file = fixture.handles[0];
        let body = body_query(&fixture, 1);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.provider_edges, 1);
        assert_eq!(inferred.edge_counts.declared_return_edges, 0);
    }

    #[test]
    fn closures_and_arrows_type_as_callables_and_invoke() {
        let fixture = fixture(&[
            "<?php
            function declared() { $g = function (): int { return 1; }; return $g(); }
            function inferred() { $g = function () { return 'a'; }; return $g(); }
            function captured() { $x = 'a'; $g = fn () => $x; return $g(); }",
        ]);
        assert_eq!(return_display(&fixture, 0), "int");
        assert_eq!(return_display(&fixture, 1), "'a'");
        assert_eq!(return_display(&fixture, 2), "'a'");
    }

    #[test]
    fn first_class_callables_project_the_declared_signature() {
        let fixture = fixture(&[
            "<?php function g(int $n): string { return 'x'; }
            function f() { $r = g(...); return $r(); }",
        ]);
        assert_eq!(return_display(&fixture, 1), "string");
    }

    #[test]
    fn function_by_reference_arguments_write_back() {
        let fixture = fixture(&[
            "<?php function fill(array &$out): void {}
            function f() { $x = null; fill($x); return $x; }",
        ]);
        assert_eq!(return_display(&fixture, 1), "array<int|string, mixed>");
    }
```

And the provider fake beside the other fixture helpers:

```rust
    fn register_fake_provider(fixture: &Fixture) {
        use crate::{
            DynamicTypeProviderRegistration, DynamicTypeProviderRegistry, Invocation, SymbolClaim,
        };

        #[derive(Debug)]
        struct FakeMaker;

        impl crate::DynamicTypeProvider for FakeMaker {
            fn claims(&self) -> Vec<SymbolClaim> {
                vec![SymbolClaim::Function {
                    key: "maker".to_owned(),
                }]
            }

            fn return_type<'db>(
                &self,
                db: &'db dyn salsa::Database,
                _invocation: &Invocation<'db>,
            ) -> Option<crate::TypeId<'db>> {
                Some(crate::TypeId::int(db))
            }
        }

        let _ = DynamicTypeProviderRegistry::builder(vec![DynamicTypeProviderRegistration {
            identity: fake_identity("fake-maker"),
            provider: std::sync::Arc::new(FakeMaker),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }
```

(with a `fake_identity` helper as in `declared.rs`'s tests if the
module does not have one yet — `PluginIdentity { name, version:
"0.0.0", configuration: String::new() }`.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -8`
Expected: FAIL — named calls and callable invocations answer `mixed`.

- [ ] **Step 3: Implement the call machinery**

In `crates/celerrate_types/src/flow.rs`:

**(a)** Callee resolution, mirroring the reference checks' candidate
order — the namespaced spelling first, the global fallback last, first
existing candidate wins (source, then stubs), the last candidate as
the never-resolves fallback (so provider claims on undeclared helpers
still match a deterministic key):

```rust
    /// The folded Function-space key a written callee resolves to,
    /// and whether a source declaration exists (the inferred-return
    /// gate: only source bodies can be inferred).
    fn resolved_function_key(&self, written: &str) -> (String, bool) {
        let db = self.db();
        let candidates = celerrate_semantics::resolve_candidates(
            written,
            celerrate_semantics::SymbolSpace::Function,
            &self.context.namespace,
            &self.context.tables,
        );
        for candidate in &candidates {
            let query = celerrate_semantics::SymbolQuery::new(
                db,
                celerrate_semantics::SymbolSpace::Function,
                candidate.clone(),
            );
            if celerrate_semantics::lookup_function_declaration(db, self.context.files, query)
                .is_some()
            {
                return (candidate.clone(), true);
            }
        }
        for candidate in &candidates {
            if celerrate_semantics::stub_symbol_table(
                db,
                self.context.stubs,
                self.context.configuration,
            )
            .lookup(celerrate_semantics::SymbolSpace::Function, candidate)
            .is_some()
            {
                return (candidate.clone(), false);
            }
        }
        (
            candidates
                .into_iter()
                .last()
                .unwrap_or_else(|| written.trim_start_matches('\\').to_ascii_lowercase()),
            false,
        )
    }

    /// Decision 3, first tier: a provider that claims the callee
    /// answers, widened at the consumption boundary — a plugin never
    /// controls termination.
    fn provider_return(
        &mut self,
        claim: crate::dynamic_type_provider::SymbolClaim,
        receiver_type: Option<TypeId<'db>>,
        argument_types: &[TypeId<'db>],
    ) -> Option<TypeId<'db>> {
        let db = self.db();
        let registry = crate::dynamic_type_provider::DynamicTypeProviderRegistry::try_get(db)?;
        for registration in registry.registrations(db) {
            if !registration.provider.claims().contains(&claim) {
                continue;
            }
            let invocation = crate::dynamic_type_provider::Invocation {
                claim: claim.clone(),
                receiver_type,
                argument_types: argument_types.to_vec(),
            };
            if let Some(answer) = registration.provider.return_type(db, &invocation) {
                self.edge_counts.provider_edges += 1;
                return Some(crate::widening::capped_child(db, answer));
            }
        }
        None
    }
```

(If `SymbolClaim` does not derive `PartialEq` yet, add
`#[derive(..., PartialEq, Eq)]` to it in `dynamic_type_provider.rs` —
claims are plain data; `validate_claims` already compares them
structurally.)

Also extend the `DynamicTypeProvider` trait's rustdoc in
`dynamic_type_provider.rs` with the monotonicity expectation the
design puts on the trait (section 6, verbatim requirement):

```rust
/// Contributions feed fixpoint iteration: a provider is expected to
/// answer monotonically with respect to its argument types (a wider
/// invocation never yields a strictly narrower answer). The
/// expectation is documented, not enforced — a non-convergent
/// contribution hits the iteration budget and the result widens to
/// `mixed`, the deterministic bailout: a plugin never controls
/// termination.
```

(Appended to the existing trait documentation; no signature changes.)

**(b)** The declared/inferred tiers of a named function call:

```rust
    /// Decision 3, tiers two and three, for a named function call.
    /// Task 10 replaces the `mixed` fallback with the fixpoint.
    fn function_call_result(&mut self, key: &str, source_exists: bool) -> TypeId<'db> {
        let db = self.db();
        let declared = declared_function_signature(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            crate::declared::FunctionQuery::new(db, key.to_owned()),
        );
        if let Some(signature) = &declared
            && self.declared_present(signature)
        {
            self.edge_counts.declared_return_edges += 1;
            return signature.value_type;
        }
        let _ = source_exists;
        TypeId::mixed(db)
    }
```

**(c)** Rewire the `Call` arm's fall-through case (after the
method-call and static-call routes of Task 6, keeping Task 5's
`assert` pre-check and Task 8's `extract` sweep):

```rust
                    Some(BodyExpression::NamedReference { text }) => {
                        self.record(callee, TypeId::mixed(db));
                        let argument_types = self.typed_arguments(&arguments, environment);
                        let (key, source_exists) = self.resolved_function_key(&text);
                        // (Task 8's extract sweep goes here, on `text`.)
                        let of = self
                            .provider_return(
                                crate::dynamic_type_provider::SymbolClaim::Function {
                                    key: key.clone(),
                                },
                                None,
                                &argument_types,
                            )
                            .unwrap_or_else(|| self.function_call_result(&key, source_exists));
                        let declared = declared_function_signature(
                            db,
                            self.context.files,
                            self.context.stubs,
                            self.context.configuration,
                            crate::declared::FunctionQuery::new(db, key),
                        );
                        if let Some(signature) = declared {
                            self.apply_by_reference(
                                &signature.parameters,
                                &arguments,
                                environment,
                            );
                        }
                        self.kill_property_bindings(environment);
                        of
                    }
                    _ => {
                        let callee_type = self.expression(callee, environment);
                        self.typed_arguments(&arguments, environment);
                        self.kill_property_bindings(environment);
                        callee_type
                            .callable_return(db)
                            .unwrap_or_else(|| TypeId::mixed(db))
                    }
```

**(d)** Closures and arrow functions. Replace Task 3's
`statements_nested`/`expression_nested` with a returns-capturing
variant, and the two arms:

```rust
    /// Walks a nested body with its own return accumulator; answers
    /// (returned types, saw yield, end reachable).
    fn nested_returns(
        &mut self,
        walk: impl FnOnce(&mut Self, &mut Environment<'db>),
        environment: &mut Environment<'db>,
    ) -> (Vec<TypeId<'db>>, bool, bool) {
        let saved_returns = std::mem::take(&mut self.returns);
        let saved_yield = std::mem::replace(&mut self.saw_yield, false);
        walk(self, environment);
        let inner_returns = std::mem::replace(&mut self.returns, saved_returns);
        let inner_yield = std::mem::replace(&mut self.saw_yield, saved_yield);
        (inner_returns, inner_yield, environment.is_reachable())
    }

    /// The callable type of a closure or arrow function: parameters
    /// from the written signature (lowered at the declaring site, the
    /// native `= null` implicit nullability included), the declared
    /// return text when present, the inner returns joined otherwise.
    fn closure_type(
        &mut self,
        parameters: &[celerrate_semantics::ParameterSignature],
        return_type_text: Option<&str>,
        returns: Vec<TypeId<'db>>,
        saw_yield: bool,
        end_reachable: bool,
    ) -> TypeId<'db> {
        let db = self.db();
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables: &self.context.tables,
        };
        let callable_parameters = parameters
            .iter()
            .map(|parameter| {
                let mut parameter_type = parameter
                    .type_text
                    .as_deref()
                    .and_then(|text| crate::declared::lower_written_text(db, &site, text))
                    .unwrap_or_else(|| TypeId::mixed(db));
                if parameter.default_text.as_deref() == Some("null") {
                    parameter_type = TypeId::union(db, [parameter_type, TypeId::null(db)]);
                }
                crate::representation::CallableParameter {
                    parameter_type,
                    optional: parameter.default_text.is_some(),
                    variadic: parameter.variadic,
                    by_reference: parameter.by_reference,
                }
            })
            .collect();
        let return_type = return_type_text
            .and_then(|text| crate::declared::lower_written_text(db, &site, text))
            .unwrap_or_else(|| {
                if saw_yield {
                    return TypeId::class(db, "Generator", vec![]);
                }
                let joined = returns.into_iter().reduce(|left, right| join(db, left, right));
                match (joined, end_reachable) {
                    (Some(joined), true) => join(db, joined, TypeId::null(db)),
                    (None, true) => TypeId::null(db),
                    (Some(joined), false) => joined,
                    (None, false) => TypeId::never(db),
                }
            });
        TypeId::callable(db, callable_parameters, return_type)
    }
```

The `Closure` arm (keeping Task 8's by-reference capture
degradation and the kill):

```rust
            BodyExpression::Closure {
                parameters,
                uses,
                return_type_text,
                is_static: _,
                by_reference: _,
                body,
            } => {
                let mut inner = Environment::new();
                for capture in &uses {
                    let subject = NarrowingSubject::Local {
                        name: capture.name.clone(),
                    };
                    if capture.by_reference {
                        inner.bind(subject.clone(), TypeId::mixed(db));
                        environment.bind(subject, TypeId::mixed(db));
                    } else {
                        let captured = self.subject_type(environment, &subject);
                        inner.bind(subject, captured);
                    }
                }
                self.seed_written_parameters(&parameters, &mut inner);
                let (returns, saw_yield, end_reachable) =
                    self.nested_returns(|walker, env| walker.statements(&body, env), &mut inner);
                self.kill_property_bindings(environment);
                self.closure_type(
                    &parameters,
                    return_type_text.as_deref(),
                    returns,
                    saw_yield,
                    end_reachable,
                )
            }
            BodyExpression::ArrowFunction {
                parameters,
                return_type_text,
                is_static: _,
                by_reference: _,
                body,
            } => {
                // Arrow functions capture the whole scope by value.
                let mut inner = environment.clone();
                self.seed_written_parameters(&parameters, &mut inner);
                let mut returned: Vec<TypeId<'db>> = Vec::new();
                let (_, saw_yield, _) = self.nested_returns(
                    |walker, env| {
                        let of = walker.expression(body, env);
                        returned.push(of);
                    },
                    &mut inner,
                );
                self.kill_property_bindings(environment);
                self.closure_type(
                    &parameters,
                    return_type_text.as_deref(),
                    returned,
                    saw_yield,
                    false,
                )
            }
```

with the seeding helper (the closure-side sibling of
`seeded_parameters` — written texts, not declared queries, because a
closure has no `FunctionQuery` identity):

```rust
    fn seed_written_parameters(
        &self,
        parameters: &[celerrate_semantics::ParameterSignature],
        environment: &mut Environment<'db>,
    ) {
        let db = self.db();
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables: &self.context.tables,
        };
        for parameter in parameters {
            let mut of = parameter
                .type_text
                .as_deref()
                .and_then(|text| crate::declared::lower_written_text(db, &site, text))
                .unwrap_or_else(|| TypeId::mixed(db));
            if parameter.default_text.as_deref() == Some("null") {
                of = TypeId::union(db, [of, TypeId::null(db)]);
            }
            if parameter.variadic {
                of = TypeId::list(db, of);
            }
            environment.bind(
                NarrowingSubject::Local {
                    name: parameter.name.clone(),
                },
                of,
            );
        }
    }
```

**(e)** The `CallableReference` arm: a named callee projects its
declared signature into a callable type; method and scoped callees
project the member signature; anything else stays `mixed`:

```rust
            BodyExpression::CallableReference { callee } => {
                let of = match self.context.ir.expression(callee).cloned() {
                    Some(BodyExpression::NamedReference { text }) => {
                        self.record(callee, TypeId::mixed(db));
                        let (key, source_exists) = self.resolved_function_key(&text);
                        self.projected_callable_of_function(&key, source_exists)
                    }
                    Some(BodyExpression::MemberAccess {
                        receiver,
                        member: MemberReference::Named { name },
                        ..
                    }) => {
                        let receiver_type = self.expression(receiver, environment);
                        self.record(callee, TypeId::mixed(db));
                        self.projected_callable_of_method(receiver_type, &name)
                    }
                    Some(BodyExpression::ScopedAccess {
                        subject,
                        member: MemberReference::Named { name },
                    }) => {
                        let (subject_type, keys) = self.scoped_subject(subject, environment);
                        self.record(callee, TypeId::mixed(db));
                        match keys {
                            Some(keys) => {
                                self.projected_callable_of_keys(&keys, subject_type, &name)
                            }
                            None => TypeId::mixed(db),
                        }
                    }
                    _ => {
                        self.expression(callee, environment);
                        TypeId::mixed(db)
                    }
                };
                of
            }
```

with three small projections sharing one body:

```rust
    fn projected_callable(
        &mut self,
        signature: Option<DeclaredSignature<'db>>,
        receiver: Option<TypeId<'db>>,
        return_fallback: TypeId<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        let Some(signature) = signature else {
            return TypeId::mixed(db);
        };
        let parameters = signature
            .parameters
            .iter()
            .map(|parameter| crate::representation::CallableParameter {
                parameter_type: parameter
                    .parameter_type
                    .unwrap_or_else(|| TypeId::mixed(db)),
                optional: parameter.optional,
                variadic: parameter.variadic,
                by_reference: parameter.by_reference,
            })
            .collect();
        let mut return_type = if self.declared_present(&signature) {
            self.edge_counts.declared_return_edges += 1;
            signature.value_type
        } else {
            return_fallback
        };
        if let Some(receiver) = receiver {
            return_type = self.substitute_receiver(return_type, receiver);
        }
        TypeId::callable(db, parameters, return_type)
    }

    fn projected_callable_of_function(
        &mut self,
        key: &str,
        _source_exists: bool, // Task 10 threads this into the fallback
    ) -> TypeId<'db> {
        let db = self.db();
        let signature = declared_function_signature(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            crate::declared::FunctionQuery::new(db, key.to_owned()),
        );
        self.projected_callable(signature, None, TypeId::mixed(db))
    }

    fn projected_callable_of_method(
        &mut self,
        receiver_type: TypeId<'db>,
        name: &str,
    ) -> TypeId<'db> {
        let db = self.db();
        let keys = match self.receiver_parts(receiver_type.without_null(db)) {
            Some(keys) => keys,
            None => return TypeId::mixed(db),
        };
        self.projected_callable_of_keys(&keys, receiver_type, name)
    }

    fn projected_callable_of_keys(
        &mut self,
        keys: &[String],
        receiver: TypeId<'db>,
        name: &str,
    ) -> TypeId<'db> {
        let db = self.db();
        let mut signatures = self.method_signatures(keys, name);
        // One key, one signature: more is a union receiver, silence.
        let signature = (signatures.len() == 1).then(|| signatures.remove(0));
        self.projected_callable(signature, Some(receiver), TypeId::mixed(db))
    }
```

**(f)** Providers for methods. In the method-call and static-call
routes of the `Call` arm, before the declared tier: when the receiver
resolves to exactly one key, consult
`SymbolClaim::Method { class_key: key.clone(), method_key:
folded_member_key(MemberKind::Method, &name) }` with the receiver
type and argument types; a `Some` answer wins the call (do not also
count a declared edge).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS.
Full local gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/inference.rs crates/celerrate_types/src/dynamic_type_provider.rs
git commit -m "✨ feat(types): function calls, callables, and provider dispatch at the call boundary"
```

---
### Task 10: The interprocedural fixpoint

`inferred_function_return` — the projection query and the workspace's
first salsa cycle recovery. The join-ascent discipline (decision 2):
`cycle_initial` answers `never`, the recovery function joins each
iterate with the previous approximation through the pure `ascend`
helper, and `FIXPOINT_ITERATION_BUDGET` (32, below salsa's 200 panic
cap) widens deterministically to `mixed`. The call boundary's third
tier goes live, and the determinism and cancellation fixtures pin the
discipline.

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs`
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/lib.rs`
- Create: `crates/celerrate_types/tests/fixpoint.rs`

**Interfaces:**
- Consumes: `celerrate_semantics::{SymbolQuery, SymbolSpace,
  analyzed_file_index, lookup_function_declaration}`, salsa 0.27's
  `cycle_fn`/`cycle_initial` tracked-function options and
  `salsa::Cycle::iteration()` (`salsa-0.27.2/tests/cycle.rs:172-189`
  is the canonical usage), `salsa::Cancelled::catch`.
- Produces: `pub const FIXPOINT_ITERATION_BUDGET: u32 = 32`,
  `#[salsa::tracked(cycle_fn = return_cycle_recover, cycle_initial =
  return_cycle_initial)] pub fn inferred_function_return<'db>(db,
  files, stubs, configuration, query: FunctionQuery<'db>) ->
  TypeId<'db>`, and `pub(crate) fn ascend<'db>(db, iteration: u32,
  last_provisional: TypeId<'db>, computed: TypeId<'db>) ->
  TypeId<'db>`. Both public names re-export from `lib.rs`. Task 12
  probes the query's cutoff behavior.

- [ ] **Step 1: Write the failing tests**

Unit tests for the pure helper, in
`crates/celerrate_types/src/inference.rs`'s test module:

```rust
    #[test]
    fn ascend_joins_monotonically_and_bails_to_mixed_past_the_budget() {
        let db = TestDatabase::default();
        let int = crate::TypeId::int(&db);
        let string = crate::TypeId::string(&db);
        let never = crate::TypeId::never(&db);
        // Ascent from the bottom.
        assert_eq!(super::ascend(&db, 0, never, int), int);
        // A widening iterate joins, never replaces.
        assert_eq!(
            super::ascend(&db, 1, int, string),
            crate::TypeId::union(&db, [int, string]),
        );
        // Convergence: identical join answers the provisional value.
        assert_eq!(super::ascend(&db, 5, int, int), int);
        // Budget exhaustion on a still-moving value: mixed, the
        // deterministic bailout — never salsa's panic.
        assert_eq!(
            super::ascend(&db, super::FIXPOINT_ITERATION_BUDGET, int, string),
            crate::TypeId::mixed(&db),
        );
        // The budget sits far below salsa's cap (MAX_ITERATIONS=200).
        assert!(super::FIXPOINT_ITERATION_BUDGET < 200);
    }
```

Integration tests, `crates/celerrate_types/tests/fixpoint.rs` (new
file; the fixture is the same duplicated pattern — recorded debt):

```rust
//! Fixpoint determinism fixtures (design section 10, harness 3): the
//! same mutual-recursion cluster queried from every entry point and
//! across thread counts answers identically, and an edit landing
//! mid-fixpoint unwinds cleanly with no provisional value served.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::sync::{Arc, Barrier};

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{FunctionQuery, inferred_function_return};

struct Fixture {
    db: TestDatabase,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    handles: Vec<SourceFile>,
}

fn fixture(sources: &[&str]) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    Fixture {
        db,
        files,
        stubs,
        configuration,
        handles,
    }
}

fn return_of(fixture: &Fixture, key: &str) -> String {
    inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        FunctionQuery::new(&fixture.db, key.to_owned()),
    )
    .display(&fixture.db)
}

const MUTUAL: &str = "<?php
function a(bool $c) { if ($c) { return b($c); } return 1; }
function b(bool $c) { if ($c) { return a($c); } return 'x'; }";

#[test]
fn direct_recursion_converges() {
    let fixture = fixture(&[
        "<?php function down(int $n) { if ($n > 0) { return down($n - 1); } return 0; }",
    ]);
    assert_eq!(return_of(&fixture, "down"), "0");
}

#[test]
fn baseless_mutual_recursion_is_never() {
    let fixture = fixture(&["<?php function a() { return b(); } function b() { return a(); }"]);
    assert_eq!(return_of(&fixture, "a"), "never");
    assert_eq!(return_of(&fixture, "b"), "never");
}

#[test]
fn every_entry_point_converges_to_the_same_fixpoint() {
    // Entry a-then-b.
    let first = fixture(&[MUTUAL]);
    let a_first = (return_of(&first, "a"), return_of(&first, "b"));
    // Entry b-then-a, a fresh database.
    let second = fixture(&[MUTUAL]);
    let b_first_b = return_of(&second, "b");
    let b_first_a = return_of(&second, "a");
    assert_eq!(a_first.0, b_first_a);
    assert_eq!(a_first.1, b_first_b);
    assert_eq!(a_first.0, a_first.1, "the cluster shares one fixpoint");
}

#[test]
fn thread_fan_out_answers_identically() {
    let fixture = fixture(&[MUTUAL]);
    // Warm the fixpoint once, then fan out over snapshots.
    let expected = (return_of(&fixture, "a"), return_of(&fixture, "b"));
    let results: Vec<(String, String)> = std::thread::scope(|scope| {
        (0..4)
            .map(|_| {
                let db = fixture.db.clone();
                let files = fixture.files;
                let stubs = fixture.stubs;
                let configuration = fixture.configuration;
                scope.spawn(move || {
                    let of = |key: &str| {
                        inferred_function_return(
                            &db,
                            files,
                            stubs,
                            configuration,
                            FunctionQuery::new(&db, key.to_owned()),
                        )
                        .display(&db)
                    };
                    (of("a"), of("b"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    for result in results {
        assert_eq!(result, expected);
    }
}

#[test]
fn a_growing_recursion_terminates_deterministically() {
    let source = "<?php function grow() { return [grow()]; }";
    let first = fixture(&[source]);
    let second = fixture(&[source]);
    // The caps and the budget guarantee termination; determinism is
    // the contract, the exact widened form is not.
    assert_eq!(return_of(&first, "grow"), return_of(&second, "grow"));
}
```

And the cancellation fixture (decision 15), same file:

```rust
/// A provider that rendezvouses with the test: entered signals the
/// test may write; released waits until the write is pending. Its
/// value contribution is deterministic (always `None`).
#[derive(Debug)]
struct BlockingProvider {
    entered: Arc<Barrier>,
    released: Arc<Barrier>,
}

impl celerrate_types::DynamicTypeProvider for BlockingProvider {
    fn claims(&self) -> Vec<celerrate_types::SymbolClaim> {
        vec![celerrate_types::SymbolClaim::Function {
            key: "block".to_owned(),
        }]
    }

    fn return_type<'db>(
        &self,
        _db: &'db dyn salsa::Database,
        _invocation: &celerrate_types::Invocation<'db>,
    ) -> Option<celerrate_types::TypeId<'db>> {
        self.entered.wait();
        self.released.wait();
        None
    }
}

#[test]
fn an_edit_mid_fixpoint_unwinds_cleanly_and_serves_no_provisional_value() {
    let source = "<?php
    function entry(int $n) { if ($n > 0) { return entry($n - 1); } return block(); }";
    let edited = "<?php
    function entry(int $n) { if ($n > 0) { return entry($n - 1); } return block() ?? 'edited'; }";

    let fixture = fixture(&[source]);
    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(Barrier::new(2));
    let _ = celerrate_types::DynamicTypeProviderRegistry::builder(vec![
        celerrate_types::DynamicTypeProviderRegistration {
            identity: celerrate_semantics::PluginIdentity {
                name: "blocking".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            provider: Arc::new(BlockingProvider {
                entered: entered.clone(),
                released: released.clone(),
            }),
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(&fixture.db);

    let worker_db = fixture.db.clone();
    let probe_db = fixture.db.clone();
    let mut setter_db = fixture.db.clone();
    let (files, stubs, configuration) = (fixture.files, fixture.stubs, fixture.configuration);
    let file = fixture.handles[0];

    let worker = std::thread::spawn(move || {
        salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            inferred_function_return(
                &worker_db,
                files,
                stubs,
                configuration,
                FunctionQuery::new(&worker_db, "entry".to_owned()),
            )
            .display(&worker_db)
        }))
    });

    // The worker is inside the provider, mid-fixpoint.
    entered.wait();
    // The pending write cancels every in-flight snapshot; the setter
    // blocks until the worker's snapshot drops.
    let setter = std::thread::spawn(move || {
        use salsa::Setter as _;
        file.set_content(&mut setter_db).to(edited.as_bytes().to_vec());
    });
    // Confirm the cancellation flag is set by catching it ourselves,
    // then release the provider so the worker can observe it.
    loop {
        let probed = salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            let _ = celerrate_db::parse(&probe_db, file);
        }));
        if probed.is_err() {
            break;
        }
        std::thread::yield_now();
    }
    released.wait();

    let unwound = worker.join().unwrap();
    assert!(unwound.is_err(), "the fixpoint unwound with Cancelled");
    setter.join().unwrap();

    // No provisional value: a fresh demand answers the post-edit
    // fixpoint, byte-identical to a from-scratch database.
    let after = return_of(&fixture, "entry");
    let fresh = fixture_with(&[edited]);
    // (fixture_with = the fixture fn; the edited source needs the
    // same provider registered for `block` to stay deterministic —
    // register a second BlockingProvider with zero-wait barriers, or
    // simpler: barriers of size 1, which never block.)
    assert_eq!(after, return_of(&fresh, "entry"));
}
```

Implementation note for the fresh-database comparison: add a
`fixture_with(sources) -> Fixture` alias that registers a
`BlockingProvider` built with `Arc::new(Barrier::new(1))` for both
barriers (a one-party barrier never blocks), so both databases resolve
`block()` through the same provider claim. Keep the name `fixture_with`
distinct from `fixture` (which registers no provider).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types 2>&1 | tail -8`
Expected: FAIL to compile — `ascend`, `FIXPOINT_ITERATION_BUDGET`,
`inferred_function_return` do not exist.

- [ ] **Step 3: Implement the fixpoint**

In `crates/celerrate_types/src/inference.rs` (new imports:
`celerrate_semantics::{SymbolQuery, analyzed_file_index,
lookup_function_declaration}`, `crate::declared::FunctionQuery`):

```rust
/// The iteration budget of the interprocedural fixpoint: exhaustion
/// widens deterministically to `mixed`. Far below salsa's own
/// `MAX_ITERATIONS = 200` panic cap (`salsa-0.27.2/src/cycle.rs`) —
/// reaching that panic would be a zero-panic breach, so the budget is
/// the bailout that makes it unreachable.
pub const FIXPOINT_ITERATION_BUDGET: u32 = 32;

/// One join-ascent step: the computed iterate joins the previous
/// approximation (monotone ascent forced structurally — oscillation
/// between two values is impossible, and every entry point converges
/// to the same fixpoint because `join` is deterministic and
/// entry-point independent). A still-moving value past the budget
/// widens to `mixed`.
pub(crate) fn ascend<'db>(
    db: &'db dyn salsa::Database,
    iteration: u32,
    last_provisional: TypeId<'db>,
    computed: TypeId<'db>,
) -> TypeId<'db> {
    let ascended = crate::widening::join(db, last_provisional, computed);
    if ascended == last_provisional {
        return ascended;
    }
    if iteration >= FIXPOINT_ITERATION_BUDGET {
        return TypeId::mixed(db);
    }
    ascended
}

fn return_cycle_initial<'db>(
    db: &'db dyn salsa::Database,
    _id: salsa::Id,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: FunctionQuery<'db>,
) -> TypeId<'db> {
    // The lattice bottom: ascent starts from nothing.
    TypeId::never(db)
}

#[allow(clippy::too_many_arguments)]
fn return_cycle_recover<'db>(
    db: &'db dyn salsa::Database,
    cycle: &salsa::Cycle,
    last_provisional: &TypeId<'db>,
    computed: TypeId<'db>,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: FunctionQuery<'db>,
) -> TypeId<'db> {
    ascend(db, cycle.iteration(), *last_provisional, computed)
}

/// The inferred return of one free function: the projection of its
/// body's inference — small, resident (never LRU-evicted), the
/// fixpoint's currency. Early cutoff is the point: a body edit that
/// leaves the inferred return identical backdates here, and callers
/// are spared. Unresolvable functions answer `mixed` (silence).
#[salsa::tracked(cycle_fn = return_cycle_recover, cycle_initial = return_cycle_initial)]
pub fn inferred_function_return<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> TypeId<'db> {
    let symbol_query = SymbolQuery::new(db, SymbolSpace::Function, query.key(db).clone());
    let Some(ast_id) = lookup_function_declaration(db, files, symbol_query) else {
        return TypeId::mixed(db);
    };
    let index = analyzed_file_index(db, files);
    let Ok(position) = index.binary_search_by_key(&ast_id.file, |(id, _)| *id) else {
        return TypeId::mixed(db);
    };
    let Some(&(_, file)) = index.get(position) else {
        return TypeId::mixed(db);
    };
    inferred_body_types(db, files, stubs, configuration, file, BodyQuery::new(db, ast_id))
        .as_ref()
        .map(|inferred| inferred.return_type)
        .unwrap_or_else(|| TypeId::mixed(db))
}
```

(If the salsa macro rejects the exact `cycle_fn` parameter list, the
canonical shape to mirror is `salsa-0.27.2/tests/cycle.rs:126-189` —
the recovery function takes `(db, &salsa::Cycle, &Output, Output,
...inputs)` and the initial function `(db, salsa::Id, ...inputs)`;
adjust the shim signatures to what the macro expands to, keeping
`ascend` as the tested pure core.)

Re-export in `crates/celerrate_types/src/lib.rs`:

```rust
pub use inference::{
    FIXPOINT_ITERATION_BUDGET, InferredBody, InterproceduralEdgeCounts, inferred_body_types,
    inferred_function_return,
};
```

- [ ] **Step 4: Wire the third tier into the call boundary**

In `crates/celerrate_types/src/flow.rs`, replace the fallback of
`function_call_result` (Task 9's `let _ = source_exists;
TypeId::mixed(db)`):

```rust
        if source_exists {
            self.edge_counts.inferred_return_edges += 1;
            return crate::inference::inferred_function_return(
                db,
                self.context.files,
                self.context.stubs,
                self.context.configuration,
                crate::declared::FunctionQuery::new(db, key.to_owned()),
            );
        }
        TypeId::mixed(db)
```

And in `projected_callable_of_function`, thread `source_exists` into
the fallback the same way (replace the `TypeId::mixed(db)` fallback
argument with the inferred return when `source_exists`, counting the
edge).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS,
including `fixpoint.rs`. Then `cargo test --workspace 2>&1 | tail -3`
and the full local gate. The thread fixture must pass repeatedly:
`cargo test --package celerrate_types --test fixpoint -- --test-threads=1`
then with default threads, three runs each.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/inference.rs crates/celerrate_types/src/flow.rs crates/celerrate_types/src/lib.rs crates/celerrate_types/tests/fixpoint.rs
git commit -m "✨ feat(types): the interprocedural fixpoint under the join-ascent discipline"
```

---

### Task 11: Assertion tags narrow at call sites

`@phpstan-assert` and the non-divergent `@psalm-assert` forms, carried
by `MemberAnnotations.assertions` since plan 3, finally consumed:
`$name` subjects map through the declared parameter list to the
argument's narrowing subject, `$this->name` subjects map to the
caller's property subject when the receiver is `$this`; `Always`
applies after the call, `IfTrue`/`IfFalse` when the call is a
condition. Tests inject a fake `TypeSyntax` — the real tag grammar is
the bridge's (plan 4b), and this layer is proven against the trait.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: `crate::type_syntax::{AssertionPolarity, ParsedAssertion}`,
  `crate::declared::{function_annotations, member_annotations}`,
  Task 8's single-signature channel from the call arms, Task 4's
  `narrowed_to`/`removed_type`, Task 9's call routes.
- Produces (on `Walker`): the `pending_condition_facts:
  Vec<PendingAssertion>` field, `struct PendingAssertion { subject:
  NarrowingSubject, asserted: TypeId<'db>, polarity:
  AssertionPolarity, negated: bool }`, and `fn
  apply_call_assertions(&mut self, assertions:
  &[crate::type_syntax::ParsedAssertion<'db>], parameters:
  &[crate::declared::DeclaredParameter<'db>], receiver_is_this: bool,
  arguments: &[celerrate_semantics::CallArgument], environment: &mut
  Environment<'db>)`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/celerrate_types/src/inference.rs`'s test module. The
fake syntax parses two markers: `@fake-assert-string` (an `Always`
assertion on `$value` to `string`) and `@fake-if-true-string` (the
`IfTrue` form), plus `@fake-assert-this-prop` (`Always` on
`$this->prop` to `string`):

```rust
    fn register_fake_assertions(fixture: &Fixture) {
        use crate::{
            AssertionPolarity, ParsedAnnotations, ParsedAssertion, TypeSyntax,
            TypeSyntaxRegistration, TypeSyntaxRegistry,
        };

        #[derive(Debug)]
        struct FakeAssertions;

        impl TypeSyntax for FakeAssertions {
            fn can_parse(&self, docblock: &str) -> bool {
                docblock.contains("@fake-")
            }

            fn parse_docblock<'db>(
                &self,
                site: &crate::AnnotationSite<'db, '_>,
                docblock: &str,
            ) -> ParsedAnnotations<'db> {
                let db = site.database();
                let mut parsed = ParsedAnnotations::default();
                if docblock.contains("@fake-assert-string") {
                    parsed.assertions.push(ParsedAssertion {
                        subject: "$value".to_owned(),
                        asserted: crate::TypeId::string(db),
                        polarity: AssertionPolarity::Always,
                        negated: false,
                    });
                }
                if docblock.contains("@fake-if-true-string") {
                    parsed.assertions.push(ParsedAssertion {
                        subject: "$value".to_owned(),
                        asserted: crate::TypeId::string(db),
                        polarity: AssertionPolarity::IfTrue,
                        negated: false,
                    });
                }
                if docblock.contains("@fake-assert-this-prop") {
                    parsed.assertions.push(ParsedAssertion {
                        subject: "$this->prop".to_owned(),
                        asserted: crate::TypeId::string(db),
                        polarity: AssertionPolarity::Always,
                        negated: false,
                    });
                }
                parsed
            }

            fn parse_type_expression<'db>(
                &self,
                _site: &crate::AnnotationSite<'db, '_>,
                _expression: &str,
            ) -> Option<crate::TypeId<'db>> {
                None
            }
        }

        let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
            identity: fake_identity("fake-assertions"),
            implementation: std::sync::Arc::new(FakeAssertions),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    #[test]
    fn an_always_assertion_narrows_after_the_call() {
        let fixture = fixture(&[
            "<?php class Assert {
                /** @fake-assert-string */
                public static function string(mixed $value): void {}
            }
            function f(mixed $x) { Assert::string($x); return $x; }",
        ]);
        register_fake_assertions(&fixture);
        assert_eq!(return_display(&fixture, 2), "string");
    }

    #[test]
    fn an_if_true_assertion_narrows_the_condition_branches() {
        let fixture = fixture(&[
            "<?php
            /** @fake-if-true-string */
            function ok(mixed $value): bool { return true; }
            function f(mixed $x) { if (ok($x)) { return $x; } return 1; }",
        ]);
        register_fake_assertions(&fixture);
        assert_eq!(return_display(&fixture, 1), "1|string");
    }

    #[test]
    fn a_this_subject_assertion_narrows_the_callers_property() {
        let fixture = fixture(&[
            "<?php class A {
                public mixed $prop = null;
                /** @fake-assert-this-prop */
                public function check(): void {}
                public function read() { $this->check(); return $this->prop; }
            }",
        ]);
        register_fake_assertions(&fixture);
        // Numbering: class 0, property 1, check 2, read 3.
        assert_eq!(return_display(&fixture, 3), "string");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types inference 2>&1 | tail -6`
Expected: FAIL — `"mixed"` where the asserted narrowing is expected.

- [ ] **Step 3: Implement the assertion channel**

In `crates/celerrate_types/src/flow.rs`:

**(a)** The pending vocabulary and walker field (initialize empty in
`walk_body`):

```rust
/// One conditional assertion collected while typing a call, applied
/// when that call is a condition (`IfTrue`/`IfFalse`).
struct PendingAssertion<'db> {
    subject: NarrowingSubject,
    asserted: TypeId<'db>,
    polarity: crate::type_syntax::AssertionPolarity,
    negated: bool,
}
```

```rust
    /// The most recently typed call's conditional assertions, drained
    /// by `branch_environments`' default arm.
    pending_condition_facts: Vec<PendingAssertion<'db>>,
```

**(b)** The application:

```rust
    /// Applies a callee's assertion tags at this call site (decision
    /// 17): `$name` subjects map through the declared parameters to
    /// the argument's subject; `$this->name` maps to the caller's
    /// property subject when the receiver is the caller's `$this`;
    /// other subject shapes are ignored (recorded). `Always` applies
    /// now; `IfTrue`/`IfFalse` queue for the condition consumer.
    fn apply_call_assertions(
        &mut self,
        assertions: &[crate::type_syntax::ParsedAssertion<'db>],
        parameters: &[crate::declared::DeclaredParameter<'db>],
        receiver_is_this: bool,
        arguments: &[celerrate_semantics::CallArgument],
        environment: &mut Environment<'db>,
    ) {
        use crate::type_syntax::AssertionPolarity;
        for assertion in assertions {
            let subject = if let Some(property) = assertion.subject.strip_prefix("$this->") {
                receiver_is_this.then(|| NarrowingSubject::ThisProperty {
                    name: property.to_owned(),
                })
            } else if let Some(name) = assertion.subject.strip_prefix('$') {
                let position = parameters
                    .iter()
                    .position(|parameter| parameter.name == name);
                position.and_then(|position| {
                    let argument = arguments
                        .iter()
                        .enumerate()
                        .find(|(index, argument)| match &argument.label {
                            Some(label) => label == name,
                            None => *index == position && !argument.spread,
                        })
                        .map(|(_, argument)| argument)?;
                    subject_of(self.context.ir, argument.value)
                })
            } else {
                None
            };
            let Some(subject) = subject else {
                continue;
            };
            match assertion.polarity {
                AssertionPolarity::Always => {
                    let current = self.subject_type(environment, &subject);
                    let narrowed = if assertion.negated {
                        self.removed_type(current, assertion.asserted)
                    } else {
                        self.narrowed_to(current, assertion.asserted)
                    };
                    environment.bind(subject, narrowed);
                }
                AssertionPolarity::IfTrue | AssertionPolarity::IfFalse => {
                    self.pending_condition_facts.push(PendingAssertion {
                        subject,
                        asserted: assertion.asserted,
                        polarity: assertion.polarity,
                        negated: assertion.negated,
                    });
                }
            }
        }
    }
```

**(c)** Wire the three call routes. In the named-function route:
`function_annotations(db, files, stubs, configuration,
FunctionQuery::new(db, key.clone()))` — apply with the declared
signature's parameters (empty slice when `None`) and
`receiver_is_this: false`, **after** `apply_by_reference` and the
kill (an assertion is knowledge about the post-call state; the kill
must not erase it). In the method and static routes: when exactly one
receiver key resolved (Task 8's single-signature channel),
`member_annotations(db, files, stubs, configuration,
MemberQuery::new(db, key, MemberKind::Method, folded_member_key(...)))`
— `receiver_is_this` is true when the receiver expression is
`Variable { name: "this" }` (method route) or the scoped subject is
`self::`/`static::` (static route).

**(d)** Drain the pending facts in `branch_environments`' default arm,
after the `type_check_facts` block:

```rust
                let pending = std::mem::take(&mut self.pending_condition_facts);
                if !pending.is_empty() {
                    let mut when_true = environment.clone();
                    let mut when_false = environment.clone();
                    for fact in pending {
                        use crate::type_syntax::AssertionPolarity;
                        let current = self.subject_type(environment, &fact.subject);
                        let (narrowed, removed) = if fact.negated {
                            (
                                self.removed_type(current, fact.asserted),
                                self.narrowed_to(current, fact.asserted),
                            )
                        } else {
                            (
                                self.narrowed_to(current, fact.asserted),
                                self.removed_type(current, fact.asserted),
                            )
                        };
                        match fact.polarity {
                            AssertionPolarity::IfTrue => {
                                when_true.bind(fact.subject.clone(), narrowed);
                            }
                            AssertionPolarity::IfFalse => {
                                when_false.bind(fact.subject.clone(), removed);
                            }
                            AssertionPolarity::Always => {}
                        }
                    }
                    return (when_true, when_false);
                }
```

Also clear `pending_condition_facts` at the top of the default arm
(before typing the condition), so facts from an unrelated earlier
call never leak into this condition.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types 2>&1 | tail -5` — PASS.
Full local gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/inference.rs
git commit -m "✨ feat(types): assertion tags narrow at their call sites"
```

---

### Task 12: Invalidation probes and the typed instrument

The plan's closure: the design's harness-2 edit classes pinned as
probe tests (a body edit with an identical inferred return spares
callers; a prose docblock edit re-runs nothing typed; a signature or
default-value edit invalidates dependents), and the edge-count
instrument pinned end to end.

**Files:**
- Modify: `crates/celerrate_types/tests/invalidation_scope.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests only)

**Interfaces:**
- Consumes: `TestDatabase::take_executed()` (the WillExecute log —
  the file's existing probe convention; reuse its local helpers),
  `salsa::Setter` for edits, Task 10's `inferred_function_return`.
- Produces: tests only.

- [ ] **Step 1: Write the probes (failing only if the machinery is wrong)**

These tests pin behavior that should already hold by construction —
they are the mechanical proof, and they go red exactly when someone
breaks the cutoff. Add to
`crates/celerrate_types/tests/invalidation_scope.rs`, following the
file's existing fixture and helper conventions (it already builds the
quartet and counts executions from `take_executed()`; reuse those
helpers — the code below writes them out in case the local names
differ, and the executor should fold it into the file's own idiom):

```rust
/// Executions of one query family in the drained log.
fn executions_of(log: &[String], query: &str) -> usize {
    log.iter().filter(|entry| entry.contains(query)).count()
}

#[test]
fn a_body_edit_with_an_identical_inferred_return_spares_callers() {
    // File 0: the caller. File 1: the callee.
    let fixture = fixture(&[
        "<?php function caller() { return callee(); }",
        "<?php function callee() { return 1; }",
    ]);
    let caller = FunctionQuery::new(&fixture.db, "caller".to_owned());
    let _ = inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        caller,
    );
    let _ = fixture.db.take_executed();

    // The callee's body changes; its inferred return does not.
    {
        use salsa::Setter as _;
        fixture.handles[1]
            .set_content(&mut fixture.db.clone())
            .to(b"<?php function callee() { $noise = 'x'; return 1; }".to_vec());
    }
    let _ = inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        caller,
    );
    let log = fixture.db.take_executed();
    // The callee re-infers; the identical return backdates; the
    // caller's inference never re-runs (design section 10, harness 2).
    assert_eq!(executions_of(&log, "inferred_body_types"), 1);
}

#[test]
fn a_prose_docblock_edit_re_runs_no_inference() {
    let fixture = fixture(&[
        "<?php function caller() { return callee(); }",
        "<?php /** a docblock */ function callee(): int { return 1; }",
    ]);
    let caller = FunctionQuery::new(&fixture.db, "caller".to_owned());
    let _ = inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        caller,
    );
    let _ = fixture.db.take_executed();
    {
        use salsa::Setter as _;
        fixture.handles[1]
            .set_content(&mut fixture.db.clone())
            .to(b"<?php /** reworded prose */ function callee(): int { return 1; }".to_vec());
    }
    let _ = inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        caller,
    );
    let log = fixture.db.take_executed();
    // The two-stage cutoff (design section 5): the annotation parse
    // re-runs and backdates; no typed query above it re-executes.
    assert_eq!(executions_of(&log, "inferred_body_types"), 0);
}

#[test]
fn a_default_value_edit_invalidates_the_signatures_dependents() {
    let fixture = fixture(&[
        "<?php function callee(?string $s = null) { return $s; }",
    ]);
    let callee = FunctionQuery::new(&fixture.db, "callee".to_owned());
    let before = inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        callee,
    );
    let _ = fixture.db.take_executed();
    {
        use salsa::Setter as _;
        fixture.handles[0]
            .set_content(&mut fixture.db.clone())
            .to(b"<?php function callee(?string $s = 'd') { return $s; }".to_vec());
    }
    let after = inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        callee,
    );
    let log = fixture.db.take_executed();
    // The default value is part of the comparable signature (the 1a
    // contract): the member projection changed, so the body re-infers.
    assert_eq!(executions_of(&log, "inferred_body_types"), 1);
    let _ = (before, after);
}

#[test]
fn editing_one_signature_spares_the_other_members_inference() {
    let source_before = "<?php class A {
        public function edited(int $n) { return $n; }
        public function bystander() { return 'x'; }
    }";
    let source_after = "<?php class A {
        public function edited(string $n) { return $n; }
        public function bystander() { return 'x'; }
    }";
    let fixture = fixture(&[source_before]);
    let file = fixture.handles[0];
    // Numbering: class 0, edited 1, bystander 2. Method-inferred
    // returns are plan 6, so both bodies' inference is demanded
    // directly.
    let edited = BodyQuery::new(&fixture.db, AstId { file: FileId::new(0), index: 1 });
    let bystander = BodyQuery::new(&fixture.db, AstId { file: FileId::new(0), index: 2 });
    for body in [edited, bystander] {
        let _ = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        );
    }
    let _ = fixture.db.take_executed();
    {
        use salsa::Setter as _;
        file.set_content(&mut fixture.db.clone())
            .to(source_after.as_bytes().to_vec());
    }
    for body in [edited, bystander] {
        let _ = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        );
    }
    let log = fixture.db.take_executed();
    // `member_tree` changed, but the per-body `body_owner` projection
    // backdates for every body whose own declaration did not: only
    // `edited`'s body re-infers (its parameter seed changed);
    // `bystander` is spared. This is the design's "editing one
    // signature does not invalidate other members' bodies" contract
    // (section 10, harness 2).
    assert_eq!(executions_of(&log, "inferred_body_types"), 1);
}
```

(`BodyQuery`, `AstId`, and `FileId` come from `celerrate_semantics` /
`celerrate_source`, both regular dependencies of this crate; add the
imports to the test file's `use` block.)

Adaptation note (not a placeholder — the contract is exact, the
spelling may differ): if `take_executed()` records event debug strings
under different names, adjust the `executions_of` needle to the actual
rendering of `inferred_body_types` in that log (run one probe with
`dbg!(&log)` once to see it); the asserted counts are the contract.
The mutable-edit shape (`fixture.db.clone()` as the setter handle)
must match the file's existing edit helper — if the file edits through
`&mut fixture.db` directly, do the same.

And the instrument's end-to-end pin, in
`crates/celerrate_types/src/inference.rs`'s test module:

```rust
    #[test]
    fn the_edge_count_instrument_counts_each_tier_once() {
        let fixture = fixture(&[
            "<?php
            function declared_edge(): int { return 1; }
            function inferred_edge() { return 'x'; }
            function maker(): string { return 'x'; }
            function f() { return [declared_edge(), inferred_edge(), maker()]; }",
        ]);
        register_fake_provider(&fixture);
        let file = fixture.handles[0];
        let body = body_query(&fixture, 3);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.declared_return_edges, 1);
        assert_eq!(inferred.edge_counts.inferred_return_edges, 1);
        assert_eq!(inferred.edge_counts.provider_edges, 1);
    }
```

- [ ] **Step 2: Run the probes**

Run: `cargo test --package celerrate_types 2>&1 | tail -5`
Expected: PASS if Tasks 1-11 kept their contracts; any red probe is a
real cutoff regression to fix **in the production code**, not in the
probe. (A probe that is red on first run is this task's actual TDD
moment: diagnose with the systematic-debugging skill before touching
anything.)

- [ ] **Step 3: The instrument's documentation**

Confirm the two rustdoc anchors exist and say what plans 8/9a/9b need
(they were written in Tasks 1 and 10; touch them up if the final
shapes drifted):

- `inferred_body_types`: the LRU-lever note (decision 14) naming plan
  9b as the owner of the capacity.
- `InterproceduralEdgeCounts`: the note that the orchestration layer
  aggregates these into `CELERRATE_CACHE_STATS` when plan 8 first
  demands inference (decision 13).

- [ ] **Step 4: Run the full gate one last time**

Run: `cargo test --workspace 2>&1 | tail -3`;
`cargo clippy --workspace --all-targets -- -D warnings`;
`cargo fmt --all`; `cargo deny check`; `cargo xtask dependency-shape`.
All green.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/tests/invalidation_scope.rs crates/celerrate_types/src/inference.rs
git commit -m "✅ test(types): the typed edit classes pin their cutoffs and the instrument counts"
```

---

## Execution notes

- Tasks run strictly in order: each one's walker builds on the
  previous one's helpers. No two tasks touch disjoint code — do not
  parallelize.
- The plan-6 seams are marked in code comments where plan 6 replaces
  scaffolding: `this_type` (placeholders), `substitute_receiver` (the
  forwarding model), the `Foreach` arm (iteration typing), the
  method-inferred `mixed` tier in `method_call_result_for_keys`.
- When a display assertion disagrees with `display.rs`, decision 16
  applies: fix the expectation, never the code.
- After the final task, do not extend the README or CHANGELOG: the
  preview's product surface is plan 9c's, and nothing user-visible
  changed here.
