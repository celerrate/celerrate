# Type Engine 6 — Interprocedural Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The interprocedural layer: late-static-binding placeholders
carried in inference results and substituted at call sites,
method-inferred returns under the existing fixpoint discipline, trait
bodies analyzed per using class, generic-argument threading from
`@extends`/`@implements`/`@use` through linearization's ancestry, the
call-site template solver (generics stay inference-only), iteration
typing through the protocol chain, and the annotation ground-truth
harness with its committed, triaged baseline. Design source:
`.claude/superpowers/specs/2026-07-14-type-engine-design.md`, sections
6 (the model, interprocedural, generics, iteration typing), 2 (the
generic-argument threading linearization reserved), 3 (declared types
inherit; template variables and conditional types as lattice
citizens), 10 (harness 1 — annotation ground truth — and harness 3
extended to method cycles), and 11 item 9 (this plan).

**Architecture:** Everything lands in existing crates; no new crate
and no new dependency edge. `celerrate_types` gains three modules —
`substitution.rs` (the structural substitution primitive: template
maps, late-static-binding resolution, conditional evaluation),
`inheritance.rs` (class-level annotations composed along
linearization's `ancestry` edges into per-ancestor argument lists),
and `solver.rs` (call-site constraint collection and solving) — plus
a second cycle-recovered query, `inferred_method_return`, mirroring
the plan-5 free-function fixpoint with per-defining-class keys and
per-using-class keys for trait bodies. The bridge grows the
inheritance-position tags; the CLI grows a hidden `ground-truth`
subcommand; `xtask` grows the harness that pins its output as a
committed, classified baseline.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), salsa 0.27.2
(`cycle_fn`/`cycle_initial` fixpoint recovery, interned queries), the
plan-5 inference engine (`inferred_body_types`, `ascend`,
`FIXPOINT_ITERATION_BUDGET`), the plan-1a linearization
(`LinearizedClass.ancestry`), the plan-4 bridge and type-syntax
extension point, the existing corpus pin (`xtask/corpus.pin`).

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
  No indexing: `.get()`, `.first()`, iterators, `.split_once()`.
  The method fixpoint reuses the plan-5 budget
  (`FIXPOINT_ITERATION_BUDGET = 32`) precisely because salsa panics
  at `MAX_ITERATIONS = 200` (`salsa-0.27.2/src/cycle.rs`) — reaching
  that panic is a zero-panic breach.
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **Inference never touches a syntax tree** (design section 2): the
  walker consumes `BodyIr` and the member/declared queries only.
  Inline `@var` text is read from `BodyIr.annotations` — it is IN the
  IR by design — never from trivia. No `TextRange` in any inference
  result.
- **Determinism**: no wall clock, no randomness, no environment reads
  inside queries. `Substitution` iterates in `BTreeMap` order;
  `ancestor_arguments` answers a `Vec` in linearization walk order
  (deterministic BFS); diamond inheritance resolves first-edge-wins.
  The ground-truth report sorts by symbol key before printing.
- **Conservative silence, never a guess**: `mixed` is the answer to
  everything this plan cannot know (unfixed ancestor templates fall
  to their bound then `mixed`; failed solver constraints fall to the
  bound then `mixed`, never a first-seen constituent; stub ancestors
  carry no generic arguments and degrade honestly).
- **Every typing form is covered by tests before it can influence a
  published diagnostic** (design section 6): each substitution rule,
  solver rule, and iteration-protocol step lands with its own test in
  the same task.
- **No diagnostics ship from this plan**: no new `CEL####`
  identifier, no rendering change. The `ground-truth` subcommand is
  internal — hidden from `--help`, undocumented (plan 9c owns the
  product surface); it prints records, never diagnostics.
- **Strict layering**: NO new inter-crate dependency edge. The bridge
  keeps its single `celerrate_plugin` dependency; `xtask` keeps
  depending on no `celerrate_*` crate (it spawns the built binary);
  `cargo xtask dependency-shape` stays green.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.

## Fixed decisions (the header the tasks implement)

1. **The placeholder model replaces plan 5's decision 6.** Inside a
   method body, `$this` and the `static` scope keyword type as
   `TypeId::static_placeholder`; `self` stays `SelfPlaceholder`,
   `parent` stays `ParentPlaceholder`. Member resolution inside the
   body is unchanged (`receiver_parts` already resolves placeholders
   against the owner class key). Placeholders survive into
   `InferredBody.return_type` and declared returns, and are resolved
   at call sites by the substitution primitive with the **declaring
   owner** and the **receiver**: `SelfPlaceholder` → the owner class,
   `ParentPlaceholder` → the owner's first `Extends` ancestor (else
   `mixed`), `StaticPlaceholder` → the receiver type.
2. **Forwarding falls out of the receiver choice.** For
   `self::m()`/`static::m()`/`parent::m()` call subjects the receiver
   passed to substitution is the **current** `static` type — the
   `StaticPlaceholder` itself — so a substituted `static` return stays
   a placeholder and forwards to the outer caller (design section 6).
   For `Foo::m()` the receiver is `TypeId::class(db, "foo", vec![])`
   (rebinding), and for `$obj->m()` it is `$obj`'s type.
3. **Method-inferred returns complete the call tiers.** New interned
   `MethodQuery { class_key, member_key }` and cycle-recovered
   `inferred_method_return`, sharing `ascend` and the budget with the
   free-function fixpoint. The method call boundary becomes four
   tiers: **provider → declared → method-inferred → `mixed`**. Each
   method-inferred edge increments the existing
   `inferred_return_edges` counter.
4. **Re-keying pins the memo space** (design section 6: the default
   key is the defining class; the receiver key is a class definition
   identity, never a type carrying generic arguments).
   `inferred_method_return` resolves its member through
   `lookup_member`: origin `Inherited` re-keys to the owner (one memo
   per defining class, shared by every subclass); origin `Own`
   analyzes in place; origin `Trait` analyzes **per using class** —
   the query's `class_key` is the using class (PHPStan's model). Stub
   and virtual members answer `mixed` (their types come from declared
   signatures, consulted at the earlier tier).
5. **The trait context is an explicit query parameter.**
   `inferred_body_types` gains `context: InferenceContext<'db>`, an
   interned `Option<String>` using-class key. `None` everywhere
   except trait bodies analyzed for a using class, so the memo space
   for non-trait bodies does not grow. With a context, the walker's
   owner class key is the using class: `self::`, `static::`, `$this`,
   and `parent::` inside the trait body resolve against the user.
6. **Class-level annotations become a query surface.**
   `ParsedAnnotations` gains three additive fields — `templates`
   (ordered `@template` declarations), `ancestors`
   (`@extends`/`@implements`/`@use` fixed generic arguments), and
   `variables` (named inline `@var Type $x` entries). The bridge
   parses the inheritance-position tags with the existing tier
   machinery (tool-prefixed wins over bare; the `@template-extends`/
   `@template-implements`/`@template-use` long forms are synonyms of
   the bare tags). New queries in `celerrate_types/src/inheritance.rs`:
   `class_annotations` (parse one class-like's own docblock at its
   declaring site), `class_templates` (ordered template list), and
   `ancestor_arguments` (per-ancestor argument lists composed
   transitively along `LinearizedClass.ancestry`).
7. **Composition semantics for `ancestor_arguments`**: walk the
   ancestry edges in linearization order, maintaining a substitution
   per already-visited class; an edge owned by `O` targeting `A`
   contributes `A`'s arguments = `O`'s written arguments for `A`
   substituted by `O`'s own composed substitution; missing or surplus
   arguments zip against `A`'s ordered `class_templates`, a missing
   argument falling to the template's **bound then `mixed`**. Diamond
   inheritance: the first edge in walk order wins, deterministically.
   Stub ancestors contribute no arguments (recorded boundary).
8. **Inherited declared signatures substitute.**
   `declared_member_signature(MemberQuery { class_key: C, .. })`
   whose lookup resolved to owner `O ≠ C` substitutes `O`-scoped
   class templates using `ancestor_arguments(C)`'s entry for `O`.
   This is the Doctrine-on-Symfony delivery path:
   `@extends ServiceEntityRepository<User>` makes `find()` answer
   `User|null` on the concrete repository.
9. **The substitution primitive owns conditional evaluation.** A
   `Conditional` whose substituted subject is decidable evaluates via
   the three-valued subtype judgment: Holds → then-branch, Fails →
   otherwise-branch, CannotProve → the branch union. Parameter-subject
   conditionals stay lowered to the branch union (the bridge's
   existing rule) — a recorded debt, not this plan's scope.
10. **The call-site solver is structural and never guesses.**
    Constraints are collected by matching declared parameter types
    against argument types: `Template` binds its argument;
    `ClassString { argument: Template }` binds through string
    literals, `Foo::class` (typed `class-string<Foo>` from this plan
    on), and other `class-string` values; `Class` recurses argument-
    wise when names match, else through the argument class's own
    `ancestor_arguments`; arrays, shapes, and callables recurse
    element-wise; unions collect from every constituent. Multiple
    constraints on one variable take `TypeId::union` (the lattice
    least upper bound); an unconstrained or failed variable in the
    return substitutes to its **bound then `mixed`** — never a
    first-seen constituent.
11. **Constructor inference and inline `@var` deliver class
    generics.** `new Foo(...)` solves `Foo`'s class-level templates
    from `__construct` arguments and answers
    `Class { name, arguments }`. A named inline `@var Collection<User>
    $c` (the `variables` field, anchored in `BodyIr.annotations`)
    binds the local before its anchored statement and re-binds after
    it, so the declaration survives the statement's own assignment.
12. **Iteration typing follows the protocol chain with a fixed
    precedence** (design section 6): array forms answer their key and
    value directly (a shape answers its key union and value union;
    a list answers `int` keys); `Generator` answers its first two
    class arguments; an `IteratorAggregate` implementor unwraps
    `getIterator()`'s declared-or-inferred return recursively under a
    depth guard of 8; an `Iterator`/`Traversable` implementor answers
    the threaded ancestor arguments when present, else the declared-
    or-inferred returns of `current()`/`key()`; unions join
    constituent answers (null and false constituents are skipped —
    iterating them yields nothing); a template recurses through its
    bound; everything else answers `mixed`/`mixed`. A by-reference
    foreach value binds like a plain value; no write-back (recorded).
13. **The ground-truth harness is a pinned pipeline, not a test.**
    The CLI gains a hidden `ground-truth <path>` subcommand printing
    one tab-separated record per divergence — a source function or
    method whose docblock-annotated return exists, whose body exists,
    and whose inferred return is **not** a subtype of the annotated
    return (`Proof::Fails`; `CannotProve` passes, per the design's
    compatibility relation) — sorted by symbol key, with a trailing
    `checked N, divergences M` summary. `cargo xtask ground-truth`
    runs it over the pinned corpus and gates against the committed
    `xtask/ground-truth-baseline.txt`; `--bless` regenerates the
    baseline **preserving the classification column** for persisting
    records, auto-classifying new records `precision-gap` when the
    inferred display is `mixed`, else `unclassified`. The gate fails
    on any divergence absent from the baseline (regressions), never
    on the baseline's size (a drowning protocol is no protocol).
14. **Anonymous-class receivers stay `mixed` and the debt re-homes
    to plan 8.** The expression-to-key path for `new class { }`
    belongs with the checks' receiver-resolution surface — no
    diagnostic consumes receiver types before plan 8, so building the
    path here would ship untested-by-consumer machinery. Recorded in
    the closing debt ledger with this justification.
15. **Termination is inherited, not re-proven.** Substitution and
    solving construct exclusively through the existing capped
    constructors (`capped_child`, the union arity cap), so the
    structural depth cap bounds every value they can build; the
    method fixpoint reuses `ascend` and the shared budget; the
    participant-set argument (finite class set, monotone resolution
    over the join discipline) now covers method cycles, pinned by the
    extended fixtures.

## File structure

Created:

- `crates/celerrate_types/src/substitution.rs` — the substitution
  map, the structural `substitute`, late-static-binding resolution,
  conditional evaluation.
- `crates/celerrate_types/src/inheritance.rs` — `class_annotations`,
  `class_templates`, `class_generic_ancestors`, `ancestor_arguments`.
- `crates/celerrate_types/src/solver.rs` — constraint collection and
  solving at call sites.
- `crates/celerrate_cli/src/ground_truth.rs` — the hidden subcommand's
  record producer.
- `xtask/src/ground_truth.rs` — the corpus harness and baseline gate.
- `xtask/ground-truth-baseline.txt` — the committed, classified
  baseline (blessed in task 12).

Modified:

- `crates/celerrate_types/src/type_syntax.rs` — `ParsedAnnotations`
  gains `templates`, `ancestors`, `variables`; new payload structs.
- `crates/celerrate_types/src/declared.rs` — inherited-signature
  substitution hook; `owner_class_docblock` and `with_declaring_site`
  become `pub(crate)` for `inheritance.rs`.
- `crates/celerrate_types/src/inference.rs` — `InferenceContext`,
  `MethodQuery`, `inferred_method_return` with its cycle functions.
- `crates/celerrate_types/src/flow.rs` — placeholder carriage,
  owner-aware substitution at every member boundary, the method-
  inferred tier, `Foo::class` as `class-string<Foo>`, call-site
  solving, constructor inference, inline `@var`, iteration typing.
- `crates/celerrate_types/src/lib.rs` — module declarations and
  re-exports (`MethodQuery`, `InferenceContext`,
  `inferred_method_return`, the inheritance queries).
- `crates/celerrate_phpdoc_bridge/src/tags.rs` — the
  inheritance-position tag roles and named `@var`.
- `crates/celerrate_phpdoc_bridge/src/lowering.rs` — lowering the new
  tag payloads into `ParsedAnnotations`.
- `crates/celerrate_cli/src/arguments.rs`, `src/lib.rs` — the hidden
  subcommand.
- `xtask/src/main.rs`, `xtask/src/lib.rs` — dispatch for
  `ground-truth [--bless]`.
- `.github/workflows/corpus.yml` — the `ground-truth` job.
- `crates/celerrate_types/tests/fixpoint.rs`,
  `crates/celerrate_types/tests/invalidation_scope.rs` — method-cycle
  determinism, the new edit classes.

Task order is strict: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 →
12 → 13. Tasks 2–4 build the generics channel, 5–7 the receiver
model, 8–10 the consumers, 11–12 the harness, 13 the closure.

---

### Task 1: The substitution primitive

The one structural walk every later task calls: template maps,
late-static-binding resolution, and conditional evaluation, in a new
`substitution.rs`. It is a plain crate-internal function, not a salsa
query — its callers are already inside queries. The recursion skeleton
deliberately mirrors `widening::widened_literals` (same variants, same
constructors), so every composite it rebuilds goes through the capped
constructors and the structural depth cap holds by construction
(decision 15). If a constructor name in this task's code disagrees
with what `construction.rs` actually exports, follow
`widened_literals`'s recursion arms — they already rebuild every
composite variant — and keep this task's semantics.

**Files:**
- Create: `crates/celerrate_types/src/substitution.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (add `mod substitution;`)

**Interfaces:**
- Consumes: `TypeId` constructors and `data(db)` access
  (`representation.rs`, `construction.rs`), `judgments::subtype_of`
  and `Proof`, the input quartet types (`AnalyzedFileSet`,
  `StubIndexInput`, `ProjectConfiguration`).
- Produces (later tasks rely on these exact names):
  - `pub(crate) struct Substitution<'db>` with `fn bind(&mut self,
    scope: &str, name: &str, to: TypeId<'db>)`, `fn binding(&self,
    scope: &str, name: &str) -> Option<TypeId<'db>>`,
    `fn is_empty(&self) -> bool`.
  - `pub(crate) struct PlaceholderResolution<'db> { pub owner:
    Option<String>, pub parent: Option<String>, pub receiver:
    Option<TypeId<'db>> }`.
  - `pub(crate) fn substitute<'db>(db, files, stubs, configuration,
    of: TypeId<'db>, map: &Substitution<'db>, placeholders:
    Option<&PlaceholderResolution<'db>>) -> TypeId<'db>`.
  - `pub(crate) fn contains_symbolic<'db>(db, of: TypeId<'db>) ->
    bool` (any `Template` or placeholder anywhere inside).

- [ ] **Step 1: Write the failing tests**

