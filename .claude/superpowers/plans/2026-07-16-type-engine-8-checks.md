# Type Engine 8 — Checks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The three diagnostic families of the preview criterion —
**unknown members** (method, property, class constant, enum case on the
receiver's resolved type), **nullability** (dereference of a
possibly-null value), and **argument types** (per-argument
assignability plus arity, named arguments included) — each under the
anti-false-positive guillotine, with their permanent `CEL####`
identifiers, the per-kind magic and dynamic suppression, the union and
intersection receiver rules, the per-file coercion posture
(`declare(strict_types=1)`), the seeded-defect recall suite, the
corpus triage, and the product wiring that makes `celerrate check`
render them. Four inherited debts close here: the anonymous-class
expression-to-key path (plan 6's ledger re-homed it to the checks'
receiver surface), the `CELERRATE_CACHE_STATS` typed counters
(plan 5 shipped the instrument as `InferredBody.edge_counts`; the
first orchestration-layer consumer aggregates it now), the implicit
`UnitEnum`/`BackedEnum` surface (`celerrate_stubs`'s `extract.rs`
recorded it verbatim for this plan: "plan 8's checks need them,
revisit there"), and the folded-key rendering debt (`display.rs`,
`construction.rs`, and `lib.rs` in `celerrate_types` assign the
written-spelling recovery to "plan 8, which renders diagnostics").
Design source:
`.claude/superpowers/specs/2026-07-14-type-engine-design.md`, sections
8 (the three families and their conservative stances), 2 (magic and
dynamic suppression, anonymous identity), 3 (the three-valued
judgments, the empty-intersection guard), 5 (virtual members count as
existing; suppression extinguishes every family), 6 (narrowing feeds
the tables the checks read), 9 (recall gates, typed counters), 10
(harness 2 over the new edit classes), and 11 item 11 (this plan).

**Architecture:** The families live in `celerrate_types/src/checks/`
— checks live beside the data they consume, the semantic-core
precedent (`reference_checks.rs` lives in `celerrate_semantics`), and
`celerrate_rules` stays sub-project 4's. `celerrate_types` gains a
`celerrate_diagnostics` dependency (a downward, DAG-legal edge) and
becomes the workspace's fifth diagnostic producer. The verdict layer
is **range-free** (design section 2): a per-body tracked query
answers `TypedVerdict { body: AstId, expression: ExpressionId, kind }`
records, a per-file query aggregates them with the inference edge
counts, and a thin mapping query reconciles arena indices to
`TextRange` through `body_source_map` at rendering time — so an edit
above a body shifts offsets, the source map changes, the verdicts
backdate, and only the mapping re-runs. `celerrate_semantics` gains
the anonymous-class synthetic key resolution, the per-file
`strict_types` query (an own-tree read, the syntax-gating
precedent), and the implicit enum edges in linearization;
`celerrate_stubs` closes its recorded enum-parents debt.
The CLI composes the typed families **fresh on every path** — the
persistent cache keeps serving untyped verdicts only, the plan-9a
typed-artifact design untouched — and the suppression filter covers
the union at the existing single composition point.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), salsa 0.27.2,
the plan-5/6 inference engine (`inferred_body_types`,
`InferredBody.expression_types`, `edge_counts`), the plan-1a member
boundary (`lookup_member`, `linearized_class`, `MagicMarkers`), the
plan-3 declared signatures (`DeclaredSignature`, `DeclaredParameter`,
the `parameter_type: None` empty-intersection guard), the plan-2
judgments (`Proof`, `subtype_of`, `assignable_to`), the plan-4c
suppression pipeline (`suppressed_ranges`, `is_suppressed`), the
persistent cache (`StoredVerdict`, `composed_diagnostics`), the
corpus pin (`xtask/corpus.pin`, symfony/demo).

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
  No indexing: `.get()`, `.first()`, iterators, `.split_once()`.
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **The plan-6 blind-spot checklist**: plan 6's review-adjudicated
  departures all traced to two recurring blind spots, so every task's
  prescribed code and tests are checked against both before execution:
  - **Union subjects**: any logic keyed by a receiver or subject
    resolves **per constituent key** and reduces with `TypeId::union`
    — never a first-seen constituent, never one owner, origin, or
    class-string shared across keys (the `member_owner` and
    `$obj::class` departures).
  - **Traits behind `extends`**: any member, origin, or adaptation
    logic is pinned by a test that reaches the trait **through a
    subclass** (`Sub extends User`, `User` uses the trait), not only
    by the direct `use` — the shape of the dormant `linearize.rs`
    gap that forced `MemberOrigin::Trait { anchor }`.
- **The guillotine is the prime directive**: a `mixed` or otherwise
  undecidable receiver, signature, or judgment is **always silence**,
  never a guess. Every conservative stance in the fixed decisions is
  a contract with its own test. `Proof::CannotProve` never produces
  a diagnostic.
- **No `TextRange` below the mapping layer**: verdicts are keyed by
  `(AstId, ExpressionId)` only; `typed_diagnostics` is the single
  place arena indices meet offsets (design section 2). A verdict
  whose pointer cannot be reconciled is dropped, never a panic.
- **Checks never touch a syntax tree**: the walkers consume `BodyIr`,
  `InferredBody`, and the member/declared queries only. The two
  exceptions are pre-existing precedents, both in
  `celerrate_semantics`: `body_source_map` (rendering
  reconciliation) and the new `file_strict_types` (an own-tree read
  for strictly-local output, the syntax-gating precedent).
- **Determinism**: no wall clock, no randomness, no environment reads
  inside queries. Verdicts are emitted in arena walk order and the
  diagnostics `.sort()` before returning, exactly like
  `reference_diagnostics`. The byte-identical harness must stay green
  across thread counts.
- **Identifiers are permanent from the preview's publication**:
  `CEL0030`–`CEL0038`, allocated once, gaplessly, in the
  composition-root registry. Never renumber.
- **Strict layering**: one new inter-crate edge only —
  `celerrate_types` → `celerrate_diagnostics` (downward). The bridge
  and the stdlib provider keep their single `celerrate_plugin`
  dependency; `cargo xtask dependency-shape` stays green.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.

## Fixed decisions (the header the tasks implement)

1. **The families live in `celerrate_types/src/checks/`**, one module
   per family plus a shared receiver surface. `celerrate_rules` (the
   rule framework, registry, rendering) is sub-project 4's crate;
   creating it now to host three functions would be speculative
   structure. When the rule framework lands, the families migrate
   into it; their identifiers and behavior are the stable part.
   `celerrate_types` gains `celerrate_diagnostics` as a dependency —
   diagnostics sit at the bottom of the DAG, the edge points down.
2. **Identifier allocation, permanent once merged**: `CEL0030`
   unknown method, `CEL0031` unknown property, `CEL0032` unknown
   class constant, `CEL0033` unknown enum case, `CEL0034`
   possibly-null dereference, `CEL0035` argument type mismatch,
   `CEL0036` too few arguments, `CEL0037` too many arguments,
   `CEL0038` unknown named argument. All `Severity::Error`.
   `celerrate_types` exports `ALLOCATED_IDENTIFIERS`; the
   composition-root registry test (`celerrate_cli/tests/registry.rs`)
   gains the fifth producer; `REGISTRY` in
   `celerrate_diagnostics/src/registry.rs` gains nine rows and its
   gapless-count assertion moves from 29 to 38.
3. **The verdict/rendering split is range-free.** Three layers in
   `checks/mod.rs`: `body_typed_verdicts` (tracked per body — editing
   one body never re-checks its siblings, the harness-2 contract),
   `typed_file_verdicts` (tracked per file: aggregates bodies
   enumerated from the member tree — free functions plus methods of
   non-trait class-likes — and sums their
   `InferredBody.edge_counts`), and
   `typed_diagnostics` (tracked per file: maps each verdict's
   `ExpressionId` through `body_source_map` to a `TextRange` and a
   `Diagnostic`; a missing pointer drops the verdict).
   **Trait-owned bodies are skipped**: plan 6 analyzes trait bodies
   per using class because in-trait `$this` resolution is wrong, and
   judging one against the trait's own surface is a guaranteed
   false-positive class (a trait method calling
   `$this->providedByTheUsingClass()` is standard PHP). Recorded in
   task 13's ledger; a pinned silence test guards it. Verdict kinds
   carry pre-rendered display strings (`String`), not `TypeId`s, so
   the records are plain `Eq` data and message rendering has no
   database dependency (no `'db` lifetimes anywhere in the model, so
   `salsa::Update` is unnecessary — the `body_owner`/`lookup_member`
   precedent for tracked-query values; `InferredBody` derives it
   only because it carries `'db`-lifetime `TypeId`s). The
   payload strings recover **originally written spellings**: class
   and enum names render through the symbol index
   (`SymbolEntry.original` for source symbols, `StubSymbol.name` for
   stubs), falling back to the folded key when nothing answers. This
   closes the rendering debt `display.rs`, `construction.rs`, and
   `lib.rs` record against plan 8 by name; the lattice-internal
   `TypeId::display` itself stays folded.