At the bottom of the new `crates/celerrate_types/src/substitution.rs`
(create the file with only a module doc and the test module for the
red step; the `#[cfg(test)]` fixture mirrors `inference.rs`'s):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, FileId, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{PlaceholderResolution, Substitution, contains_symbolic, substitute};
    use crate::representation::TypeId;

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    fn fixture() -> Fixture {
        let db = TestDatabase::default();
        let handles = vec![SourceFile::new(&db, FileId::new(0), b"<?php".to_vec())];
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Fixture { db, files, stubs, configuration }
    }

    #[test]
    fn a_bound_template_substitutes_and_an_unbound_template_stays() {
        let f = fixture();
        let db = &f.db;
        let user = TypeId::class(db, "app\\user", vec![]);
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "app\\collection", "T", bound);
        let u = TypeId::template(db, "app\\collection", "U", bound);
        let mut map = Substitution::default();
        map.bind("app\\collection", "T", user);
        let substituted_t =
            substitute(db, f.files, f.stubs, f.configuration, t, &map, None);
        let substituted_u =
            substitute(db, f.files, f.stubs, f.configuration, u, &map, None);
        assert_eq!(substituted_t, user, "a bound template takes its binding");
        assert_eq!(substituted_u, u, "an unbound template stays itself");
    }

    #[test]
    fn placeholders_resolve_against_owner_parent_and_receiver() {
        let f = fixture();
        let db = &f.db;
        let receiver = TypeId::class(db, "app\\child", vec![]);
        let resolution = PlaceholderResolution {
            owner: Some("app\\base".to_owned()),
            parent: Some("app\\grandbase".to_owned()),
            receiver: Some(receiver),
        };
        let map = Substitution::default();
        let cases = [
            (TypeId::self_placeholder(db), TypeId::class(db, "app\\base", vec![])),
            (TypeId::parent_placeholder(db), TypeId::class(db, "app\\grandbase", vec![])),
            (TypeId::static_placeholder(db), receiver),
        ];
        for (input, expected) in cases {
            let answer = substitute(
                db, f.files, f.stubs, f.configuration, input, &map, Some(&resolution),
            );
            assert_eq!(answer, expected);
        }
    }

    #[test]
    fn an_unresolvable_placeholder_widens_to_mixed_and_none_leaves_it_intact() {
        let f = fixture();
        let db = &f.db;
        let map = Substitution::default();
        let no_parent = PlaceholderResolution {
            owner: Some("app\\base".to_owned()),
            parent: None,
            receiver: None,
        };
        let parent = TypeId::parent_placeholder(db);
        let widened = substitute(
            db, f.files, f.stubs, f.configuration, parent, &map, Some(&no_parent),
        );
        assert_eq!(widened, TypeId::mixed(db), "no parent resolves to mixed");
        let untouched =
            substitute(db, f.files, f.stubs, f.configuration, parent, &map, None);
        assert_eq!(untouched, parent, "no resolution requested leaves it intact");
    }

    #[test]
    fn a_static_placeholder_receiver_forwards() {
        // `self::create()` inside a method: the receiver for
        // substitution is the current `static` type — the placeholder
        // itself — so a `static` return survives substitution and
        // forwards to the outer caller (decision 2).
        let f = fixture();
        let db = &f.db;
        let resolution = PlaceholderResolution {
            owner: Some("app\\base".to_owned()),
            parent: None,
            receiver: Some(TypeId::static_placeholder(db)),
        };
        let map = Substitution::default();
        let answer = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            TypeId::static_placeholder(db),
            &map,
            Some(&resolution),
        );
        assert_eq!(answer, TypeId::static_placeholder(db));
    }

    #[test]
    fn substitution_recurses_through_composites() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let mut map = Substitution::default();
        map.bind("s", "T", user);
        let composite = TypeId::union(
            db,
            [
                TypeId::class(db, "app\\collection", vec![t]),
                TypeId::null(db),
            ],
        );
        let expected = TypeId::union(
            db,
            [
                TypeId::class(db, "app\\collection", vec![user]),
                TypeId::null(db),
            ],
        );
        let answer =
            substitute(db, f.files, f.stubs, f.configuration, composite, &map, None);
        assert_eq!(answer, expected);
    }

    #[test]
    fn a_decided_conditional_picks_its_branch_and_negation_flips_it() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let admin = TypeId::class(db, "app\\admin", vec![]);
        let then_branch = TypeId::class(db, "app\\then", vec![]);
        let otherwise_branch = TypeId::class(db, "app\\otherwise", vec![]);
        // (T is app\user ? then : otherwise) with T := app\user.
        let conditional =
            TypeId::conditional(db, t, user, then_branch, otherwise_branch, false);
        let negated =
            TypeId::conditional(db, t, user, then_branch, otherwise_branch, true);
        let mut holds = Substitution::default();
        holds.bind("s", "T", user);
        let mut fails = Substitution::default();
        fails.bind("s", "T", admin);
        let picked =
            substitute(db, f.files, f.stubs, f.configuration, conditional, &holds, None);
        assert_eq!(picked, then_branch, "Holds picks the then branch");
        let flipped =
            substitute(db, f.files, f.stubs, f.configuration, negated, &holds, None);
        assert_eq!(flipped, otherwise_branch, "negation flips the pick");
        let missed =
            substitute(db, f.files, f.stubs, f.configuration, conditional, &fails, None);
        assert_eq!(missed, otherwise_branch, "Fails picks the otherwise branch");
    }

    #[test]
    fn an_undecided_conditional_answers_the_branch_union() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let then_branch = TypeId::class(db, "app\\then", vec![]);
        let otherwise_branch = TypeId::class(db, "app\\otherwise", vec![]);
        let conditional =
            TypeId::conditional(db, t, user, then_branch, otherwise_branch, false);
        // T bound to another template: still symbolic — branch union.
        let other = TypeId::template(db, "other", "U", bound);
        let mut map = Substitution::default();
        map.bind("s", "T", other);
        let answer =
            substitute(db, f.files, f.stubs, f.configuration, conditional, &map, None);
        assert_eq!(answer, TypeId::union(db, [then_branch, otherwise_branch]));
    }

    #[test]
    fn contains_symbolic_sees_through_composites() {
        let f = fixture();
        let db = &f.db;
        let t = TypeId::template(db, "s", "T", TypeId::mixed(db));
        let nested = TypeId::class(db, "app\\collection", vec![t]);
        assert!(contains_symbolic(db, nested));
        assert!(contains_symbolic(db, TypeId::static_placeholder(db)));
        assert!(!contains_symbolic(db, TypeId::class(db, "app\\user", vec![])));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types substitution -- --nocapture`
Expected: FAIL to compile — `Substitution`, `PlaceholderResolution`,
`substitute`, `contains_symbolic` do not exist.

- [ ] **Step 3: Implement the module**

Above the test module in
`crates/celerrate_types/src/substitution.rs`:

```rust
//! The structural substitution primitive (design section 6): template
//! maps solved at call sites, late-static-binding resolution against
//! an owner and a receiver, and conditional-type evaluation once a
//! subject becomes decidable. A plain function, not a query — every
//! caller is already inside one. The recursion mirrors
//! [`crate::widening::widened_literals`]: every composite rebuilds
//! through the capped constructors, so the structural depth cap
//! bounds every value this module can produce (decision 15).

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::judgments::{Proof, subtype_of};
use crate::representation::{CallableParameter, ShapeField, TypeData, TypeId};

/// A finite map from template variables — keyed by `(scope, name)` —
/// to their substituted types. `BTreeMap` for deterministic iteration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Substitution<'db> {
    bindings: BTreeMap<(String, String), TypeId<'db>>,
}

impl<'db> Substitution<'db> {
    pub(crate) fn bind(&mut self, scope: &str, name: &str, to: TypeId<'db>) {
        self.bindings
            .insert((scope.to_owned(), name.to_owned()), to);
    }

    pub(crate) fn binding(&self, scope: &str, name: &str) -> Option<TypeId<'db>> {
        self.bindings
            .get(&(scope.to_owned(), name.to_owned()))
            .copied()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

/// The late-static-binding targets of one call site (decision 1):
/// `self` resolves to the declaring owner, `parent` to the owner's
/// first `Extends` ancestor (the caller walks the ancestry — this
/// module stays lattice-pure), `static` to the receiver. A `None`
/// field widens its placeholder to `mixed`; passing no resolution at
/// all (`placeholders: None` on [`substitute`]) leaves placeholders
/// intact for pure template substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaceholderResolution<'db> {
    pub owner: Option<String>,
    pub parent: Option<String>,
    pub receiver: Option<TypeId<'db>>,
}

/// Substitutes templates and (optionally) placeholders through `of`,
/// evaluating conditionals whose substituted subject is decidable.
pub(crate) fn substitute<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    of: TypeId<'db>,
    map: &Substitution<'db>,
    placeholders: Option<&PlaceholderResolution<'db>>,
) -> TypeId<'db> {
    let recurse = |child: TypeId<'db>| {
        substitute(db, files, stubs, configuration, child, map, placeholders)
    };
    match of.data(db) {
        TypeData::Mixed
        | TypeData::Never
        | TypeData::Void
        | TypeData::Null
        | TypeData::Object
        | TypeData::Resource
        | TypeData::Bool { .. }
        | TypeData::Int { .. }
        | TypeData::Float { .. }
        | TypeData::String { .. }
        | TypeData::EnumCase { .. } => of,
        TypeData::Template { scope, name, .. } => {
            map.binding(scope, name).unwrap_or(of)
        }
        TypeData::SelfPlaceholder => match placeholders {
            None => of,
            Some(resolution) => match &resolution.owner {
                Some(owner) => TypeId::class(db, owner, vec![]),
                None => TypeId::mixed(db),
            },
        },
        TypeData::ParentPlaceholder => match placeholders {
            None => of,
            Some(resolution) => match &resolution.parent {
                Some(parent) => TypeId::class(db, parent, vec![]),
                None => TypeId::mixed(db),
            },
        },
        TypeData::StaticPlaceholder => match placeholders {
            None => of,
            Some(resolution) => resolution.receiver.unwrap_or_else(|| TypeId::mixed(db)),
        },
        TypeData::Union { constituents } => {
            let substituted: Vec<_> = constituents.iter().copied().map(recurse).collect();
            TypeId::union(db, substituted)
        }
        TypeData::Intersection { intersectands } => {
            let substituted: Vec<_> = intersectands.iter().copied().map(recurse).collect();
            TypeId::intersection(db, substituted)
        }
        TypeData::Array { key, value, is_list, non_empty } => {
            TypeId::array_form(db, recurse(*key), recurse(*value), *is_list, *non_empty)
        }
        TypeData::Shape { fields } => {
            let substituted: Vec<ShapeField<'db>> = fields
                .iter()
                .map(|field| ShapeField {
                    key: field.key.clone(),
                    optional: field.optional,
                    value: recurse(field.value),
                })
                .collect();
            TypeId::shape(db, substituted)
        }
        TypeData::ClassString { argument } => {
            TypeId::class_string(db, argument.map(recurse))
        }
        TypeData::Class { name, arguments } => {
            let substituted: Vec<_> = arguments.iter().copied().map(recurse).collect();
            TypeId::class(db, name, substituted)
        }
        TypeData::Callable { parameters, return_type } => {
            let substituted: Vec<CallableParameter<'db>> = parameters
                .iter()
                .map(|parameter| CallableParameter {
                    parameter_type: recurse(parameter.parameter_type),
                    optional: parameter.optional,
                    variadic: parameter.variadic,
                    by_reference: parameter.by_reference,
                })
                .collect();
            TypeId::callable(db, substituted, recurse(*return_type))
        }
        TypeData::KeyOf { subject } => TypeId::key_of(db, recurse(*subject)),
        TypeData::ValueOf { subject } => TypeId::value_of(db, recurse(*subject)),
        TypeData::Conditional { subject, matches, then_branch, otherwise_branch, negated } => {
            let subject = recurse(*subject);
            let matches = recurse(*matches);
            let then_branch = recurse(*then_branch);
            let otherwise_branch = recurse(*otherwise_branch);
            if contains_symbolic(db, subject) {
                return TypeId::conditional(
                    db, subject, matches, then_branch, otherwise_branch, *negated,
                );
            }
            let (on_holds, on_fails) = if *negated {
                (otherwise_branch, then_branch)
            } else {
                (then_branch, otherwise_branch)
            };
            match subtype_of(db, files, stubs, configuration, subject, matches) {
                Proof::Holds => on_holds,
                Proof::Fails => on_fails,
                Proof::CannotProve => TypeId::union(db, [then_branch, otherwise_branch]),
            }
        }
    }
}

/// Whether any `Template` or late-static-binding placeholder occurs
/// anywhere inside `of` — the "still symbolic" test conditional
/// evaluation and the solver's return-substitution use.
pub(crate) fn contains_symbolic<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    match of.data(db) {
        TypeData::Template { .. }
        | TypeData::SelfPlaceholder
        | TypeData::ParentPlaceholder
        | TypeData::StaticPlaceholder => true,
        TypeData::Mixed
        | TypeData::Never
        | TypeData::Void
        | TypeData::Null
        | TypeData::Object
        | TypeData::Resource
        | TypeData::Bool { .. }
        | TypeData::Int { .. }
        | TypeData::Float { .. }
        | TypeData::String { .. }
        | TypeData::EnumCase { .. } => false,
        TypeData::Union { constituents } => constituents
            .iter()
            .any(|child| contains_symbolic(db, *child)),
        TypeData::Intersection { intersectands } => intersectands
            .iter()
            .any(|child| contains_symbolic(db, *child)),
        TypeData::Array { key, value, .. } => {
            contains_symbolic(db, *key) || contains_symbolic(db, *value)
        }
        TypeData::Shape { fields } => fields
            .iter()
            .any(|field| contains_symbolic(db, field.value)),
        TypeData::ClassString { argument } => argument
            .map(|child| contains_symbolic(db, child))
            .unwrap_or(false),
        TypeData::Class { arguments, .. } => arguments
            .iter()
            .any(|child| contains_symbolic(db, *child)),
        TypeData::Callable { parameters, return_type } => {
            parameters
                .iter()
                .any(|parameter| contains_symbolic(db, parameter.parameter_type))
                || contains_symbolic(db, *return_type)
        }
        TypeData::KeyOf { subject } | TypeData::ValueOf { subject } => {
            contains_symbolic(db, *subject)
        }
        TypeData::Conditional { subject, matches, then_branch, otherwise_branch, .. } => {
            contains_symbolic(db, *subject)
                || contains_symbolic(db, *matches)
                || contains_symbolic(db, *then_branch)
                || contains_symbolic(db, *otherwise_branch)
        }
    }
}
```

Add `mod substitution;` to `crates/celerrate_types/src/lib.rs` next to
the other private modules (alphabetical order). Two adjustment rules
for this step, both mechanical: (a) if `TypeId::array_form` (or any
other constructor named here) does not exist under that name, use the
exact constructor `widening::widened_literals` uses for the same
variant; (b) if `TypeData` variants have gained or lost a field, match
the compiler — semantics above, spelling from the code.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types substitution`
Expected: PASS (8 tests). Then the full gate:
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/substitution.rs crates/celerrate_types/src/lib.rs
git commit -m "✨ feat(types): the structural substitution primitive"
```

---

### Task 2: The inheritance-position tags and the annotation payload

The bridge learns `@extends`/`@implements`/`@use` (and their
`@template-*` long forms and tool-prefixed tiers) plus the named
inline `@var Type $x` form; `ParsedAnnotations` gains the three
additive fields every later task reads. Loss stays per construct: one
unparseable ancestor tag drops, its siblings survive.

**Files:**
- Modify: `crates/celerrate_types/src/type_syntax.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/dialect/mod.rs` (and the
  per-dialect classification tables it dispatches to)
- Modify: `crates/celerrate_phpdoc_bridge/src/tags.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/lowering.rs`

**Interfaces:**
- Consumes: `TagRole`/`ClassifiedTag`/`TagTier` (dialect
  classification), `MemberDocblock` and `extract_member_docblock`
  (tags.rs), `parse_type_expression_prefix`, `TypeExpression`,
  `AnnotationSite` (`qualify_class_name`, `database`).
- Produces (later tasks rely on these exact names):
  - `TagRole::Extends`, `TagRole::Implements`, `TagRole::UseTrait`
    (dialect classification; `@template-extends` classifies as
    `Extends`, `@template-implements` as `Implements`,
    `@template-use` and `@use` as `UseTrait`).
  - `pub struct AncestorDeclaration { pub expression: TypeExpression }`
    and `MemberDocblock.ancestors: Vec<AncestorDeclaration>` —
    slot-resolved per written head name under the tier rule.
  - `MemberDocblock.variable_values: Vec<(String, TypeExpression)>` —
    named `@var` entries, slot-resolved per variable name; the
    unnamed `@var` keeps feeding `value_type`.
  - In `celerrate_types::type_syntax`:
    `pub struct ParsedTemplate<'db> { pub name: String, pub bound:
    Option<TypeId<'db>> }`,
    `pub struct ParsedAncestor<'db> { pub class_name: String, pub
    arguments: Vec<TypeId<'db>> }` (class_name pre-folded, qualified
    at the declaring site), and on `ParsedAnnotations`:
    `pub templates: Vec<ParsedTemplate<'db>>`,
    `pub ancestors: Vec<ParsedAncestor<'db>>`,
    `pub variables: Vec<(String, TypeId<'db>)>`.

- [ ] **Step 1: Write the failing extraction tests**

In the existing `#[cfg(test)]` module of
`crates/celerrate_phpdoc_bridge/src/tags.rs`, following its existing
helper for building a `Tag` stream from a docblock string (the lexer
entry the sibling tests use — reuse it verbatim):

```rust
#[test]
fn extends_implements_and_use_extract_with_their_long_forms() {
    let docblock = "/**\n * @extends Base<User>\n * @implements Countable<int>\n * @template-use Loggable<string>\n */";
    let extracted = extract_member_docblock(&lex(docblock));
    let written: Vec<String> = extracted
        .ancestors
        .iter()
        .map(|ancestor| ancestor.expression.display_for_tests())
        .collect();
    assert_eq!(written, ["Base<User>", "Countable<int>", "Loggable<string>"]);
}

#[test]
fn a_tool_prefixed_ancestor_tag_wins_its_bare_sibling() {
    let docblock =
        "/**\n * @extends Base<User>\n * @phpstan-extends Base<Admin>\n */";
    let extracted = extract_member_docblock(&lex(docblock));
    let written: Vec<String> = extracted
        .ancestors
        .iter()
        .map(|ancestor| ancestor.expression.display_for_tests())
        .collect();
    assert_eq!(written, ["Base<Admin>"], "PHPStan tier wins per head name");
}

#[test]
fn a_named_inline_var_lands_in_variable_values_and_unnamed_var_stays() {
    let named = extract_member_docblock(&lex("/** @var Collection<User> $items */"));
    assert_eq!(named.variable_values.len(), 1);
    let (name, _) = named.variable_values.first().unwrap();
    assert_eq!(name, "items");
    assert!(named.value_type.is_none(), "the named form never fills the slot");

    let unnamed = extract_member_docblock(&lex("/** @var Collection<User> */"));
    assert!(unnamed.value_type.is_some());
    assert!(unnamed.variable_values.is_empty());
}

#[test]
fn an_unparseable_ancestor_drops_and_its_siblings_survive() {
    let docblock = "/**\n * @extends <<<broken\n * @implements Countable<int>\n */";
    let extracted = extract_member_docblock(&lex(docblock));
    assert_eq!(extracted.ancestors.len(), 1, "loss is per construct");
}
```

If `TypeExpression` has no test-display helper, assert on the parsed
structure the way the module's existing template tests do (match the
head name and argument count through the expression's public fields)
— keep the assertions' meaning, adapt their spelling to the module's
idiom.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_phpdoc_bridge tags`
Expected: FAIL to compile — `ancestors`, `variable_values`,
`AncestorDeclaration` do not exist.

- [ ] **Step 3: Implement extraction**

1. `crates/celerrate_phpdoc_bridge/src/dialect/mod.rs`: add
   `Extends`, `Implements`, `UseTrait` variants to `TagRole`. In each
   dialect's classification table, route: `extends` and
   `template-extends` → `Extends`; `implements` and
   `template-implements` → `Implements`; `use` and `template-use` →
   `UseTrait`; tool-prefixed forms (`phpstan-extends`,
   `psalm-extends`, and so on) classify at their tool's tier exactly
   like the existing `param`/`return` arms. Keep `@use` classification
   scoped to docblock tags (it collides with nothing: the lexer only
   yields `@`-tags).
2. `crates/celerrate_phpdoc_bridge/src/tags.rs`: add

```rust
/// One inheritance-position tag (`@extends`, `@implements`, `@use`
/// and their `@template-*` long forms): the written ancestor with its
/// fixed generic arguments, an unresolved type expression until the
/// declaring site qualifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorDeclaration {
    pub expression: TypeExpression,
}
```

   `MemberDocblock` gains `pub ancestors: Vec<AncestorDeclaration>`
   and `pub variable_values: Vec<(String, TypeExpression)>`. In
   `extract_member_docblock`, accumulate ancestors in a
   `Vec<(String, TagTier, AncestorDeclaration)>` slot-resolved per
   **written head name** (the first identifier token of the parsed
   expression) with the same offer-pattern `offer_template` uses;
   answer them in first-seen order. For `TagRole::Var`, after
   `parse_type_expression_prefix` succeeds, scan the remaining
   content: a leading `$name` token routes the entry to
   `variable_values` (slot-resolved per variable name under the tier
   rule) instead of the `value_type` slot.
3. Adjust `extract_member_docblock`'s existing exhaustive `match` on
   `TagRole` for the three new variants.

- [ ] **Step 4: Run the extraction tests, then write the failing lowering tests**

Run: `cargo test -p celerrate_phpdoc_bridge tags` — expected: PASS.

Then, in `crates/celerrate_types/src/type_syntax.rs`, extend the
payload (this is the consuming side's contract — write its test in
the bridge's `lowering.rs` test module, which already constructs an
`AnnotationSite` against a test database; reuse that harness):

```rust
#[test]
fn ancestors_lower_qualified_with_their_argument_types() {
    // Harness: the module's existing site fixture, namespace `App`,
    // a `use Doctrine\Repo as Base;` import in scope.
    let parsed = parse_with_site_fixture(
        "/** @extends Base<User> */",
    );
    let ancestor = parsed.ancestors.first().unwrap();
    assert_eq!(ancestor.class_name, "doctrine\\repo", "qualified and folded");
    assert_eq!(ancestor.arguments.len(), 1);
}

#[test]
fn templates_lower_in_declaration_order_with_their_bounds() {
    let parsed = parse_with_site_fixture(
        "/**\n * @template TKey of int\n * @template TValue\n */",
    );
    let names: Vec<&str> = parsed
        .templates
        .iter()
        .map(|template| template.name.as_str())
        .collect();
    assert_eq!(names, ["TKey", "TValue"]);
}

#[test]
fn named_variables_lower_into_the_variables_field() {
    let parsed = parse_with_site_fixture("/** @var Collection<User> $items */");
    assert_eq!(parsed.variables.len(), 1);
    let (name, _) = parsed.variables.first().unwrap();
    assert_eq!(name, "items");
}
```

Run: `cargo test -p celerrate_phpdoc_bridge lowering` — expected:
FAIL to compile (`ancestors`, `templates`, `variables` missing on
`ParsedAnnotations`).

- [ ] **Step 5: Implement the payload and lowering**

1. `crates/celerrate_types/src/type_syntax.rs`:

```rust
/// One `@template` declaration of the parsed docblock, in declaration
/// order (the order `ancestor_arguments` zips against).
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct ParsedTemplate<'db> {
    pub name: String,
    pub bound: Option<TypeId<'db>>,
}

/// One inheritance-position declaration: the ancestor's fully
/// qualified, pre-folded class name and its fixed generic arguments.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct ParsedAncestor<'db> {
    pub class_name: String,
    pub arguments: Vec<TypeId<'db>>,
}
```

(The `salsa::Update` derives are load-bearing: task 3 embeds both
structs in a tracked query's result.)

   `ParsedAnnotations` gains `pub templates: Vec<ParsedTemplate<'db>>`,
   `pub ancestors: Vec<ParsedAncestor<'db>>`, and
   `pub variables: Vec<(String, TypeId<'db>)>` (all `Default`-empty;
   re-export the two new structs from `lib.rs` beside
   `ParsedAnnotations`).
2. `crates/celerrate_phpdoc_bridge/src/lowering.rs`: populate the
   three fields. Templates: the `LoweringScope`'s declarations, in
   order, each bound lowered (absent bound → `None`). Ancestors: for
   each `AncestorDeclaration`, lower the expression; read back the
   head and arguments through the lowered `TypeId`'s `class_name`/
   `class_arguments` accessors (the expression lowered to something
   that is not a class type — a malformed tag — drops, per-construct
   loss); `class_name` arrives pre-folded because `lower` already
   qualified it at the site. Variables: lower each
   `variable_values` entry's expression; entries whose expression
   fails to lower drop.
3. Class-site parsing needs the class's own templates in scope for
   `@extends Base<T>` (a template used as a fixed argument): when the
   docblock being parsed declares templates, they enter the
   `LoweringScope` before ancestors lower — mirror how member
   docblocks already bring `enclosing_class_docblock` templates into
   scope.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p celerrate_phpdoc_bridge && cargo test -p celerrate_types`
Expected: PASS. Then the full gate:
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types/src/type_syntax.rs crates/celerrate_types/src/lib.rs crates/celerrate_phpdoc_bridge/src
git commit -m "✨ feat(bridge): the inheritance-position tags and named inline @var"
```

---

### Task 3: The generic ancestry (`inheritance.rs`)

The threading the design reserved in linearization (the module doc of
`linearize.rs` says so verbatim): class-level annotations parsed at
the declaring site, then composed transitively along
`LinearizedClass.ancestry` into per-ancestor argument lists. All new
queries live in `celerrate_types` — `celerrate_semantics` stays below
the type layer and keeps carrying unresolved text only.

**Files:**
- Create: `crates/celerrate_types/src/inheritance.rs`
- Modify: `crates/celerrate_types/src/declared.rs` (make
  `declaring_site`, `owner_class_docblock`, and `with_declaring_site`
  `pub(crate)` so `inheritance.rs` reuses them)
- Modify: `crates/celerrate_types/src/lib.rs` (module declaration,
  re-export `ClassAnnotations`, `class_annotations`,
  `ancestor_arguments`)

**Interfaces:**
- Consumes: `ClassQuery` and `linearized_class`
  (`celerrate_semantics::linearize`), `AncestorEdge { relation,
  written, resolved, stub, owner }`, `ParsedAnnotations.templates` /
  `.ancestors` (task 2), `Substitution` and `substitute` (task 1),
  `type_syntax::annotations_for_docblock`, the `pub(crate)` declared
  helpers above.
- Produces (later tasks rely on these exact names):
  - `pub struct ClassAnnotations<'db> { pub templates:
    Vec<ParsedTemplate<'db>>, pub ancestors:
    Vec<ParsedAncestor<'db>> }` (derives `salsa::Update`; task 2
    already put the same derive on `ParsedTemplate` and
    `ParsedAncestor`).
  - `#[salsa::tracked(returns(ref))] pub fn class_annotations<'db>(db,
    files: AnalyzedFileSet, stubs: StubIndexInput, configuration:
    ProjectConfiguration, class: ClassQuery<'db>) ->
    ClassAnnotations<'db>`.
  - `#[salsa::tracked(returns(ref))] pub fn ancestor_arguments<'db>(db,
    files, stubs, configuration, class: ClassQuery<'db>) ->
    Vec<(String, Vec<TypeId<'db>>)>` — folded ancestor key to fixed
    arguments, linearization walk order.
  - `pub(crate) fn ancestor_substitution<'db>(db, files, stubs,
    configuration, class_key: &str, owner_key: &str) ->
    Option<Substitution<'db>>` — the ready-to-apply map for a member
    declared on `owner_key` consulted through `class_key`; `None`
    when the two keys are equal or no arguments are threaded.

- [ ] **Step 1: Write the failing tests**

At the bottom of the new `crates/celerrate_types/src/inheritance.rs`.
The fixture is `inference.rs`'s `fixture(sources)` shape; the fake
`TypeSyntax` registration mirrors `register_fake_assertions` in
`inference.rs`'s tests (same registry-builder idiom — copy it and
swap the implementation):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use std::sync::Arc;

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, FileId, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::ClassQuery;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{ancestor_arguments, ancestor_substitution, class_annotations};
    use crate::representation::TypeId;
    use crate::type_syntax::{
        AnnotationSite, ParsedAncestor, ParsedAnnotations, ParsedTemplate, TypeSyntax,
    };

    /// A deliberately tiny notation for these tests, one tag per
    /// docblock line: `@template NAME`, `@extends NAME<ARG, ...>`,
    /// `@implements NAME<ARG, ...>`, `@return NAME`. An `ARG` or a
    /// `@return` target that names a template declared in the same
    /// docblock lowers to that template; anything else lowers to a
    /// class qualified at the site and folded.
    struct FakeSyntax;

    impl FakeSyntax {
        fn lower_name<'db>(
            site: &AnnotationSite<'db, '_>,
            templates: &[String],
            written: &str,
        ) -> TypeId<'db> {
            let db = site.database();
            if templates.iter().any(|name| name == written) {
                return TypeId::template(
                    db,
                    site.declaring_scope(),
                    written,
                    TypeId::mixed(db),
                );
            }
            let qualified = site.qualify_class_name(written).to_lowercase();
            TypeId::class(db, &qualified, vec![])
        }
    }

    impl TypeSyntax for FakeSyntax {
        fn can_parse(&self, docblock: &str) -> bool {
            docblock.contains('@')
        }

        fn parse_docblock<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            docblock: &str,
        ) -> ParsedAnnotations<'db> {
            let db = site.database();
            let template_names: Vec<String> = docblock
                .lines()
                .filter_map(|line| line.trim().trim_start_matches("* ").strip_prefix("@template "))
                .map(|rest| rest.trim().to_owned())
                .collect();
            let mut parsed = ParsedAnnotations::default();
            for name in &template_names {
                parsed.templates.push(ParsedTemplate {
                    name: name.clone(),
                    bound: None,
                });
            }
            for line in docblock.lines() {
                let line = line.trim().trim_start_matches("* ");
                let tag_content = line
                    .strip_prefix("@extends ")
                    .or_else(|| line.strip_prefix("@implements "));
                if let Some(content) = tag_content
                    && let Some((head, rest)) = content.trim().split_once('<')
                    && let Some(arguments_text) = rest.strip_suffix('>')
                {
                    let arguments: Vec<TypeId<'db>> = arguments_text
                        .split(',')
                        .map(|argument| {
                            Self::lower_name(site, &template_names, argument.trim())
                        })
                        .collect();
                    let qualified = site.qualify_class_name(head.trim()).to_lowercase();
                    parsed.ancestors.push(ParsedAncestor {
                        class_name: qualified,
                        arguments,
                    });
                }
                if let Some(written) = line.strip_prefix("@return ") {
                    // Class templates come into scope through the
                    // enclosing class docblock, like the bridge does.
                    let enclosing: Vec<String> = site
                        .enclosing_class_docblock()
                        .map(|docblock| {
                            docblock
                                .lines()
                                .filter_map(|line| {
                                    line.trim()
                                        .trim_start_matches("* ")
                                        .strip_prefix("@template ")
                                })
                                .map(|rest| rest.trim().to_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    let scope = site.enclosing_class_scope().unwrap_or("");
                    let written = written.trim();
                    parsed.return_type = Some(if enclosing.iter().any(|name| name == written) {
                        TypeId::template(db, scope, written, TypeId::mixed(db))
                    } else {
                        Self::lower_name(site, &template_names, written)
                    });
                }
            }
            parsed
        }

        fn parse_type_expression<'db>(
            &self,
            _site: &AnnotationSite<'db, '_>,
            _expression: &str,
        ) -> Option<TypeId<'db>> {
            None
        }
    }

    struct Fixture {
        db: TestDatabase,
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
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        // Register the fake exactly the way inference.rs's
        // `register_fake_assertions` registers its fake: same
        // registration struct, same HIGH durability, implementation
        // `Arc::new(FakeSyntax)`.
        register_fake_syntax(&db);
        Fixture { db, files, stubs, configuration }
    }

    fn arguments_of<'db>(
        f: &'db Fixture,
        class_key: &str,
    ) -> &'db Vec<(String, Vec<TypeId<'db>>)> {
        let class = ClassQuery::new(&f.db, class_key.to_owned());
        ancestor_arguments(&f.db, f.files, f.stubs, f.configuration, class)
    }

    #[test]
    fn a_direct_extends_threads_its_fixed_arguments() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Base {}
/** @extends Base<User> */
class Child extends Base {}
class User {}
"#]);
        let user = TypeId::class(&f.db, "app\\user", vec![]);
        assert_eq!(
            arguments_of(&f, "app\\child"),
            &vec![("app\\base".to_owned(), vec![user])],
        );
    }

    #[test]
    fn arguments_compose_transitively_through_the_chain() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Grand {}
/**
 * @template U
 * @extends Grand<U>
 */
class Middle extends Grand {}
/** @extends Middle<User> */
class Leaf extends Middle {}
class User {}
"#]);
        let user = TypeId::class(&f.db, "app\\user", vec![]);
        assert_eq!(
            arguments_of(&f, "app\\leaf"),
            &vec![
                ("app\\middle".to_owned(), vec![user]),
                ("app\\grand".to_owned(), vec![user]),
            ],
        );
    }

    #[test]
    fn a_missing_argument_falls_to_the_bound_then_mixed() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Base {}
class Child extends Base {}
"#]);
        // No `@extends` tag at all: the template zips against nothing
        // and falls to its bound (`mixed` here — the fake declares
        // boundless templates).
        assert_eq!(
            arguments_of(&f, "app\\child"),
            &vec![("app\\base".to_owned(), vec![TypeId::mixed(&f.db)])],
        );
    }

    #[test]
    fn diamond_inheritance_takes_the_first_edge_in_walk_order() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