4. **The ternary existence reduction.** `member_existence` answers
   `Exists | PossiblyExists | Missing`. The design's union rule
   (report only if missing on **all** non-null constituents, an
   undecidable constituent counts as possibly-having) and its
   intersection dual (exists if **any** intersectand has it,
   suppressed if any suppresses) coincide in this reduction: a
   union or intersection is `Missing` **iff every part is
   `Missing`**; any `Exists` part makes it `Exists`; otherwise
   `PossiblyExists`. Null parts are skipped (the null constituent
   belongs to the nullability family); a receiver that is *only*
   null answers `PossiblyExists` (dereferencing null is nullability's
   report, not unknown-members'). `PossiblyExists` is always silence.
5. **Silent receiver atoms, enumerated** (each is a pinned test):
   `mixed`, `object`, an unresolvable class name (`CEL0018` already
   reported it; double-reporting is a false positive by design),
   template variables (through-bound reporting is deferred — the
   stated posture toward `CannotProve`), `class-string` subjects,
   scalars, arrays, and callables as receivers (a "call on
   non-object" family is future work, not unknown-members), dynamic
   member names (`MemberReference::Variable`, `::Computed`,
   `::Missing`), and dynamic scoped subjects (`$class::method()`).
6. **Magic and dynamic suppression per kind, uniformly through
   `lookup_member`** — which walks source linearization *and* the
   stub graph, so one rule covers both surfaces: a resolvable
   `__call` makes instance methods `PossiblyExists`, `__callStatic`
   static methods, `__get` **or** `__set` properties (read and write
   positions both; over-suppression is the conservative direction).
   On top, from `linearized_class`: `allows_dynamic_properties`
   (`#[AllowDynamicProperties]`, own or inherited), a
   `stub_ancestors` entry `stdclass`, the receiver key `stdclass`
   itself — each makes properties `PossiblyExists`; and
   `has_opaque_edge` or `cyclic` makes **every** kind
   `PossiblyExists` (absence cannot be proven across an opaque or
   cyclic boundary).
7. **Virtual members and the implicit enum surface count as
   existing.** `lookup_member` already answers
   `MemberResolution::Virtual` for `@property`/`@method`
   declarations; the family sees them as any other member. Nothing to
   build there — a test pins it (design section 5). The implicit enum
   surface is real work, closing the debt `extract.rs` recorded for
   this plan: every PHP enum implicitly implements `UnitEnum`
   (`cases()`), backed enums also `BackedEnum` (`from()`,
   `tryFrom()`), and instances carry the engine-provided `name` and
   `value` properties — `Status::cases()` is ubiquitous 8.1 code and
   must never report. Three pieces: (a) linearization synthesizes
   `Implements` edges to `\UnitEnum` (and `\BackedEnum` for backed
   enums) for source enum groups, **only when the compiled stub graph
   knows the parent key** — a stub set without `UnitEnum` adds no
   edge (never a synthetic opaque edge, which would blanket-silence
   enums in stub-less fixtures); (b) the stub compiler's enum arm
   pushes the same implicit parents into `StubClassSurface.parents`,
   closing the `extract.rs` comment (blob recompiled); (c)
   `atom_existence` answers `Exists` for the `name` and `value`
   properties on any enum-keyed receiver (interfaces cannot declare
   properties, so no stub can ever carry them). If the member
   projection does not expose the backed-versus-pure fact at a seam,
   synthesize both parents: over-suppression is the conservative
   direction.
8. **Anonymous classes complete their identity** (plan 6's ledger,
   verbatim: "the expression-to-key path is plan 8's, with the
   checks' receiver surface"). A synthetic folded key
   `class@anonymous:{file}:{index}` (the `@` and `:` are illegal in
   PHP names — no collision with real folded keys):
   `anonymous_class_key(AstId) -> String` and
   `parse_anonymous_class_key(&str) -> Option<AstId>` in
   `celerrate_semantics`; `linearize.rs::fetch` resolves the
   synthetic key by loading the member group directly by `AstId`;
   `body_owner` keys anonymous-class methods with it (so `$this`
   resolves inside them); `flow.rs`'s `New`-with-`Anonymous` arm
   answers `TypeId::class(db, &key, vec![])` instead of `mixed`;
   `display.rs` renders any `class@anonymous:` key as
   `class@anonymous` (coordinates stripped — stable across edits in
   messages).
9. **Nullability reports on explicit null only.**
   `TypeId::contains_null` is the predicate — it answers `false` for
   `mixed` and for templates, so the design's mixed-receiver silence
   holds by construction. The subjects are `MemberAccess` expressions
   with `null_safe: false` (property reads, property writes, and
   method-call callees are all the same arena node — one verdict
   site, no double report); `?->` never reports (plan 5's chain rule
   already types the suffix non-null and re-acquires `|null` only at
   the chain end — a later real dereference of that end reports,
   correctly). The tables are post-narrowing by construction, so
   "un-narrowed" needs no extra machinery. A pure-`null` receiver
   reports too (`contains_null(null)` is true). **Guarded property
   reads are exempt**: a `MemberAccess` consumed by `isset()` or
   `empty()`, or sitting anywhere on the left operand of `??` or the
   target of `??=`, never reports — PHP evaluates a property read on
   null to null there without a fatal error and these constructs
   suppress even the warning, so they are the idiomatic guards
   themselves (PHPStan is silent there too). The exemption follows
   the receiver chain across property accesses and `Index` subjects
   and **stops at any call boundary**: a method call on a `??` left
   operand still throws at runtime and still reports. The `??=`
   write path can still throw when the receiver itself is null;
   exempting it anyway is accepted over-suppression, the
   conservative direction.
10. **The coercion posture follows the calling file.** New query
    `file_strict_types(db, file) -> bool` in
    `celerrate_semantics/src/strict_types.rs` (own-tree read; a
    `declare` whose directives contain `strict_types` `= 1` at the
    top level). `pub enum CoercionMode { Strict, Weak }` in
    `judgments.rs`; `assignable_to` gains the trailing parameter
    `mode: CoercionMode`. `Strict` = `subtype_of` **plus** the
    int-to-float widening PHP performs even under strict types.
    `Weak` additionally accepts scalar interchange (an
    `int|float|string|bool` source against a scalar target — `null`
    is never coercible) and a `Stringable` source against a `string`
    target (the receiver class resolves `__toString` through
    `lookup_member`, or names `Stringable` in its surface). The
    check reports **only `Proof::Fails`**: `Holds` and `CannotProve`
    are silence. Two guards sit in the family walk **before** the
    judgment, because the shipped `judge` refutes set-theoretically
    (a `mixed` candidate answers `Fails`, and a union candidate folds
    through `Proof::all`, so one failing constituent fails the whole
    union): (a) a `mixed` argument source is silence by an explicit
    `is_mixed` check, never by relying on the `Proof` value; (b) a
    union source reports only when **every** constituent fails
    assignability on its own — one assignable constituent is silence
    (a partial fit is a future "possibly invalid argument"
    diagnostic, recorded in task 13's ledger). "Mixed passes
    everywhere" is therefore a structural guarantee of the walk, not
    a property of the judgment.
11. **Argument checks run against exactly one resolved signature.**
    Free functions and stub functions by folded key
    (`declared_function_signature`); methods, static calls, and
    `new` only when the receiver resolves to **one** class key
    (`declared_member_signature`); union receivers are silent — a
    recorded stance, not a gap. The **declared tier only**: dynamic
    type providers compute returns, never parameter contracts. Per
    argument: a by-reference parameter is exempt (the `preg_match`
    `$matches` idiom, design section 6), and `parameter_type: None`
    — the empty-intersection stub guard — is exempt (plan 3's
    contract, consumed here for the first time).
12. **Arity, exactly.** Required = `!optional && !variadic`.
    Positional arguments beyond a non-variadic parameter list →
    `CEL0037`. A required parameter bound neither positionally nor
    by name → `CEL0036`. A named argument matching no declared
    parameter name → `CEL0038` (checked against the **declared
    receiver type's** parameter names — PHP permits overrides to
    rename parameters; PHPStan's stance too). A signature whose last
    parameter is variadic accepts **any** named argument: PHP 8.0
    collects unknown named arguments into a trailing variadic (and
    8.1's string-keyed spread does the same), so `CEL0038` is
    silenced for such a call. Any spread argument
    (`...$args`) silences all three for that call — spread makes
    missing and excess undecidable, and string-keyed spread acts as
    named arguments since 8.1 (broader than design section 8, which
    qualifies "unpacking of a non-shape value": shape-value spreads
    are silenced too, recorded in task 13's ledger); positional
    arguments **before** the first spread still type-check. A **source** callee whose body
    calls `func_get_args` silences excess only (a
    variadic-by-capture function called with extra arguments is
    working code; reporting it is what the guillotine forbids). A
    constructor-less `new Foo(1)` is silent — PHP evaluates and
    discards the arguments without error.
13. **The cache posture until plan 9a**: typed diagnostics are
    composed **fresh on every path**. `StoredVerdict` stays
    untyped-only — the packs never carried typed families, so no
    format bump is needed and old caches stay valid. The single
    composition point (`analysis.rs::composed_diagnostics`) appends
    `typed_diagnostics` after the cacheable compose and before the
    suppression filter; the persist path gets a
    `persistable_diagnostics` variant without the typed append. The
    equivalence net (cache-served == recomputed) extends over the
    union. Consequence, stated: warm runs re-infer — the warm number
    is measured honestly in plan 9b and the typed-artifact classes
    of plan 9a are the fix, not a snapshot hack here.
14. **`CELERRATE_CACHE_STATS` grows the typed counters** (plan 5's
    decision 13 lands): `CacheStatistics` gains `typed_bodies`,
    `typed_declared_edges`, `typed_inferred_edges`,
    `typed_provider_edges` (all `AtomicU64`), incremented at the
    orchestration layer (`analyze_one`) from
    `typed_file_verdicts(...).edge_counts`, rendered as an
    additional clause inserted before the persist clause of the
    existing one-line summary. Counters never live
    inside queries — the workspace rule, unchanged.
15. **The corpus gate and the guillotine.** `cargo xtask corpus`
    runs with the families live. Every typed line in the report is
    triaged: a false positive is **fixed in this plan** (each fix
    lands with a regression fixture in the family's tests), a
    verified true positive is blessed into `corpus-snapshot.txt`.
    The unknown-member identifiers (`CEL0030`–`CEL0033`) join the
    hard-refusal list (`corpus.rs`, the `UNKNOWN_SYMBOL_IDENTIFIERS`
    pattern) **only if** triage lands them at zero on the corpus. A
    family that cannot be triaged clean is written up in the closing
    memo as a guillotine candidate; the cut itself — dropping the
    family's walk from the composed verdict set — is plan 9c's
    release decision, one line in
    `crates/celerrate_types/src/checks/mod.rs` (the family-walk
    composition, not the composition point
    `analysis.rs::composed_diagnostics`: a cut there would not
    filter what plan 9a's typed cache persists and serves).
16. **The seeded-defect suite is the recall gate** (design section
    9): end-to-end through `run()` — the product pipeline, source
    map, suppression, and rendering included — one seeded defect per
    identifier, nine in all, each of which MUST report. A silent
    engine passes every precision gate; this is the gate it cannot
    pass.
17. **No new public API beyond the checks.** `checks::` exports the
    identifiers, `TypedVerdict`, `TypedVerdictKind`,
    `TypedFileResult`, `typed_file_verdicts`, `typed_diagnostics`,
    and `ALLOCATED_IDENTIFIERS`; `receivers.rs`, the family
    walkers, and the written-spelling display stay `pub(crate)`.
    `celerrate_semantics` adds `anonymous_class_key`,
    `parse_anonymous_class_key`, `class_surface`/`ClassSurface`,
    `file_strict_types`, and the widened
    `ExpressionId::from_index` to its surface.

## File structure

Created:

- `crates/celerrate_types/src/checks/mod.rs` — the identifiers, the
  verdict model (`TypedVerdict`, `TypedVerdictKind`,
  `TypedFileResult`), message rendering, the three queries
  (`body_typed_verdicts`, `typed_file_verdicts`,
  `typed_diagnostics`), `ALLOCATED_IDENTIFIERS`.
- `crates/celerrate_types/src/checks/receivers.rs` — placeholder
  resolution against the body owner, receiver decomposition, the
  ternary `member_existence`, the per-kind suppression rules, the
  class-surface classification (source / stub / unknown).
- `crates/celerrate_types/src/checks/members.rs` — the
  unknown-members walk (`CEL0030`–`CEL0033`).
- `crates/celerrate_types/src/checks/nullability.rs` — the
  possibly-null dereference walk (`CEL0034`).
- `crates/celerrate_types/src/checks/arguments.rs` — the argument
  walk (`CEL0035`–`CEL0038`), signature resolution, arity, named
  arguments, the `func_get_args` probe.
- `crates/celerrate_semantics/src/strict_types.rs` — the per-file
  `declare(strict_types=1)` fact.
- `crates/celerrate_cli/tests/seeded_defects.rs` — the recall gate.

Modified:

- `crates/celerrate_types/Cargo.toml` — the `celerrate_diagnostics`
  dependency.
- `crates/celerrate_types/src/lib.rs` — module declaration `checks`,
  the re-exports of decision 17, `pub use` of
  `checks::ALLOCATED_IDENTIFIERS`.
- `crates/celerrate_types/src/flow.rs` — the `New`-with-`Anonymous`
  arm (decision 8).
- `crates/celerrate_types/src/inference.rs` — `body_owner`'s
  anonymous branch keys with `anonymous_class_key`.
- `crates/celerrate_types/src/display.rs` — the
  `class@anonymous` rendering rule; the name-resolver threading for
  written spellings (decision 3).
- `crates/celerrate_semantics/src/body.rs` —
  `ExpressionId::from_index` widened from `pub(crate)` to `pub` and
  exported (the walkers construct ids from enumeration indices).
- `crates/celerrate_stubs/src/compiler/extract.rs` — the implicit
  `UnitEnum`/`BackedEnum` parents of stub enums (decision 7; the
  compiled blob is regenerated with `cargo xtask compile-stubs`).
- `crates/celerrate_types/src/judgments.rs` — `CoercionMode`, the
  `assignable_to` mode parameter, the strict and weak coercion
  rules.
- `crates/celerrate_semantics/src/linearize.rs` — the synthetic-key
  branch in `fetch`, `anonymous_class_key`,
  `parse_anonymous_class_key`.
- `crates/celerrate_semantics/src/lib.rs` — module `strict_types`,
  the new exports.
- `crates/celerrate_diagnostics/src/registry.rs` — nine `REGISTRY`
  rows, the gapless count 29 → 38.
- `crates/celerrate_cli/src/analysis.rs` — the typed append at the
  composition point, `persistable_diagnostics`, the statistics
  aggregation.
- `crates/celerrate_cli/src/cache/mod.rs` — `composed_verdict`
  switches to `persistable_diagnostics`.
- `crates/celerrate_cli/src/cache/statistics.rs` — the typed
  counters and the extended render line.
- `crates/celerrate_cli/tests/registry.rs` — the fifth producer.
- `crates/celerrate_cli/tests/suppressions.rs` — suppression
  extinguishes a typed diagnostic.
- `crates/celerrate_cli/tests/cache_equivalence.rs` — the
  equivalence net over the union.
- `crates/celerrate_types/tests/invalidation_scope.rs` — the typed
  edit classes.
- `xtask/src/corpus.rs` — the unknown-member hard-refusal list
  (conditional on task 12's triage).
- `xtask/corpus-snapshot.txt` — re-blessed by task 12's triage.

Task order is strict: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 →
12 → 13. Task 1 closes the identity debt the receiver surface needs;
2 builds the skeleton and the producer registration; 3 the shared
receiver surface; 4–5 the unknown-members family; 6 nullability; 7
the coercion posture 8 needs; 8–9 the argument family; 10 the product
wiring; 11 the recall gate; 12 the corpus triage; 13 the closure
pins and the debt ledger. Do not parallelize: 3, 4, 5, and 6 all
touch `checks/` seams, and 10–12 all touch the CLI composition.

---

### Task 1: Anonymous classes complete their identity

The plan-6 ledger, verbatim: "anonymous-class receivers stay `mixed`;
the expression-to-key path is plan 8's, with the checks' receiver
surface (`flow.rs`, the `New`-with-`Anonymous` arm)." Without this,
every `new class { }` receiver is silent and the fixture idiom PHP
test suites love (`new class extends Base { }`) is invisible to all
three families.

**Files:**
- Modify: `crates/celerrate_semantics/src/linearize.rs` (the
  synthetic key, the `fetch` branch)
- Modify: `crates/celerrate_semantics/src/members.rs` (`ClassMembers`
  gains its heritage fields)
- Modify: `crates/celerrate_semantics/src/lib.rs` (exports)
- Modify: `crates/celerrate_types/src/inference.rs` (`body_owner`'s
  anonymous branch)
- Modify: `crates/celerrate_types/src/flow.rs` (the `New` arm: the
  `ClassReference::Anonymous { .. } | ClassReference::Missing`
  match, near line 2035 at review time; find it by the variant
  names, the line drifts as plan 7 merges)
- Modify: `crates/celerrate_types/src/display.rs` (the rendering
  rule)
- Test: the touched modules' own test blocks

**Interfaces:**
- Consumes: `AstId { file: FileId, index: u32 }` (public fields),
  `FileId::new`/`FileId::as_u32`, `member_tree`, `item_tree`,
  `linearized_class`, `body_owner` (plan 5, `pub(crate)`), the
  `ClassReference::Anonymous { declaration: AstId }` IR variant.
- Produces:
  - `celerrate_semantics::anonymous_class_key(ast_id: AstId) ->
    String` — the form `class@anonymous:{file}:{index}` (`@` and `:`
    are illegal in PHP names: no collision with folded keys, and
    `folded_symbol_key` is the identity on it — already lowercase, no
    leading backslash).
  - `celerrate_semantics::parse_anonymous_class_key(key: &str) ->
    Option<AstId>` — the inverse; `None` for anything else.
  - `linearized_class(ClassQuery::new(db, anonymous_class_key(id)))`
    answers the anonymous class's member table, heritage included.
  - `ClassMembers` gains `pub extends: Vec<String>` and
    `pub implements: Vec<String>` (written names; linearization
    resolves them in the group's namespace). Populated for **every**
    class-like — named classes keep using their `Declaration` edges,
    so nothing changes for them; the group fields serve the
    declaration-less anonymous case.
  - `flow.rs`: `new class ... { }` types as
    `TypeId::class(db, &anonymous_class_key(declaration), vec![])`.
  - `display.rs`: any class name starting `class@anonymous:` renders
    as `class@anonymous` (coordinates stripped — messages must not
    change when an unrelated earlier declaration renumbers the file).

- [ ] **Step 1: Write the failing key and linearization tests**

In `linearize.rs`'s test module:

```rust
#[test]
fn the_anonymous_key_round_trips_and_never_collides() {
    let ast_id = AstId { file: FileId::new(3), index: 7 };
    let key = anonymous_class_key(ast_id);
    assert_eq!(key, "class@anonymous:3:7");
    assert_eq!(parse_anonymous_class_key(&key), Some(ast_id));
    // Real folded keys never parse: the prefix is not a PHP name.
    assert_eq!(parse_anonymous_class_key("app\\kernel"), None);
    assert_eq!(parse_anonymous_class_key("class@anonymous:x:y"), None);
}

#[test]
fn an_anonymous_class_linearizes_by_its_synthetic_key() {
    let fixture = fixture(&[r#"<?php
function build(): void {
    $listener = new class {
        public function handle(): int { return 1; }
    };
}
"#]);
    // Numbering: function = 0, anonymous class = 1, method = 2.
    let key = anonymous_class_key(AstId { file: FileId::new(0), index: 1 });
    let linearized = linearized_class(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration,
        ClassQuery::new(&fixture.db, key),
    )
    .as_ref()
    .expect("the synthetic key resolves");
    assert!(linearized.members.iter().any(|member| member.key == "handle"));
}

#[test]
fn an_anonymous_class_inherits_through_its_heritage() {
    let fixture = fixture(&[r#"<?php
class Base { public function inherited(): int { return 1; } }
function build(): void {
    $listener = new class extends Base {};
}
"#]);
    // Numbering: Base = 0, its method = 1, build = 2, anonymous = 3.
    let key = anonymous_class_key(AstId { file: FileId::new(0), index: 3 });
    let resolution = lookup_member(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration,
        MemberQuery::new(&fixture.db, key, MemberKind::Method, "inherited".to_owned()),
    );
    assert!(matches!(
        resolution,
        Some(MemberResolution::Source { origin: MemberOrigin::Inherited, .. })
    ));
}
```

(Adjust the expected `AstId` indices to the numbering the existing
`ast_id.rs` tests pin — declaration nodes only, tree order — if a
fixture disagrees, fix the index in the test, never the numbering.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_semantics linearize -- anonymous`
Expected: FAIL — `anonymous_class_key` not found; then the
linearization test fails because `fetch` cannot resolve the key.

- [ ] **Step 3: Implement the key and the heritage projection**

1. In `members.rs`, add the fields to `ClassMembers` and fill them
   where the member tree lowers a class-like (read the heritage
   clause names exactly as the item-tree lowering reads
   `Declaration.extends`/`implements` — same AST accessors, written
   names, no resolution):

```rust
pub struct ClassMembers {
    // ... existing fields ...
    /// Written `extends` names, for the declaration-less anonymous
    /// case; named classes keep resolving heritage through their
    /// `Declaration`.
    pub extends: Vec<String>,
    /// Written `implements` names, same purpose.
    pub implements: Vec<String>,
}
```

2. In `linearize.rs`:

```rust
/// The synthetic folded key of an anonymous class. `@` and `:` are
/// illegal in PHP names, so the form can never collide with a real
/// folded key, and `folded_symbol_key` maps it to itself.
pub fn anonymous_class_key(ast_id: AstId) -> String {
    format!(
        "class@anonymous:{}:{}",
        ast_id.file.as_u32(),
        ast_id.index
    )
}

/// The inverse of [`anonymous_class_key`]; `None` for any real name.
pub fn parse_anonymous_class_key(key: &str) -> Option<AstId> {
    let rest = key.strip_prefix("class@anonymous:")?;
    let (file, index) = rest.split_once(':')?;
    Some(AstId {
        file: FileId::new(file.parse().ok()?),
        index: index.parse().ok()?,
    })
}
```

3. Branch `fetch` before the symbol lookup:

```rust
fn fetch(db: &dyn salsa::Database, files: AnalyzedFileSet, key: &str) -> Option<Fetched> {
    if let Some(ast_id) = parse_anonymous_class_key(key) {
        let file = file_of(db, files, ast_id.file)?;
        let group = member_tree(db, file)
            .classes
            .iter()
            .find(|group| group.ast_id == ast_id)?
            .clone();
        let namespace = group.namespace.clone();
        return Some(Fetched { group, declaration: None, file, namespace });
    }
    // ... the existing named path, unchanged ...
}
```

4. Where the walk derives edges from `fetched.declaration`, fall back
   to the group when the declaration is absent:

```rust
/// The inheritance edges of a declaration-less (anonymous) class,
/// from the member group's heritage projection: traits first, then
/// `extends`, then `implements` — the same precedence as `edges_of`.
fn edges_of_group(group: &ClassMembers) -> Vec<(AncestorRelation, String)> {
    let mut edges = Vec::new();
    for trait_use in &group.trait_uses {
        for name in &trait_use.names {
            edges.push((AncestorRelation::UsesTrait, name.clone()));
        }
    }
    for name in &group.extends {
        edges.push((AncestorRelation::Extends, name.clone()));
    }
    for name in &group.implements {
        edges.push((AncestorRelation::Implements, name.clone()));
    }
    edges
}
```

(If the existing walk already reads `group.trait_uses` for trait
edges rather than `declaration.trait_uses`, keep its spelling and
only add the extends/implements fallback — follow the shipped code.)

5. Export both functions from `lib.rs`.

- [ ] **Step 4: Run the semantics tests**

Run: `cargo test --package celerrate_semantics`
Expected: PASS, including the pre-existing invalidation-scope suite
(the heritage fields are member-tree data: an anonymous class's
`extends` edit invalidates like any member edit).

- [ ] **Step 5: Write the failing typed-side tests**

In `flow.rs`'s test module (its established fixture idiom):

```rust
#[test]
fn a_new_anonymous_expression_types_as_its_synthetic_class() {
    let fixture = fixture(&[r#"<?php
function build(): int {
    $listener = new class {
        public function handle(): int { return 1; }
    };
    return $listener->handle();
}
"#]);
    let inferred = inferred_body(&fixture, 0);
    assert_eq!(inferred.return_type.display(&fixture.db), "int");
}

#[test]
fn this_resolves_inside_an_anonymous_class_method() {
    let fixture = fixture(&[r#"<?php
$listener = new class {
    public function helper(): string { return 'x'; }
    public function handle(): string { return $this->helper(); }
};
"#]);
    // The handle body: owner is the anonymous class's synthetic key.
    let inferred = inferred_body(&fixture, /* handle's body index */ 2);
    assert_eq!(inferred.return_type.display(&fixture.db), "string");
}
```

(Reuse the module's existing `fixture`/`inferred_body` helpers —
plan 5 built them; spell the body indices per the numbering.)

And in `display.rs`'s tests:

```rust
#[test]
fn an_anonymous_class_displays_without_coordinates() {
    let db = TestDatabase::default();
    let anonymous = TypeId::class(&db, "class@anonymous:0:3", vec![]);
    assert_eq!(anonymous.display(&db), "class@anonymous");
}
```

- [ ] **Step 6: Run them to verify they fail**

Run: `cargo test --package celerrate_types flow -- anonymous`
Expected: FAIL — the `New` arm still answers `mixed`.

- [ ] **Step 7: Implement the typed side**

1. `flow.rs`, the combined
   `ClassReference::Anonymous { .. } | ClassReference::Missing` arm
   (find it by the variant names) — split it:

```rust
ClassReference::Anonymous { declaration } => TypeId::class(
    db,
    &anonymous_class_key(*declaration),
    vec![],
),
ClassReference::Missing => TypeId::mixed(db),
```

2. `inference.rs::body_owner` — the anonymous branch keys with the
   synthetic key instead of `None`:

```rust
let class_key = Some(match class.name.as_deref() {
    Some(name) => folded_symbol_key(
        SymbolSpace::ClassLike,
        &fully_qualified_name(&class.namespace, name),
    ),
    None => anonymous_class_key(class.ast_id),
});
```

   Update `BodyOwner`'s rustdoc: the `Option` stays (the type is
   shared), but the anonymous case now carries `Some`; reword the
   decision-12 reference to shipped behavior.

3. `display.rs` — where a `Class` name renders, strip the
   coordinates:

```rust
fn class_display_name(name: &str) -> &str {
    if name.starts_with("class@anonymous:") {
        "class@anonymous"
    } else {
        name
    }
}
```

- [ ] **Step 8: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS. The plan-6 ground-truth baseline may move (anonymous
receivers got precise); re-bless preserving classifications if so.

```bash
git add crates
git commit -m "✨ feat(semantics): anonymous classes resolve through their synthetic key"
```

---

### Task 2: The checks skeleton and the fifth producer

**Files:**
- Create: `crates/celerrate_types/src/checks/mod.rs`
- Modify: `crates/celerrate_types/Cargo.toml` (the
  `celerrate_diagnostics` dependency)
- Modify: `crates/celerrate_types/src/lib.rs` (module + re-exports)
- Modify: `crates/celerrate_diagnostics/src/registry.rs` (nine rows,
  the count 29 → 38)
- Modify: `crates/celerrate_cli/tests/registry.rs` (the fifth
  producer)
- Test: `checks/mod.rs` tests, the two registry suites

**Interfaces:**
- Consumes: `Diagnostic { id, severity, file, range, message }`
  (struct literal, no constructor), `DiagnosticId::new`,
  `Severity::Error`, `member_tree` (functions + non-trait class
  methods, filtered on `ClassMembers.kind` /
  `DeclarationKind::Trait`),
  `BodyQuery::new(db, ast_id)`, `body_ir`, `body_source_map`
  (`expression_pointer(id) -> Option<SyntaxNodePtr>`, then
  `.text_range()`), `inferred_body_types`,
  `InferredBody.edge_counts`, `SourceFile.file_id(db)`.
- Produces (later tasks and the CLI rely on these exact names):
  - the nine identifier consts (`UNKNOWN_METHOD` … 
    `UNKNOWN_NAMED_ARGUMENT`) and
    `celerrate_types::ALLOCATED_IDENTIFIERS`.
  - `pub struct TypedVerdict { pub body: AstId, pub expression:
    ExpressionId, pub kind: TypedVerdictKind }` (`Clone, Debug,
    PartialEq, Eq` — no `'db` lifetimes, so no `salsa::Update`: the
    `body_owner`/`lookup_member` precedent for tracked-query values;
    same for `TypedVerdictKind`, `ArgumentLabel`, and
    `TypedFileResult`).
  - `pub enum ArgumentLabel { Positional(usize), Named(String) }`
    (positions are 1-based in messages).
  - `pub enum TypedVerdictKind` with variants
    `UnknownMethod { member: String, receiver: String }`,
    `UnknownProperty { member: String, receiver: String }`,
    `UnknownClassConstant { member: String, receiver: String }`,
    `UnknownEnumCase { member: String, receiver: String }`,
    `NullDereference { member: String, receiver: String }`,
    `ArgumentType { label: ArgumentLabel, callee: String,
    expected: String, given: String }`,
    `TooFewArguments { callee: String, given: usize,
    required: usize }`,
    `TooManyArguments { callee: String, given: usize,
    accepted: usize }`,
    `UnknownNamedArgument { callee: String, name: String }` —
    all payloads pre-rendered `String`s, never `TypeId`s.
  - `TypedVerdictKind::identifier(&self) -> DiagnosticId` and
    `::message(&self) -> String`.
  - `pub struct TypedFileResult { pub verdicts: Vec<TypedVerdict>,
    pub bodies: u32, pub edge_counts: InterproceduralEdgeCounts }`
    (`Default`).
  - `pub fn typed_file_verdicts(db, files, stubs, configuration,
    file: SourceFile) -> TypedFileResult` (tracked, returns ref).
  - `pub fn typed_diagnostics(db, files, stubs, configuration,
    file: SourceFile) -> Vec<Diagnostic>` (tracked, returns ref).
  - `pub(crate) fn body_typed_verdicts(db, files, stubs,
    configuration, file: SourceFile, body: BodyQuery) ->
    Vec<TypedVerdict>` (tracked, returns ref) and
    `pub(crate) struct CheckContext<'db, 'body>` — the walker
    context tasks 4–9 fill in.

- [ ] **Step 1: Write the failing registry and message tests**

1. In `crates/celerrate_diagnostics/src/registry.rs`, extend the
   pinned count test's expectation:

```rust
// the_registry_is_sorted_unique_and_gapless: the trailing assertion
assert_eq!(previous, 38, "thirty-eight identifiers allocated so far");
```

2. In `crates/celerrate_cli/tests/registry.rs`, add the producer:

```rust
fn producers() -> Vec<(&'static str, &'static [DiagnosticId])> {
    vec![
        ("celerrate_db", celerrate_db::ALLOCATED_IDENTIFIERS),
        ("celerrate_syntax", celerrate_syntax::ALLOCATED_IDENTIFIERS),
        ("celerrate_semantics", celerrate_semantics::ALLOCATED_IDENTIFIERS),
        ("celerrate_project", celerrate_project::ALLOCATED_IDENTIFIERS),
        ("celerrate_types", celerrate_types::ALLOCATED_IDENTIFIERS),
    ]
}
```

3. In the new `checks/mod.rs` test module, the pure rendering table:

```rust
#[test]
fn every_kind_names_its_identifier_and_message() {
    let cases: Vec<(TypedVerdictKind, &str, &str)> = vec![
        (
            TypedVerdictKind::UnknownMethod {
                member: "save".to_owned(),
                receiver: "App\\User".to_owned(),
            },
            "CEL0030",
            "unknown method `save` on `App\\User`",
        ),
        (
            TypedVerdictKind::UnknownProperty {
                member: "name".to_owned(),
                receiver: "App\\User".to_owned(),
            },
            "CEL0031",
            "unknown property `$name` on `App\\User`",
        ),
        (
            TypedVerdictKind::UnknownClassConstant {
                member: "LIMIT".to_owned(),
                receiver: "App\\User".to_owned(),
            },
            "CEL0032",
            "unknown class constant `LIMIT` on `App\\User`",
        ),
        (
            TypedVerdictKind::UnknownEnumCase {
                member: "Draft".to_owned(),
                receiver: "App\\Status".to_owned(),
            },
            "CEL0033",
            "unknown enum case `Draft` on `App\\Status`",
        ),
        (
            TypedVerdictKind::NullDereference {
                member: "save".to_owned(),
                receiver: "App\\User|null".to_owned(),
            },
            "CEL0034",
            "accessing `save` on a possibly null `App\\User|null`",
        ),
        (
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(2),
                callee: "substr".to_owned(),
                expected: "int".to_owned(),
                given: "string".to_owned(),
            },
            "CEL0035",
            "argument 2 of `substr` expects `int`, `string` given",
        ),
        (
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Named("offset".to_owned()),
                callee: "substr".to_owned(),
                expected: "int".to_owned(),
                given: "string".to_owned(),
            },
            "CEL0035",
            "argument `$offset` of `substr` expects `int`, `string` given",
        ),
        (
            TypedVerdictKind::TooFewArguments {
                callee: "str_repeat".to_owned(),
                given: 1,
                required: 2,
            },
            "CEL0036",
            "too few arguments to `str_repeat`: 1 given, 2 required",
        ),
        (
            TypedVerdictKind::TooManyArguments {
                callee: "strlen".to_owned(),
                given: 2,
                accepted: 1,
            },
            "CEL0037",
            "too many arguments to `strlen`: 2 given, at most 1 accepted",
        ),
        (
            TypedVerdictKind::UnknownNamedArgument {
                callee: "str_repeat".to_owned(),
                name: "count".to_owned(),
            },
            "CEL0038",
            "unknown named argument `$count` on `str_repeat`",
        ),
    ];
    for (kind, id, message) in cases {
        assert_eq!(kind.identifier().as_str(), id);
        assert_eq!(kind.message(), message);
    }
}

#[test]
fn a_file_without_defects_produces_no_typed_diagnostics() {
    let fixture = fixture(&[r#"<?php
function greet(string $name): string { return "hello " . $name; }
"#]);
    let diagnostics = typed_diagnostics(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration,
        handle_of(&fixture, 0),
    );
    assert!(diagnostics.is_empty());
}
```

(Build the module's `fixture`/`handle_of` helpers on the plan-5
idiom already in `inference.rs`'s tests: `TestDatabase`,
`SourceFile::new`, `AnalyzedFileSet`, an empty `StubIndex`, the
8.1–8.5 configuration.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_diagnostics registry && cargo test --package celerrate_types checks`
Expected: FAIL — the count is still 29; the module does not exist.

- [ ] **Step 3: Implement the skeleton**

1. `crates/celerrate_types/Cargo.toml`:

```toml
celerrate_diagnostics = { path = "../celerrate_diagnostics" }
```

2. `checks/mod.rs` — the identifiers and the model:

```rust
//! The typed check families: unknown members, nullability, argument
//! types (design section 8). Verdicts are range-free — keyed by
//! `(AstId, ExpressionId)` — and reconcile to `TextRange` through the
//! body source map only at the `typed_diagnostics` layer, so an edit
//! above a body backdates every verdict and re-runs only the mapping.

pub(crate) mod arguments;
pub(crate) mod members;
pub(crate) mod nullability;
pub(crate) mod receivers;

/// Unknown method on the receiver's resolved type.
pub const UNKNOWN_METHOD: DiagnosticId = DiagnosticId::new("CEL0030");
/// Unknown property on the receiver's resolved type.
pub const UNKNOWN_PROPERTY: DiagnosticId = DiagnosticId::new("CEL0031");
/// Unknown class constant on the receiver's resolved type.
pub const UNKNOWN_CLASS_CONSTANT: DiagnosticId = DiagnosticId::new("CEL0032");
/// Unknown case on the receiver's resolved enum.
pub const UNKNOWN_ENUM_CASE: DiagnosticId = DiagnosticId::new("CEL0033");
/// Dereference of a possibly-null value.
pub const NULL_DEREFERENCE: DiagnosticId = DiagnosticId::new("CEL0034");
/// An argument fails assignability against its parameter.
pub const ARGUMENT_TYPE: DiagnosticId = DiagnosticId::new("CEL0035");
/// A required parameter is bound by no argument.
pub const TOO_FEW_ARGUMENTS: DiagnosticId = DiagnosticId::new("CEL0036");
/// More positional arguments than the signature accepts.
pub const TOO_MANY_ARGUMENTS: DiagnosticId = DiagnosticId::new("CEL0037");
/// A named argument matching no declared parameter.
pub const UNKNOWN_NAMED_ARGUMENT: DiagnosticId = DiagnosticId::new("CEL0038");

/// Every identifier this crate allocates, for the composition-root
/// registry test.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[
    UNKNOWN_METHOD,
    UNKNOWN_PROPERTY,
    UNKNOWN_CLASS_CONSTANT,
    UNKNOWN_ENUM_CASE,
    NULL_DEREFERENCE,
    ARGUMENT_TYPE,
    TOO_FEW_ARGUMENTS,
    TOO_MANY_ARGUMENTS,
    UNKNOWN_NAMED_ARGUMENT,
];
```

The model (same file):

```rust
/// How one argument is addressed in a message: by 1-based position
/// or by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentLabel {
    Positional(usize),
    Named(String),
}

/// One range-free finding: the body it lives in, the arena expression
/// it anchors to, and what went wrong. Payloads are pre-rendered
/// display strings so the record is plain `Eq` data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedVerdict {
    pub body: AstId,
    pub expression: ExpressionId,
    pub kind: TypedVerdictKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedVerdictKind {
    UnknownMethod { member: String, receiver: String },
    UnknownProperty { member: String, receiver: String },
    UnknownClassConstant { member: String, receiver: String },
    UnknownEnumCase { member: String, receiver: String },
    NullDereference { member: String, receiver: String },
    ArgumentType { label: ArgumentLabel, callee: String, expected: String, given: String },
    TooFewArguments { callee: String, given: usize, required: usize },
    TooManyArguments { callee: String, given: usize, accepted: usize },
    UnknownNamedArgument { callee: String, name: String },
}

impl TypedVerdictKind {
    /// The permanent identifier of this finding's family.
    pub fn identifier(&self) -> DiagnosticId {
        match self {
            Self::UnknownMethod { .. } => UNKNOWN_METHOD,
            Self::UnknownProperty { .. } => UNKNOWN_PROPERTY,
            Self::UnknownClassConstant { .. } => UNKNOWN_CLASS_CONSTANT,
            Self::UnknownEnumCase { .. } => UNKNOWN_ENUM_CASE,
            Self::NullDereference { .. } => NULL_DEREFERENCE,
            Self::ArgumentType { .. } => ARGUMENT_TYPE,
            Self::TooFewArguments { .. } => TOO_FEW_ARGUMENTS,
            Self::TooManyArguments { .. } => TOO_MANY_ARGUMENTS,
            Self::UnknownNamedArgument { .. } => UNKNOWN_NAMED_ARGUMENT,
        }
    }

    /// The one-sentence message, following the reference-check idiom.
    pub fn message(&self) -> String {
        match self {
            Self::UnknownMethod { member, receiver } => {
                format!("unknown method `{member}` on `{receiver}`")
            }
            Self::UnknownProperty { member, receiver } => {
                format!("unknown property `${member}` on `{receiver}`")
            }
            Self::UnknownClassConstant { member, receiver } => {
                format!("unknown class constant `{member}` on `{receiver}`")
            }
            Self::UnknownEnumCase { member, receiver } => {
                format!("unknown enum case `{member}` on `{receiver}`")
            }
            Self::NullDereference { member, receiver } => {
                format!("accessing `{member}` on a possibly null `{receiver}`")
            }
            Self::ArgumentType { label, callee, expected, given } => match label {
                ArgumentLabel::Positional(position) => format!(
                    "argument {position} of `{callee}` expects `{expected}`, `{given}` given"
                ),
                ArgumentLabel::Named(name) => format!(
                    "argument `${name}` of `{callee}` expects `{expected}`, `{given}` given"
                ),
            },
            Self::TooFewArguments { callee, given, required } => format!(
                "too few arguments to `{callee}`: {given} given, {required} required"
            ),
            Self::TooManyArguments { callee, given, accepted } => format!(
                "too many arguments to `{callee}`: {given} given, at most {accepted} accepted"
            ),
            Self::UnknownNamedArgument { callee, name } => {
                format!("unknown named argument `${name}` on `{callee}`")
            }
        }
    }
}

/// One file's typed findings plus the inference instrument the
/// orchestration layer aggregates (plan 5's decision 13 lands here).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TypedFileResult {
    pub verdicts: Vec<TypedVerdict>,
    pub bodies: u32,
    pub edge_counts: InterproceduralEdgeCounts,
}
```

The queries (same file; the walker context that tasks 4–9 consume):

```rust
/// Everything one body's walkers need, borrowed once. `namespace`
/// and `tables` mirror the `FlowContext` construction in
/// `inferred_body_types` (the owner's namespace, the file's use
/// tables) so scoped subjects (`Foo::bar()`) resolve written names
/// exactly as inference does.
pub(crate) struct CheckContext<'db, 'body> {
    pub db: &'db dyn salsa::Database,
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
    pub file: SourceFile,
    pub body: AstId,
    pub ir: &'body BodyIr,
    pub inferred: &'body InferredBody<'db>,
    pub owner: Option<&'body BodyOwner>,
    pub namespace: String,
    pub tables: UseTables,
}

/// The typed findings of one body. Tracked per body on purpose:
/// editing one body never re-checks its siblings (harness 2).
#[salsa::tracked(returns(ref))]
pub(crate) fn body_typed_verdicts<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Vec<TypedVerdict> {
    let Some(ir) = body_ir(db, file, body).as_ref() else {
        return Vec::new();
    };
    let Some(inferred) =
        inferred_body_types(db, files, stubs, configuration, file, body).as_ref()
    else {
        return Vec::new();
    };
    let owner = body_owner(db, file, body).as_ref();
    let namespace = match owner {
        Some(BodyOwner::Function(function)) => function.namespace.clone(),
        Some(BodyOwner::Method { namespace, .. }) => namespace.clone(),
        None => String::new(),
    };
    let context = CheckContext {
        db,
        files,
        stubs,
        configuration,
        file,
        body: body.ast_id(db),
        ir,
        inferred,
        owner,
        tables: UseTables::for_namespace(item_tree(db, file), &namespace),
        namespace,
    };
    let mut verdicts = Vec::new();
    members::check(&context, &mut verdicts);
    nullability::check(&context, &mut verdicts);
    arguments::check(&context, &mut verdicts);
    verdicts
}

/// The typed findings of one file: every body the member tree names
/// (free functions and methods of non-trait class-likes), in tree
/// order, plus the summed inference instrument. Trait-owned bodies
/// are skipped (decision 3: plan 6 analyzes them per using class;
/// checking one against the trait's own surface is a false-positive
/// class — task 13's ledger). Top-level statement code has no
/// member-tree body — if the shipped body IR exposes a file-level
/// body form, include it in this enumeration; otherwise it stays
/// unchecked, a recorded debt (task 13's ledger).
#[salsa::tracked(returns(ref))]
pub fn typed_file_verdicts<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
) -> TypedFileResult {
    let tree = member_tree(db, file);
    let mut result = TypedFileResult::default();
    let function_bodies = tree.functions.iter().map(|function| function.ast_id);
    let method_bodies = tree
        .classes
        .iter()
        .filter(|class| class.kind != DeclarationKind::Trait)
        .flat_map(|class| {
            class
                .members
                .iter()
                .filter(|member| member.kind == MemberKind::Method)
                .map(|member| member.ast_id)
        });
    for ast_id in function_bodies.chain(method_bodies) {
        let body = BodyQuery::new(db, ast_id);
        result.verdicts.extend(
            body_typed_verdicts(db, files, stubs, configuration, file, body)
                .iter()
                .cloned(),
        );
        if let Some(inferred) =
            inferred_body_types(db, files, stubs, configuration, file, body).as_ref()
        {
            result.bodies += 1;
            result.edge_counts.accumulate(&inferred.edge_counts);
        }
    }
    result
}

/// Verdicts reconciled to offsets: the only layer where arena indices
/// meet `TextRange`. A verdict whose pointer is gone is dropped —
/// never a panic (the map and the verdicts move together on any edit
/// that could orphan one).
#[salsa::tracked(returns(ref))]
pub fn typed_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
) -> Vec<Diagnostic> {
    let result = typed_file_verdicts(db, files, stubs, configuration, file);
    let file_id = file.file_id(db);
    let mut diagnostics: Vec<Diagnostic> = result
        .verdicts
        .iter()
        .filter_map(|verdict| {
            let map =
                body_source_map(db, file, BodyQuery::new(db, verdict.body)).as_ref()?;
            let pointer = map.expression_pointer(verdict.expression)?;
            Some(Diagnostic {
                id: verdict.kind.identifier(),
                severity: Severity::Error,
                file: file_id,
                range: pointer.text_range(),
                message: verdict.kind.message(),
            })
        })
        .collect();
    diagnostics.sort();
    diagnostics
}
```

Add `InterproceduralEdgeCounts::accumulate` in `inference.rs`:

```rust
impl InterproceduralEdgeCounts {
    /// Sums another body's counts into this one, saturating.
    pub fn accumulate(&mut self, other: &Self) {
        self.declared_return_edges =
            self.declared_return_edges.saturating_add(other.declared_return_edges);
        self.inferred_return_edges =
            self.inferred_return_edges.saturating_add(other.inferred_return_edges);
        self.provider_edges = self.provider_edges.saturating_add(other.provider_edges);
    }
}
```

The three family modules land as empty walkers for now:

```rust
// checks/members.rs, checks/nullability.rs, checks/arguments.rs
pub(crate) fn check(_context: &CheckContext<'_, '_>, _verdicts: &mut Vec<TypedVerdict>) {}
```

and `checks/receivers.rs` as an empty module with the task-3 doc.

3. `registry.rs` — the nine rows, in order, owner `celerrate_types`,
   through the module's `registered(...)` const helper (the shipped
   style):

```rust
registered("CEL0030", "unknown method", "celerrate_types"),
registered("CEL0031", "unknown property", "celerrate_types"),
registered("CEL0032", "unknown class constant", "celerrate_types"),
registered("CEL0033", "unknown enum case", "celerrate_types"),
registered("CEL0034", "possibly null dereference", "celerrate_types"),
registered("CEL0035", "argument type mismatch", "celerrate_types"),
registered("CEL0036", "too few arguments", "celerrate_types"),
registered("CEL0037", "too many arguments", "celerrate_types"),
registered("CEL0038", "unknown named argument", "celerrate_types"),
```

4. `lib.rs` re-exports:

```rust
pub mod checks;
pub use checks::{
    ALLOCATED_IDENTIFIERS, ArgumentLabel, TypedFileResult, TypedVerdict, TypedVerdictKind,
    typed_diagnostics, typed_file_verdicts,
};
```

- [ ] **Step 4: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo xtask dependency-shape`
Expected: PASS — the registry suites see the fifth producer, the
empty walkers produce nothing, the new edge points down.

```bash
git add crates
git commit -m "✨ feat(types): the typed check skeleton allocates CEL0030-CEL0038"
```

---

### Task 3: The receiver surface

The shared foundation of the three families: placeholder resolution,
decomposition, the ternary existence judgment, and the per-kind
suppression rules — decision 4's reduction, decision 5's silent
atoms, decision 6's magic table. Two decision-7 and decision-3
pieces land here because every family reads them: the implicit enum
surface and the written-spelling display.

**Files:**
- Create: `crates/celerrate_types/src/checks/receivers.rs` (replace
  the task-2 stub)
- Modify: `crates/celerrate_semantics/src/member_lookup.rs` (the
  `class_surface` helper)
- Modify: `crates/celerrate_semantics/src/linearize.rs` (the
  implicit enum edges, decision 7)
- Modify: `crates/celerrate_stubs/src/compiler/extract.rs` (the
  implicit stub-enum parents; reword the closed debt comment;
  regenerate the blob with `cargo xtask compile-stubs`)
- Modify: `crates/celerrate_types/src/display.rs` (the name-resolver
  threading; update the rendering-debt header comment)
- Modify: `crates/celerrate_types/src/construction.rs` and
  `crates/celerrate_types/src/lib.rs` (the two other rendering-debt
  comments reworded to shipped behavior)
- Modify: `crates/celerrate_semantics/src/lib.rs` (export)
- Test: `receivers.rs`, `member_lookup.rs`, `linearize.rs`, and
  `extract.rs` test modules

**Interfaces:**
- Consumes: `lookup_member`/`MemberQuery`/`MemberResolution`,
  `linearized_class` (`MagicMarkers`, `stub_ancestors`,
  `has_opaque_edge`, `cyclic`, `ancestry`), `folded_member_key`,
  `TypeId` interrogation (`constituents`, `intersectands`,
  `is_null`, `is_mixed`, `class_name`, `class_arguments`,
  `enum_case_parts`, `template_bound`, `data` for the placeholder
  variants — in-crate matching, the flow.rs precedent),
  `BodyOwner::Method { class_key, .. }`, `source_symbol_table` and
  `stub_symbol_table` (the written-spelling recovery),
  `StubClassSurface.parents` (the stub-compiler side of decision 7).
- Produces (tasks 4–6 rely on these exact names):
  - `celerrate_semantics::class_surface(db, files, stubs,
    configuration, key: &str) -> ClassSurface` with
    `pub enum ClassSurface { Source, Stub, Unknown }` — `Source`
    when the key names a source class-like (synthetic anonymous keys
    included), `Stub` when the compiled stub graph knows the key,
    `Unknown` otherwise. Lives in `member_lookup.rs` because that
    module owns the stub-table access.
  - `pub(crate) enum MemberExistence { Exists, PossiblyExists,
    Missing }` — `PossiblyExists` is always silence.
  - `pub(crate) enum ReceiverAtom { Class { key: String },
    Case { enum_key: String }, Null, Undecidable }`.
  - `pub(crate) fn atoms_of(context: &CheckContext, receiver:
    TypeId) -> Vec<ReceiverAtom>` — placeholders resolved against
    the owner, unions and intersections flattened (the reduction
    makes flattening sound: both rules read "Missing iff every part
    is Missing").
  - `pub(crate) fn member_existence(context: &CheckContext,
    receiver: TypeId, kind: MemberKind, member_name: &str,
    scoped: bool) -> MemberExistence` — the composed judgment.
  - `pub(crate) fn written_type_display(context: &CheckContext,
    of: TypeId) -> String` — renders like `TypeId::display` but
    class and enum names are recovered to their written spellings
    through `source_symbol_table(...).lookup(SymbolSpace::ClassLike,
    key)`'s `original` field, then
    `stub_symbol_table(...)`'s `StubSymbol.name`, falling back to
    the folded key; `class@anonymous:` keys keep their
    coordinate-stripped rendering (decision 3).
  - `pub(crate) fn receiver_display(context: &CheckContext,
    receiver: TypeId) -> String` — `without_null` first, then
    `written_type_display` (messages name the part the member was
    looked up on).
  - `pub(crate) fn class_kind(context: &CheckContext, key: &str) ->
    Option<DeclarationKind>` — the declaring group's kind (enum
    detection for `CEL0033`).
  - `pub(crate) fn is_enum_key(context: &CheckContext, key: &str) ->
    bool` — `class_kind` answers `Enum` for source keys, the stub
    symbol table's `StubSymbolKind::Enum` for stub keys (the
    `name`/`value` property rule of decision 7).

- [ ] **Step 1: Write the failing `class_surface` tests**

In `member_lookup.rs`'s test module (its existing fixture idiom, a
synthetic stub index with one class):

```rust
#[test]
fn class_surface_distinguishes_source_stub_and_unknown() {
    let fixture = fixture_with_stub_class(
        &["<?php class Own {}"],
        "DateTime",
    );
    let surface = |key: &str| {
        class_surface(
            &fixture.db, fixture.files, fixture.stubs, fixture.configuration, key,
        )
    };
    assert_eq!(surface("own"), ClassSurface::Source);
    assert_eq!(surface("datetime"), ClassSurface::Stub);
    assert_eq!(surface("app\\ghost"), ClassSurface::Unknown);
}
```

- [ ] **Step 2: Implement `class_surface`**

```rust
/// What kind of surface one folded class key has: a source
/// class-like (absence of a member is provable through
/// linearization), a compiled stub class (the stub graph is closed —
/// absence is provable there too), or nothing (the unknown-symbol
/// family already reported it; nothing typed may pile on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassSurface {
    Source,
    Stub,
    Unknown,
}

pub fn class_surface(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    key: &str,
) -> ClassSurface {
    let class = ClassQuery::new(db, key.to_owned());
    if linearized_class(db, files, stubs, configuration, class).is_some() {
        return ClassSurface::Source;
    }
    let table = stub_signature_table(db, stubs);
    if stub_class_exists(table, key) {
        return ClassSurface::Stub;
    }
    ClassSurface::Unknown
}
```

(`stub_class_exists` wraps whatever accessor `StubSignatureTable`
offers for "this key names a stub class-like" — the same table
`stub_member` reads; add the accessor to `index.rs` if none exists.)

- [ ] **Step 3: Write the failing receiver-surface tests**

In `receivers.rs`'s test module — table-driven over the ternary,
using the checks fixture from task 2 plus a `context_for(fixture,
body_index)` helper that builds a `CheckContext` the way
`body_typed_verdicts` does:

```rust
const SURFACE_SOURCES: &str = r#"<?php
class Plain { public function known(): int { return 1; } }
class Magic { public function __call(string $n, array $a): mixed {} }
class Getter { public function __get(string $n): mixed {} }
#[AllowDynamicProperties]
class Bag {}
class Opaque extends GhostBase {}
enum Status { case Active; }
function scene(Plain $p, Magic $m, Getter $g, Bag $b, Opaque $o, Status $s): void {}
"#;

#[test]
fn the_ternary_existence_judgment() {
    let fixture = fixture(&[SURFACE_SOURCES]);
    let context = context_for(&fixture, /* scene's body */ 8);
    let plain = TypeId::class(&fixture.db, "plain", vec![]);
    let magic = TypeId::class(&fixture.db, "magic", vec![]);
    let getter = TypeId::class(&fixture.db, "getter", vec![]);
    let bag = TypeId::class(&fixture.db, "bag", vec![]);
    let opaque = TypeId::class(&fixture.db, "opaque", vec![]);
    let ghost = TypeId::class(&fixture.db, "app\\ghost", vec![]);
    let mixed = TypeId::mixed(&fixture.db);
    let judge = |receiver, kind, name: &str| {
        member_existence(&context, receiver, kind, name, false)
    };
    use MemberExistence::*;
    use MemberKind::*;
    // A resolvable member exists; a missing one on a closed source
    // surface is provably missing.
    assert!(matches!(judge(plain, Method, "known"), Exists));
    assert!(matches!(judge(plain, Method, "ghost"), Missing));
    assert!(matches!(judge(plain, Property, "ghost"), Missing));
    // Magic suppression is per kind: __call silences methods only.
    assert!(matches!(judge(magic, Method, "anything"), PossiblyExists));
    assert!(matches!(judge(magic, Property, "anything"), Missing));
    // __get silences properties only.
    assert!(matches!(judge(getter, Property, "anything"), PossiblyExists));
    assert!(matches!(judge(getter, Method, "anything"), Missing));
    // #[AllowDynamicProperties] silences properties only.
    assert!(matches!(judge(bag, Property, "anything"), PossiblyExists));
    assert!(matches!(judge(bag, Method, "anything"), Missing));
    // An opaque inheritance edge silences every kind.
    assert!(matches!(judge(opaque, Method, "anything"), PossiblyExists));
    // Undecidable atoms are silence.
    assert!(matches!(judge(mixed, Method, "anything"), PossiblyExists));
    assert!(matches!(judge(ghost, Method, "anything"), PossiblyExists));
}

#[test]
fn unions_and_intersections_reduce_over_their_parts() {
    let fixture = fixture(&[SURFACE_SOURCES]);
    let context = context_for(&fixture, 8);
    let db = &fixture.db;
    let plain = TypeId::class(db, "plain", vec![]);
    let magic = TypeId::class(db, "magic", vec![]);
    let judge = |receiver, name: &str| {
        member_existence(&context, receiver, MemberKind::Method, name, false)
    };
    use MemberExistence::*;
    // Union: report only if missing on every non-null constituent.
    let with_null = TypeId::union(db, [plain, TypeId::null(db)]);
    assert!(matches!(judge(with_null, "ghost"), Missing));
    assert!(matches!(judge(with_null, "known"), Exists));
    let with_magic = TypeId::union(db, [plain, magic]);
    assert!(matches!(judge(with_magic, "ghost"), PossiblyExists));
    // Intersection: exists if any intersectand has it, suppressed if
    // any suppresses — the dual, same reduction.
    let narrowed = TypeId::intersection(db, [plain, magic]);
    assert!(matches!(judge(narrowed, "known"), Exists));
    assert!(matches!(judge(narrowed, "anything"), PossiblyExists));
    // A receiver that is only null: the nullability family's beat.
    assert!(matches!(judge(TypeId::null(db), "anything"), PossiblyExists));
}

#[test]
fn placeholders_resolve_against_the_owner() {
    let fixture = fixture(&[r#"<?php
class Base { public function up(): int { return 1; } }
class Child extends Base {
    public function probe(): void {}
}
"#]);
    let context = context_for(&fixture, /* probe's body */ 3);
    let db = &fixture.db;
    use MemberExistence::*;
    let judge = |receiver, name: &str| {
        member_existence(&context, receiver, MemberKind::Method, name, false)
    };
    assert!(matches!(judge(TypeId::static_placeholder(db), "probe"), Exists));
    assert!(matches!(judge(TypeId::static_placeholder(db), "up"), Exists));
    assert!(matches!(judge(TypeId::static_placeholder(db), "ghost"), Missing));
    assert!(matches!(judge(TypeId::parent_placeholder(db), "up"), Exists));
    assert!(matches!(judge(TypeId::self_placeholder(db), "ghost"), Missing));
}

#[test]
fn the_implicit_enum_surface_counts_as_existing() {
    // Decision 7: a synthetic stub index carrying `UnitEnum` (with
    // `cases`) and `BackedEnum` (with `from`, `tryFrom`) — mirror
    // the member-lookup synthetic-stub fixture idiom.
    let fixture = fixture_with_stub_enum_interfaces(&[r#"<?php
enum Status: string { case Active = 'active'; }
function scene(Status $s): void {}
"#]);
    let context = context_for(&fixture, /* scene's body */ 2);
    let status = TypeId::class(&fixture.db, "status", vec![]);
    use MemberExistence::*;
    // The implicit methods resolve through the synthesized edges.
    assert!(matches!(
        member_existence(&context, status, MemberKind::Method, "cases", true),
        Exists
    ));
    assert!(matches!(
        member_existence(&context, status, MemberKind::Method, "from", true),
        Exists
    ));
    // The engine-provided instance properties always exist on enums.
    assert!(matches!(
        member_existence(&context, status, MemberKind::Property, "value", false),
        Exists
    ));
    assert!(matches!(
        member_existence(&context, status, MemberKind::Property, "name", false),
        Exists
    ));
    // The surface stays closed: a genuine ghost still proves missing.
    assert!(matches!(
        member_existence(&context, status, MemberKind::Method, "ghost", true),
        Missing
    ));
}

#[test]
fn message_displays_recover_written_spellings() {
    let fixture = fixture(&[r#"<?php
namespace App;
class User {}
function scene(): void {}
"#]);
    let context = context_for(&fixture, /* scene's body */ 1);
    let db = &fixture.db;
    let user = TypeId::class(db, "app\\user", vec![]);
    assert_eq!(written_type_display(&context, user), "App\\User");
    let with_null = TypeId::union(db, [user, TypeId::null(db)]);
    assert_eq!(written_type_display(&context, with_null), "App\\User|null");
    // Nothing answers the key: the folded key is the fallback.
    let ghost = TypeId::class(db, "app\\ghost", vec![]);
    assert_eq!(written_type_display(&context, ghost), "app\\ghost");
    // Anonymous keys keep their coordinate-stripped rendering.
    let anonymous = TypeId::class(db, "class@anonymous:0:1", vec![]);
    assert_eq!(written_type_display(&context, anonymous), "class@anonymous");
}
```

(As everywhere: if a body index or a display disagrees with the
shipped numbering, fix the test's expectation, never the code. The
implicit-enum edges also need their own linearization pin in
`linearize.rs`'s test module — a source enum with the synthetic stub
index linearizes with resolved `\UnitEnum`/`\BackedEnum` stub edges
and **no** opaque edge — and the stub-compiler side needs an
`extract.rs` pin: a stub enum's `StubClassSurface.parents` carries
the implicit parents. Write both failing first, in this step.)

- [ ] **Step 4: Run them to verify they fail**

Run: `cargo test --package celerrate_types checks::receivers`
Expected: FAIL — the module is the task-2 stub.

- [ ] **Step 5: Implement the surface**

```rust
//! The shared receiver surface (design sections 2 and 8): decompose
//! a receiver into atoms, resolve placeholders against the owner,
//! judge member existence ternarily. `PossiblyExists` is always
//! silence — the guillotine's currency. The union rule (missing on
//! all non-null constituents) and the intersection dual (exists on
//! any intersectand, suppressed by any) coincide in the reduction
//! "Missing iff every part is Missing", which is why flattening both
//! into one atom list is sound.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberExistence {
    Exists,
    PossiblyExists,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReceiverAtom {
    Class { key: String },
    Case { enum_key: String },
    Null,
    Undecidable,
}

/// Decomposes a receiver into atoms. Placeholders resolve against
/// the owner (`self`/`static` → the owner class, `parent` → its
/// first `Extends` ancestor); unions and intersections flatten;
/// every silent form of decision 5 lands on `Undecidable`.
pub(crate) fn atoms_of(
    context: &CheckContext<'_, '_>,
    receiver: TypeId<'_>,
) -> Vec<ReceiverAtom> {
    let db = context.db;
    if receiver.is_null(db) {
        return vec![ReceiverAtom::Null];
    }
    let parts = receiver.constituents(db);
    if parts.len() > 1 {
        return parts
            .into_iter()
            .flat_map(|part| atoms_of(context, part))
            .collect();
    }
    let intersectands = receiver.intersectands(db);
    if intersectands.len() > 1 {
        return intersectands
            .into_iter()
            .flat_map(|part| atoms_of(context, part))
            .collect();
    }
    if let Some((enum_name, _)) = receiver.enum_case_parts(db) {
        return vec![ReceiverAtom::Case { enum_key: enum_name }];
    }
    if receiver.is_mixed(db) {
        return vec![ReceiverAtom::Undecidable];
    }
    if let Some(resolved) = resolve_placeholder(context, receiver) {
        return resolved;
    }
    match receiver.class_name(db) {
        // `object` has no key; `class_name` answers `None` for it —
        // like every scalar, array, callable, template, and
        // class-string: all `Undecidable` by falling through.
        Some(key) => vec![ReceiverAtom::Class { key }],
        None => vec![ReceiverAtom::Undecidable],
    }
}

/// `self`/`static` → the owner class key; `parent` → the owner's
/// first `Extends` ancestor; `None` when the receiver is no
/// placeholder. An owner-less body (a free function) or an
/// unresolvable parent answers `Undecidable`.
fn resolve_placeholder(
    context: &CheckContext<'_, '_>,
    receiver: TypeId<'_>,
) -> Option<Vec<ReceiverAtom>> {
    use crate::representation::TypeData;
    let owner_key = || match context.owner {
        Some(BodyOwner::Method { class_key: Some(key), .. }) => {
            Some(ReceiverAtom::Class { key: key.clone() })
        }
        _ => None,
    };
    match receiver.data(context.db) {
        TypeData::SelfPlaceholder | TypeData::StaticPlaceholder => {
            Some(vec![owner_key().unwrap_or(ReceiverAtom::Undecidable)])
        }
        TypeData::ParentPlaceholder => {
            let parent = match context.owner {
                Some(BodyOwner::Method { class_key: Some(key), .. }) => {
                    parent_key(context, key)
                }
                _ => None,
            };
            Some(vec![match parent {
                Some(key) => ReceiverAtom::Class { key },
                None => ReceiverAtom::Undecidable,
            }])
        }
        _ => None,
    }
}

/// The owner's first resolved `Extends` ancestor, if any.
fn parent_key(context: &CheckContext<'_, '_>, class_key: &str) -> Option<String> {
    let linearized = linearized_class(
        context.db,
        context.files,
        context.stubs,
        context.configuration,
        ClassQuery::new(context.db, class_key.to_owned()),
    )
    .as_ref()?;
    linearized
        .ancestry
        .iter()
        .find(|edge| edge.relation == AncestorRelation::Extends)
        .and_then(|edge| edge.resolved.clone().or_else(|| edge.stub.clone()))
}

/// The ternary judgment of one atomic class constituent.
fn atom_existence(
    context: &CheckContext<'_, '_>,
    key: &str,
    kind: MemberKind,
    member_name: &str,
    scoped: bool,
) -> MemberExistence {
    let db = context.db;
    let lookup = |kind: MemberKind, name: &str| {
        lookup_member(
            db,
            context.files,
            context.stubs,
            context.configuration,
            MemberQuery::new(db, key.to_owned(), kind, folded_member_key(kind, name)),
        )
    };
    if lookup(kind, member_name).is_some() {
        return MemberExistence::Exists;
    }
    // The engine-provided enum instance properties (decision 7):
    // interfaces cannot declare properties, so no stub can ever
    // carry `name`/`value` — they exist on every enum by fiat.
    if kind == MemberKind::Property
        && (member_name == "name" || member_name == "value")
        && is_enum_key(context, key)
    {
        return MemberExistence::Exists;
    }
    // The surface decides whether absence is provable at all.
    match class_surface(db, context.files, context.stubs, context.configuration, key) {
        ClassSurface::Unknown => return MemberExistence::PossiblyExists,
        ClassSurface::Source => {
            let linearized = linearized_class(
                db,
                context.files,
                context.stubs,
                context.configuration,
                ClassQuery::new(db, key.to_owned()),
            );
            let Some(linearized) = linearized.as_ref() else {
                return MemberExistence::PossiblyExists;
            };
            if linearized.has_opaque_edge || linearized.cyclic {
                return MemberExistence::PossiblyExists;
            }
            if kind == MemberKind::Property
                && (linearized.magic.allows_dynamic_properties
                    || linearized.stub_ancestors.iter().any(|s| s == "stdclass"))
            {
                return MemberExistence::PossiblyExists;
            }
        }
        ClassSurface::Stub => {}
    }
    if kind == MemberKind::Property && key == "stdclass" {
        return MemberExistence::PossiblyExists;
    }
    // Magic, per kind, uniformly through `lookup_member` (which walks
    // source linearization and the stub graph alike). Scoped calls
    // consult both call interceptors: `self::m()` may target an
    // instance method — over-suppression is the conservative side.
    let magic_names: &[&str] = match kind {
        MemberKind::Method if scoped => &["__call", "__callstatic"],
        MemberKind::Method => &["__call"],
        MemberKind::Property => &["__get", "__set"],
        MemberKind::ClassConstant | MemberKind::EnumCase => &[],
    };
    for magic in magic_names {
        if lookup(MemberKind::Method, magic).is_some() {
            return MemberExistence::PossiblyExists;
        }
    }
    MemberExistence::Missing
}

/// The composed judgment: `Missing` iff every non-null atom is
/// `Missing`; any `Exists` wins; a receiver with no decidable atom
/// (mixed, only-null, scalars…) is `PossiblyExists`.
pub(crate) fn member_existence(
    context: &CheckContext<'_, '_>,
    receiver: TypeId<'_>,
    kind: MemberKind,
    member_name: &str,
    scoped: bool,
) -> MemberExistence {
    let mut decidable = 0usize;
    let mut all_missing = true;
    let mut any_exists = false;
    for atom in atoms_of(context, receiver) {
        let verdict = match &atom {
            ReceiverAtom::Null => continue,
            ReceiverAtom::Undecidable => MemberExistence::PossiblyExists,
            ReceiverAtom::Class { key } => {
                atom_existence(context, key, kind, member_name, scoped)
            }
            ReceiverAtom::Case { enum_key } => {
                atom_existence(context, enum_key, kind, member_name, scoped)
            }
        };
        decidable += 1;
        match verdict {
            MemberExistence::Exists => any_exists = true,
            MemberExistence::PossiblyExists => all_missing = false,
            MemberExistence::Missing => {}
        }
        if verdict != MemberExistence::Missing {
            all_missing = false;
        }
    }
    if any_exists {
        MemberExistence::Exists
    } else if decidable > 0 && all_missing {
        MemberExistence::Missing
    } else {
        MemberExistence::PossiblyExists
    }
}

/// The message spelling of a receiver: the non-null part it was
/// looked up on.
pub(crate) fn receiver_display(
    context: &CheckContext<'_, '_>,
    receiver: TypeId<'_>,
) -> String {
    let stripped = receiver.without_null(context.db);
    if stripped.is_never(context.db) {
        written_type_display(context, receiver)
    } else {
        written_type_display(context, stripped)
    }
}

/// `TypeId::display` with written spellings (decision 3): class and
/// enum names map through the symbol index — the source table's
/// `original`, then the stub table's `StubSymbol.name` — and fall
/// back to the folded key when nothing answers. Anonymous keys keep
/// the coordinate-stripped `class@anonymous` rendering.
pub(crate) fn written_type_display(
    context: &CheckContext<'_, '_>,
    of: TypeId<'_>,
) -> String {
    let resolve = |key: &str| -> Option<String> {
        if key.starts_with("class@anonymous:") {
            return None; // display.rs's stripping rule applies.
        }
        if let Some(entry) = source_symbol_table(context.db, context.files)
            .lookup(SymbolSpace::ClassLike, key)
        {
            return Some(entry.original.clone());
        }
        stub_symbol_table(context.db, context.stubs, context.configuration)
            .lookup(SymbolSpace::ClassLike, key)
            .map(|entry| entry.symbol.name.clone())
    };
    of.display_with_names(context.db, &resolve)
}

/// The declaring group's kind, for enum detection.
pub(crate) fn class_kind(
    context: &CheckContext<'_, '_>,
    key: &str,
) -> Option<DeclarationKind> {
    // The member group carries the kind; reuse the same lookup path
    // `linearize::fetch` uses (source classes only — stub enums
    // answer `None` and fall to the class-constant identifier, the
    // conservative direction).
    class_members_of(context.db, context.files, key).map(|group| group.kind)
}
```

(`class_members_of` is whatever small helper reaches the
`ClassMembers` group by folded key — extract it from `fetch`'s
existing body in `linearize.rs` and re-export `pub(crate)`-to-public
as needed; do not duplicate the lookup logic. Note the double
bookkeeping in the fold above: keep the code minimal — `all_missing`
already covers the `Exists` case; simplify while keeping the tests
green. Spell `source_symbol_table`/`stub_symbol_table` exactly as
the shipped signatures demand.)

Then the two decision-7 seams and the decision-3 display threading:

1. `linearize.rs`: where the walk derives a class-like's edges, when
   the group's kind is `DeclarationKind::Enum`, append synthetic
   `Implements` edges written `\UnitEnum` (and `\BackedEnum` when
   the enum declares a backing type; if the member projection lacks
   that fact, append both — over-suppression is the conservative
   direction). Spell the written names absolutely so namespace
   resolution cannot capture them, and append an edge **only when
   the compiled stub graph knows the parent key** — a stub set
   without `UnitEnum` adds nothing, never a synthetic opaque edge
   (which would blanket-silence enums in stub-less fixtures).
2. `extract.rs`: the stub compiler's enum arm pushes `UnitEnum` (and
   `BackedEnum` for backed stub enums) into
   `StubClassSurface.parents`; reword the closed debt comment
   ("Implicit `UnitEnum`/`BackedEnum` parents are not synthesized
   here...") to shipped behavior; run
   `cargo xtask compile-stubs` and commit the regenerated blob.
3. `display.rs`: thread an optional name resolver
   (`Option<&dyn Fn(&str) -> Option<String>>`) through
   `display_type`'s recursion at the `Class` and enum-case arms;
   `TypeId::display` passes `None` and stays byte-identical;
   `TypeId::display_with_names(db, resolve)` is the new entry point
   (`pub(crate)` — decision 17: no new public API beyond the
   checks). Update the three rendering-debt comments to shipped
   behavior in the same change: the `display.rs` header, the
   `TypeId::class` rustdoc in `construction.rs`, and the "Rendering
   debt" paragraph in `lib.rs` (the checks layer now recovers
   written spellings; the lattice-internal `display` stays folded on
   purpose).

- [ ] **Step 6: Run the tests, then the full gate, and commit**

Run: `cargo test --package celerrate_types checks && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates
git commit -m "✨ feat(types): the ternary receiver surface with per-kind suppression"
```

---

### Task 4: Unknown methods (CEL0030)

**Files:**
- Modify: `crates/celerrate_types/src/checks/members.rs`
- Modify: `crates/celerrate_semantics/src/body.rs`
  (`ExpressionId::from_index` visibility) and its `lib.rs` export
- Test: its test module

**Interfaces:**
- Consumes: task 3's surface, `BodyExpression::{Call,
  CallableReference, MemberAccess, ScopedAccess}`,
  `MemberReference::Named`, `InferredBody::expression_type`,
  `resolve_name`/`SymbolSources` (the `reference_checks.rs`
  spelling) for scoped subjects.
- Produces: `members::check(context, verdicts)` emits
  `UnknownMethod` verdicts; and two `pub(crate)` helpers tasks 5 and
  8 reuse —
  - `called_member_accesses(ir: &BodyIr) -> HashSet<ExpressionId>`:
    every expression that is the `callee` of a `Call` or
    `CallableReference` (so task 5 can treat the remaining
    `MemberAccess` as property reads, and this task the called ones
    as methods).
  - `scoped_subject_keys(context, subject: ExpressionId) ->
    Option<Vec<String>>`: the folded class keys of a scoped subject —
    `NamedReference` texts `self`/`static` resolve to the owner,
    `parent` to its parent, any other name through
    `resolve_name(SymbolSpace::ClassLike)` in the body's namespace
    and use tables (`None` when unknown — `CEL0018`'s beat, silence
    here); a non-`NamedReference` subject (a variable, an
    expression) answers `None` (decision 5: dynamic subjects are
    silent).

- [ ] **Step 1: Write the failing tests**

```rust
fn method_verdicts(source: &str) -> Vec<TypedVerdictKind> {
    let fixture = fixture(&[source]);
    typed_file_verdicts(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration,
        handle_of(&fixture, 0),
    )
    .verdicts
    .iter()
    .map(|verdict| verdict.kind.clone())
    .collect()
}

#[test]
fn an_unknown_instance_method_reports() {
    let verdicts = method_verdicts(r#"<?php
class User { public function save(): void {} }
function f(User $u): void { $u->svae(); }
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::UnknownMethod {
            member: "svae".to_owned(),
            receiver: "User".to_owned(),
        }],
    );
}

#[test]
fn known_inherited_virtual_and_magic_receivers_are_silent() {
    let verdicts = method_verdicts(r#"<?php
class Base { public function up(): void {} }
/** @method void annotated() */
class User extends Base {
    public function save(): void {}
    public function all(): void {
        $this->save();
        $this->up();
        $this->annotated();
        static::save();
        parent::up();
        self::save();
    }
}
class Magic { public function __call(string $n, array $a): mixed {} }
function f(User $u, Magic $m, mixed $x): void {
    $u->save();
    $u->up();
    $u->annotated();
    $m->whatever();
    $x->anything();
    $u->save(...);
}
"#);
    assert_eq!(verdicts, vec![]);
}

#[test]
fn union_receivers_report_only_when_missing_everywhere() {
    let verdicts = method_verdicts(r#"<?php
class A { public function shared(): void {} public function onlyA(): void {} }
class B { public function shared(): void {} }
function f(A|B $either, ?A $nullable): void {
    $either->shared();
    $either->onlyA();      // possibly undefined: future family, silent
    $either->nowhere();    // missing on both: reports
    $nullable->nowhere();  // missing on the non-null part: reports
}
"#);
    assert_eq!(
        verdicts,
        vec![
            TypedVerdictKind::UnknownMethod {
                member: "nowhere".to_owned(),
                receiver: "A|B".to_owned(),
            },
            TypedVerdictKind::UnknownMethod {
                member: "nowhere".to_owned(),
                receiver: "A".to_owned(),
            },
        ],
    );
}

// Signpost: task 6's nullability walker will additionally report a
// `NullDereference` for `$nullable->nowhere()` above (the receiver
// carries null); task 6 extends this expectation when it lands —
// that update is expected, not a regression.

#[test]
fn trait_owned_bodies_are_not_checked() {
    // Decision 3: plan 6 analyzes trait bodies per using class;
    // judged against the trait's own surface, this call would be a
    // false positive.
    let verdicts = method_verdicts(r#"<?php
trait Caching {
    public function warm(): void { $this->providedByTheUsingClass(); }
}
"#);
    assert_eq!(verdicts, vec![]);
}

#[test]
fn scoped_calls_resolve_their_subject_symbolically() {
    let verdicts = method_verdicts(r#"<?php
class Tool { public static function make(): static { return new static(); } }
function f(string $class): void {
    Tool::make();
    Tool::nowhere();
    Ghost::anything();     // unknown class: CEL0018's beat, silent here
    $class::dynamic();     // dynamic subject: silent
    Tool::class;           // the ::class constant is never a member
}
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::UnknownMethod {
            member: "nowhere".to_owned(),
            receiver: "Tool".to_owned(),
        }],
    );
}

#[test]
fn an_anonymous_class_receiver_is_checked() {
    let verdicts = method_verdicts(r#"<?php
function f(): void {
    $listener = new class { public function handle(): void {} };
    $listener->handle();
    $listener->nowhere();
}
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::UnknownMethod {
            member: "nowhere".to_owned(),
            receiver: "class@anonymous".to_owned(),
        }],
    );
}
```

(The `receiver` display strings follow task 3's
`written_type_display` — written spellings recovered through the
symbol index, so `A`, not the folded `a`; if the shipped rendering
spells `A|B` differently, fix the expectation.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_types checks::members`
Expected: FAIL — the walker is empty.

- [ ] **Step 3: Implement the method walk**

```rust
//! The unknown-members family, method kind (CEL0030): every called
//! member access — `$x->m()`, `Foo::m()`, `$x->m(...)` — judged
//! through the ternary receiver surface. `Missing` reports;
//! everything else is silence.

pub(crate) fn check(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
) {
    let called = called_member_accesses(context.ir);
    for (index, expression) in context.ir.expressions.iter().enumerate() {
        let Some(id) = ExpressionId::from_index(index) else { continue };
        match expression {
            BodyExpression::MemberAccess {
                receiver,
                member: MemberReference::Named { name },
                ..
            } if called.contains(&id) => {
                let Some(receiver_type) = context.inferred.expression_type(*receiver)
                else {
                    continue;
                };
                if member_existence(
                    context, receiver_type, MemberKind::Method, name, false,
                ) == MemberExistence::Missing
                {
                    verdicts.push(TypedVerdict {
                        body: context.body,
                        expression: id,
                        kind: TypedVerdictKind::UnknownMethod {
                            member: name.clone(),
                            receiver: receiver_display(context, receiver_type),
                        },
                    });
                }
            }
            BodyExpression::ScopedAccess {
                subject,
                member: MemberReference::Named { name },
            } if called.contains(&id) => {
                check_scoped_method(context, verdicts, id, *subject, name);
            }
            _ => {}
        }
    }
}

/// Every expression consumed as a callee: `Call.callee` and
/// `CallableReference.callee`.
pub(crate) fn called_member_accesses(ir: &BodyIr) -> HashSet<ExpressionId> {
    ir.expressions
        .iter()
        .filter_map(|expression| match expression {
            BodyExpression::Call { callee, .. }
            | BodyExpression::CallableReference { callee } => Some(*callee),
            _ => None,
        })
        .collect()
}

fn check_scoped_method(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    id: ExpressionId,
    subject: ExpressionId,
    name: &str,
) {
    let Some(keys) = scoped_subject_keys(context, subject) else {
        return;
    };
    let all_missing = !keys.is_empty()
        && keys.iter().all(|key| {
            atom_existence(context, key, MemberKind::Method, name, true)
                == MemberExistence::Missing
        });
    if all_missing {
        let Some(first) = keys.first() else { return };
        verdicts.push(TypedVerdict {
            body: context.body,
            expression: id,
            kind: TypedVerdictKind::UnknownMethod {
                member: name.to_owned(),
                receiver: written_class_display(context, subject, first),
            },
        });
    }
}
```

Notes for the implementer:

- `ExpressionId::from_index` already exists in `body.rs` but is
  `pub(crate)` to `celerrate_semantics`: widen it to `pub` and
  export it so the walkers can construct ids from enumeration
  indices; never cast with `as`.
- `atom_existence` is task 3's private function made `pub(crate)` —
  scoped subjects are already keys, no decomposition needed.
- `scoped_subject_keys`: read the subject expression; a
  `NamedReference { text }` whose lowered text is `self` or
  `static` answers the owner key, `parent` the owner's parent (task
  3's `parent_key`), anything else resolves through `resolve_name`
  with `SymbolSpace::ClassLike`, the context's namespace and tables
  (mirror the `reference_checks.rs` spelling for `SymbolSources`);
  unresolved →
  `None`. Anything not a `NamedReference` → `None`.
- `written_class_display`: the subject's written text as the message
  receiver (`Tool`, not the folded `tool`); fall back to the folded
  key if the written text is unavailable.
- The `::class` guard: a scoped member named `class`
  (case-insensitive) is never checked — it is PHP syntax, not a
  member. Place the guard where the scoped arms match, both here
  and in task 5.

- [ ] **Step 4: Run the tests, the full gate, and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS, including the anti-false-positive smoke suite in
`celerrate_semantics/tests/false_positives.rs` (its sources exercise
inheritance and traits; a failure there is a bug in this walk).

```bash
git add crates
git commit -m "✨ feat(types): unknown methods report through the receiver surface"
```

---

### Task 5: Unknown properties, class constants, enum cases (CEL0031–CEL0033)

**Files:**
- Modify: `crates/celerrate_types/src/checks/members.rs`
- Test: its test module

**Interfaces:**
- Consumes: task 4's walk and helpers, `class_kind` (task 3),
  `MemberReference::Variable` (static properties),
  `DeclarationKind::Enum`.
- Produces: `members::check` additionally emits `UnknownProperty`,
  `UnknownClassConstant`, `UnknownEnumCase`. No new names.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn unknown_properties_report_with_their_suppressions() {
    let verdicts = method_verdicts(r#"<?php
class User { public string $name = ''; }
class Getter { public function __get(string $n): mixed {} }
#[AllowDynamicProperties]
class Bag {}
/** @property string $virtual */
class Annotated {}
function f(User $u, Getter $g, Bag $b, Annotated $a): void {
    $u->name;
    $u->nmae;          // reports
    $u->nmae = 'x';    // the write position is the same node kind
    $g->anything;
    $b->anything;
    $a->virtual;
}
"#);
    assert_eq!(
        verdicts,
        vec![
            TypedVerdictKind::UnknownProperty {
                member: "nmae".to_owned(),
                receiver: "User".to_owned(),
            },
            TypedVerdictKind::UnknownProperty {
                member: "nmae".to_owned(),
                receiver: "User".to_owned(),
            },
        ],
    );
}

#[test]
fn stdclass_descendants_accept_dynamic_properties() {
    // `json_decode` alone makes this mandatory on any real corpus
    // (design section 2). `method_verdicts_with_stub_stdclass` is
    // the task-4 helper over a fixture whose stub input is a
    // synthetic `StubIndex::from_symbols` carrying a `stdClass`
    // class — mirror the synthetic-stub fixtures the member-lookup
    // tests already build.
    let verdicts = method_verdicts_with_stub_stdclass(r#"<?php
class Payload extends \stdClass {}
function f(Payload $p, \stdClass $raw): void {
    $p->anything;
    $raw->anything;
}
"#);
    assert_eq!(verdicts, vec![]);
}

#[test]
fn static_properties_constants_and_cases_report() {
    let verdicts = method_verdicts(r#"<?php
class Config {
    public static int $limit = 10;
    public const RETRIES = 3;
}
enum Status: string {
    case Active = 'active';
    public const DEFAULT = self::Active;
}
function f(): void {
    Config::$limit;
    Config::$limti;        // reports CEL0031
    Config::RETRIES;
    Config::MISSING;       // reports CEL0032
    Status::Active;
    Status::DEFAULT;       // constants on enums resolve
    Status::Draft;         // reports CEL0033
    Config::class;         // never a member
    Status::Active->value; // enum-case receiver: backing property
}
"#);
    assert_eq!(
        verdicts,
        vec![
            TypedVerdictKind::UnknownProperty {
                member: "limti".to_owned(),
                receiver: "Config".to_owned(),
            },
            TypedVerdictKind::UnknownClassConstant {
                member: "MISSING".to_owned(),
                receiver: "Config".to_owned(),
            },
            TypedVerdictKind::UnknownEnumCase {
                member: "Draft".to_owned(),
                receiver: "Status".to_owned(),
            },
        ],
    );
}
```

(`Status::Active->value` stays silent through task 3's decision-7
rule: `name`/`value` always exist on enum-keyed receivers. Note the
plain fixture above has no stub index, so the implicit
`UnitEnum`/`BackedEnum` edges are not synthesized there — decision
7's stub-known guard — and `Status::Draft` still proves missing.)

And the implicit enum surface end to end through the family walk
(`method_verdicts_with_stub_enum_interfaces` builds on the same
synthetic-stub idiom as the stdClass helper above, carrying
`UnitEnum` and `BackedEnum` interface surfaces):

```rust
#[test]
fn the_implicit_enum_surface_is_silent_and_its_ghosts_still_report() {
    let verdicts = method_verdicts_with_stub_enum_interfaces(r#"<?php
enum Status: string { case Active = 'active'; }
function f(Status $s): void {
    Status::cases();
    Status::from('active');
    Status::tryFrom('x');
    $s->value;
    $s->name;
    Status::ghost();       // the surface stays closed: reports
}
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::UnknownMethod {
            member: "ghost".to_owned(),
            receiver: "Status".to_owned(),
        }],
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_types checks::members`
Expected: FAIL — only methods report so far.

- [ ] **Step 3: Implement the remaining kinds**

Extend the task-4 walk's match:

1. `MemberAccess` with a `Named` member **not** in `called` →
   property read/write. Same shape as the method arm with
   `MemberKind::Property` and `UnknownProperty`.
2. `ScopedAccess` with a `Variable { name }` member → static
   property: `scoped_subject_keys`, `MemberKind::Property`,
   `UnknownProperty`.
3. `ScopedAccess` with a `Named { name }` member **not** in `called`
   and not the `::class` keyword → constant-or-case, the dual
   lookup:

```rust
fn check_scoped_constant(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    id: ExpressionId,
    subject: ExpressionId,
    name: &str,
) {
    let Some(keys) = scoped_subject_keys(context, subject) else {
        return;
    };
    let missing = |key: &String| {
        atom_existence(context, key, MemberKind::ClassConstant, name, true)
            == MemberExistence::Missing
            && atom_existence(context, key, MemberKind::EnumCase, name, true)
                == MemberExistence::Missing
    };
    if keys.is_empty() || !keys.iter().all(missing) {
        return;
    }
    let Some(first) = keys.first() else { return };
    let receiver = written_class_display(context, subject, first);
    let kind = if class_kind(context, first) == Some(DeclarationKind::Enum) {
        TypedVerdictKind::UnknownEnumCase { member: name.to_owned(), receiver }
    } else {
        TypedVerdictKind::UnknownClassConstant { member: name.to_owned(), receiver }
    };
    verdicts.push(TypedVerdict { body: context.body, expression: id, kind });
}
```

4. Enum-case receivers (`Status::Active->value`) already decompose
   through `ReceiverAtom::Case`, and the `value`/`name`
   always-exists rule landed with task 3 (decision 7); nothing new
   to build for them here beyond the pinned tests.

- [ ] **Step 4: Run the tests, the full gate, and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates
git commit -m "✨ feat(types): unknown properties, constants, and enum cases report"
```

---

### Task 6: The nullability family (CEL0034)

Entirely dependent on narrowing (design section 8): the tables the
walk reads are post-narrowing by construction, so this family is what
puts the plan-5 floor to the test. The predicate is
`TypeId::contains_null` — explicitly-null unions and `null` itself,
never `mixed`, never templates: the design's mixed-receiver silence
holds by construction.

**Files:**
- Modify: `crates/celerrate_types/src/checks/nullability.rs`
- Test: its test module

**Interfaces:**
- Consumes: `BodyExpression::MemberAccess { receiver, member,
  null_safe }`, `BodyExpression::{Isset, Empty, Binary, Assignment,
  Index}` (the decision-9 guard seams), task 4's
  `called_member_accesses`, `TypeId::contains_null`, task 3's
  `written_type_display`.
- Produces: `nullability::check(context, verdicts)` emits
  `NullDereference`; and `pub(crate) fn
  null_guarded_property_reads(ir: &BodyIr) -> HashSet<ExpressionId>`
  — the expressions exempt under decision 9's guard rule.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_possibly_null_dereference_reports() {
    let verdicts = family_verdicts(r#"<?php
class User { public string $name = ''; public function save(): void {} }
function f(?User $u): void {
    $u->save();
    $u->name;
    $u->name = 'x';
}
"#);
    assert_eq!(
        verdicts,
        vec![
            TypedVerdictKind::NullDereference {
                member: "save".to_owned(),
                receiver: "User|null".to_owned(),
            },
            TypedVerdictKind::NullDereference {
                member: "name".to_owned(),
                receiver: "User|null".to_owned(),
            },
            TypedVerdictKind::NullDereference {
                member: "name".to_owned(),
                receiver: "User|null".to_owned(),
            },
        ],
    );
}

#[test]
fn narrowing_and_the_null_safe_operator_silence() {
    let verdicts = family_verdicts(r#"<?php
class Address { public string $city = ''; }
class User {
    public ?Address $address = null;
    public function save(): void {}
}
function f(?User $u, mixed $anything): void {
    if ($u !== null) {
        $u->save();                    // narrowed: silent
    }
    $u?->save();                       // null-safe: silent
    $u?->address?->city;               // whole-chain short circuit: silent
    if ($u === null) {
        return;
    }
    $u->save();                        // early return narrowed: silent
    $u->address->city;                 // ?Address un-narrowed: reports
    $anything->whatever();             // mixed: silent by construction
}
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::NullDereference {
            member: "city".to_owned(),
            receiver: "Address|null".to_owned(),
        }],
    );
}

#[test]
fn a_chain_end_re_acquires_null_and_a_real_dereference_of_it_reports() {
    // Plan 5's chain rule: only the final chain result re-acquires
    // `|null`. Dereferencing that end without narrowing is a real
    // possible-null dereference.
    let verdicts = family_verdicts(r#"<?php
class Profile { public function refresh(): void {} }
class User { public function profile(): Profile { return new Profile(); } }
function f(?User $u): void {
    $profile = $u?->profile();
    $profile->refresh();
}
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::NullDereference {
            member: "refresh".to_owned(),
            receiver: "Profile|null".to_owned(),
        }],
    );
}

#[test]
fn guarded_property_reads_are_exempt_but_calls_still_report() {
    // Decision 9: isset(), empty(), and the ??/??= left side are
    // the idiomatic guards themselves — property reads there are
    // non-fatal and warning-suppressed. A call is a hard boundary:
    // it still throws on a null receiver and still reports.
    let verdicts = family_verdicts(r#"<?php
class Box {
    public ?Box $inner = null;
    public function get(): ?Box { return null; }
}
function f(?Box $b, array $bag): void {
    isset($b->inner);                  // exempt
    empty($b->inner);                  // exempt
    $x = $b->inner ?? null;            // exempt
    $y = $b->inner->inner ?? null;     // the whole chain is exempt
    $z = $bag[0]->inner ?? null;       // Index subjects thread too
    $b->inner ??= null;                // exempt (recorded stance)
    $w = $b->get() ?? null;            // a call boundary: reports
}
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::NullDereference {
            member: "get".to_owned(),
            receiver: "Box|null".to_owned(),
        }],
    );
}
```

(`family_verdicts` is task 4's `method_verdicts` renamed once the
module tests share it; the expected `receiver` strings follow task
3's `written_type_display` union rendering — fix expectations, never
the code. Note the receiver in the message is the **full** display,
null included: this family's message is about the null, unlike
unknown-members' which is about the non-null part. This step also
updates task 4's `union_receivers_report_only_when_missing_everywhere`
expectation: `$nullable->nowhere()` now additionally yields a
`NullDereference { member: "nowhere", receiver: "A|null" }` after
the two `UnknownMethod` verdicts — the signposted, expected change,
not a regression.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_types checks::nullability`
Expected: FAIL — the walker is empty.

- [ ] **Step 3: Implement the walk**

```rust
//! The nullability family (CEL0034): a dereference — property read,
//! property write, or method call, all the same `MemberAccess` arena
//! node — whose receiver's type explicitly contains `null`.
//! `contains_null` is the whole predicate: `mixed` and templates
//! answer `false`, so the design's undecidable-receiver silence is
//! structural. `?->` accesses never report (the chain rule types the
//! short-circuited suffix non-null; the chain end re-acquires `|null`
//! and a later real dereference of it reports there). Guarded
//! property reads never report either (decision 9): `isset()`,
//! `empty()`, and the `??`/`??=` left side evaluate a null property
//! chain without a fatal error — a method call inside them still
//! throws and still reports.

pub(crate) fn check(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
) {
    let called = called_member_accesses(context.ir);
    let guarded = null_guarded_property_reads(context.ir);
    for (index, expression) in context.ir.expressions.iter().enumerate() {
        let Some(id) = ExpressionId::from_index(index) else { continue };
        let BodyExpression::MemberAccess {
            receiver,
            member: MemberReference::Named { name },
            null_safe: false,
        } = expression
        else {
            continue;
        };
        // The decision-9 exemption covers property positions only:
        // a guarded expression consumed as a callee is a real call.
        if guarded.contains(&id) && !called.contains(&id) {
            continue;
        }
        let Some(receiver_type) = context.inferred.expression_type(*receiver) else {
            continue;
        };
        if receiver_type.contains_null(context.db) {
            verdicts.push(TypedVerdict {
                body: context.body,
                expression: id,
                kind: TypedVerdictKind::NullDereference {
                    member: name.clone(),
                    receiver: written_type_display(context, receiver_type),
                },
            });
        }
    }
}

/// The expressions decision 9 exempts: the targets of `isset()` and
/// `empty()`, the left operand of `??`, and the target of `??=`,
/// expanded along their receiver chains — through `MemberAccess`
/// receivers and `Index` subjects, stopping at anything else (a call
/// in particular is a hard boundary).
pub(crate) fn null_guarded_property_reads(ir: &BodyIr) -> HashSet<ExpressionId> {
    let mut seeds: Vec<ExpressionId> = Vec::new();
    for expression in &ir.expressions {
        match expression {
            BodyExpression::Isset { targets } => seeds.extend(targets.iter().copied()),
            BodyExpression::Empty { target } => seeds.push(*target),
            BodyExpression::Binary { operator, lhs, .. }
                if *operator == SyntaxKind::QuestionQuestion =>
            {
                seeds.push(*lhs);
            }
            BodyExpression::Assignment { operator, target, .. }
                if *operator == SyntaxKind::QuestionQuestionEquals =>
            {
                seeds.push(*target);
            }
            _ => {}
        }
    }
    let mut guarded: HashSet<ExpressionId> = HashSet::new();
    while let Some(id) = seeds.pop() {
        if !guarded.insert(id) {
            continue;
        }
        match ir.expression(id) {
            Some(BodyExpression::MemberAccess { receiver, .. }) => seeds.push(*receiver),
            Some(BodyExpression::Index { subject, .. }) => seeds.push(*subject),
            _ => {}
        }
    }
    guarded
}
```

(Spell the two `SyntaxKind` operator names exactly as the lexer
names the `??` and `??=` tokens — the identifiers above stand for
whatever the shipped `SyntaxKind` calls them.)

The dynamic-member stance, recorded here as elsewhere: only `Named`
members report — a `$nullable->$dynamic` dereference is real, but
dynamic member names are silent across all families in this preview
(task 13's ledger).

- [ ] **Step 4: Run the tests, the full gate, and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS. If the narrowing-silence test fails because a floor
form types differently than expected, that is a plan-5/6 bug — apply
`superpowers:systematic-debugging` before touching this walk.

```bash
git add crates
git commit -m "✨ feat(types): possibly-null dereferences report after narrowing"
```

---

### Task 7: The coercion posture

The argument family's judgment layer, built before the family so task
8 lands against a tested predicate. The rule that keeps every
consumer honest: **coercion never proves, it only un-fails** —
`assignable_to` answers `subtype_of`'s verdict, upgraded from `Fails`
to `CannotProve` when a mode-legal runtime coercion could make the
call work. The check reports only `Fails`, so an upgraded verdict is
silence, exactly what "coercions PHP performs at runtime are not
reported" demands — without ever claiming a proof that does not
hold set-theoretically.

**Files:**
- Create: `crates/celerrate_semantics/src/strict_types.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (module + export)
- Modify: `crates/celerrate_types/src/judgments.rs` (`CoercionMode`,
  the `assignable_to` mode parameter)
- Test: both modules' test blocks

**Interfaces:**
- Consumes: the file's syntax root (the own-tree precedent:
  `syntax_gating.rs` reads it for strictly-local output),
  `DeclareStatement::declare_directives()`, `subtype_of`,
  `lookup_member` (the `__toString` probe), `linearized_class`
  (the `Stringable` ancestry probe).
- Produces:
  - `celerrate_semantics::file_strict_types(db, file: SourceFile) ->
    bool` (tracked) — `true` iff a top-level
    `declare(strict_types=1)` exists.
  - `celerrate_types::CoercionMode { Strict, Weak }` (exported).
  - `assignable_to(db, files, stubs, configuration, source, target,
    mode: CoercionMode) -> Proof` — the extended signature; every
    existing caller passes `CoercionMode::Strict` (semantics
    unchanged for them).

- [ ] **Step 1: Write the failing strict-types tests**

In `strict_types.rs`:

```rust
#[test]
fn the_declare_directive_is_read_from_the_top_of_the_file() {
    let cases: &[(&str, bool)] = &[
        ("<?php declare(strict_types=1);\nfunction f() {}", true),
        ("<?php declare(strict_types = 1);\nfunction f() {}", true),
        ("<?php declare(STRICT_TYPES=1);\nfunction f() {}", true),
        ("<?php declare(strict_types=0);\nfunction f() {}", false),
        ("<?php declare(ticks=1);\nfunction f() {}", false),
        ("<?php function f() {}", false),
        ("", false),
        // PHP requires the directive to be the file's very first
        // statement; a later placement is a compile error, so the
        // file cannot run either way. Accepting it as strict is
        // recorded over-acceptance (it only tightens the checks).
        ("<?php $x = 1; declare(strict_types=1);", true),
    ];
    for (source, expected) in cases {
        let fixture = fixture(&[source]);
        assert_eq!(
            file_strict_types(&fixture.db, handle_of(&fixture, 0)),
            *expected,
            "source: {source:?}",
        );
    }
}
```

- [ ] **Step 2: Implement `file_strict_types`**

```rust
//! The per-file coercion mode (design section 8): whether the file
//! declares `strict_types=1`. An own-tree read for strictly-local
//! output — the syntax-gating precedent — so nothing above the file
//! is invalidated by the directive, and nothing here survives a
//! parse change it should not.

#[salsa::tracked]
pub fn file_strict_types(db: &dyn salsa::Database, file: SourceFile) -> bool {
    let parse = parsed_file(db, file);
    let root = parse.tree();
    root.statements().any(|statement| {
        let Statement::DeclareStatement(declare) = statement else {
            return false;
        };
        declare.declare_directives().any(|directive| {
            directive_is_strict_types(&directive)
        })
    })
}

/// `strict_types` compared case-insensitively (PHP's directive names
/// are), value literal `1`.
fn directive_is_strict_types(directive: &DeclareDirective) -> bool {
    let name_matches = directive
        .name_text()
        .is_some_and(|name| name.eq_ignore_ascii_case("strict_types"));
    let value_is_one = directive
        .value_text()
        .is_some_and(|value| value.trim() == "1");
    name_matches && value_is_one
}
```

(Spell the parse entry point and the `DeclareDirective` accessors
exactly as `syntax_gating.rs` spells its own-tree read and as
`generated.rs` names the node methods — `name_text`/`value_text`
here stand for whatever token accessors the generated AST offers;
extract the text through them, never by re-lexing.)

- [ ] **Step 3: Write the failing judgment tests**

In `judgments.rs`'s test module:

```rust
#[test]
fn coercion_never_proves_it_only_un_fails() {
    let fixture = fixture(&["<?php class WithString { public function __toString(): string { return ''; } } class Plain {}"]);
    let db = &fixture.db;
    let judge = |source, target, mode| {
        assignable_to(
            db, fixture.files, fixture.stubs, fixture.configuration,
            source, target, mode,
        )
    };
    let int = TypeId::int(db);
    let float = TypeId::float(db);
    let string = TypeId::string(db);
    let bool_type = TypeId::bool(db);
    let null = TypeId::null(db);
    let stringable = TypeId::class(db, "withstring", vec![]);
    let plain = TypeId::class(db, "plain", vec![]);
    use CoercionMode::{Strict, Weak};
    use Proof::{CannotProve, Fails, Holds};
    // Subtyping is untouched by the mode.
    assert_eq!(judge(int, int, Strict), Holds);
    assert_eq!(judge(int, string, Strict), Fails);
    // The one strict-mode coercion PHP performs: int to float.
    assert_eq!(judge(int, float, Strict), CannotProve);
    // Weak mode un-fails scalar interchange…
    assert_eq!(judge(string, int, Weak), CannotProve);
    assert_eq!(judge(bool_type, string, Weak), CannotProve);
    assert_eq!(judge(int, string, Weak), CannotProve);
    // …but never null, and never non-scalar targets.
    assert_eq!(judge(null, string, Weak), Fails);
    assert_eq!(judge(string, plain, Weak), Fails);
    // Stringable passes a string parameter in weak mode only.
    assert_eq!(judge(stringable, string, Weak), CannotProve);
    assert_eq!(judge(stringable, string, Strict), Fails);
    assert_eq!(judge(plain, string, Weak), Fails);
}
```

- [ ] **Step 4: Run them to verify they fail**

Run: `cargo test --package celerrate_types judgments && cargo test --package celerrate_semantics strict_types`
Expected: FAIL — no `mode` parameter, no module.

- [ ] **Step 5: Implement the mode**

```rust
/// The calling file's coercion posture (design section 8): strict
/// under `declare(strict_types=1)`, weak otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoercionMode {
    Strict,
    Weak,
}

#[salsa::tracked]
pub fn assignable_to<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    source: TypeId<'db>,
    target: TypeId<'db>,
    mode: CoercionMode,
) -> Proof {
    match subtype_of(db, files, stubs, configuration, source, target) {
        Proof::Fails if coercion_could_apply(db, files, stubs, configuration, source, target, mode) => {
            Proof::CannotProve
        }
        verdict => verdict,
    }
}

/// Whether a runtime coercion the mode permits could make the value
/// pass: int to float always (PHP performs it under strict types
/// too); in weak mode, scalar interchange (never from null) and a
/// `Stringable` object against a string target. Union sources must
/// be entirely coercible; union targets need one coercible arm.
fn coercion_could_apply<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    source: TypeId<'db>,
    target: TypeId<'db>,
    mode: CoercionMode,
) -> bool {
    let sources = source.constituents(db);
    if sources.len() > 1 {
        return sources.into_iter().all(|part| {
            subtype_of(db, files, stubs, configuration, part, target) != Proof::Fails
                || coercion_could_apply(db, files, stubs, configuration, part, target, mode)
        });
    }
    let targets = target.constituents(db);
    if targets.len() > 1 {
        return targets.into_iter().any(|part| {
            coercion_could_apply(db, files, stubs, configuration, source, part, mode)
        });
    }
    if is_int_family(db, source) && is_float_family(db, target) {
        return true;
    }
    if mode == CoercionMode::Weak {
        if is_coercible_scalar(db, source) && is_scalar_target(db, target) {
            return true;
        }
        if is_string_family(db, target) && is_stringable(db, files, stubs, configuration, source) {
            return true;
        }
    }
    false
}
```

Notes for the implementer:

- The `is_*_family` predicates interrogate through the public
  methods (`int_bounds`, `int_literal_value`, string constructors'
  duals) or in-crate `TypeData` matching — int covers literals and
  ranges; string covers every `StringConstraint`; scalar =
  bool/int/float/string, **never null, never mixed**. Note the
  shipped `judge` answers `Fails` for a `mixed` candidate (a genuine
  set-theoretic refutation), so a `mixed` source can reach this
  layer with `Fails` in hand: the family walk guards `mixed` and
  per-constituent union fits **before** the judgment (decision 10),
  and nothing here may rely on the `Proof` value to silence them.
- `is_stringable`: the source is a class whose key resolves
  `__toString` through `lookup_member`, or whose `linearized_class`
  ancestry (or `stub_ancestors`) names `stringable`.
- Fix every existing `assignable_to` caller to pass
  `CoercionMode::Strict` — `cargo check` enumerates them; their
  semantics are unchanged by construction (`Strict` only un-fails
  int→float, and re-check each caller's context: if one is a
  subtype-style consumer that must not accept int→float, switch it
  to `subtype_of` instead and say so in the commit).
- Export `CoercionMode` from `lib.rs`; export `file_strict_types`
  from `celerrate_semantics`.

- [ ] **Step 6: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates
git commit -m "✨ feat(types): the per-file coercion posture un-fails, never proves"
```

---

### Task 8: Argument types (CEL0035)

**Files:**
- Modify: `crates/celerrate_types/src/checks/arguments.rs`
- Test: its test module

**Interfaces:**
- Consumes: `BodyExpression::{Call, New}`, `CallArgument { label,
  spread, value }`, `declared_function_signature`/`FunctionQuery`,
  `declared_member_signature`/`MemberQuery`,
  `DeclaredSignature { parameters, .. }`, `DeclaredParameter { name,
  parameter_type, optional, variadic, by_reference, .. }`,
  `assignable_to`/`CoercionMode`, `file_strict_types`, task 4's
  `scoped_subject_keys`, the receiver atoms, and task 3's
  `written_type_display`.
- Produces: `arguments::check(context, verdicts)` emits
  `ArgumentType`; plus the resolution helper task 9 reuses —
  `pub(crate) fn resolved_call_signature(context, callee:
  ExpressionId) -> Option<ResolvedCall>` with
  `pub(crate) struct ResolvedCall<'db> { pub callee_display: String,
  pub signature: DeclaredSignature<'db>, pub source_body:
  Option<(SourceFile, BodyQuery<'db>)> }` — `None` unless exactly
  one signature resolves (decision 11); `source_body` names the
  callee's body when it is source code (task 9's `func_get_args`
  probe).

- [ ] **Step 1: Write the failing tests**

```rust
const STRICT: &str = "<?php declare(strict_types=1);\n";

#[test]
fn a_failing_argument_reports_in_a_strict_file() {
    let verdicts = family_verdicts(&format!("{STRICT}{}", r#"
function takes(int $n, string $s = ''): void {}
function f(mixed $anything, int|string $either): void {
    takes(1, 'ok');
    takes('wrong');        // reports: string against int
    takes($anything);      // mixed: guarded before the judgment, silent
    takes($either);        // int|string: one constituent fits, silent
    takes(s: 42);          // named argument, reports: int against string
}
"#));
    assert_eq!(
        verdicts,
        vec![
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(1),
                callee: "takes".to_owned(),
                expected: "int".to_owned(),
                given: "'wrong'".to_owned(),
            },
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Named("s".to_owned()),
                callee: "takes".to_owned(),
                expected: "string".to_owned(),
                given: "42".to_owned(),
            },
        ],
    );
}

#[test]
fn a_weak_file_does_not_report_runtime_coercions() {
    let verdicts = family_verdicts(r#"<?php
function takes(int $n): void {}
class Plain {}
function f(Plain $object): void {
    takes('42');       // weak mode coerces: silent
    takes($object);    // no coercion exists: reports
}
"#);
    assert_eq!(
        verdicts,
        vec![TypedVerdictKind::ArgumentType {
            label: ArgumentLabel::Positional(1),
            callee: "takes".to_owned(),
            expected: "int".to_owned(),
            given: "Plain".to_owned(),
        }],
    );
}

#[test]
fn the_exemptions_are_structural() {
    let verdicts = family_verdicts(&format!("{STRICT}{}", r#"
function fills(array &$out, int $n): void {}
class A { public function m(int $n): void {} }
class B { public function m(int $n): void {} }
function f(A|B $either): void {
    fills($undefined, 1);   // by-reference parameter: exempt
    $either->m('x');        // union receiver: silent (recorded stance)
}
"#));
    assert_eq!(verdicts, vec![]);
}

#[test]
fn methods_static_calls_constructors_and_variadics_are_checked() {
    let verdicts = family_verdicts(&format!("{STRICT}{}", r#"
class Mailer {
    public function __construct(private string $dsn) {}
    public function send(string $to): void {}
    public static function make(string $dsn): static { return new static($dsn); }
}
function f(Mailer $m): void {
    $m->send('a@b');
    $m->send(42);              // reports
    Mailer::make(42);          // reports
    new Mailer(42);            // reports
    variadic('a', 'b', 42);    // reports on the third
}
function variadic(string ...$parts): void {}
"#));
    assert_eq!(
        verdicts,
        vec![
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(1),
                callee: "send".to_owned(),
                expected: "string".to_owned(),
                given: "42".to_owned(),
            },
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(1),
                callee: "make".to_owned(),
                expected: "string".to_owned(),
                given: "42".to_owned(),
            },
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(1),
                callee: "Mailer".to_owned(),
                expected: "string".to_owned(),
                given: "42".to_owned(),
            },
            TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(3),
                callee: "variadic".to_owned(),
                expected: "string".to_owned(),
                given: "42".to_owned(),
            },
        ],
    );
}
```

(`given` and `expected` strings follow task 3's
`written_type_display` — class names in written spelling, literal
types as literals (`'wrong'`, `42`); fix the expectations to the
shipped spellings.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_types checks::arguments`
Expected: FAIL — the walker is empty.

- [ ] **Step 3: Implement resolution and the type walk**

```rust
//! The argument family (design section 8): per-argument
//! assignability under the calling file's coercion posture, against
//! exactly one resolved declared signature. `Proof::Fails` reports;
//! `Holds` and `CannotProve` are silence — which is how weak-mode
//! coercions stay unreported. `mixed` and partially fitting unions
//! are guarded **before** the judgment (decision 10): the shipped
//! `judge` refutes them set-theoretically, so their silence is this
//! walk's structural job, never the `Proof` value's.

pub(crate) struct ResolvedCall<'db> {
    pub callee_display: String,
    pub signature: DeclaredSignature<'db>,
    pub source_body: Option<(SourceFile, BodyQuery<'db>)>,
}

/// Exactly one declared signature for a call's callee, or `None`
/// (decision 11): a named free function or stub function; a method,
/// static call, or constructor whose receiver decomposes to exactly
/// one class atom (unions are silent, a recorded stance). The
/// **declared tier only** — providers compute returns, never
/// parameter contracts.
pub(crate) fn resolved_call_signature<'db>(
    context: &CheckContext<'db, '_>,
    expression: ExpressionId,
) -> Option<ResolvedCall<'db>> { /* … */ }
```

Resolution, case by case (each a small function; mirror the
`flow.rs` call-boundary spellings for name resolution):

1. `Call { callee }` where the callee expression is
   `NamedReference { text }`: resolve in `SymbolSpace::Function`
   through `resolve_name` with the context's namespace and tables
   (the fallback-to-global rule lives inside resolution, as at the
   flow boundary); `declared_function_signature` on the folded key.
   `callee_display` = the written text's last segment.
   `source_body`: when the resolution is a source function, the
   `(file, BodyQuery)` pair through `lookup_function_declaration` +
   `analyzed_file_index` (the `inferred_function_return` idiom).
2. `Call { callee }` where the callee is `MemberAccess { receiver,
   member: Named { name }, .. }`: `atoms_of(receiver's type)` must
   be exactly one `Class`/`Case` atom after dropping `Null` atoms —
   else `None`; `declared_member_signature(MemberQuery::new(db, key,
   MemberKind::Method, folded_member_key(Method, name)))`.
   `source_body` through `lookup_member`'s `Source` resolution
   (`member.ast_id`, the declaring file via the owner's group).
3. `Call { callee }` where the callee is `ScopedAccess { subject,
   member: Named { name } }`: `scoped_subject_keys` must answer
   exactly one key; same member query.
4. `New { class: Named { name }, .. }`: resolve the class,
   `declared_member_signature(key, Method, "__construct")`; `None`
   (no constructor) → the whole call is silent (decision 12).
   `New { class: Anonymous { declaration } }`: the synthetic key,
   same query. `callee_display` = the written class name.

The type walk:

```rust
pub(crate) fn check(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
) {
    let mode = if file_strict_types(context.db, context.file) {
        CoercionMode::Strict
    } else {
        CoercionMode::Weak
    };
    for (index, expression) in context.ir.expressions.iter().enumerate() {
        let Some(id) = ExpressionId::from_index(index) else { continue };
        let arguments = match expression {
            BodyExpression::Call { arguments, .. }
            | BodyExpression::New { arguments, .. } => arguments,
            _ => continue,
        };
        let Some(resolved) = resolved_call_signature(context, id) else {
            continue;
        };
        check_argument_types(context, verdicts, id, &resolved, arguments, mode);
    }
}

fn check_argument_types(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    call: ExpressionId,
    resolved: &ResolvedCall<'_>,
    arguments: &[CallArgument],
    mode: CoercionMode,
) {
    let parameters = &resolved.signature.parameters;
    let mut position = 0usize;
    for argument in arguments {
        if argument.spread {
            // Spread makes later positional matching undecidable;
            // arguments before the first spread were already checked.
            break;
        }
        let (parameter, label) = match &argument.label {
            Some(name) => {
                let Some(parameter) =
                    parameters.iter().find(|parameter| parameter.name == *name)
                else {
                    continue; // task 9 reports the unknown name
                };
                (parameter, ArgumentLabel::Named(name.clone()))
            }
            None => {
                position += 1;
                let Some(parameter) = parameter_at(parameters, position) else {
                    continue; // task 9 reports the excess
                };
                (parameter, ArgumentLabel::Positional(position))
            }
        };
        if parameter.by_reference {
            continue; // the `preg_match` exemption (design section 6)
        }
        let Some(parameter_type) = parameter.parameter_type else {
            continue; // the empty-intersection stub guard (plan 3)
        };
        let Some(argument_type) = context.inferred.expression_type(argument.value)
        else {
            continue;
        };
        // Decision 10's pre-judgment guards. The shipped `judge`
        // refutes set-theoretically: a `mixed` candidate answers
        // `Fails`, and a union candidate folds through `Proof::all`,
        // so one failing constituent fails the whole union. The
        // family silences both structurally: `mixed` passes
        // everywhere, and a union reports only when every
        // constituent fails on its own.
        if argument_type.is_mixed(context.db) {
            continue;
        }
        let every_constituent_fails = argument_type
            .constituents(context.db)
            .into_iter()
            .all(|part| {
                assignable_to(
                    context.db, context.files, context.stubs, context.configuration,
                    part, parameter_type, mode,
                ) == Proof::Fails
            });
        if every_constituent_fails {
            verdicts.push(TypedVerdict {
                body: context.body,
                expression: argument.value,
                kind: TypedVerdictKind::ArgumentType {
                    label,
                    callee: resolved.callee_display.clone(),
                    expected: written_type_display(context, parameter_type),
                    given: written_type_display(context, argument_type),
                },
            });
        }
    }
    let _ = call; // task 9 anchors arity verdicts on the call itself
}

/// The parameter a 1-based position binds: the last variadic absorbs
/// everything past the list.
fn parameter_at(parameters: &[DeclaredParameter<'_>], position: usize) -> Option<&DeclaredParameter<'_>> {
    match parameters.get(position - 1) {
        Some(parameter) => Some(parameter),
        None => parameters.last().filter(|parameter| parameter.variadic),
    }
}
```

(One subtlety the tests will surface: a declared parameter type may
carry template variables or placeholders — `assignable_to` already
answers `CannotProve` for them, which is silence; no substitution
happens in this family, a recorded stance.)

- [ ] **Step 4: Run the tests, the full gate, and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates
git commit -m "✨ feat(types): argument types report only proven failures"
```

---

### Task 9: Arity and named arguments (CEL0036–CEL0038)

**Files:**
- Modify: `crates/celerrate_types/src/checks/arguments.rs`
- Test: its test module

**Interfaces:**
- Consumes: task 8's `resolved_call_signature`/`ResolvedCall`
  (including `source_body`), `body_ir` (the `func_get_args` probe).
- Produces: `arguments::check` additionally emits
  `TooFewArguments`, `TooManyArguments`, `UnknownNamedArgument`;
  and `pub(crate) fn captures_arguments(db, file, body) -> bool`
  (tracked) — whether a source body calls `func_get_args`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn arity_reports_missing_excess_and_unknown_names() {
    let verdicts = family_verdicts(&format!("{STRICT}{}", r#"
function takes(int $a, string $b = '', int ...$rest): void {}
function pair(int $a, int $b): void {}
function f(): void {
    takes(1);                  // optional + variadic satisfied: silent
    takes(1, 'x', 2, 3);       // variadic absorbs: silent
    pair(1, 2, 3);             // reports CEL0037
    pair(1);                   // reports CEL0036
    pair(b: 2, a: 1);          // named fill: silent
    pair(1, c: 2);             // reports CEL0038 (and CEL0036 for $b)
}
"#));
    assert_eq!(
        verdicts,
        vec![
            TypedVerdictKind::TooManyArguments {
                callee: "pair".to_owned(),
                given: 3,
                accepted: 2,
            },
            TypedVerdictKind::TooFewArguments {
                callee: "pair".to_owned(),
                given: 1,
                required: 2,
            },
            TypedVerdictKind::UnknownNamedArgument {
                callee: "pair".to_owned(),
                name: "c".to_owned(),
            },
            TypedVerdictKind::TooFewArguments {
                callee: "pair".to_owned(),
                given: 2,
                required: 2,
            },
        ],
    );
}

#[test]
fn a_variadic_signature_accepts_any_named_argument() {
    // PHP 8.0 collects unknown named arguments into a trailing
    // variadic (decision 12); reporting CEL0038 here would flag
    // working code.
    let verdicts = family_verdicts(&format!("{STRICT}{}", r#"
function sink(int $first, int ...$rest): void {}
function f(): void {
    sink(1, extra: 2, more: 3);
}
"#));
    assert_eq!(verdicts, vec![]);
}

#[test]
fn spread_and_argument_capture_silence_arity() {
    let verdicts = family_verdicts(&format!("{STRICT}{}", r#"
function pair(int $a, int $b): void {}
function capturing(): void { $all = func_get_args(); }
function f(array $bag): void {
    pair(...$bag);             // spread: all three arity checks silent
    capturing(1, 2, 3);        // captures arguments: excess silent
    new \DateTime('now');      // stub constructors resolve normally
}
"#));
    assert_eq!(verdicts, vec![]);
}
```

(The `\DateTime` line needs the embedded-stub fixture variant —
reuse the synthetic-stub helper from task 5 or the
`embedded_stub_index` idiom the semantics false-positive suite uses;
if neither fits the module's fixture, drop that line here — the
seeded suite of task 11 exercises stubs end to end.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_types checks::arguments`
Expected: FAIL — no arity verdicts yet.

- [ ] **Step 3: Implement arity**

Extend `check` after the type loop with:

```rust
fn check_arity(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    call: ExpressionId,
    resolved: &ResolvedCall<'_>,
    arguments: &[CallArgument],
) {
    if arguments.iter().any(|argument| argument.spread) {
        return; // undecidable in both directions (decision 12)
    }
    let parameters = &resolved.signature.parameters;
    let positional = arguments.iter().filter(|a| a.label.is_none()).count();
    let named: Vec<&String> =
        arguments.iter().filter_map(|a| a.label.as_ref()).collect();
    let variadic = parameters.last().is_some_and(|parameter| parameter.variadic);
    // Unknown names first: each is its own verdict. A trailing
    // variadic accepts any named argument (PHP 8.0 collects unknown
    // names into it; decision 12), so the whole loop is silenced.
    if !variadic {
        for name in &named {
            if !parameters.iter().any(|parameter| parameter.name == **name) {
                verdicts.push(TypedVerdict {
                    body: context.body,
                    expression: call,
                    kind: TypedVerdictKind::UnknownNamedArgument {
                        callee: resolved.callee_display.clone(),
                        name: (*name).clone(),
                    },
                });
            }
        }
    }
    // Excess: positional arguments past a non-variadic list.
    if !variadic && positional > parameters.len() && !captures(context, resolved) {
        verdicts.push(TypedVerdict {
            body: context.body,
            expression: call,
            kind: TypedVerdictKind::TooManyArguments {
                callee: resolved.callee_display.clone(),
                given: positional,
                accepted: parameters.len(),
            },
        });
    }
    // Missing: a required parameter bound neither by position nor name.
    let required = parameters
        .iter()
        .filter(|parameter| !parameter.optional && !parameter.variadic)
        .count();
    let unbound = parameters
        .iter()
        .enumerate()
        .filter(|(index, parameter)| {
            !parameter.optional
                && !parameter.variadic
                && *index >= positional
                && !named.iter().any(|name| **name == parameter.name)
        })
        .count();
    if unbound > 0 {
        verdicts.push(TypedVerdict {
            body: context.body,
            expression: call,
            kind: TypedVerdictKind::TooFewArguments {
                callee: resolved.callee_display.clone(),
                given: arguments.len(),
                required,
            },
        });
    }
}

/// Whether the source callee captures its arguments with
/// `func_get_args` — a variadic-by-capture function called with
/// extra arguments is working code (the guillotine forbids
/// reporting it).
fn captures(context: &CheckContext<'_, '_>, resolved: &ResolvedCall<'_>) -> bool {
    resolved
        .source_body
        .is_some_and(|(file, body)| captures_arguments(context.db, file, body))
}

/// Tracked per body: any call whose callee text folds to
/// `func_get_args` (bare or fully qualified).
#[salsa::tracked]
pub(crate) fn captures_arguments<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> bool {
    let Some(ir) = body_ir(db, file, body).as_ref() else {
        return false;
    };
    ir.expressions.iter().any(|expression| {
        let BodyExpression::Call { callee, .. } = expression else {
            return false;
        };
        let Some(BodyExpression::NamedReference { text }) = ir.expression(*callee)
        else {
            return false;
        };
        text.trim_start_matches('\\').eq_ignore_ascii_case("func_get_args")
    })
}
```

Duplicate binding (`pair(1, a: 2)`) stays silent — a PHP `Error` this
preview does not own; task 13's ledger records it.

- [ ] **Step 4: Run the tests, the full gate, and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates
git commit -m "✨ feat(types): arity and named arguments complete the argument family"
```

---

### Task 10: The product wiring

`celerrate check` renders the families; the cache stays honest;
suppression covers everything; the typed counters land.

**Files:**
- Modify: `crates/celerrate_cli/src/analysis.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs`
- Modify: `crates/celerrate_cli/src/cache/statistics.rs`
- Modify: `crates/celerrate_cli/tests/check.rs`
- Modify: `crates/celerrate_cli/tests/suppressions.rs`
- Modify: `crates/celerrate_cli/tests/cache_equivalence.rs`
- Test: the three CLI integration suites

**Interfaces:**
- Consumes: `typed_diagnostics`, `typed_file_verdicts`,
  `suppressed_ranges`/`is_suppressed`, `composed_diagnostics`,
  `lookup_verdict`/`VerdictLookup`, `persist`/`collect_entries`,
  `CacheStatistics`.
- Produces:
  - `analysis.rs::persistable_diagnostics(inputs, file) ->
    Vec<Diagnostic>` — exactly yesterday's `composed_diagnostics`
    (syntax + semantic + suppression): what packs persist.
  - `analysis.rs::typed_portion(inputs, file) -> Vec<Diagnostic>` —
    the typed families, suppression applied.
  - `composed_diagnostics(inputs, file)` = `persistable ∪ typed`,
    sorted — still the single composition point (`analyze_one` on a
    miss, `persist` re-composes the persistable part, the
    equivalence harness recomputes the union).
  - `CacheStatistics` gains `typed_bodies`,
    `typed_declared_edges`, `typed_inferred_edges`,
    `typed_provider_edges` (`AtomicU64`) and
    `record_typed(&TypedFileResult)`; `render()` gains
    `; typed {n} bodies, edges {d} declared / {i} inferred / {p}
    provider`, inserted before the persist clause.

- [ ] **Step 1: Write the failing end-to-end test**

In `crates/celerrate_cli/tests/check.rs` (its `project`/`check`
idiom):

```rust
#[test]
fn the_typed_families_render_through_check() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Service.php",
            r#"<?php
declare(strict_types=1);
namespace App;

class User
{
    public function save(): void
    {
    }
}

class Service
{
    public function run(?User $user): void
    {
        $user->save();
        $user?->svae();
    }
}
"#,
        ),
    ]);
    let (outcome, output) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    let output = normalize_location_separators(&output);
    assert!(output.contains("CEL0034"), "the null dereference renders: {output}");
    assert!(output.contains("CEL0030"), "the unknown method renders: {output}");
    assert!(
        output.contains("accessing `save` on a possibly null `App\\User|null`"),
        "{output}",
    );
}
```

And the warm-run consistency (same file, second `check` over the
persisted cache):

```rust
#[test]
fn a_warm_run_reports_the_same_typed_diagnostics() {
    let root = project(&[/* the same two files */]);
    let (_, cold) = check(root.path());
    let (_, warm) = check(root.path());
    assert_eq!(
        normalize_location_separators(&cold),
        normalize_location_separators(&warm),
        "the pack serves untyped verdicts; typed recompute must agree",
    );
}
```

In `suppressions.rs` (its established idiom):

```rust
#[test]
fn a_suppression_extinguishes_a_typed_diagnostic() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Service.php",
            r#"<?php
namespace App;

class User { public function save(): void {} }

function run(?User $user): void
{
    /** @phpstan-ignore-next-line */
    $user->save();
}
"#,
        ),
    ]);
    let (outcome, output) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "suppression is family-agnostic: {output}");
}
```

In `cache_equivalence.rs`: extend the harness's fixture set with one
file carrying a typed finding, so served-equals-recomputed covers
the union (follow the suite's existing shape — the assertion is the
suite's own, only the fixture is new).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --package celerrate_cli --test check --test suppressions`
Expected: FAIL — nothing composes the typed families yet.

- [ ] **Step 3: Wire the composition**

In `analysis.rs`:

```rust
/// The cache-servable portion: syntax, decode, and semantic
/// families, suppression applied. Exactly what `StoredVerdict`
/// persists — the typed families stay out of the packs until plan
/// 9a designs their revalidation records.
pub fn persistable_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    // The previous `composed_diagnostics` body, verbatim.
}

/// The typed families, suppression applied — computed fresh on every
/// path (decision 13): a cache hit serves the untyped verdict and
/// appends this.
pub fn typed_portion(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let mut diagnostics = celerrate_types::typed_diagnostics(
        database, inputs.files, inputs.stubs, inputs.configuration, file,
    )
    .clone();
    let suppressed = celerrate_semantics::suppressed_ranges(database, file);
    if !suppressed.is_empty() {
        // The same filter application `persistable_diagnostics` uses;
        // extract the shared helper rather than duplicating it.
        retain_unsuppressed(database, file, suppressed, &mut diagnostics);
    }
    diagnostics
}

/// The single composition point, now the union.
pub fn composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let mut diagnostics = persistable_diagnostics(inputs, file);
    diagnostics.extend(typed_portion(inputs, file));
    diagnostics.sort();
    diagnostics
}
```

In `analyze_one`, the cache-hit path appends `typed_portion` to the
served verdict's diagnostics (then sorts); the miss path already
goes through `composed_diagnostics`. Right after either path,
aggregate the instrument:

```rust
let typed = celerrate_types::typed_file_verdicts(
    database, inputs.files, inputs.stubs, inputs.configuration, file,
);
inputs.statistics.record_typed(typed);
```

In `cache/mod.rs`, `composed_verdict` switches from
`composed_diagnostics` to `persistable_diagnostics` — one
identifier; the module doc gains a line saying the typed families
are plan 9a's artifact classes.

In `cache/statistics.rs`:

```rust
/// Bodies the typed families walked this run.
pub typed_bodies: AtomicU64,
/// Interprocedural edges the walked bodies consumed, by tier.
pub typed_declared_edges: AtomicU64,
pub typed_inferred_edges: AtomicU64,
pub typed_provider_edges: AtomicU64,
```

```rust
/// Aggregates one file's typed instrument (plan 5's decision 13:
/// counters live at the orchestration layer, never inside queries).
pub fn record_typed(&self, result: &TypedFileResult) {
    self.typed_bodies.fetch_add(u64::from(result.bodies), Ordering::Relaxed);
    self.typed_declared_edges.fetch_add(
        u64::from(result.edge_counts.declared_return_edges), Ordering::Relaxed);
    self.typed_inferred_edges.fetch_add(
        u64::from(result.edge_counts.inferred_return_edges), Ordering::Relaxed);
    self.typed_provider_edges.fetch_add(
        u64::from(result.edge_counts.provider_edges), Ordering::Relaxed);
}
```

and the `render()` line grows
`"; typed {} bodies, edges {} declared / {} inferred / {} provider"`.
Update the statistics module's own render test in the same change.

- [ ] **Step 4: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS — including `cache_consistency.rs` and
`cache_seeding.rs` untouched (packs carry exactly what they carried).

```bash
git add crates
git commit -m "✨ feat(cli): celerrate check composes the typed families"
```

---

### Task 11: The seeded-defect recall suite

Every existing gate is precision-side; a silent engine would pass
them all (design section 9). One seeded defect per identifier, end
to end through `run()` — pipeline, source map, suppression, renderer
included — each of which MUST report.

**Files:**
- Create: `crates/celerrate_cli/tests/seeded_defects.rs`
- Test: this task is tests

**Interfaces:**
- Consumes: the `project`/`check` idiom from `check.rs` (copy the
  helpers; test files cannot import each other).
- Produces: the recall gate — nine assertions, one per identifier.

- [ ] **Step 1: Write the suite (it must pass immediately — the
  families exist; a failure here is a task-4-through-9 bug)**

```rust
//! The seeded-defect recall suite (design section 9): the gate a
//! silent engine cannot pass. One known defect per identifier; each
//! MUST be reported through the full product pipeline. These
//! fixtures are the family's substance contract — never weaken an
//! assertion to unblock a refactor.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

const MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

fn seeded(identifier: &str, source: &str) {
    let root = project(&[("composer.json", MANIFEST), ("src/Seed.php", source)]);
    let (outcome, output) = check(root.path());
    assert_eq!(
        outcome,
        Outcome::DiagnosticsReported,
        "{identifier} must be reported:\n{output}",
    );
    assert!(
        output.contains(identifier),
        "{identifier} must appear in the report:\n{output}",
    );
}

#[test]
fn cel0030_a_known_unknown_method_is_reported() {
    seeded("CEL0030", r#"<?php
namespace App;
class User { public function save(): void {} }
function f(User $u): void { $u->svae(); }
"#);
}

#[test]
fn cel0031_a_known_unknown_property_is_reported() {
    seeded("CEL0031", r#"<?php
namespace App;
class User { public string $name = ''; }
function f(User $u): void { $x = $u->nmae; }
"#);
}

#[test]
fn cel0032_a_known_unknown_class_constant_is_reported() {
    seeded("CEL0032", r#"<?php
namespace App;
class Config { public const LIMIT = 10; }
function f(): int { return Config::LIMTI; }
"#);
}

#[test]
fn cel0033_a_known_unknown_enum_case_is_reported() {
    seeded("CEL0033", r#"<?php
namespace App;
enum Status { case Active; }
function f(): Status { return Status::Draft; }
"#);
}

#[test]
fn cel0034_a_known_null_dereference_is_reported() {
    seeded("CEL0034", r#"<?php
namespace App;
class User { public function save(): void {} }
function f(?User $u): void { $u->save(); }
"#);
}

#[test]
fn cel0035_a_known_wrong_argument_is_reported() {
    seeded("CEL0035", r#"<?php
declare(strict_types=1);
namespace App;
class Plain {}
function takes(int $n): void {}
function f(Plain $p): void { takes($p); }
"#);
}

#[test]
fn cel0036_a_known_missing_argument_is_reported() {
    seeded("CEL0036", r#"<?php
namespace App;
function pair(int $a, int $b): void {}
function f(): void { pair(1); }
"#);
}

#[test]
fn cel0037_a_known_excess_argument_is_reported() {
    seeded("CEL0037", r#"<?php
namespace App;
function single(int $a): void {}
function f(): void { single(1, 2); }
"#);
}

#[test]
fn cel0038_a_known_unknown_named_argument_is_reported() {
    seeded("CEL0038", r#"<?php
namespace App;
function single(int $a): void {}
function f(): void { single(b: 1); }
"#);
}
```

(Note `cel0035` seeds a class-against-int mismatch, not a
scalar-against-scalar one, so the fixture reports in **both**
coercion modes — the seed must not depend on the file's mode beyond
what it declares.)

- [ ] **Step 2: Run the suite**

Run: `cargo test --package celerrate_cli --test seeded_defects`
Expected: PASS — nine of nine. Any failure is a recall bug in tasks
4–9: fix it there (with the seeded fixture reduced into that task's
unit tests), never by weakening the seed.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_cli/tests/seeded_defects.rs
git commit -m "✅ test(cli): the seeded-defect recall suite gates the three families"
```

---

### Task 12: Corpus triage — the guillotine's first application

The pinned symfony/demo corpus runs with the families live. The exit
of this task is a **triaged, re-blessed snapshot**: every typed line
is either fixed (a false positive, with a regression fixture) or
verified true. This is deliberately open-ended work with a closed
protocol — budgeted, like plan 6's ground-truth triage.

**Files:**
- Modify: `xtask/src/corpus.rs` (the typed refusal list, conditional)
- Modify: `xtask/corpus-snapshot.txt` (re-blessed)
- Modify: family fixtures for every false positive found
- Test: `cargo xtask corpus`

**Interfaces:**
- Consumes: `cargo xtask corpus [--bless]`, the release binary, the
  committed snapshot, `unknown_symbol_violations` (the hard-refusal
  pattern).
- Produces: the re-blessed snapshot; possibly
  `TYPED_MEMBER_IDENTIFIERS` joining the hard-refusal check; the
  closing triage memo.

- [ ] **Step 1: Run the corpus cold**

Run: `cargo xtask corpus`
Expected: likely a snapshot divergence — the report now carries
typed lines. `target/corpus/actual-snapshot.txt` holds the actual.

- [ ] **Step 2: The triage loop**

For every typed line in the actual snapshot, in identifier order:

1. Reproduce it minimally: extract the receiver/callee shape into a
   unit fixture in the owning family's test module.
2. Classify:
   - **False positive** — the fixture shows working PHP reported.
     Fix the stance (usually: a receiver shape that must be
     `PossiblyExists`, a coercion the mode must un-fail, an arity
     silencer). The fixture stays as the regression test. Re-run the
     corpus. This is the guillotine doing its job.
   - **True positive** — the corpus code is genuinely defective (or
     depends on vendor annotations that are wrong). Keep the line;
     it will be blessed. Record it in the memo with one sentence of
     verification.
3. Loop until every remaining typed line is a classified true
   positive.

Rules of the loop: never fix a false positive by weakening a seeded
defect; never bless a line you have not classified; if one family's
false positives resist stance-level fixes (they demand machinery
this plan does not own — e.g. a narrowing form outside the floor),
STOP and record it as a **guillotine candidate** in the memo — the
family cut is plan 9c's release decision, not a silent snapshot
bless.

- [ ] **Step 3: Bless and pin the refusal**

Run: `cargo xtask corpus --bless`
Then, **only if** the triage left `CEL0030`–`CEL0033` at zero
occurrences, extend the hard-refusal in `xtask/src/corpus.rs`:

```rust
/// Unknown-member diagnostics on the corpus are refused even under
/// `--bless` — the same posture as unknown symbols: a false positive
/// here is a priority bug, never a snapshot entry.
const TYPED_MEMBER_IDENTIFIERS: [&str; 4] =
    ["CEL0030", "CEL0031", "CEL0032", "CEL0033"];
```

wired next to `unknown_symbol_violations` with the same
line-scanning shape and the same un-blessable failure. If triage
left verified true positives in those families, skip the refusal
list (the snapshot equality already gates regressions) and say so in
the memo.

- [ ] **Step 4: Re-run the sibling harnesses**

Run: `cargo xtask ground-truth && cargo xtask mixed-rate`
Expected: green, or baseline moves reviewed and re-blessed with
classifications preserved (plan 6/7 rules). The task-1 anonymous
precision and any triage stance fixes can legitimately move both.

- [ ] **Step 5: Write the triage memo and commit**

Append the memo to this plan file under a `## Corpus triage memo`
heading: per family — lines found, false positives fixed (with the
fixture names), true positives blessed (with one-line
verifications), guillotine candidates if any.

```bash
git add xtask crates .claude/superpowers/plans/2026-07-16-type-engine-8-checks.md
git commit -m "✅ test(corpus): the typed families triaged clean on symfony/demo"
```

---

### Task 13: Closure — invalidation, determinism, and the debt ledger

**Files:**
- Modify: `crates/celerrate_types/tests/invalidation_scope.rs`
- Modify: `crates/celerrate_types/tests/fixpoint.rs` (or wherever
  the plan-6/7 determinism fixtures landed)
- Modify: rustdoc seams for the ledger (step 3)
- Test: this task is tests

**Interfaces:**
- Consumes: `TestDatabase::take_executed`/`executions_of`, the
  established edit-and-count fixtures.
- Produces: pins only, plus the ledger.

- [ ] **Step 1: Write the invalidation pins (harness 2 over the
  typed edit classes)**

In `invalidation_scope.rs`, the suite's edit-and-count shape:

```rust
#[test]
fn a_body_edit_rechecks_only_the_editing_body() {
    let before = r#"<?php
class User { public function save(): void {} }
function editing(User $u): void { $u->save(); }
function bystander(User $u): void { $u->save(); }
"#;
    let after = before.replace("function editing(User $u): void { $u->save(); }",
                               "function editing(User $u): void { $u->save(); $u->save(); }");
    let mut f = checks_fixture(&[before]);
    let _ = typed_file_verdicts(&f.db, f.files, f.stubs, f.configuration, f.handle(0));
    f.db.take_executed();
    f.set_source(0, &after);
    let _ = typed_file_verdicts(&f.db, f.files, f.stubs, f.configuration, f.handle(0));
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "body_typed_verdicts"),
        1,
        "only the editing body re-checks: {log:?}",
    );
}

#[test]
fn an_edit_above_a_body_reruns_only_the_mapping() {
    // A comment line prepended above every body: offsets shift, the
    // source map changes, the verdicts backdate.
    let before = r#"<?php
class User { public function save(): void {} }
function f(?User $u): void { $u->save(); }
"#;
    let after = before.replace("<?php", "<?php\n// a comment line");
    let mut f = checks_fixture(&[before]);
    let _ = typed_diagnostics(&f.db, f.files, f.stubs, f.configuration, f.handle(0));
    f.db.take_executed();
    f.set_source(0, &after);
    let second = typed_diagnostics(&f.db, f.files, f.stubs, f.configuration, f.handle(0));
    let log = f.db.take_executed();
    assert_eq!(executions_of(&log, "body_typed_verdicts"), 0,
        "range-free verdicts backdate under an offset shift: {log:?}");
    assert_eq!(second.len(), 1, "the diagnostic moved with its range");
}

#[test]
fn a_callee_signature_edit_rechecks_the_calling_body() {
    // Non-coercible on purpose: a class argument against `Plain`
    // holds before the edit and fails against `Other` after it, in
    // either coercion mode — the edit genuinely flips the verdict.
    let before = r#"<?php
class Plain {}
class Other {}
function takes(Plain $p): void {}
function caller(Plain $p): void { takes($p); }
"#;
    let after = before.replace("function takes(Plain $p)", "function takes(Other $p)");
    let mut f = checks_fixture(&[before]);
    assert!(typed_file_verdicts(&f.db, f.files, f.stubs, f.configuration, f.handle(0))
        .verdicts.is_empty());
    f.set_source(0, &after);
    let second = typed_file_verdicts(&f.db, f.files, f.stubs, f.configuration, f.handle(0));
    assert_eq!(second.verdicts.len(), 1, "the callee's new parameter type reaches the caller");
}
```

(Spell `checks_fixture`/`set_source`/`handle` on the file's existing
helpers.)

- [ ] **Step 2: Write the determinism pin**

Next to the plan-6/7 fixtures:

```rust
#[test]
fn typed_diagnostics_are_identical_across_fresh_databases() {
    let render = || {
        let f = checks_fixture(&[r#"<?php
class A { public function shared(): void {} }
class B {}
function f(A|B $either, ?A $nullable): void {
    $either->nowhere();
    $nullable->shared();
}
"#]);
        typed_diagnostics(&f.db, f.files, f.stubs, f.configuration, f.handle(0))
            .iter()
            .map(|d| format!("{} {}", d.id.as_str(), d.message))
            .collect::<Vec<String>>()
    };
    assert_eq!(render(), render());
}
```

The thread-count byte-identity over the full product is already the
corpus/equivalence harness's job — extended in task 10; nothing more
to build here.

- [ ] **Step 3: The re-export audit and the debt ledger**

1. Surface audit: `celerrate_types` adds exactly the decision-17
   names; `checks::receivers`, the family walkers, and
   `display_with_names` stay `pub(crate)`; `celerrate_semantics`
   adds `anonymous_class_key`, `parse_anonymous_class_key`,
   `class_surface`/`ClassSurface`, `file_strict_types`, and the
   widened `ExpressionId::from_index`.
   `cargo xtask dependency-shape` green.
2. Record the debts as rustdoc at their seams, one line each,
   naming the owner:
   - top-level statement code carries no member-tree body and is
     unchecked by the typed families (`checks/mod.rs`, the
     enumeration doc; owner: the rule framework of sub-project 4,
     or earlier if the corpus demands it).
   - union receivers are silent for the argument family
     (`arguments.rs::resolved_call_signature`; owner: a
     "possibly wrong argument" refinement, future).
   - a union argument source that partially fits its parameter is
     silent (`arguments.rs`, the decision-10 guard; owner: a
     "possibly invalid argument" refinement, future).
   - typed checks never run inside trait-owned bodies
     (`checks/mod.rs`, the enumeration filter; owner: a
     per-using-class walk over plan 6's `InferenceContext` seam,
     future).
   - scalar, array, and callable receivers are silent — a "call on
     non-object" family is future work (`receivers.rs::atoms_of`).
   - template receivers are silent; through-bound reporting is the
     stated `CannotProve` posture (`receivers.rs`).
   - `class-string` scoped subjects are silent
     (`members.rs::scoped_subject_keys`).
   - missing-on-some-constituents is a future "possibly undefined
     member" diagnostic (`receivers.rs::member_existence`).
   - dynamic member names are silent across all families
     (`members.rs`, `nullability.rs`).
   - duplicate argument binding (positional plus named for one
     parameter) is silent (`arguments.rs::check_arity`).
   - shape-value spreads silence the argument checks too (decision
     12 silences every spread; design section 8 qualifies only
     unpacking of a non-shape value) —
     arity-through-known-array-shapes is future work
     (`arguments.rs`, the spread guard).
   - the typed families are recomputed on warm runs; the
     typed-artifact cache classes are plan 9a's
     (`analysis.rs::typed_portion`).
3. Sweep the forward-pointing comments this plan fulfilled and
   reword each to shipped behavior: `judgments.rs`'s
   "the coercion posture lands in plan 8" note on `assignable_to`,
   the `Proof` rustdoc's "plan 8 is where those postures are
   declared" (the postures exist now: name them),
   `inference.rs`'s decision-13 counter comment (the printed line
   exists now), `flow.rs`'s anonymous-receiver ledger line (closed
   by task 1), the plan-6 `BodyOwner` anonymous rustdoc, and any
   remaining "(plan 8)" marker `grep -rn "plan 8" crates` still
   finds that this plan satisfied. Verify task 3 already reworded
   the rendering-debt comments (`display.rs`, `construction.rs`,
   `lib.rs`) and `extract.rs`'s implicit-enum-parents comment; a
   "plan 8" marker this plan deliberately did not satisfy is
   re-homed with an owner, never left dangling.

- [ ] **Step 4: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo xtask dependency-shape && cargo xtask corpus && cargo xtask ground-truth && cargo xtask mixed-rate`
Expected: everything green.

```bash
git add crates xtask
git commit -m "✅ test(types): the typed edit classes and determinism pinned"
```

---

## Execution notes

- Tasks run strictly in order. **This plan builds on plan 6's and
  plan 7's shipped names** (`inferred_body_types` with
  `InferenceContext`, `lookup_member`, `linearized_class`,
  `declared_member_signature`, `edge_counts`). Where a plan-6/7 name
  shipped slightly differently, follow the shipped code — the
  Interfaces blocks state the contracts, the repository states the
  spellings. **Plans 6 and 7 must be merged before this plan
  starts.**
- The `inferred_body_types` signature grew an `InferenceContext`
  parameter in plan 6. Every code block in this plan spells the
  older six-parameter form and omits the context, deliberately
  (plan 6's final spelling was unknown at writing time): add the
  parameter mechanically at implementation time, following the
  shipped signature (`InferenceContext::new(db, None)` outside
  trait bodies). Same for `SymbolSources` in `resolve_name` calls
  (the shipped spelling lives in `reference_checks.rs`).
- Display assertions: when an expected string disagrees with
  `display.rs`'s rendering or a body index disagrees with the
  `AstId` numbering, fix the expectation, never the code (the plan-5
  rule, reconducted).
- The corpus snapshot changes **only** in task 12, deliberately.
  Through tasks 1–11, `cargo xtask corpus` is expected to diverge
  once the families compose (task 10) — run it locally for
  information, but the gate's re-bless belongs to the triage task.
  If CI runs the corpus job on every push, land tasks 10–12 as one
  push or accept the red window knowingly.
- The anti-false-positive smoke suite
  (`celerrate_semantics/tests/false_positives.rs`) must stay green
  from task 4 on — it is the in-repository miniature of the corpus
  gate.
- No product-surface work here beyond rendering: README, CHANGELOG,
  and identifier documentation are plan 9c's; the `CEL0030+`
  identifiers stay repository-documented only.


---

## Corpus triage memo

Cold `cargo xtask corpus` over the pinned symfony/demo (commit
`03fe2567…`) diverged with **17 typed lines**: 0 unknown-member
(CEL0030–CEL0033), 1 nullability (CEL0034), 13 argument-type (CEL0035),
3 too-few-arguments (CEL0036); no CEL0037/CEL0038. Every one of the 17
was reproduced minimally in its owning family's test module and
classified a **false positive** on correct code; each was closed by a
family **stance** fix landing with a regression fixture. No true
positives, no guillotine candidates. The re-blessed snapshot is
`0 notices, 0 diagnostics` (byte-identical to the committed one). After
every stance fix `cargo test --package celerrate_types checks` (37
tests) and `cargo test --package celerrate_cli --test seeded_defects`
(9 tests) stayed green.

### Unknown members — CEL0030 / CEL0031 / CEL0032 / CEL0033
Lines found: **0**. The unknown-member family reported nothing on the
corpus. Because triage left all four at zero occurrences, they join the
hard-refusal list in `xtask/src/corpus.rs`
(`TYPED_MEMBER_IDENTIFIERS`, scanned by `typed_member_violations` with
the same line shape as `unknown_symbol_violations`): an unknown member
on this correct code is a priority bug, refused even under `--bless`,
never a snapshot entry. Test:
`corpus.rs::the_unknown_member_family_is_caught_line_by_line`.

### Nullability — CEL0034
Lines found: **1**.
- `src/EventSubscriber/CheckRequirementsSubscriber.php:63` —
  `$event->getCommand()->getName()` inside
  `if ($event->getCommand() && \in_array($event->getCommand()->getName(), …))`.
  **False positive**: the `getName()` receiver is a *repeated*
  `getCommand()` call whose `?Command` the plan-5/6 narrowing floor
  does not track (`narrowing::subject_of` narrows variables and
  `$this->prop` chains only, never a call result), so the `&&` guard is
  invisible to the check and the report is spurious on working PHP.
- **Stance**: nullability skips a receiver that is itself a `Call`
  expression — its `|null` is a return-type artefact the floor can
  neither narrow nor see guarded, so silence (design section 8: an
  undecidable receiver is never a guess). A variable or property
  receiver holding a nullable call result
  (`$p = $u?->profile(); $p->refresh();`) is narrowable and still
  reports.
- **Fixture**:
  `nullability.rs::a_call_result_receiver_is_not_a_tracked_dereference`.
- **Recall reduction, recorded as deferred debt**: this stance
  silences *every* nullable call-result receiver, which is broader
  than the exact guarded-`&&` false-positive shape — an unguarded
  `getThing()->doStuff()` on a `?Thing` return is now silent too (a
  real dereference PHPStan would flag). This is the conservative
  direction under the guillotine (the plan-5/6 floor cannot
  distinguish a guarded from an unguarded call-result subject, so the
  whole category is undecidable), but it is a genuine recall loss.
  Tighten it when the narrowing floor gains call-result subject
  tracking (`narrowing::subject_of` extended beyond variables and
  `$this->prop` chains). Deferred, same posture as the CEL0036
  stub-optionality alternative below.

### Argument types — CEL0035
Lines found: **13** — 5 callable-inner-signature, 8 array-generic.
- Callable (Symfony console `ask`×4, `askHidden`×1, in
  `AddUserCommand.php` / `DeleteUserCommand.php`):
  `callable(string|null): string` first-class callables
  (`$this->validator->validate…(...)`) against the `?callable`
  validator parameter, which phpdoc types `callable(mixed): mixed|null`.
- Array (`findBy`, `\array_slice`, `array_unique`, private `trim`,
  `implode`, `setInputs`, `executeCommand`×2): array arguments against
  array parameters whose phpdoc type arguments diverge —
  `array<TKey, TValue>` (refined `array_slice`/`array_unique`),
  `array<string, mixed>`, `list<string>`, an empty `array{}`, and
  `UnicodeString[]` against `string[]`.
- **False positives, all**: PHP verifies only the outer runtime kind
  (`array` / `callable`) at a parameter boundary — never the phpdoc
  type arguments nor a callable's declared inner signature — so none of
  these raise a runtime `TypeError`. The reports came from the shipped
  `judge`/`subtype_of` refuting a nested `mixed`/template array value
  (`candidate.is_mixed → Fails` fires below the top-level guard) and the
  contravariant `mixed` callable parameter.
- **Stance**: a source-argument constituent that shares an *unenforced
  container kind* with the parameter — both array-shaped
  (`Array`/`Shape`) or both callable-shaped (`Callable`) — never counts
  as failing (`shares_unenforced_container_kind`), folded into
  decision 10's per-constituent union test. Cross-kind mismatches
  (scalar vs array, scalar vs callable) share no kind and still report.
- **Fixtures**:
  `arguments.rs::a_matching_array_kind_is_not_enforced_on_its_element_types`
  (embedded `array_slice`, the faithful `AppFixtures` reproduction) and
  `arguments.rs::the_unenforced_container_predicate_covers_arrays_and_callables`
  (both branches directly, including the `?callable` nullable-union
  parameter; the parametrised `callable(...)` type text only lowers
  through the compiled overlay, so the callable branch is pinned at the
  predicate).

### Too few arguments — CEL0036
Lines found: **3** — all `mt_rand()` (`BlogControllerTest.php` ×3).
- **False positive**: `mt_rand()` (zero arguments) is valid,
  documented PHP, but phpstorm-stubs declares
  `mt_rand(int $min, int $max): int` with the parameters marked
  `[optional]` in the docblock while the *signature* carries no
  default; the stub compiler (`celerrate_stubs …/extract.rs`, line
  ~468) reads optionality from the signature default alone, so both are
  recorded required. A vendor-stub gap (`rand` shares the exact shape).
- **Stance**: CEL0036 fires for **source callees only**
  (`resolved.source_body.is_some()`). A stub signature's optionality is
  not decidably complete under this `[optional]` convention, so
  too-few stays silent for stub callees — the guillotine's mandated
  over-suppression when a signature is not provably complete. CEL0037
  (excess) and CEL0038 (unknown name) are untouched: the gap can only
  over-declare a parameter as required, never drop one. Source callees
  keep full too-few recall (the seeded defect `pair(1)` still reports).
- **Fixture**:
  `arguments.rs::too_few_arguments_is_reported_for_source_callees_only`.
- **Recorded alternative (more precise, deferred)**: honour the
  `@param … [optional]` docblock marker in
  `celerrate_stubs …/compiler/extract.rs` and recompile `stubs.bin`,
  which would restore stub too-few recall for genuinely-required
  parameters. Deferred from this triage task to avoid a compiled-blob
  regeneration reaching into plan-7 stub-compiler territory; the
  source-only stance is the contained fix and can be revisited in
  plan 9b/9c.

### Sibling harnesses
- `cargo xtask ground-truth`: checked 18, divergences 1 — **baseline
  holds**, no re-bless.
- `cargo xtask mixed-rate`: **matches the committed baseline**, no
  re-bless.
- Neither moved: all three stances narrow argument/nullability
  *reporting* only and never touch inferred *return* types, which is
  what those two harnesses measure. Task 1's anonymous-class precision
  landed before this task and had already been absorbed by their
  baselines.

### Guillotine candidates
**None.** Every one of the 17 typed lines was a false positive closed
by a family stance with a regression fixture; the seeded-defect recall
suite and all family unit tests stayed green after each fix.