interface Shared {}
/** @implements Shared<User> */
class Left implements Shared {}
/** @implements Shared<Admin> */
interface Right extends Shared {}
/** @extends Left<User> */
class Diamond extends Left implements Right {}
class User {}
class Admin {}
"#]);
        let user = TypeId::class(&f.db, "app\\user", vec![]);
        let shared = arguments_of(&f, "app\\diamond")
            .iter()
            .find(|(key, _)| key == "app\\shared")
            .cloned();
        assert_eq!(
            shared,
            Some(("app\\shared".to_owned(), vec![user])),
            "the first edge in linearization walk order fixes the diamond",
        );
    }

    #[test]
    fn an_unresolved_ancestor_contributes_nothing() {
        let f = fixture(&[r#"<?php
namespace App;
/** @extends Vanished<User> */
class Child extends Vanished {}
class User {}
"#]);
        assert!(arguments_of(&f, "app\\child").is_empty());
    }

    #[test]
    fn ancestor_substitution_maps_the_owner_templates() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Base {}
/** @extends Base<User> */
class Child extends Base {}
class User {}
"#]);
        let map = ancestor_substitution(
            &f.db, f.files, f.stubs, f.configuration, "app\\child", "app\\base",
        )
        .unwrap();
        assert_eq!(
            map.binding("app\\base", "T"),
            Some(TypeId::class(&f.db, "app\\user", vec![])),
        );
        assert!(
            ancestor_substitution(
                &f.db, f.files, f.stubs, f.configuration, "app\\child", "app\\child",
            )
            .is_none(),
            "an own member never substitutes",
        );
    }
}
```

The `register_fake_syntax` helper is written in this step too —
verbatim from `inference.rs`'s `register_fake_assertions`, with the
implementation swapped for `Arc::new(FakeSyntax)` (keep the same
`PluginIdentity` shape and durability).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types inheritance`
Expected: FAIL to compile — the module and its queries do not exist.

- [ ] **Step 3: Implement the module**

`crates/celerrate_types/src/inheritance.rs`:

```rust
//! Generic-argument threading (design sections 2 and 6): class-level
//! annotations parsed at their declaring site, composed transitively
//! along linearization's ancestry into per-ancestor argument lists.
//! This is the delivery path of the Doctrine-on-Symfony repository
//! pattern: `@extends ServiceEntityRepository<User>` reaches
//! `$repository->find($id)` through these queries.

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{ClassQuery, linearized_class};
use celerrate_stubs::StubIndexInput;

use crate::representation::TypeId;
use crate::substitution::{Substitution, substitute};
use crate::type_syntax::{ParsedAncestor, ParsedTemplate};

/// One class-like's own docblock, parsed: its ordered `@template`
/// declarations and its inheritance-position fixed arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq, salsa::Update)]
pub struct ClassAnnotations<'db> {
    pub templates: Vec<ParsedTemplate<'db>>,
    pub ancestors: Vec<ParsedAncestor<'db>>,
}

/// Parses `class`'s own docblock at its declaring site. No docblock,
/// no registered syntax, or an unresolvable class all answer the
/// default (no templates, no ancestors) — never an error.
#[salsa::tracked(returns(ref))]
pub fn class_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> ClassAnnotations<'db> {
    let key = class.key(db);
    let Some(docblock) = crate::declared::owner_class_docblock(db, files, key) else {
        return ClassAnnotations::default();
    };
    crate::declared::with_declaring_site(db, files, key, |site| {
        let context = crate::type_syntax::AnnotationContext {
            declaring_scope: key,
            enclosing_class_scope: None,
            enclosing_class_docblock: None,
        };
        let parsed =
            crate::type_syntax::annotations_for_docblock(db, site, &context, &docblock);
        ClassAnnotations {
            templates: parsed.templates,
            ancestors: parsed.ancestors,
        }
    })
}

/// The fixed generic arguments of every ancestor of `class`, composed
/// transitively along linearization's ancestry, in walk order.
/// Diamond inheritance resolves first-edge-wins; a stub or otherwise
/// unresolved ancestor contributes nothing; a missing argument falls
/// to the template's bound, then `mixed` (decision 7).
#[salsa::tracked(returns(ref))]
pub fn ancestor_arguments<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> Vec<(String, Vec<TypeId<'db>>)> {
    let Some(linearized) = linearized_class(db, files, stubs, configuration, class) else {
        return Vec::new();
    };
    let class_key = class.key(db).clone();
    let mut substitutions: BTreeMap<String, Substitution<'db>> = BTreeMap::new();
    substitutions.insert(class_key, Substitution::default());
    let mut answers = Vec::new();
    for edge in &linearized.ancestry {
        let Some(target) = edge.resolved.clone() else {
            continue;
        };
        if substitutions.contains_key(&target) {
            continue;
        }
        let Some(owner_substitution) = substitutions.get(&edge.owner).cloned() else {
            continue;
        };
        let owner_query = ClassQuery::new(db, edge.owner.clone());
        let written_arguments: Vec<TypeId<'db>> =
            class_annotations(db, files, stubs, configuration, owner_query)
                .ancestors
                .iter()
                .find(|ancestor| ancestor.class_name == target)
                .map(|ancestor| ancestor.arguments.clone())
                .unwrap_or_default();
        let substituted: Vec<TypeId<'db>> = written_arguments
            .iter()
            .map(|argument| {
                substitute(
                    db, files, stubs, configuration, *argument, &owner_substitution, None,
                )
            })
            .collect();
        let target_query = ClassQuery::new(db, target.clone());
        let templates =
            &class_annotations(db, files, stubs, configuration, target_query).templates;
        let mut composed = Substitution::default();
        let mut fixed = Vec::new();
        for (position, template) in templates.iter().enumerate() {
            let argument = substituted.get(position).copied().unwrap_or_else(|| {
                template.bound.unwrap_or_else(|| TypeId::mixed(db))
            });
            composed.bind(&target, &template.name, argument);
            fixed.push(argument);
        }
        substitutions.insert(target.clone(), composed);
        if !fixed.is_empty() {
            answers.push((target, fixed));
        }
    }
    answers
}

/// The ready-to-apply substitution for a member declared on
/// `owner_key`, consulted through `class_key`. `None` when the member
/// is the class's own or nothing is threaded — callers skip the walk.
pub(crate) fn ancestor_substitution<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class_key: &str,
    owner_key: &str,
) -> Option<Substitution<'db>> {
    if class_key == owner_key {
        return None;
    }
    let class = ClassQuery::new(db, class_key.to_owned());
    let fixed = ancestor_arguments(db, files, stubs, configuration, class)
        .iter()
        .find(|(ancestor, _)| ancestor == owner_key)
        .map(|(_, arguments)| arguments.clone())?;
    let owner = ClassQuery::new(db, owner_key.to_owned());
    let templates = &class_annotations(db, files, stubs, configuration, owner).templates;
    let mut map = Substitution::default();
    for (position, template) in templates.iter().enumerate() {
        let argument = fixed
            .get(position)
            .copied()
            .unwrap_or_else(|| template.bound.unwrap_or_else(|| TypeId::mixed(db)));
        map.bind(owner_key, &template.name, argument);
    }
    if map.is_empty() { None } else { Some(map) }
}
```

Supporting changes: mark `declaring_site`, `owner_class_docblock`,
and `with_declaring_site` in `declared.rs` as `pub(crate)`; confirm
`ParsedTemplate` and `ParsedAncestor` carry the `salsa::Update`
derive task 2 gave them; declare `mod inheritance;` and re-export
`ClassAnnotations`, `class_annotations`, `ancestor_arguments` from
`lib.rs`. Note the deliberate asymmetry: `class_annotations` runs even
for classes with only a `@template` docblock and no ancestors — the
solver (task 8) and the receiver substitution (task 5) read its
template list.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types inheritance`
Expected: PASS (6 tests). Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src
git commit -m "✨ feat(types): generic-argument threading along the linearized ancestry"
```

---

### Task 4: Inherited declared signatures substitute

`declared_member_signature` consulted through a subclass now answers
the ancestor's annotation with the threaded arguments applied — the
design's "declared types inherit" (section 3) meets the threading of
task 3. This is where `@extends ServiceEntityRepository<User>` turns
`find()` into a `User`-typed member on the concrete repository.

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`

**Interfaces:**
- Consumes: `ancestor_substitution` (task 3), `substitute` (task 1),
  the existing `declared_member_signature` resolution path and its
  `MemberResolution::Source { member, owner, origin }` handling.
- Produces: no new names — `declared_member_signature` keeps its
  exact signature; its answers change for inherited members of
  generic ancestors. Every consumer (flow's `method_signatures`,
  `member_value_type`, the checks of plan 8) inherits the precision
  without code changes.

- [ ] **Step 1: Write the failing tests**

In `declared.rs`'s existing `#[cfg(test)]` module (reuse its fixture;
register the task-3 fake syntax — lift `FakeSyntax` and
`register_fake_syntax` into a `#[cfg(test)] pub(crate) mod
test_support;` module in `inheritance.rs` if sharing beats
duplication; the crate has no shared test-support module yet, and the
semantic-core crates record the same debt — either way, no test code
in production paths):

```rust
#[test]
fn an_inherited_signature_substitutes_the_threaded_arguments() {
    let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Repository {
    /** @return T */
    public function find(int $identifier) {}
}
/** @extends Repository<User> */
class UserRepository extends Repository {}
class User {}
"#]);
    let query = MemberQuery::new(
        &f.db,
        "app\\userrepository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let signature =
        declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query)
            .unwrap();
    assert_eq!(
        signature.value_type,
        TypeId::class(&f.db, "app\\user", vec![]),
        "the ancestor's template return arrives fixed to User",
    );
}

#[test]
fn an_inherited_parameter_substitutes_too() {
    let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Collection {
    /** @param T $item */
    public function add($item) {}
}
/** @extends Collection<User> */
class UserCollection extends Collection {}
class User {}
"#]);
    let query = MemberQuery::new(
        &f.db,
        "app\\usercollection".to_owned(),
        MemberKind::Method,
        "add".to_owned(),
    );
    let signature =
        declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query)
            .unwrap();
    let parameter = signature
        .parameters
        .iter()
        .find(|parameter| parameter.name == "item")
        .unwrap();
    assert_eq!(
        parameter.parameter_type,
        Some(TypeId::class(&f.db, "app\\user", vec![])),
    );
}

#[test]
fn the_owner_consulted_directly_keeps_its_template() {
    let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Repository {
    /** @return T */
    public function find(int $identifier) {}
}
"#]);
    let query = MemberQuery::new(
        &f.db,
        "app\\repository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let signature =
        declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query)
            .unwrap();
    assert_eq!(
        signature.value_type,
        TypeId::template(&f.db, "app\\repository", "T", TypeId::mixed(&f.db)),
        "no threading applies at the declaring class itself",
    );
}
```

The `@param T $item` form requires the task-3 fake to also honor
`@param NAME $name` (template-aware like its `@return` arm) — extend
the fake's line loop with a `@param` arm that pushes into
`parsed.parameters`; ten lines, same pattern.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types declared::tests::an_inherited`
Expected: FAIL — the signature still answers the raw template.

- [ ] **Step 3: Implement the substitution hook**

In `declared_member_signature`, at the single point where the
resolved `Source { owner, .. }` signature is fully assembled (after
the trust rule and docblock inheritance have produced the final
`DeclaredSignature`), apply:

```rust
// Design section 3, "declared types inherit", completed by the
// threading of task 3: a member declared on a generic ancestor is
// answered with the receiver class's fixed arguments applied.
if let Some(map) = crate::inheritance::ancestor_substitution(
    db,
    files,
    stubs,
    configuration,
    query.class_key(db),
    &owner,
) {
    signature.value_type = crate::substitution::substitute(
        db, files, stubs, configuration, signature.value_type, &map, None,
    );
    for parameter in &mut signature.parameters {
        if let Some(parameter_type) = parameter.parameter_type {
            parameter.parameter_type = Some(crate::substitution::substitute(
                db, files, stubs, configuration, parameter_type, &map, None,
            ));
        }
    }
}
```

Mind the docblock-inheritance interaction: when the annotation was
inherited from an ancestor further up (the nearest-ancestor walk of
plan 3), the annotation's templates are scoped to the class that
**declared** it — the walk already records that declaring owner; use
that owner for `ancestor_substitution`, not the member's structural
owner, so the map's scope matches the template's scope. Placeholders
stay untouched here (`placeholders: None`): late-static-binding
resolution is the call site's job (task 5), not the signature's.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types declared`
Expected: PASS, including every pre-existing declared-layer test
(threading must change nothing when no arguments are threaded). Then
the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src
git commit -m "✨ feat(types): inherited signatures answer with threaded generic arguments"
```

---

### Task 5: Late-static-binding placeholders, carried and substituted

The receiver model (decisions 1 and 2) replaces plan 5's decision-6
scaffolding — the three seams marked "Plan 6 replaces this" in
`flow.rs` (`this_type` at `:300`, `substitute_receiver` at `:409`,
`scope_keyword_class` at `:449`). Inference results now carry symbolic
placeholders; every member boundary in the walker resolves them
against the declaring owner and the receiver through the task-1
primitive, and binds the receiver's class arguments while it is there.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests)

**Interfaces:**
- Consumes: `Substitution`, `PlaceholderResolution`, `substitute`
  (task 1), `class_annotations(...).templates` (task 3),
  `lookup_member`/`MemberResolution` (for the declaring owner),
  the existing `parent_class_key` ancestry walk.
- Produces (later tasks rely on these exact names, all on `Walker`):
  - `fn member_boundary_type(&mut self, of: TypeId<'db>, owner:
    Option<&str>, receiver: TypeId<'db>) -> TypeId<'db>` — the one
    funnel every member read, method call, callable projection, and
    `new` result goes through: builds the receiver-argument
    `Substitution` (zip the receiver's `class_arguments` against its
    class's `class_annotations(...).templates`), the
    `PlaceholderResolution { owner, parent: parent of owner,
    receiver }`, and calls `substitute`.
  - `fn member_owner(&self, keys: &[String], kind: MemberKind, name:
    &str) -> Option<String>` — the declaring owner through
    `lookup_member`, for `self`/`parent` resolution.
  - `substitute_receiver` is deleted; `this_type` answers
    `TypeId::static_placeholder(db)` in a non-static method (still
    `mixed` in functions and static contexts without a class);
    `scope_keyword_class("static")` answers the placeholder;
    `receiver_parts` gains a `ParentPlaceholder` arm resolving
    through `parent_class_key` so member lookups on `parent::` keep
    working.

- [ ] **Step 1: Write the failing tests**

In `crates/celerrate_types/src/inference.rs`'s test module (the
`fixture`/`return_of`-style helpers already there):

```rust
#[test]
fn a_native_static_return_substitutes_to_the_receiver() {
    let f = fixture(&[r#"<?php
namespace App;
class Base {
    public static function create(): static { return new static(); }
}
class Child extends Base {}
function caller(Child $c) { return $c::create(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\child");
}

#[test]
fn a_native_self_return_stays_the_declaring_class() {
    let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function make(): self { return $this; }
}
class Child extends Base {}
function caller(Child $c) { return $c->make(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\base");
}

#[test]
fn static_forwards_through_self_calls_and_rebinds_on_a_named_class() {
    let f = fixture(&[r#"<?php
namespace App;
class Base {
    public static function create(): static { return new static(); }
    public static function viaSelf(): static { return self::create(); }
}
class Child extends Base {}
function forwarded(Child $c) { return $c::viaSelf(); }
function rebound() { return Base::create(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\forwarded"), "app\\child");
    assert_eq!(caller_return_display(&f, "app\\rebound"), "app\\base");
}

#[test]
fn this_types_as_the_static_placeholder_inside_the_body() {
    let f = fixture(&[r#"<?php
namespace App;
class Chainable {
    public function itself(): static { return $this; }
}
"#]);
    // The method body's return carries the placeholder symbolically:
    // substitution is the call site's job, not the body's.
    assert_eq!(method_return_display(&f, "app\\chainable", "itself"), "static");
}

#[test]
fn parent_calls_resolve_members_and_keep_forwarding() {
    let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function name(): string { return 'base'; }
}
class Child extends Base {
    public function viaParent() { return parent::name(); }
}
"#]);
    assert_eq!(method_return_display(&f, "app\\child", "viaParent"), "string");
}
```

Helper shapes for this step (define beside `return_of` if absent):
`caller_return_display` runs `inferred_function_return` for the given
function key and answers `.display(&f.db)`;
`method_return_display` finds the method's `AstId` through
`member_tree`, builds its `BodyQuery`, runs `inferred_body_types`
with the default context, and displays the return. If a display
string in an assertion disagrees with `display.rs`'s actual
rendering, fix the expectation, never the code (plan-5 decision 16,
reconducted).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types inference`
Expected: the new tests FAIL — `static` currently substitutes to the
defining class (decision-6 scaffolding), `$this` types concrete, and
`parent::` member results never substitute.

- [ ] **Step 3: Implement the receiver model**

In `crates/celerrate_types/src/flow.rs`:

1. `this_type`: non-static method with a known owner class →
   `TypeId::static_placeholder(db)`; everything else stays `mixed`.
2. `scope_keyword_class`: `self` → `TypeId::self_placeholder(db)`,
   `static` → `TypeId::static_placeholder(db)`, `parent` →
   `TypeId::parent_placeholder(db)` (each only when an owner class
   exists, else `mixed` as today).
3. `receiver_parts`: add the `ParentPlaceholder` arm answering the
   owner's `parent_class_key` (the existing walk); keep the
   `SelfPlaceholder`/`StaticPlaceholder` arms answering the owner.
4. Generalize `parent_class_key` into
   `fn parent_class_key_of(&self, class_key: &str) -> Option<String>`
   (the existing body, parameterized); keep the old name delegating
   to it for the owner.
5. Add the funnel and the owner lookup:

```rust
/// Every member boundary funnels through here (decision 1): the
/// declared or inferred member type is resolved against the
/// declaring `owner` and the `receiver` — late-static-binding
/// placeholders substitute (`self` → owner, `parent` → the owner's
/// first `Extends` ancestor, `static` → the receiver, which may
/// itself be a placeholder and forward, decision 2) — and the
/// receiver's class arguments bind its class-level templates.
fn member_boundary_type(
    &self,
    of: TypeId<'db>,
    owner: Option<&str>,
    receiver: TypeId<'db>,
) -> TypeId<'db> {
    let db = self.db;
    let mut map = crate::substitution::Substitution::default();
    if let (Some(name), Some(arguments)) =
        (receiver.class_name(db), receiver.class_arguments(db))
        && !arguments.is_empty()
    {
        let class = celerrate_semantics::ClassQuery::new(db, name.clone());
        let templates = &crate::inheritance::class_annotations(
            db, self.files, self.stubs, self.configuration, class,
        )
        .templates;
        for (position, template) in templates.iter().enumerate() {
            if let Some(argument) = arguments.get(position) {
                map.bind(&name, &template.name, *argument);
            }
        }
    }
    let resolution = crate::substitution::PlaceholderResolution {
        owner: owner.map(str::to_owned),
        parent: owner.and_then(|key| self.parent_class_key_of(key)),
        receiver: Some(receiver),
    };
    crate::substitution::substitute(
        db, self.files, self.stubs, self.configuration, of, &map, Some(&resolution),
    )
}

/// The declaring owner of the first key that resolves the member —
/// `self` and `parent` placeholders substitute against it.
fn member_owner(&self, keys: &[String], kind: MemberKind, name: &str) -> Option<String> {
    let db = self.db;
    keys.iter().find_map(|key| {
        let query = MemberQuery::new(
            db,
            key.clone(),
            kind,
            celerrate_semantics::folded_member_key(kind, name),
        );
        match lookup_member(db, self.files, self.stubs, self.configuration, query) {
            Some(
                MemberResolution::Source { owner, .. }
                | MemberResolution::Stub { owner, .. }
                | MemberResolution::Virtual { owner, .. },
            ) => Some(owner),
            None => None,
        }
    })
}
```

6. Replace every `substitute_receiver(of, receiver)` call with
   `member_boundary_type(of, member_owner(&keys, kind, name).as_deref(), receiver)`
   — the sites are `method_call_result_for_keys`,
   `member_value_type`'s consumers (property and constant reads,
   `ScopedAccess`), the callable projections
   (`projected_callable_of_method`/`_of_keys`), and the `New` arm
   (`new static` answers the placeholder, `new self` the owner).
   Delete `substitute_receiver`.
7. The **receiver for substitution** at each call form (decision 2):
   `$obj->m()` → `$obj`'s type; `Foo::m()` → `TypeId::class(db,
   resolved_key, vec![])`; `self::m()`, `static::m()`, `parent::m()`
   → the current `static` type, `TypeId::static_placeholder(db)`
   (forwarding). `scoped_subject` already distinguishes these
   subjects; thread its answer through.

Plan-5 tests that pinned the decision-6 behavior (by name:
`parent_and_self_resolve_against_the_defining_class` and any sibling
asserting concrete `self`/`static` substitution) get their
expectations updated to the placeholder model in this step — the
design supersedes them; update expectations, keep their scenarios.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS — new tests green, updated plan-5 tests green,
everything else untouched. Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src
git commit -m "✨ feat(types): late-static-binding placeholders substitute at member boundaries"
```

---

### Task 6: Method-inferred returns under the fixpoint

The fourth call tier (decision 3) and the second cycle-recovered
query in the workspace: `inferred_method_return`, keyed per defining
class (decision 4), sharing `ascend` and the budget with the plan-5
free-function fixpoint. `inferred_body_types` gains the
`InferenceContext` parameter (decision 5) — threaded mechanically
here, exercised by traits in task 7.

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs`
- Modify: `crates/celerrate_types/src/flow.rs` (the method call tier)
- Modify: `crates/celerrate_types/src/lib.rs` (re-export
  `MethodQuery`, `InferenceContext`, `inferred_method_return`)

**Interfaces:**
- Consumes: `inferred_body_types`, `ascend`,
  `FIXPOINT_ITERATION_BUDGET`, `lookup_member`/`MemberResolution`/
  `MemberOrigin`, `analyzed_file_index`, `BodyQuery`,
  `member_boundary_type` (task 5).
- Produces (later tasks rely on these exact names):
  - `#[salsa::interned(debug)] pub struct InferenceContext<'db> {
    #[returns(ref)] pub using_class_key: Option<String> }`.
  - `inferred_body_types(db, files, stubs, configuration, file, body,
    context: InferenceContext<'db>)` — the new final parameter; every
    existing caller passes `InferenceContext::new(db, None)`.
  - `#[salsa::interned(debug)] pub struct MethodQuery<'db> {
    #[returns(ref)] pub class_key: String, #[returns(ref)] pub
    member_key: String }` (both pre-folded).
  - `#[salsa::tracked(cycle_fn = method_return_cycle_recover,
    cycle_initial = method_return_cycle_initial)] pub fn
    inferred_method_return<'db>(db, files: AnalyzedFileSet, stubs:
    StubIndexInput, configuration: ProjectConfiguration, query:
    MethodQuery<'db>) -> TypeId<'db>`.

- [ ] **Step 1: Write the failing tests**

In `inference.rs`'s test module:

```rust
#[test]
fn a_method_call_takes_the_inferred_return_when_no_declaration_exists() {
    let f = fixture(&[r#"<?php
namespace App;
class Greeter {
    public function greeting() { return 'hello'; }
}
function caller(Greeter $g) { return $g->greeting(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "'hello'");
}

#[test]
fn a_declared_return_still_wins_over_the_body() {
    let f = fixture(&[r#"<?php
namespace App;
class Greeter {
    public function greeting(): string { return 'hello'; }
}
function caller(Greeter $g) { return $g->greeting(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "string");
}

#[test]
fn mutual_method_recursion_converges_to_the_joined_union() {
    let f = fixture(&[r#"<?php
namespace App;
class Pair {
    public function left(bool $flip) {
        if ($flip) { return 1; }
        return $this->right($flip);
    }
    public function right(bool $flip) {
        if ($flip) { return 'one'; }
        return $this->left($flip);
    }
}
function caller(Pair $p) { return $p->left(true); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "1|'one'");
}

#[test]
fn an_inherited_method_infers_once_per_defining_class() {
    let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function answer() { return 42; }
}
class LeftChild extends Base {}
class RightChild extends Base {}
"#]);
    let left = MethodQuery::new(&f.db, "app\\leftchild".to_owned(), "answer".to_owned());
    let right = MethodQuery::new(&f.db, "app\\rightchild".to_owned(), "answer".to_owned());
    f.db.take_executed();
    let left_return =
        inferred_method_return(&f.db, f.files, f.stubs, f.configuration, left);
    let right_return =
        inferred_method_return(&f.db, f.files, f.stubs, f.configuration, right);
    assert_eq!(left_return, right_return);
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        1,
        "both subclasses share the defining class's memo: {log:?}",
    );
}

#[test]
fn a_growing_method_recursion_bails_to_mixed_within_the_budget() {
    let f = fixture(&[r#"<?php
namespace App;
class Nest {
    public function deeper() { return [$this->deeper()]; }
}
function caller(Nest $n) { return $n->deeper(); }
"#]);
    // The array constructor grows the type every iterate; the budget
    // widens deterministically to mixed — never salsa's panic.
    assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
}

#[test]
fn an_inferred_this_return_substitutes_to_the_receiver() {
    let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function itself() { return $this; }
}
class Child extends Base {}
function caller(Child $c) { return $c->itself(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\child");
}

#[test]
fn a_stub_or_unknown_receiver_method_stays_mixed() {
    let f = fixture(&[r#"<?php
namespace App;
function caller($anything) { return $anything->whatever(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types inference`
Expected: FAIL — `MethodQuery` does not exist; method calls without a
declaration answer `mixed` (the plan-5 tier comment at `flow.rs:852`
says so verbatim).

- [ ] **Step 3: Implement the context, the query, and the tier**

1. `InferenceContext` in `inference.rs` as specified in Interfaces;
   thread it through `inferred_body_types` as the final parameter.
   The walker's owner class key becomes
   `context.using_class_key(db).clone().or(owner_from_body_owner)`
   (one line in the unpacking that builds `FlowContext`). Update
   `inferred_function_return` and every test caller to pass
   `InferenceContext::new(db, None)`.
2. The method fixpoint, mirroring the free-function one:

```rust
#[salsa::interned(debug)]
pub struct MethodQuery<'db> {
    /// Pre-folded ClassLike key: the receiver-resolution class.
    #[returns(ref)]
    pub class_key: String,
    /// Pre-folded method key (`folded_member_key(Method, name)`).
    #[returns(ref)]
    pub member_key: String,
}

fn method_return_cycle_initial<'db>(
    db: &'db dyn salsa::Database,
    _id: salsa::Id,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: MethodQuery<'db>,
) -> TypeId<'db> {
    TypeId::never(db)
}

fn method_return_cycle_recover<'db>(
    db: &'db dyn salsa::Database,
    cycle: &salsa::Cycle,
    last_provisional: &TypeId<'db>,
    computed: TypeId<'db>,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: MethodQuery<'db>,
) -> TypeId<'db> {
    ascend(db, cycle.iteration(), *last_provisional, computed)
}

/// The inferred return of one method, keyed per defining class
/// (decision 4): an inherited member re-keys to its owner so every
/// subclass shares one memo; a trait member analyzes per using class
/// (the query's `class_key`); stub and virtual members answer
/// `mixed` — their types are declared, consulted at the earlier
/// tier. The second cycle-recovered query in the workspace; the
/// discipline (join ascent, shared budget, deterministic bailout) is
/// plan 5's, unchanged.
#[salsa::tracked(cycle_fn = method_return_cycle_recover, cycle_initial = method_return_cycle_initial)]
pub fn inferred_method_return<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MethodQuery<'db>,
) -> TypeId<'db> {
    let member_query = MemberQuery::new(
        db,
        query.class_key(db).clone(),
        MemberKind::Method,
        query.member_key(db).clone(),
    );
    let Some(MemberResolution::Source { member, owner, origin }) =
        lookup_member(db, files, stubs, configuration, member_query)
    else {
        return TypeId::mixed(db);
    };
    let context = match origin {
        MemberOrigin::Inherited => {
            let owner_query = MethodQuery::new(db, owner, query.member_key(db).clone());
            return inferred_method_return(db, files, stubs, configuration, owner_query);
        }
        MemberOrigin::Own => InferenceContext::new(db, None),
        MemberOrigin::Trait => {
            InferenceContext::new(db, Some(query.class_key(db).clone()))
        }
    };
    let index = analyzed_file_index(db, files);
    let Ok(position) = index.binary_search_by_key(&member.ast_id.file, |(id, _)| *id)
    else {
        return TypeId::mixed(db);
    };
    let Some(&(_, file)) = index.get(position) else {
        return TypeId::mixed(db);
    };
    let body = BodyQuery::new(db, member.ast_id);
    match inferred_body_types(db, files, stubs, configuration, file, body, context) {
        Some(inferred) => inferred.return_type,
        None => TypeId::mixed(db),
    }
}
```

3. The tier in `flow.rs`: in `method_call_result_for_keys`, where a
   key's declared signature is absent (`!declared_present`), replace
   the `mixed` answer with

```rust
self.edge_counts.inferred_return_edges += 1;
let method = MethodQuery::new(
    db,
    key.clone(),
    celerrate_semantics::folded_member_key(MemberKind::Method, name),
);
let inferred =
    inferred_method_return(db, self.files, self.stubs, self.configuration, method);
self.member_boundary_type(inferred, self.member_owner(keys, MemberKind::Method, name).as_deref(), receiver)
```

   (follow the function's existing counter idiom — the counters live
   on the walker exactly where `function_call_result` increments
   them). The per-key results keep reducing with `TypeId::union` as
   today.
4. Re-export `MethodQuery`, `InferenceContext`,
   `inferred_method_return` from `lib.rs`.
5. `crates/celerrate_types/tests/invalidation_scope.rs:1194` — the
   test whose comment reads "returns are plan 6, so both bodies'
   inference is demanded": update its expectation, since a caller now
   demands the callee's `inferred_method_return` instead of walking
   both bodies itself; keep the scenario.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS (7 new tests; the invalidation-scope expectation
updated). Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): method-inferred returns under the shared fixpoint discipline"
```

---

### Task 7: Trait bodies analyze per using class

PHPStan's model, fixed by the design (section 6): a trait method's
body is re-analyzed for each class that uses it, because `$this`,
`self::`, and property lookups resolve against the **user**, not the
trait. The machinery landed in task 6 (`MemberOrigin::Trait` →
`InferenceContext` with the using class); this task pins its
semantics — aliasing, `insteadof`, per-user divergence, and the memo
shape — and fixes whatever the pins flush out.

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs` (tests; fixes if
  the pins flush any out)
- Modify: `crates/celerrate_types/src/flow.rs` (only if fixes demand)

**Interfaces:**
- Consumes: `MethodQuery`, `inferred_method_return`,
  `InferenceContext` (task 6), the linearization's trait adaptation
  handling (aliases keep the trait method's `AstId`; `insteadof`
  excludes).
- Produces: no new names — behavioral pins only.

- [ ] **Step 1: Write the failing (or pinning) tests**

In `inference.rs`'s test module:

```rust
#[test]
fn a_trait_body_types_against_each_using_class() {
    let f = fixture(&[r#"<?php
namespace App;
trait Reader {
    public function read() { return $this->value; }
}
class IntBox {
    use Reader;
    public int $value = 0;
}
class StringBox {
    use Reader;
    public string $value = '';
}
function ints(IntBox $box) { return $box->read(); }
function strings(StringBox $box) { return $box->read(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\ints"), "int");
    assert_eq!(caller_return_display(&f, "app\\strings"), "string");
}

#[test]
fn two_using_classes_mean_two_memos_for_one_trait_body() {
    let f = fixture(&[r#"<?php
namespace App;
trait Reader {
    public function read() { return $this->value; }
}
class IntBox { use Reader; public int $value = 0; }
class StringBox { use Reader; public string $value = ''; }
"#]);
    f.db.take_executed();
    let _ = inferred_method_return(
        &f.db, f.files, f.stubs, f.configuration,
        MethodQuery::new(&f.db, "app\\intbox".to_owned(), "read".to_owned()),
    );
    let _ = inferred_method_return(
        &f.db, f.files, f.stubs, f.configuration,
        MethodQuery::new(&f.db, "app\\stringbox".to_owned(), "read".to_owned()),
    );
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        2,
        "the per-receiver key exists exactly where substitution is impossible: {log:?}",
    );
}

#[test]
fn an_aliased_trait_method_still_finds_its_body() {
    let f = fixture(&[r#"<?php
namespace App;
trait Maker {
    public function make() { return 42; }
}
class Factory {
    use Maker { make as build; }
}
function caller(Factory $factory) { return $factory->build(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "42");
}

#[test]
fn insteadof_routes_to_the_chosen_trait_body() {
    let f = fixture(&[r#"<?php
namespace App;
trait Ints {
    public function pick() { return 1; }
}
trait Strings {
    public function pick() { return 'one'; }
}
class Chooser {
    use Ints, Strings {
        Ints::pick insteadof Strings;
    }
}
function caller(Chooser $chooser) { return $chooser->pick(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "1");
}

#[test]
fn a_trait_body_calling_the_users_helper_resolves_against_the_user() {
    let f = fixture(&[r#"<?php
namespace App;
trait Delegating {
    public function invoke() { return $this->helper(); }
}
class WithHelper {
    use Delegating;
    public function helper() { return 'helped'; }
}
function caller(WithHelper $subject) { return $subject->invoke(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "'helped'");
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p celerrate_types inference`
Expected: task 6 built the machinery, so some of these may already
pass — that is fine; they are pins. Any failure is a real gap in the
trait path (the likely ones: the body-file lookup for a member whose
`AstId` lives in the trait's file, not the using class's; the
using-class context not reaching `receiver_parts` for `$this`
property lookups). Fix forward in step 3.

- [ ] **Step 3: Fix what the pins flushed out**

Apply the minimal fixes in `inference.rs`/`flow.rs`. Two known
sharp edges to check while here, both from the task-6 code: (a) the
`analyzed_file_index` lookup in `inferred_method_return` uses
`member.ast_id.file` — the trait's file — which is correct by
construction; leave a one-line comment saying so; (b) the walker's
`method_is_static`/parameter seeding comes from `body_owner` (the
trait's syntactic member), while the owner class key comes from the
context — that split is the design's intent (the signature is the
trait's, the receiver is the user's); comment it where the
`FlowContext` is built.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS (5 pins). Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src
git commit -m "✅ test(types): trait bodies analyze per using class, pinned"
```

(Use `✨ feat(types): ...` instead if step 3 changed production code.)

---

### Task 8: The call-site template solver

Generics resolve at the call site from argument types (design
section 6) so that `$collection->first()` answers `User|null` instead
of `mixed` — and **no generic mismatch diagnostic is emitted**,
neither here nor in plan 8. Solver failure semantics are the
design's, verbatim: multiple constraints take the least upper bound;
conflicting or failed constraints fall back to the template bound,
then `mixed` — never the first-seen constituent.

**Files:**
- Create: `crates/celerrate_types/src/solver.rs`
- Modify: `crates/celerrate_types/src/flow.rs` (pair alignment, the
  `Foo::class` type, applying the solver at call results)
- Modify: `crates/celerrate_types/src/lib.rs` (`mod solver;`)

**Interfaces:**
- Consumes: `Substitution`/`substitute`/`contains_symbolic` (task 1),
  `ancestor_arguments` (task 3), `DeclaredSignature`/
  `DeclaredParameter`, `TypeId::key_of`/`value_of`, `widening` caps.
- Produces (flow and task 9 rely on these exact names):
  - `pub(crate) fn solve<'db>(db, files, stubs, configuration, pairs:
    &[(TypeId<'db>, TypeId<'db>)]) -> Substitution<'db>` — each pair
    is (declared parameter type, argument type).
  - `pub(crate) fn finalize_return<'db>(db, files, stubs,
    configuration, of: TypeId<'db>) -> TypeId<'db>` — replaces every
    template still left in a call result by its bound, then `mixed`
    (call results are concrete; placeholders are untouched — they
    belong to `member_boundary_type`).
  - On `Walker`: `fn solver_pairs(&self, parameters:
    &[DeclaredParameter<'db>], arguments: &[CallArgument], types:
    ...) -> Vec<(TypeId<'db>, TypeId<'db>)>` — positional alignment,
    labels matched by parameter name, surplus arguments paired with
    the variadic parameter's type, a spread argument ends alignment.

- [ ] **Step 1: Write the failing tests**

In `inference.rs`'s test module. These lean on the shared fake
syntax (task 3's `test_support`), which this step extends with three
grammar arms — `@template NAME of NAME` (bounds), `@param
class-string<NAME> $name`, and the conditional return form
`@return (NAME is NAME ? NAME : NAME)` — each following the fake's
existing line-parsing pattern:

```rust
#[test]
fn a_template_parameter_solves_from_its_argument() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
/**
 * @template T
 * @param T $value
 * @return T
 */
function identity($value) { return $value; }
function caller() { return identity(new User()); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
}

#[test]
fn multiple_constraints_take_the_least_upper_bound() {
    let f = fixture(&[r#"<?php
namespace App;
/**
 * @template T
 * @param T $left
 * @param T $right
 * @return T
 */
function pick($left, $right) { return $left; }
function caller() { return pick(1, 'one'); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "1|'one'");
}

#[test]
fn class_string_binds_the_template_through_class_constants() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
/**
 * @template T
 * @param class-string<T> $name
 * @return T
 */
function make(string $name) {}
function caller() { return make(User::class); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
}

#[test]
fn a_class_constant_types_as_a_class_string_of_its_class() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
function caller() { return User::class; }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "class-string<app\\user>");
}

#[test]
fn an_unconstrained_template_falls_to_its_bound_then_mixed() {
    let f = fixture(&[r#"<?php
namespace App;
class Fallback {}
/**
 * @template T of Fallback
 * @return T
 */
function bounded() {}
/**
 * @template U
 * @return U
 */
function boundless() {}
function bound_caller() { return bounded(); }
function mixed_caller() { return boundless(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\bound_caller"), "app\\fallback");
    assert_eq!(caller_return_display(&f, "app\\mixed_caller"), "mixed");
}

#[test]
fn a_generic_class_parameter_recurses_through_the_ancestry() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
/** @template T */
class Collection {}
/** @extends Collection<User> */
class UserCollection extends Collection {}
/**
 * @template T
 * @param Collection<T> $collection
 * @return T
 */
function first($collection) {}
function caller(UserCollection $users) { return first($users); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
}

#[test]
fn a_conditional_return_evaluates_at_the_call_site() {
    let f = fixture(&[r#"<?php
namespace App;
/**
 * @template T
 * @param T $value
 * @return (T is int ? string : bool)
 */
function flip($value) {}
function on_int() { return flip(1); }
function on_string() { return flip('text'); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\on_int"), "string");
    assert_eq!(caller_return_display(&f, "app\\on_string"), "bool");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types inference`
Expected: FAIL — call results carry unsolved templates (today they
flow through unsubstituted, plan 5's recorded boundary), and
`User::class` does not type as `class-string<app\user>`.

- [ ] **Step 3: Implement the solver**

`crates/celerrate_types/src/solver.rs`:

```rust
//! The call-site template solver (design section 6, generics
//! inference-only): structural constraint collection over
//! (declared parameter, argument) pairs, least-upper-bound
//! resolution, and the bound-then-mixed fallback — never the
//! first-seen constituent, which would leak wrong member sets into
//! the unknown-members family.

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::ClassQuery;
use celerrate_stubs::StubIndexInput;

use crate::representation::{StringConstraint, TypeData, TypeId};
use crate::substitution::Substitution;

/// Solves the template variables constrained by `pairs`. Multiple
/// constraints on one variable take `TypeId::union` (the lattice
/// least upper bound); a variable no pair constrains is simply
/// absent from the map — `finalize_return` owns its fallback.
pub(crate) fn solve<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    pairs: &[(TypeId<'db>, TypeId<'db>)],
) -> Substitution<'db> {
    let mut constraints: BTreeMap<(String, String), Vec<TypeId<'db>>> = BTreeMap::new();
    for (declared, argument) in pairs {
        collect(db, files, stubs, configuration, *declared, *argument, &mut constraints);
    }
    let mut map = Substitution::default();
    for ((scope, name), collected) in constraints {
        map.bind(&scope, &name, TypeId::union(db, collected));
    }
    map
}

fn collect<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    declared: TypeId<'db>,
    argument: TypeId<'db>,
    constraints: &mut BTreeMap<(String, String), Vec<TypeId<'db>>>,
) {
    let recurse = |declared: TypeId<'db>, argument: TypeId<'db>,
                   constraints: &mut BTreeMap<(String, String), Vec<TypeId<'db>>>| {
        collect(db, files, stubs, configuration, declared, argument, constraints);
    };
    match declared.data(db) {
        TypeData::Template { scope, name, .. } => {
            constraints
                .entry((scope.clone(), name.clone()))
                .or_default()
                .push(argument);
        }
        TypeData::ClassString { argument: Some(inner) } => {
            let extracted = match argument.data(db) {
                TypeData::String { constraint: StringConstraint::Literal(text) } => {
                    Some(TypeId::class(db, &text.to_lowercase(), vec![]))
                }
                TypeData::ClassString { argument: Some(carried) } => Some(*carried),
                _ => None,
            };
            if let Some(extracted) = extracted {
                recurse(*inner, extracted, constraints);
            }
        }
        TypeData::Class { name: declared_name, arguments: declared_arguments } => {
            match argument.data(db) {
                TypeData::Class { name, arguments } if name == declared_name => {
                    for (left, right) in declared_arguments.iter().zip(arguments.iter()) {
                        recurse(*left, *right, constraints);
                    }
                }
                TypeData::Class { name, .. } => {
                    // The argument is a subclass: its threaded
                    // arguments for the declared ancestor constrain.
                    let class = ClassQuery::new(db, name.clone());
                    let threaded = crate::inheritance::ancestor_arguments(
                        db, files, stubs, configuration, class,
                    )
                    .iter()
                    .find(|(ancestor, _)| ancestor == declared_name)
                    .map(|(_, fixed)| fixed.clone());
                    if let Some(fixed) = threaded {
                        for (left, right) in declared_arguments.iter().zip(fixed.iter()) {
                            recurse(*left, *right, constraints);
                        }
                    }
                }
                _ => {}
            }
        }
        TypeData::Array { key, value, .. } => {
            match argument.data(db) {
                TypeData::Array { key: argument_key, value: argument_value, .. } => {
                    recurse(*key, *argument_key, constraints);
                    recurse(*value, *argument_value, constraints);
                }
                TypeData::Shape { .. } => {
                    // `key-of`/`value-of` evaluate shapes eagerly.
                    recurse(*key, TypeId::key_of(db, argument), constraints);
                    recurse(*value, TypeId::value_of(db, argument), constraints);
                }
                _ => {}
            }
        }
        TypeData::Callable { return_type, .. } => {
            if let TypeData::Callable { return_type: argument_return, .. } =
                argument.data(db)
            {
                recurse(*return_type, *argument_return, constraints);
            }
        }
        TypeData::Union { constituents } => {
            for constituent in constituents {
                recurse(*constituent, argument, constraints);
            }
        }
        TypeData::Intersection { intersectands } => {
            for intersectand in intersectands {
                recurse(*intersectand, argument, constraints);
            }
        }
        _ => {}
    }
}

/// A call result is concrete: any template the solver left unbound
/// substitutes to its bound, then `mixed` — the design's fallback,
/// never a first-seen constituent. Placeholders pass through
/// untouched (they belong to `member_boundary_type`).
pub(crate) fn finalize_return<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    of: TypeId<'db>,
) -> TypeId<'db> {
    if !crate::substitution::contains_symbolic(db, of) {
        return of;
    }
    let mut map = Substitution::default();
    collect_remaining_templates(db, of, &mut map);
    crate::substitution::substitute(db, files, stubs, configuration, of, &map, None)
}

fn collect_remaining_templates<'db>(
    db: &'db dyn salsa::Database,
    of: TypeId<'db>,
    map: &mut Substitution<'db>,
) {
    match of.data(db) {
        TypeData::Template { scope, name, bound } => {
            // The bound IS the fallback: a boundless template carries
            // `mixed` as its bound, so bound-then-mixed is one move.
            map.bind(scope, name, *bound);
        }
        TypeData::Mixed
        | TypeData::Never
        | TypeData::Void
        | TypeData::Null
        | TypeData::Object
        | TypeData::Resource
        | TypeData::Bool { .. }
        | TypeData::Int { .. }
        | TypeData::Float { .. }
        | TypeData::String { .. }
        | TypeData::EnumCase { .. }
        | TypeData::SelfPlaceholder
        | TypeData::ParentPlaceholder
        | TypeData::StaticPlaceholder => {}
        TypeData::Union { constituents } => {
            for child in constituents {
                collect_remaining_templates(db, *child, map);
            }
        }
        TypeData::Intersection { intersectands } => {
            for child in intersectands {
                collect_remaining_templates(db, *child, map);
            }
        }
        TypeData::Array { key, value, .. } => {
            collect_remaining_templates(db, *key, map);
            collect_remaining_templates(db, *value, map);
        }
        TypeData::Shape { fields } => {
            for field in fields {
                collect_remaining_templates(db, field.value, map);
            }
        }
        TypeData::ClassString { argument } => {
            if let Some(child) = argument {
                collect_remaining_templates(db, *child, map);
            }
        }
        TypeData::Class { arguments, .. } => {
            for child in arguments {
                collect_remaining_templates(db, *child, map);
            }
        }
        TypeData::Callable { parameters, return_type } => {
            for parameter in parameters {
                collect_remaining_templates(db, parameter.parameter_type, map);
            }
            collect_remaining_templates(db, *return_type, map);
        }
        TypeData::KeyOf { subject } | TypeData::ValueOf { subject } => {
            collect_remaining_templates(db, *subject, map);
        }
        TypeData::Conditional { subject, matches, then_branch, otherwise_branch, .. } => {
            collect_remaining_templates(db, *subject, map);
            collect_remaining_templates(db, *matches, map);
            collect_remaining_templates(db, *then_branch, map);
            collect_remaining_templates(db, *otherwise_branch, map);
        }
    }
}
```

In `flow.rs`:

1. **`Foo::class`**: in the `ScopedAccess` arm, a `Named` member
   spelled exactly `class` (case-insensitive, PHP's rule) on a
   resolved class subject answers
   `TypeId::class_string(db, Some(TypeId::class(db, &key, vec![])))`.
2. **Pair alignment** (`solver_pairs`): positional index over
   non-labeled arguments; a labeled argument matches the parameter of
   that name; surplus positional arguments pair with the last
   parameter when it is variadic; a spread argument ends alignment
   (conservative). Each pair is (declared parameter type or skip when
   `None`, the argument expression's inferred type).
3. **Application**: in the `Call` arm (both the function and method
   paths), after the result type and its `DeclaredSignature` are
   known — and after `member_boundary_type` ran for methods — when
   `contains_symbolic(db, result)`:

```rust
let pairs = self.solver_pairs(&signature.parameters, arguments, environment);
let solved = crate::solver::solve(db, self.files, self.stubs, self.configuration, &pairs);
let result = crate::substitution::substitute(
    db, self.files, self.stubs, self.configuration, result, &solved, None,
);
let result = crate::solver::finalize_return(
    db, self.files, self.stubs, self.configuration, result,
);
```

   The provider tier is exempt (providers answer concrete types,
   already widened at the consumption boundary); the declared and
   inferred tiers both flow through.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS (7 new tests). Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src
git commit -m "✨ feat(types): templates solve at call sites under the bound-then-mixed fallback"
```

---

### Task 9: Constructor inference and inline `@var`

The two remaining delivery channels for class-level generics
(decision 11): `new Box($user)` solves `Box`'s templates from its
constructor's parameters, and a named inline
`@var Collection<User> $c` binds the local — which finally exercises
the receiver-argument zip `member_boundary_type` carries since
task 5.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests)

**Interfaces:**
- Consumes: `solve`/`solver_pairs`/`finalize_return` (task 8),
  `class_annotations(...).templates` (task 3),
  `declared_member_signature` for `__construct`,
  `ParsedAnnotations.variables` (task 2), `BodyIr.annotations`
  (`BodyAnnotation { text, anchor }`), `annotations_for_docblock`,
  `owner_class_docblock` (task 3 made it `pub(crate)`).
- Produces (task 10 and plan 8 rely on the behavior, not new names):
  `New` results may carry `Class { name, arguments }`; locals may be
  bound by inline `@var`.

- [ ] **Step 1: Write the failing tests**

In `inference.rs`'s test module (the fake gains two arms in this
step, following its established line-parsing pattern: generic class
references `NAME<NAME, ...>` wherever a bare `NAME` lowers today, and
the named `@var NAME<NAME> $name` form feeding
`ParsedAnnotations.variables`):

```rust
#[test]
fn constructor_arguments_solve_the_class_templates() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
/** @template T */
class Box {
    /** @param T $item */
    public function __construct($item) {}
    /** @return T */
    public function get() {}
}
function caller() {
    $box = new Box(new User());
    return $box->get();
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
}

#[test]
fn a_constructorless_generic_new_stays_a_plain_class() {
    let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Box {
    /** @return T */
    public function get() {}
}
function build() { return new Box(); }
function read() { return (new Box())->get(); }
"#]);
    assert_eq!(caller_return_display(&f, "app\\build"), "app\\box");
    assert_eq!(
        caller_return_display(&f, "app\\read"),
        "mixed",
        "an unconstrained template falls to its bound then mixed",
    );
}

#[test]
fn an_inline_var_declares_the_local_through_its_assignment() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
/** @template T */
class Collection {
    /** @return T */
    public function first() {}
}
function caller($opaque) {
    /** @var Collection<User> $items */
    $items = $opaque;
    return $items->first();
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
}

#[test]
fn an_inline_var_binds_before_its_anchored_statement_too() {
    let f = fixture(&[r#"<?php
namespace App;
function caller($input) {
    /** @var int $input */
    return $input;
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "int");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types inference`
Expected: FAIL — `new Box(new User())` answers a plain `app\box`
(so `get()` answers `mixed`), and inline `@var` is not consumed
anywhere (the annotations sit unread in the IR since plan 1b).

- [ ] **Step 3: Implement the two channels**

In `flow.rs`:

1. **Constructor inference**, in the `New` arm after the class key
   resolves: when `class_annotations(...).templates` is non-empty and
   the call has arguments, fetch the `__construct` declared signature
   (`declared_member_signature` with
   `folded_member_key(MemberKind::Method, "__construct")` — the
   linearized lookup already covers inherited and trait
   constructors), align `solver_pairs`, `solve`, and build the
   arguments in template-declaration order:

```rust
let solved = crate::solver::solve(db, self.files, self.stubs, self.configuration, &pairs);
let mut any_bound = false;
let arguments: Vec<TypeId<'db>> = templates
    .iter()
    .map(|template| match solved.binding(&key, &template.name) {
        Some(argument) => {
            any_bound = true;
            argument
        }
        None => template.bound.unwrap_or_else(|| TypeId::mixed(db)),
    })
    .collect();
let result = if any_bound {
    TypeId::class(db, &key, arguments)
} else {
    TypeId::class(db, &key, vec![])
};
```

   The `any_bound` guard keeps unconstrained `new Box()` at the plain
   class — the canonical receiver everywhere else in the corpus —
   instead of minting a `Box<mixed>` spelling of the same thing.
2. **Inline `@var`**: `FlowContext` gains `scope_key: String` (the
   declaring-scope key the annotation context needs: the function's
   folded key, or `<class key>::<member key>` for methods — built
   where `inferred_body_types` unpacks `body_owner`, matching the
   `TypeId::template` scope convention `declared.rs` already uses).
   In the walker, group `BodyIr.annotations` by anchor once at entry
   (`BTreeMap<Option<StatementId>, Vec<&str>>`). For each annotation
   text, parse on demand:

```rust
fn inline_variables(&self, text: &str) -> Vec<(String, TypeId<'db>)> {
    let db = self.db;
    let tables = &self.tables;
    let site = NameSite::Source { namespace: &self.namespace, tables };
    let owner_docblock = self
        .owner_class_key
        .as_deref()
        .and_then(|owner| crate::declared::owner_class_docblock(db, self.files, owner));
    let context = crate::type_syntax::AnnotationContext {
        declaring_scope: &self.scope_key,
        enclosing_class_scope: self.owner_class_key.as_deref(),
        enclosing_class_docblock: owner_docblock.as_deref(),
    };
    crate::type_syntax::annotations_for_docblock(db, &site, &context, text).variables
}
```

   Application rule (decision 11): entries anchored to statement `S`
   bind their `NarrowingSubject::Local` immediately **before** the
   walker processes `S` and re-bind immediately **after** it (the
   declaration survives `S`'s own assignment); entries with no anchor
   bind once at body entry. Mirror the exact arm structure of the
   statement loop — the two hook points are where `looped` already
   brackets statement walks.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS (4 new tests). Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src
git commit -m "✨ feat(types): constructor inference and inline @var deliver class generics"
```

---

### Task 10: Iteration typing

The delivery mechanism for the generics precision of section 6, and
the replacement for the honest `mixed` stub at `flow.rs:1030`:
`foreach` resolves element and key types through the protocol chain,
with template substitution through each step.

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (tests)

**Interfaces:**
- Consumes: `member_boundary_type` (task 5), the method result path
  (task 6: `getIterator`/`current`/`key` returns, declared or
  inferred), `ancestor_arguments` (task 3), `TypeId::key_of`/
  `value_of` (eager over shapes).
- Produces: `fn iteration_types(&mut self, subject: TypeId<'db>,
  depth: u32) -> (TypeId<'db>, TypeId<'db>)` on `Walker` — `(key,
  value)`; the `Foreach` arm binds both subjects through it.

- [ ] **Step 1: Write the failing tests**

In `inference.rs`'s test module:

```rust
#[test]
fn foreach_over_an_array_literal_types_key_and_value() {
    let f = fixture(&[r#"<?php
namespace App;
function values() {
    foreach ([1, 2] as $key => $value) { return $value; }
    return 0;
}
function keys() {
    foreach ([1, 2] as $key => $value) { return $key; }
    return 0;
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\values"), "1|2|0");
    assert_eq!(caller_return_display(&f, "app\\keys"), "int|0");
}

#[test]
fn a_declared_generator_return_drives_foreach() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
/** @return \Generator<int, User> */
function stream() {}
function caller() {
    foreach (stream() as $user) { return $user; }
    return null;
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user|null");
}

#[test]
fn the_protocol_interfaces_carry_their_own_arguments() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
/** @return \Iterator<string, User> */
function iterate() {}
function caller() {
    foreach (iterate() as $key => $user) { return $key; }
    return '';
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "string|''");
}

#[test]
fn an_iterator_aggregate_unwraps_get_iterator() {
    let f = fixture(&[r#"<?php
namespace App;
class User {}
class Users implements \IteratorAggregate {
    /** @return \Generator<int, User> */
    public function getIterator(): \Generator {}
}
function caller(Users $users) {
    foreach ($users as $user) { return $user; }
    return null;
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user|null");
}

#[test]
fn a_protocol_class_without_threading_falls_to_current_and_key() {
    let f = fixture(&[r#"<?php
namespace App;
class Numbers implements \Iterator {
    public function current(): int {}
    public function key(): string {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
function values(Numbers $numbers) {
    foreach ($numbers as $value) { return $value; }
    return 0;
}
function keys(Numbers $numbers) {
    foreach ($numbers as $key => $value) { return $key; }
    return '';
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\values"), "int|0");
    assert_eq!(caller_return_display(&f, "app\\keys"), "string|''");
}

#[test]
fn a_union_subject_joins_and_skips_its_null_constituent() {
    let f = fixture(&[r#"<?php
namespace App;
function caller(bool $flag) {
    $subject = $flag ? [1, 2] : null;
    foreach ($subject as $value) { return $value; }
    return 0;
}
"#]);
    assert_eq!(caller_return_display(&f, "app\\caller"), "1|2|0");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types inference`
Expected: FAIL — every foreach binding is `mixed` (the plan-5 stub).

- [ ] **Step 3: Implement the protocol chain**

Replace the `Foreach` arm's `mixed` bindings with
`iteration_types(subject_type, 0)` and implement, on `Walker`:

```rust
/// Element and key types through the iteration protocol chain
/// (decision 12). Precedence per subject constituent: array forms
/// answer directly; a `Generator`, `Iterator`, `IteratorAggregate`,
/// or `Traversable` class type carrying two or more arguments
/// answers them; a class declaring or inheriting `getIterator`
/// unwraps its return recursively (depth guard 8); a class with
/// threaded protocol-ancestor arguments answers them; a class
/// declaring or inheriting both `current` and `key` answers their
/// returns; a union joins its constituents, skipping `null` and
/// `false` (iterating them yields nothing); a template recurses
/// through its bound. Everything else — including a plain object,
/// whose property iteration is a recorded stance — answers `mixed`.
fn iteration_types(
    &mut self,
    subject: TypeId<'db>,
    depth: u32,
) -> (TypeId<'db>, TypeId<'db>) {
    let db = self.db;
    let mixed = (TypeId::mixed(db), TypeId::mixed(db));
    if depth > 8 {
        return mixed;
    }
    match subject.data(db) {
        TypeData::Array { key, value, .. } => (*key, *value),
        TypeData::Shape { .. } => (
            TypeId::key_of(db, subject),
            TypeId::value_of(db, subject),
        ),
        TypeData::Union { constituents } => {
            let mut keys = Vec::new();
            let mut values = Vec::new();
            for constituent in constituents {
                if matches!(
                    constituent.data(db),
                    TypeData::Null | TypeData::Bool { literal: Some(false) }
                ) {
                    continue;
                }
                let (key, value) = self.iteration_types(*constituent, depth + 1);
                keys.push(key);
                values.push(value);
            }
            if values.is_empty() {
                return mixed;
            }
            (TypeId::union(db, keys), TypeId::union(db, values))
        }
        TypeData::Template { bound, .. } => self.iteration_types(*bound, depth + 1),
        TypeData::Class { name, arguments } => {
            self.class_iteration_types(name, arguments, subject, depth)
        }
        _ => mixed,
    }
}

const ITERATION_PROTOCOL: [&str; 4] =
    ["generator", "iterator", "iteratoraggregate", "traversable"];

fn class_iteration_types(
    &mut self,
    name: &str,
    arguments: &[TypeId<'db>],
    subject: TypeId<'db>,
    depth: u32,
) -> (TypeId<'db>, TypeId<'db>) {
    let db = self.db;
    let mixed = (TypeId::mixed(db), TypeId::mixed(db));
    // The protocol interfaces themselves, carrying their arguments:
    // `Generator<int, User>`, `Iterator<string, User>`, ...
    if Self::ITERATION_PROTOCOL.contains(&name)
        && let (Some(key), Some(value)) = (arguments.first(), arguments.get(1))
    {
        return (*key, *value);
    }
    let keys = vec![name.to_owned()];
    // An implementor declaring or inheriting `getIterator`: unwrap
    // its declared-or-inferred return through the standard method
    // result path (substitution included), then recurse.
    if self.member_owner(&keys, MemberKind::Method, "getIterator").is_some() {
        let (inner, _) = self.method_call_result_for_keys(&keys, subject, "getIterator");
        return self.iteration_types(inner, depth + 1);
    }
    // Threaded protocol-ancestor arguments:
    // `@implements Iterator<string, User>` composed by task 3.
    let class = celerrate_semantics::ClassQuery::new(db, name.to_owned());
    let threaded = crate::inheritance::ancestor_arguments(
        db, self.files, self.stubs, self.configuration, class,
    )
    .iter()
    .find(|(ancestor, _)| Self::ITERATION_PROTOCOL.contains(&ancestor.as_str()))
    .map(|(_, fixed)| fixed.clone());
    if let Some(fixed) = threaded
        && let (Some(key), Some(value)) = (fixed.first(), fixed.get(1))
    {
        let key = self.member_boundary_type(*key, Some(name), subject);
        let value = self.member_boundary_type(*value, Some(name), subject);
        return (key, value);
    }
    // The `current`/`key` protocol members, declared or inferred.
    if self.member_owner(&keys, MemberKind::Method, "current").is_some()
        && self.member_owner(&keys, MemberKind::Method, "key").is_some()
    {
        let (value, _) = self.method_call_result_for_keys(&keys, subject, "current");
        let (key, _) = self.method_call_result_for_keys(&keys, subject, "key");
        return (key, value);
    }
    mixed
}
```

Adjust the two `method_call_result_for_keys` call shapes to the
function's real signature (it answers `(TypeId,
Option<DeclaredSignature>)` today — destructure accordingly). The
`by_reference` foreach value keeps binding like a plain value
(decision 12's recorded stance — no write-back).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS (6 new tests; every earlier foreach expectation that
pinned `mixed` gets updated in the same spirit as decision 16 — the
stub's honesty clause has expired). Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src
git commit -m "✨ feat(types): iteration typing through the protocol chain"
```

---

### Task 11: The ground-truth channel in the CLI

The hidden `ground-truth <path>` subcommand (decision 13): for every
source function and method with a docblock-annotated return and a
body, confront the inferred return with the annotated one under the
compatibility relation — **inferred is a subtype of annotated**;
`Fails` is a divergence, `CannotProve` passes (inference-only
generics make precision asymmetric by design). Records print sorted
and tab-separated; the gate lives in `xtask` (task 12), not here.

**Files:**
- Create: `crates/celerrate_cli/src/ground_truth.rs`
- Modify: `crates/celerrate_cli/src/arguments.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (dispatch)
- Create: `crates/celerrate_cli/tests/ground_truth.rs`

**Interfaces:**
- Consumes: the exact project-loading path `Command::Check` uses up
  to an analyzable database (the session machinery in `session.rs` —
  reuse its entry, never a parallel loader), `member_tree`,
  `member_annotations`/`function_annotations` (annotation presence),
  `declared_member_signature`/`declared_function_signature` (the
  annotated side), `inferred_method_return`/`inferred_function_return`
  (the inferred side), `subtype_of`/`Proof`, `body_ir` (body
  presence), `folded_symbol_key`/`fully_qualified_name`/
  `folded_member_key`.
- Produces (task 12 relies on this exact format):
  - `Command::GroundTruth { path: PathBuf }`, hidden from help
    (`#[command(hide = true)]`).
  - stdout: zero or more record lines
    `<symbol>\t<inferred display>\t<annotated display>` — `<symbol>`
    is `<class key>::<member key>` for methods, the folded function
    key for functions — sorted by the full line, followed by exactly
    one summary line `checked <N>, divergences <M>`. Exit code 0
    whenever the analysis ran (divergences are data, not failure).

- [ ] **Step 1: Write the failing tests**

`crates/celerrate_cli/tests/ground_truth.rs` (the tempdir-project
idiom of `cache_consistency.rs` — reuse its project-scaffolding
helper shape):

```rust
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;

#[test]
fn divergences_print_sorted_with_the_summary_line() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("code.php"),
        r#"<?php
namespace App;
/** @return string */
function wrong() { return 1; }
/** @return int */
function right() { return 1; }
class Holder {
    /** @return string */
    public function alsoWrong() { return 2; }
}
"#,
    )
    .unwrap();
    let mut output = Vec::new();
    let outcome = celerrate_cli::run(
        vec![
            "celerrate".into(),
            "ground-truth".into(),
            project.path().as_os_str().to_owned(),
        ],
        &mut output,
    );
    let printed = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(
        lines,
        [
            "app\\holder::alsowrong\t2\tstring",
            "app\\wrong\t1\tstring",
            "checked 3, divergences 2",
        ],
        "sorted records, then the summary; the compatible function is silent",
    );
    assert!(matches!(outcome, celerrate_cli::Outcome::Success));
}

#[test]
fn bodiless_and_unannotated_members_are_not_checked() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("code.php"),
        r#"<?php
namespace App;
interface Contract {
    /** @return string */
    public function declared(): string;
}
function unannotated() { return 1; }
"#,
    )
    .unwrap();
    let mut output = Vec::new();
    let _ = celerrate_cli::run(
        vec![
            "celerrate".into(),
            "ground-truth".into(),
            project.path().as_os_str().to_owned(),
        ],
        &mut output,
    );
    let printed = String::from_utf8(output).unwrap();
    assert_eq!(printed.lines().last(), Some("checked 0, divergences 0"));
}

#[test]
fn the_subcommand_is_hidden_from_help() {
    let mut output = Vec::new();
    let _ = celerrate_cli::run(vec!["celerrate".into(), "--help".into()], &mut output);
    let printed = String::from_utf8(output).unwrap();
    assert!(
        !printed.contains("ground-truth"),
        "internal channel, plan 9c owns the product surface: {printed}",
    );
}
```

Adjust the exact `Outcome` variant name and the `--help` capture
path to `lib.rs`'s real API (both are pinned by existing tests in
`arguments.rs` and `lib.rs` — mirror them). If the literal expected
displays (`1`, `2`) disagree with `display.rs`, fix the expectation.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli ground_truth`
Expected: FAIL — the subcommand does not exist (clap errors).

- [ ] **Step 3: Implement the channel**

1. `arguments.rs`: add the variant to `Command`:

```rust
/// Internal: the annotation ground-truth records (design section
/// 10, harness 1). Consumed by `cargo xtask ground-truth`; hidden
/// from help — the product surface is plan 9c's.
#[command(hide = true)]
GroundTruth { path: std::path::PathBuf },
```

2. `ground_truth.rs`: load the project exactly like `check` (same
   session entry, same plugin registration — the real bridge parses
   the docblocks here), then over every analyzed file's
   `member_tree`:
   - **functions**: annotation presence via
     `function_annotations(...).value.is_some()`; skip without it.
     Inferred side `inferred_function_return`, annotated side
     `declared_function_signature(...).value_type`. Body presence
     via `body_ir(db, file, BodyQuery::new(db, function.ast_id))`.
   - **methods**: for each class group with a name (anonymous
     classes have no key — skip), for each own member of kind
     `Method`: annotation presence via `member_annotations` on
     `MemberQuery::new(db, class_key, MemberKind::Method,
     folded_member_key(MemberKind::Method, &member.name))`; body
     presence via `body_ir`; inferred side `inferred_method_return`
     with `MethodQuery::new(db, class_key, member_key)`; annotated
     side `declared_member_signature(...).value_type`.
   - each checked pair: `subtype_of(db, files, stubs, configuration,
     inferred, annotated)`; `Proof::Fails` pushes
     `format!("{symbol}\t{inferred_display}\t{annotated_display}")`.
   - sort the records, write them, write the summary line.
3. `lib.rs`: dispatch `Command::GroundTruth { path }` to the module,
   mirroring the `Check` arm's session setup and error rendering.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli`
Expected: PASS (3 new tests, everything existing untouched). Then
the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): the hidden ground-truth record channel"
```

---

### Task 12: The ground-truth harness, baseline, and triage

`cargo xtask ground-truth [--bless]` runs the channel over the pinned
corpus and gates against the committed baseline (decision 13): the
gate fails on any divergence absent from the baseline — regressions —
never on the baseline's size. `--bless` preserves the human
classification column across regeneration. The initial triage is
budgeted work in this task, per the design.

**Files:**
- Create: `xtask/src/ground_truth.rs`
- Modify: `xtask/src/main.rs`, `xtask/src/lib.rs`
- Create: `xtask/ground-truth-baseline.txt` (blessed here)
- Modify: `.github/workflows/corpus.yml`

**Interfaces:**
- Consumes: `xtask::corpus::prepare()` (the pinned checkout plus
  vendor), `xtask::release_binary()`, the task-11 record format.
- Produces:
  - `xtask/ground-truth-baseline.txt`: header comments (`#`-prefixed:
    the corpus pin, the format), then one line per divergence:
    `<classification>\t<symbol>\t<inferred>\t<annotated>` where
    `<classification>` ∈ `wrong-vendor-annotation`, `precision-gap`,
    `suspected-inference-bug`, `unclassified`.
  - `cargo xtask ground-truth` (gate) and
    `cargo xtask ground-truth --bless` (regenerate, classifications
    preserved; new records auto-classify `precision-gap` when the
    inferred column is exactly `mixed`, else `unclassified`).

- [ ] **Step 1: Write the failing tests**

The merge logic is a pure function — test it in
`xtask/src/ground_truth.rs`'s `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use super::{merge_baseline, parse_baseline, BaselineEntry};

    #[test]
    fn blessing_preserves_classifications_for_persisting_records() {
        let existing = parse_baseline(
            "# header\nsuspected-inference-bug\tapp\\a\tmixed\tstring\n",
        );
        let produced = vec![
            "app\\a\tmixed\tstring".to_owned(),
            "app\\b\tmixed\tint".to_owned(),
            "app\\c\t'x'\tint".to_owned(),
        ];
        let merged = merge_baseline(&existing, &produced);
        let classifications: Vec<(&str, &str)> = merged
            .iter()
            .map(|entry| (entry.classification.as_str(), entry.symbol.as_str()))
            .collect();
        assert_eq!(
            classifications,
            [
                ("suspected-inference-bug", "app\\a"),
                ("precision-gap", "app\\b"),
                ("unclassified", "app\\c"),
            ],
            "kept, auto-classified mixed, auto-classified other",
        );
    }

    #[test]
    fn the_gate_flags_only_records_absent_from_the_baseline() {
        let baseline = parse_baseline("precision-gap\tapp\\a\tmixed\tstring\n");
        let produced = vec![
            "app\\a\tmixed\tstring".to_owned(),
            "app\\new\tmixed\tint".to_owned(),
        ];
        let new_records = super::regressions(&baseline, &produced);
        assert_eq!(new_records, ["app\\new\tmixed\tint"]);
    }

    #[test]
    fn a_stale_baseline_entry_is_reported_but_never_fails_the_gate() {
        let baseline = parse_baseline("precision-gap\tapp\\gone\tmixed\tstring\n");
        let produced: Vec<String> = Vec::new();
        assert!(super::regressions(&baseline, &produced).is_empty());
        assert_eq!(super::stale(&baseline, &produced), ["app\\gone\tmixed\tstring"]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p xtask ground_truth`
Expected: FAIL to compile — the module does not exist.

- [ ] **Step 3: Implement the harness**

`xtask/src/ground_truth.rs`, following `corpus.rs`'s structure
(prepare → run the release binary → compare → actionable messages):

```rust
//! The annotation ground-truth harness (design section 10, harness
//! 1): the hidden CLI channel run over the pinned corpus, gated
//! against a committed baseline classified by divergence class. The
//! gate is on regressions, never on the baseline's size — a drowning
//! protocol is no protocol.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineEntry {
    pub classification: String,
    pub symbol: String,
    pub record: String, // "<symbol>\t<inferred>\t<annotated>"
}

/// Skips `#` lines; each remaining line splits on the FIRST tab into
/// the classification and the record. A line without a tab is
/// ignored (a hand-editing accident must not poison the gate).
pub fn parse_baseline(text: &str) -> Vec<BaselineEntry> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let (classification, record) = line.split_once('\t')?;
            let symbol = record.split('\t').next().unwrap_or(record).to_owned();
            Some(BaselineEntry {
                classification: classification.to_owned(),
                symbol,
                record: record.to_owned(),
            })
        })
        .collect()
}

/// Produced records no baseline entry carries: the regressions the
/// gate fails on.
pub fn regressions(baseline: &[BaselineEntry], produced: &[String]) -> Vec<String> {
    let known: std::collections::BTreeSet<&str> =
        baseline.iter().map(|entry| entry.record.as_str()).collect();
    produced
        .iter()
        .filter(|record| !known.contains(record.as_str()))
        .cloned()
        .collect()
}

/// Baseline records the run no longer produces — printed as a
/// re-bless hint, never a failure.
pub fn stale(baseline: &[BaselineEntry], produced: &[String]) -> Vec<String> {
    let current: std::collections::BTreeSet<&str> =
        produced.iter().map(String::as_str).collect();
    baseline
        .iter()
        .filter(|entry| !current.contains(entry.record.as_str()))
        .map(|entry| entry.record.clone())
        .collect()
}

/// Per produced record: keep the existing classification when the
/// record persists; a new record auto-classifies `precision-gap`
/// when its inferred column is exactly `mixed`, else `unclassified`.
pub fn merge_baseline(
    existing: &[BaselineEntry],
    produced: &[String],
) -> Vec<BaselineEntry> {
    let known: BTreeMap<&str, &str> = existing
        .iter()
        .map(|entry| (entry.record.as_str(), entry.classification.as_str()))
        .collect();
    produced
        .iter()
        .map(|record| {
            let mut columns = record.split('\t');
            let symbol = columns.next().unwrap_or("").to_owned();
            let inferred = columns.next().unwrap_or("");
            let classification = known
                .get(record.as_str())
                .copied()
                .unwrap_or(if inferred == "mixed" {
                    "precision-gap"
                } else {
                    "unclassified"
                })
                .to_owned();
            BaselineEntry {
                classification,
                symbol,
                record: record.clone(),
            }
        })
        .collect()
}
```

`run(bless: bool)` follows `corpus.rs`'s structure and helpers
one-for-one (same `Error` type, same `prepare()` reuse, same
diff-and-fail idiom): prepare the pinned corpus, build the release
binary (`release_binary()`), spawn `celerrate ground-truth <corpus>`
capturing stdout, split off the trailing `checked N, divergences M`
summary line (echo it), and treat the remaining lines as the
produced records. Gate mode: read
`xtask/ground-truth-baseline.txt`, compute `regressions` — non-empty
→ print them, write all produced records to
`target/corpus/actual-ground-truth.txt`, and answer an error; print
`stale` records as a re-bless hint. Bless mode: write
`merge_baseline`'s result with the header (`# celerrate ground-truth
baseline — regenerate with: cargo xtask ground-truth --bless`, plus
the pinned commit echoed from `corpus.pin`), one
`<classification>\t<record>` line each. Wire `main.rs`
(`ground-truth` / `ground-truth --bless` arms plus the usage string)
and `lib.rs` (`pub mod ground_truth;`). Add the CI job to
`.github/workflows/corpus.yml`, a sibling of `snapshot`:

```yaml
  ground-truth:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - uses: Swatinem/rust-cache@v2
      - uses: actions/cache@v4
        with:
          path: target/corpus
          key: corpus-${{ hashFiles('xtask/corpus.pin') }}
      - run: cargo xtask ground-truth
```

- [ ] **Step 4: Bless and triage the initial baseline**

Run: `cargo xtask fetch-corpus && cargo xtask ground-truth --bless`

Then the budgeted triage over every `unclassified` record, editing
the classification column in place:

- `wrong-vendor-annotation` — reading the corpus source shows the
  docblock is wrong; the inference is right.
- `precision-gap` — the inference is honestly imprecise (a `mixed`
  from an unimplemented channel, a stub without generic threading, a
  provider not yet written — plan 7 owes most of these).
- `suspected-inference-bug` — the inference is wrong on correct
  code. Each of these gets investigated **now**: fix the bug in this
  task when it is small, or record it with a one-line note in the
  task-13 debt ledger when it is not. A suspected bug is never left
  bare in the baseline.

Re-run `cargo xtask ground-truth` — the gate must pass on the
freshly triaged baseline.

- [ ] **Step 5: Run the tests and the gate**

Run: `cargo test -p xtask && cargo xtask ground-truth`
Expected: PASS, gate green. Then the full workspace gate.

- [ ] **Step 6: Commit**

```bash
git add xtask .github/workflows/corpus.yml
git commit -m "✨ feat(xtask): the annotation ground-truth harness and its triaged baseline"
```

---

### Task 13: Closure — determinism, invalidation, and the debt ledger

The harness extensions the design names (section 10, harnesses 2 and
3 over the new machinery), the re-export audit, and the honest record
of what this plan leaves behind.

**Files:**
- Modify: `crates/celerrate_types/tests/fixpoint.rs`
- Modify: `crates/celerrate_types/tests/invalidation_scope.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (re-export audit)

**Interfaces:**
- Consumes: everything this plan built; `TestDatabase`'s
  `take_executed`/`executions_of` and the fixpoint harness's
  barrier/cancellation idioms (mirror the existing fixtures' shapes
  exactly — same helpers, new clusters).
- Produces: pins only, plus the debt ledger below recorded as
  rustdoc where each debt lives.

- [ ] **Step 1: Write the determinism fixtures**

In `crates/celerrate_types/tests/fixpoint.rs`, mirroring the existing
mutual-recursion fixtures (same `return_of`-style helper, adapted to
`inferred_method_return`):

```rust
#[test]
fn a_method_cycle_answers_identically_from_every_entry_point() {
    // The same two-class mutual-recursion cluster, queried
    // method-first, function-first, and callee-first over fresh
    // databases: byte-identical display every time.
    let source = r#"<?php
namespace App;
class Left {
    public function ping(Right $right, bool $stop) {
        if ($stop) { return 1; }
        return $right->pong($this, $stop);
    }
}
class Right {
    public function pong(Left $left, bool $stop) {
        if ($stop) { return 'one'; }
        return $left->ping($this, $stop);
    }
}
"#;
    let orders: [&[(&str, &str)]; 2] = [
        &[("app\\left", "ping"), ("app\\right", "pong")],
        &[("app\\right", "pong"), ("app\\left", "ping")],
    ];
    let mut answers = Vec::new();
    for order in orders {
        let f = fixture(&[source]);
        let displays: Vec<String> = order
            .iter()
            .map(|(class, method)| method_return_display(&f, class, method))
            .collect();
        answers.push(displays);
    }
    let mut sorted_first = answers[0].clone();
    sorted_first.sort();
    let mut sorted_second = answers[1].clone();
    sorted_second.sort();
    assert_eq!(sorted_first, sorted_second, "entry order never changes a fixpoint");
}
```

Add, mirroring the existing concurrency and cancellation fixtures
one-for-one with the method cluster: the same cluster queried across
thread counts answers identically (the `Barrier` idiom), and an edit
landing mid-method-fixpoint unwinds cleanly with no provisional value
served (the `AtomicBool`/cancellation idiom). These two reuse the
existing fixtures' scaffolding verbatim — only the queried cluster
changes.

- [ ] **Step 2: Write the invalidation probes**

In `crates/celerrate_types/tests/invalidation_scope.rs` (the
`take_executed`/`executions_of` idiom):

The four probes below use the file's existing fixture and
`executions_of` helpers plus the shared fake syntax (`test_support`);
`edit` means `handle.set_bytes(&mut db).to(...)` on the file's
`SourceFile` handle, exactly like the file's existing edits. If a
helper name differs, mirror the sibling tests in the same file.

```rust
#[test]
fn a_callee_body_edit_with_an_identical_inferred_return_backdates_callers() {
    let before = r#"<?php
namespace App;
class Greeter {
    public function greeting() { $word = 'hello'; return $word; }
}
function caller(Greeter $greeter) { return $greeter->greeting(); }
"#;
    // The local is renamed: the body IR changes, the callee re-infers,
    // but the inferred return is identical — early cutoff on
    // inferred_method_return spares the caller.
    let after = before.replace("$word", "$greeting");
    let mut f = fixture(&[before]);
    let _ = caller_return_display(&f, "app\\caller");
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    let _ = caller_return_display(&f, "app\\caller");
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        1,
        "only the edited callee re-infers: {log:?}",
    );
}

#[test]
fn a_trait_body_edit_reaches_each_using_class_and_only_them() {
    let before = r#"<?php
namespace App;
trait Reader {
    public function read() { return $this->value; }
}
class IntBox { use Reader; public int $value = 0; }
class StringBox { use Reader; public string $value = ''; }
class Unrelated {
    public function noop() { return 1; }
}
"#;
    let after = before.replace("return $this->value;", "return $this->value ?? null;");
    let mut f = fixture(&[before]);
    for class in ["app\\intbox", "app\\stringbox", "app\\unrelated"] {
        let member = if class == "app\\unrelated" { "noop" } else { "read" };
        let _ = inferred_method_return(
            &f.db, f.files, f.stubs, f.configuration,
            MethodQuery::new(&f.db, class.to_owned(), member.to_owned()),
        );
    }
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    for class in ["app\\intbox", "app\\stringbox", "app\\unrelated"] {
        let member = if class == "app\\unrelated" { "noop" } else { "read" };
        let _ = inferred_method_return(
            &f.db, f.files, f.stubs, f.configuration,
            MethodQuery::new(&f.db, class.to_owned(), member.to_owned()),
        );
    }
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        2,
        "one re-inference per using class, none for the bystander: {log:?}",
    );
}

#[test]
fn a_prose_only_class_docblock_edit_backdates_class_annotations_dependents() {
    let before = r#"<?php
namespace App;
/**
 * The repository.
 * @template T
 */
class Repository {
    /** @return T */
    public function find(int $identifier) {}
}
/** @extends Repository<User> */
class UserRepository extends Repository {}
class User {}
"#;
    let after = before.replace("The repository.", "The repository, but described better.");
    let mut f = fixture(&[before]);
    let query = MemberQuery::new(
        &f.db,
        "app\\userrepository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let _ = declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query);
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    let _ = declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query);
    let log = f.db.take_executed();
    assert!(
        executions_of(&log, "class_annotations") >= 1,
        "the annotation parse re-runs over the edited docblock: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "declared_member_signature"),
        0,
        "identical parsed annotations backdate — the two-stage cutoff: {log:?}",
    );
}

#[test]
fn an_extends_argument_edit_invalidates_inherited_signature_dependents() {
    let before = r#"<?php
namespace App;
/** @template T */
class Repository {
    /** @return T */
    public function find(int $identifier) {}
}
/** @extends Repository<User> */
class UserRepository extends Repository {}
class User {}
class Admin {}
"#;
    let after = before.replace("Repository<User>", "Repository<Admin>");
    let mut f = fixture(&[before]);
    let query = MemberQuery::new(
        &f.db,
        "app\\userrepository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let first = declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query)
        .unwrap()
        .value_type;
    assert_eq!(first, TypeId::class(&f.db, "app\\user", vec![]));
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    let second = declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query)
        .unwrap()
        .value_type;
    assert_eq!(
        second,
        TypeId::class(&f.db, "app\\admin", vec![]),
        "the threaded argument flows through on the next demand",
    );
}
```

- [ ] **Step 3: Run all tests, fix what the probes flush out**

Run: `cargo test -p celerrate_types`
Expected: the probes pass, or they flush a real cutoff hole (the
likely one: `class_annotations` returning owned `Vec`s without
`returns(ref)` somewhere, defeating backdating) — fix forward.

- [ ] **Step 4: The re-export audit and the debt ledger**

1. `lib.rs`: confirm the public surface additions are exactly
   `MethodQuery`, `InferenceContext`, `inferred_method_return`,
   `ClassAnnotations`, `class_annotations`, `ancestor_arguments`,
   `ParsedTemplate`, `ParsedAncestor` (plus the `ParsedAnnotations`
   fields) — `substitution`, `solver`, and their types stay
   `pub(crate)`.
2. Record the debts as rustdoc at their seams, each one line, each
   naming its owner:
   - anonymous-class receivers stay `mixed`; the expression-to-key
     path is plan 8's, with the checks' receiver surface
     (`flow.rs`, the `New`-with-`Anonymous` arm).
   - parameter-subject conditionals stay the branch union
     (`celerrate_phpdoc_bridge/src/lowering.rs`, the existing
     comment updated: the debt is now permanent-until-demanded, no
     longer "plan 6's").
   - stub ancestors carry no generic arguments — `ArrayIterator`-
     style stdlib generics degrade to the protocol-member fallback
     (`inheritance.rs` module doc; plan 7's curation owes the stub
     side).
   - plain-object property iteration stays `mixed`
     (`flow.rs::iteration_types` doc — already written in task 10).
   - by-reference foreach values get no write-back
     (`flow.rs`, the `Foreach` arm).
   - solver alignment ends at a spread argument
     (`flow.rs::solver_pairs` doc).
3. Sweep the forward-pointing "(plan 6)" comments this plan
   fulfilled and reword each to describe the shipped behavior:
   `representation.rs` (the `Conditional` and placeholder rank
   comments), `widening.rs:217`, `declared.rs:127`,
   `linearize.rs:8` and `:75`, and the
   `celerrate_phpdoc_bridge/src/expression/mod.rs:100` parameter-
   subject note (now a permanent recorded debt, decision 9).

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo xtask dependency-shape && cargo xtask corpus && cargo xtask ground-truth`
Expected: everything green.

```bash
git add crates/celerrate_types
git commit -m "✅ test(types): interprocedural determinism and invalidation pinned"
```

---

## Execution notes

- Tasks run strictly in order — the receiver model (5) needs the
  substitution primitive (1), the tiers (6) need the receiver model,
  the solver (8) needs the threading (2–4), the harness (11–12)
  needs the tiers. No two tasks touch disjoint code; do not
  parallelize.
- The shared fake `TypeSyntax` grows monotonically across tasks 3, 4,
  8, and 9 (templates and ancestors → `@param` → bounds,
  `class-string`, conditionals → generic references and named
  `@var`). Keep it in one `test_support` module from task 4 on; every
  arm follows the same line-parsing pattern, and none of it ships in
  production code.
- Display assertions: when an expected string disagrees with
  `display.rs`'s rendering, fix the expectation, never the code
  (plan-5 decision 16, reconducted).
- Plan-5 scaffolding this plan deletes or supersedes: `this_type`'s
  concrete answer, `substitute_receiver`, the method-inferred `mixed`
  tier comment at `flow.rs:852`, the `Foreach` `mixed` stub, and the
  decision-6 test expectations — update the tests' expectations, keep
  their scenarios.
- The corpus snapshot (`cargo xtask corpus`) must stay green
  throughout: nothing in this plan emits a diagnostic, so any
  snapshot change is a bug in the plan's execution.
- After the final task, do not extend the README or CHANGELOG: the
  preview's product surface is plan 9c's. The `ground-truth`
  subcommand stays hidden and undocumented.
