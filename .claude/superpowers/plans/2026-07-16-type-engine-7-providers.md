# Type Engine 7 — Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The provider layer: the stdlib type provider (a first-party
plugin computing the stdlib signatures no declarative stub can
express — `array_map` from its callable, `json_decode` from its flags,
`preg_match` `$matches` shapes, `array_filter`, `explode`, `current`),
the by-reference contribution channel the `$matches` refinement
demands, the "Celerrate refinements" overlay (the functionMap
equivalent: enriched stub signatures written in the internal Celerrate
norm, merged into the blob at build time), stub-class refinements that
settle plan 6's stub-ancestor generics debt, the residual mixed-rate
instrument with its committed corpus baseline, and the curation
workstream with its measured exit. The norm draft finds its first real
consumer and answers its open questions. Design source:
`.claude/superpowers/specs/2026-07-14-type-engine-design.md`, sections
7 (providers, stub curation, the norm draft), 4 (dispatch rules, the
claims model, "extended, never bypassed", the WASM sketch families), 3
(the stub signature payload and source precedence), 9 (the substance
metric stub curation uses), and 11 item 10 (this plan). The norm
draft: `.claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md`.

**Architecture:** One new crate, `celerrate_stdlib_provider`,
depending only on `celerrate_plugin` (the bridge's shape, enforced by
`cargo xtask dependency-shape`). `celerrate_types` gains `norm.rs`
(lowering norm text into lattice types — the norm is first-party and
internal, so its parser lives with the lattice, not behind the plugin
API) and a defaulted `by_reference_types` method on the
`DynamicTypeProvider` trait. `celerrate_stubs` gains the refinements
payload (`refinements.rs`), the live third blob section
(`SECTION_OVERLAYS`), and a compiler-side parser for the
`refinements.celerrate` source file. `celerrate_types/declared.rs`
consults function refinements at the stub-signature fold under the
existing three-valued `refine` rule; `inheritance.rs` (plan 6) grows a
stub branch so class refinements thread templates and generic
ancestors. The CLI grows a hidden `mixed-rate` subcommand; `xtask`
grows the baseline gate that measures curation's exit.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), salsa 0.27.2
(singleton registry inputs at HIGH durability), the plan-5/6 inference
engine (`inferred_body_types`, the four call tiers, the call-site
solver, iteration typing), the plan-3 stub signature fold
(`value_type_across_range`, `parameter_type_across_range`, `refine`),
the plan-4a plugin facade (`celerrate_plugin`), the stub compiler
(`--features compiler`, `cargo xtask compile-stubs`), the corpus pin
(`xtask/corpus.pin`, symfony/demo).

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
  No indexing: `.get()`, `.first()`, iterators, `.split_once()`.
  The norm parser and the pattern scanner take arbitrary text and
  must never panic — same contract as the docblock lexer.
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
- **Providers are pure functions of their `Invocation`**: no wall
  clock, no randomness, no environment reads, no state across calls
  (the stdlib provider is a zero-sized type like the bridge). A
  provider never controls termination: every contribution is widened
  at the consumption boundary (`capped_child`), on both the return
  channel and the new by-reference channel.
- **Provider answers are concrete**: never a template, never a
  late-static-binding placeholder. The call-site solver is exempt for
  the provider tier (plan 6 pinned this); the stdlib provider computes
  concrete types from concrete argument types and answers `None`
  otherwise.
- **Conservative silence, never a guess**: every handler's fallback
  is `None` (the call falls through to the declared tier), and every
  refined text that fails to lower falls back to the native fold —
  never a crash, never `never`, never a fabricated literal.
- **No diagnostics ship from this plan**: no new `CEL####`
  identifier, no rendering change. The `mixed-rate` subcommand is
  internal — hidden from `--help`, undocumented (plan 9c owns the
  product surface); it prints counters, never diagnostics.
- **The norm stays internal**: no public documentation, no README
  mention, no stability promise. The draft revision happens in
  `.claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md`,
  nowhere user-facing.
- **Strict layering**: the new crate `celerrate_stdlib_provider`
  depends on `celerrate_plugin` only (dev-dependencies exempt);
  `xtask/src/dependency_shape.rs`'s `PLUGIN_CRATES` grows the crate
  name in the same task that creates it. No other new inter-crate
  edge: `celerrate_types` already depends on `celerrate_stubs`;
  `norm.rs` stays `pub(crate)`.
- **Determinism**: claims are validated in registered order;
  refinement entries are sorted by key inside `StubRefinements::new`;
  the mixed-rate report sorts by callee key; the blob checksum stays
  byte-reproducible (`--check` in CI).
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`. Tasks that touch `celerrate_stubs` with the
  compiler feature also run
  `cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings`.

## Fixed decisions (the header the tasks implement)

1. **Two channels, split by computability.** The **refinements
   overlay** carries everything declaratively expressible: a
   signature whose precision needs only templates and the call-site
   solver (`array_keys(array<TKey, TValue>): list<TKey>`). The
   **stdlib provider** carries only what genuinely depends on the
   call's argument *values*: `array_map` (callable composition, null
   callback, the multi-array form), `array_filter` (mode flags),
   `current`/`reset`/`end` (value projection with the `false`
   miss), `explode` (limit sign), `json_decode` (associative flag and
   `JSON_OBJECT_AS_ARRAY`), `preg_match` (pattern-derived `$matches`
   shapes). Both lists are corpus-driven, extended in curation
   (task 11), never completeness-driven. Handlers read
   `Invocation.argument_types` positionally: arguments travel in
   written order, so a labeled argument occupies its written index
   and an unrecognized literal simply widens the answer (sound by
   construction; the by-reference channel additionally skips labeled
   arguments, decision 10).
2. **The norm parser lives in `celerrate_types/src/norm.rs`,
   `pub(crate)`.** The norm is Celerrate's own internal syntax, not a
   plugin notation, so it does not go through the type-syntax
   extension point and is not exposed on the facade. The v0 subset is
   exactly what refinements need (decision 13 lists the exclusions);
   `lower_norm_text` answers `Option<TypeId>` — `None` on anything
   outside the subset, tolerant of arbitrary bytes, never a panic.
3. **Refinement validation is two-stage.** The stub compiler (below
   `celerrate_types`, cannot lower) validates **structure and
   existence**: every refined function exists in the compiled
   snapshot, every refined class exists, every refined method exists
   on its class surface, every refined parameter name exists in the
   base signature — a violation fails `compile-stubs` with the entry
   named. `celerrate_types` validates **lowering totality**: a unit
   test iterates every text in the embedded blob's refinements and
   asserts `lower_norm_text` answers `Some` — a typo in
   `refinements.celerrate` is a CI failure, never a silent `mixed`.
4. **The blob's third section goes live and the format version bumps
   to 2.** `SECTION_OVERLAYS = 3` carries the encoded
   `StubRefinements`; `BLOB_FORMAT_VERSION = 2`; `table_entries = 3`.
   The enumerated pinned tests in `blob.rs` update in the same task
   (`the_blob_starts_with_magic_and_format_version`,
   `unknown_sections_are_skipped_for_forward_compatibility`,
   `a_blob_without_the_signature_section_decodes_with_empty_payloads`
   — every hand-built blob in tests gains the version-2 header). A
   missing overlays section decodes as empty refinements (the same
   tolerance the signatures section has), so hand-built two-section
   test blobs stay valid.
5. **Refinements override per element under the existing trust
   rule.** At the stub fold, a refined return or parameter text
   lowers through `norm.rs` and passes through `declared.rs::refine`
   against the delta-folded native type: `Holds` → `Trust::Refined`
   (a template candidate proves through its bound, so a
   `mixed`-bounded template in a value position against the fold's
   `mixed` value holds; refined returns commonly land here),
   `CannotProve` (a template in a position its bound cannot decide,
   `TKey` against a parameter's `int|string` key, principally) →
   `Trust::RefinedUnproven`, `Fails` → the native fold wins with
   `Trust::RejectedAnnotation` — a curation typo is structurally
   contained, exactly like a wrong docblock. Unrefined elements keep
   the plan-3 delta fold untouched (union for returns, most
   restrictive for parameters, `None` silences).
6. **Refinements are version-agnostic in v0.** One signature per
   function; the per-version deltas remain the base stubs' job (the
   norm draft's open question 1, answered by deferral — recorded in
   the draft revision, task 11). A refined element replaces the
   folded answer for the whole configured range.
7. **Function templates scope to the function key; class templates
   to the class key.** `array_keys`'s `TKey` lowers to
   `TypeId::template(db, "array_keys", "TKey", bound)`; the plan-6
   solver binds it at call sites through the existing
   `solver_pairs`/`solve`/`finalize_return` path with no new wiring
   (the solver already runs for both the function and method call
   paths when the result `contains_symbolic`). A refined stub
   method's texts lower under the class templates plus its own
   method templates.
8. **Stub-class refinements route through the plan-6 queries, not
   around them.** `class_templates` answers a stub-resolved class's
   templates from its `RefinedClass`; the `ancestor_arguments`
   composition walk lets a stub-owned edge contribute the refined
   ancestor arguments (lowered under the owner's template scope),
   replacing plan 6's "stub ancestors contribute no arguments" with
   "none unless curated". `resolve_stub_member_signature` consults
   the class refinement's methods. Consequence: `new
   ArrayIterator([1, 2])` solves `TValue = int` through constructor
   inference, `foreach` over it types through iteration typing's
   threaded-ancestors step, and plan 6's recorded stub-generics debt
   (`inheritance.rs` module doc) is settled on the curated classes.
9. **The provider crate is `celerrate_stdlib_provider`, public name
   `stdlib-provider`.** Zero-sized `StdlibProvider`, a `descriptor()`
   mirroring the bridge's, claims from a sorted
   `CLAIMED_FUNCTIONS: &[&str]` const, registered at the composition
   root after the bridge. `PLUGIN_CRATES` gains the crate name. The
   claim-conflict branch in `plugins.rs` becomes a real rebuild loop:
   exclusion removes the later registrant's registrations and
   re-validates until clean — the recorded plan-7 gap, closed.
10. **The by-reference channel is a defaulted trait method** —
    `DynamicTypeProvider::by_reference_types(db, invocation) ->
    Vec<(usize, TypeId)>` (positional parameter indices), default
    empty. Additive and object-safe: `PLUGIN_API_VERSION` stays 0.
    `flow.rs` applies contributions **after** `apply_by_reference`
    (the provider overrides the declared write-back), capped-widened
    per contribution, positional arguments only, application stops at
    a spread argument. Wired at the free-function call site; the
    method-call symmetry is a recorded debt (no method claimant
    exists). The WASM sketch gains the matching guest-export family
    in the same task.
11. **`preg_match` semantics, fixed.** Return: `0|1|false`. The
    `$matches` contribution (parameter index 2), flags judged first:
    when the flags argument is present and not the literal `0`,
    `array<int|string, mixed>` regardless of the pattern
    (`PREG_OFFSET_CAPTURE` and friends change the value shape).
    When the flags argument is absent or the literal `0` and the
    pattern argument is a string literal, a shape with **every field
    optional** — `0`, each capturing group's number, each named
    group's name (named groups contribute both keys), values
    `string`; all-optional is what makes the no-match `[]` a subtype
    of the answer. When the pattern is not a literal (flags still
    absent or `0`): `array<int|string, string>`. The group scanner
    handles paired
    and identical delimiters, escapes, character classes, `(?:`-style
    non-capturing groups, and the three named forms `(?P<name>`,
    `(?<name>`, `(?'name'`; it is a lexical scanner, not a regex
    parser — alternation-aware group optionality is out of scope
    (recorded).
12. **`json_decode` semantics, fixed (amended).** `JSON_OBJECT_AS_ARRAY
    = 1`. Real PHP (`ext/json/json.c`, verified against PHP 8.5.0)
    carries an explicit "for BC reasons" comment: a non-`null`
    `$associative` overrides the `JSON_OBJECT_AS_ARRAY` flag in BOTH
    directions. The associative argument (index 1): literal `true` →
    the array branch, regardless of the flags argument; literal
    `false` → the object branch, regardless of the flags argument. The
    flags argument (index 3) decides ONLY when `associative` is the
    `null` literal or absent (the signature is `?bool $associative =
    null` since PHP 7.4: an explicit `null` behaves exactly like an
    absent argument) — an integer literal with bit 1 set → the array
    branch, without it → the object branch, a non-literal flags
    argument → both branches (undecided). An associative argument that
    is neither a bool literal nor `null` → both branches, regardless
    of flags (it may be `false` at runtime). Answer: branch
    (`array<array-key, mixed>` and/or `stdClass`) unioned with
    `bool|float|int|string|null`. `null` stays in every answer
    (`"null"` decodes to `null`; `JSON_THROW_ON_ERROR` does not
    remove it).
13. **The norm v0 subset.** Lowered: the keyword atoms (`mixed`,
    `never`, `void`, `null`, `object`, `resource`, `bool`, `true`,
    `false`, `int`, `float`, `string`, `non-empty-string`,
    `numeric-string`, `literal-string`, `array-key` as sugar for
    `int|string`, `static`, `self`, `parent`), integer / float /
    string literals, integer ranges `int<1..5>` / `int<1..>` /
    `int<..5>`, `array<K, V>` and `non-empty-array<K, V>` (single
    argument sugar `array<V>` = `array<int|string, V>`), `list<T>`
    and `non-empty-list<T>`, `iterable<K, V>` (single argument sugar
    `iterable<V>` = `iterable<mixed, V>`: iterable keys are
    unconstrained, the array-key default is only correct for
    arrays), shapes
    `{id: int, name?: string}` with integer and identifier keys,
    `class-string` / `class-string<T>`, `key-of<T>` / `value-of<T>`,
    `callable(T, U=, V...): R`, enum cases `Status::Active`, class
    references with generic arguments, template references resolved
    against the scope, `?T`, unions, intersections, parentheses.
    Excluded and answering `None` (recorded as debt): conditional
    types, `key-of`/`value-of` over anything the constructors reject,
    callable by-reference parameters. Nullable binds tighter than
    `|` and `&`: `?A|B` is `(A|null)|B` (open question 2, answered).
14. **The mixed-rate instrument records at the source, not by
    re-derivation.** `InferredBody` gains
    `stub_calls: Vec<StubCallRecord { callee, mixed }>`, appended in
    the free-function call arm when the resolved key exists only in
    stubs (`source_exists == false` and the stub symbol table has the
    function) — the walker already knows both facts; nothing outside
    the walker re-implements callee resolution. The hidden
    `mixed-rate <path>` subcommand aggregates over every body:
    first line `expressions <total>\tmixed <count>`, then one
    `<callee>\t<mixed>\t<total>` line per stub callee, sorted by
    callee. `cargo xtask mixed-rate` runs it cold over the pinned
    corpus and byte-compares `xtask/mixed-rate-baseline.txt`;
    `--bless` rewrites it (the corpus-snapshot pattern, not the
    ground-truth merge — counters have no classification column).
    Scope, stated: the instrument records free-function calls only;
    stub method calls (the task-5 class-refinement channel) move the
    global expressions-mixed counter but never enter the per-callee
    table, so decision 15's per-callee exit does not see them
    (recorded in the task-12 ledger).
15. **Curation's measured exit.** After the curated set lands: the
    ten most-called stub callees in the corpus baseline each show
    `mixed == 0`, or carry a one-line debt entry naming why not; the
    before/after counter pairs (global expressions-mixed, per-callee)
    are recorded in the task-11 commit message. "Good enough for the
    corpus" is those numbers, not a feeling.
16. **Invalidation posture: refinements are binary-static.** They
    compile into `stubs.bin`, which is embedded; a refinements edit
    without recompilation is caught by `cargo xtask compile-stubs
    --check` in CI (the existing freshness gate — the blob hash
    changes with the refinements file). At runtime nothing new
    invalidates: the registry inputs stay HIGH-durability singletons,
    and a provider answer changes only when its `Invocation` changes
    (an argument-literal edit re-runs that body only — pinned in
    task 12).

## File structure

Created:

- `crates/celerrate_stdlib_provider/Cargo.toml`, `src/lib.rs`,
  `src/array_functions.rs`, `src/string_functions.rs`,
  `src/json_functions.rs`, `src/pattern_functions.rs` — the provider
  plugin: claims, dispatch, per-family handlers, the pattern group
  scanner.
- `crates/celerrate_types/src/norm.rs` — the norm text lowering
  (`NormTemplate`, `NormScope`, `lower_norm_text`), `pub(crate)`.
- `crates/celerrate_stubs/src/refinements.rs` — the payload
  (`StubRefinements`, `RefinedSignature`, `RefinedClass`,
  `RefinedTemplate`, `RefinedAncestor`) and its blob
  encoding/decoding.
- `crates/celerrate_stubs/src/compiler/refinement_source.rs`
  (feature `compiler`) — the `refinements.celerrate` parser and the
  existence validation.
- `crates/celerrate_stubs/refinements.celerrate` — the curated
  refinement entries (seeded in task 3, grown in task 11).
- `crates/celerrate_cli/src/mixed_rate.rs` — the hidden subcommand's
  aggregator.
- `xtask/src/mixed_rate.rs` — the corpus harness and baseline gate.
- `xtask/mixed-rate-baseline.txt` — the committed counter baseline
  (blessed in task 10, re-blessed in task 11).

Modified:

- `crates/celerrate_types/src/dynamic_type_provider.rs` — the
  defaulted `by_reference_types` method.
- `crates/celerrate_types/src/declared.rs` — refinement consultation
  in the stub fold (`resolve_stub_signature`,
  `resolve_stub_member_signature`, `declared_stub_parameter`).
- `crates/celerrate_types/src/inheritance.rs` — the stub branch in
  `class_templates` and the `ancestor_arguments` composition;
  constructor inference's template source confirmed on
  `class_templates`.
- `crates/celerrate_types/src/flow.rs` — `provider_by_reference`,
  `apply_provider_by_reference`, the `stub_calls` recording.
- `crates/celerrate_types/src/inference.rs` — `StubCallRecord`,
  the `InferredBody.stub_calls` field.
- `crates/celerrate_types/src/lib.rs` — module declaration `norm`,
  re-export `StubCallRecord`.
- `crates/celerrate_stubs/src/blob.rs` — version 2, the live third
  section, the pinned-test updates.
- `crates/celerrate_stubs/src/index.rs` — `StubIndex.refinements`,
  the `function_refinement`/`class_refinement` accessors, the
  four-parameter `new`.
- `crates/celerrate_stubs/src/lib.rs` — module declarations and
  re-exports for the refinements types.
- `crates/celerrate_stubs/src/bin/stub-compiler.rs` — the
  `--refinements <path>` argument, parse + validate + attach.
- `crates/celerrate_stubs/src/stubs.bin` — recompiled (tasks 3
  and 11).
- `crates/celerrate_cli/src/plugins.rs` — stdlib-provider
  registration, the claim-conflict rebuild loop.
- `crates/celerrate_cli/src/arguments.rs`, `src/lib.rs` — the hidden
  `mixed-rate` subcommand.
- `xtask/src/dependency_shape.rs` — `PLUGIN_CRATES` gains
  `celerrate_stdlib_provider`.
- `xtask/src/stubs.rs` — passes `--refinements` to the compiler.
- `xtask/src/main.rs`, `xtask/src/lib.rs` — `mixed-rate [--bless]`
  dispatch.
- `.github/workflows/corpus.yml` — the `mixed-rate` job.
- `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md` —
  the by-reference guest-export family.
- `.claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md` —
  the section-5 answers and the first-consumer note.

Task order is strict: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 →
12. Tasks 1–5 build the refinements channel bottom-up (lowering →
payload → compiler → declared tier → inheritance), 6–9 the provider,
10 the instrument, 11 the curation with its measured exit, 12 the
closure.

---

### Task 1: The norm lowering (`celerrate_types/src/norm.rs`)

**Files:**
- Create: `crates/celerrate_types/src/norm.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (module declaration
  only — `norm` stays `pub(crate)`)
- Test: `crates/celerrate_types/src/norm.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Consumes: `TypeId` constructors (`construction.rs`),
  `CallableParameter`/`ShapeField`/`ShapeKey` (`representation.rs`).
- Produces (later tasks rely on these exact names):
  - `pub(crate) struct NormTemplate<'db> { pub name: String, pub
    bound: Option<TypeId<'db>> }`
  - `pub(crate) struct NormScope<'db, 'a> { pub key: &'a str, pub
    templates: &'a [NormTemplate<'db>] }`
  - `pub(crate) fn lower_norm_text<'db>(db: &'db dyn
    salsa::Database, scope: &NormScope<'db, '_>, text: &str) ->
    Option<TypeId<'db>>` — `Some` for the decision-13 subset, `None`
    for everything else, never a panic, full-consume (trailing
    tokens fail the parse).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_types/src/norm.rs` with only the test
module first:

```rust
//! Lowering the Celerrate norm's written form into lattice types
//! (norm draft, sections 2 and 3). First consumer: the stub
//! refinements overlay (design section 7). Internal by design: the
//! norm is not a plugin notation, so this module is `pub(crate)`
//! and never crosses the facade. Tolerant: anything outside the v0
//! subset answers `None`, never a panic (decision 13).

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::{NormScope, NormTemplate, lower_norm_text};
    use crate::representation::TypeId;

    fn scope<'a>() -> NormScope<'static, 'a> {
        NormScope { key: "test_scope", templates: &[] }
    }

    fn lowered(db: &TestDatabase, text: &str) -> String {
        lower_norm_text(db, &scope(), text)
            .map(|type_id| type_id.display(db))
            .unwrap_or_else(|| "<none>".to_owned())
    }

    #[test]
    fn keyword_atoms_lower_to_their_constructors() {
        let db = TestDatabase::default();
        for (text, expected) in [
            ("mixed", "mixed"),
            ("never", "never"),
            ("void", "void"),
            ("null", "null"),
            ("object", "object"),
            ("resource", "resource"),
            ("bool", "bool"),
            ("true", "true"),
            ("false", "false"),
            ("int", "int"),
            ("float", "float"),
            ("string", "string"),
            ("non-empty-string", "non-empty-string"),
            ("numeric-string", "numeric-string"),
            ("literal-string", "literal-string"),
        ] {
            assert_eq!(lowered(&db, text), expected, "for {text}");
        }
    }

    #[test]
    fn array_key_is_sugar_for_int_or_string() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "array-key").unwrap(),
            TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
        );
    }

    #[test]
    fn literals_lower() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "42").unwrap(),
            TypeId::int_literal(&db, 42),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "-7").unwrap(),
            TypeId::int_literal(&db, -7),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "'active'").unwrap(),
            TypeId::string_literal(&db, "active"),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "3.5").unwrap(),
            TypeId::float_literal(&db, 3.5),
        );
    }

    #[test]
    fn integer_ranges_use_the_dotdot_spelling() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "int<1..5>").unwrap(),
            TypeId::int_range(&db, Some(1), Some(5)),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "int<1..>").unwrap(),
            TypeId::int_range(&db, Some(1), None),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "int<..5>").unwrap(),
            TypeId::int_range(&db, None, Some(5)),
        );
        // The PHPStan `min`/`max` keywords do not exist in the norm.
        assert_eq!(lowered(&db, "int<1, max>"), "<none>");
    }

    #[test]
    fn arrays_lists_and_iterables_lower() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        assert_eq!(
            lower_norm_text(&db, &scope(), "array<int, string>").unwrap(),
            TypeId::array(&db, int, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "array<string>").unwrap(),
            TypeId::array(&db, TypeId::union(&db, [int, string]), string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "non-empty-array<int, string>").unwrap(),
            TypeId::non_empty_array(&db, int, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "list<string>").unwrap(),
            TypeId::list(&db, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "non-empty-list<string>").unwrap(),
            TypeId::non_empty_list(&db, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "iterable<int, string>").unwrap(),
            TypeId::iterable(&db, int, string),
        );
        // Single-argument sugar: iterable keys are unconstrained.
        assert_eq!(
            lower_norm_text(&db, &scope(), "iterable<string>").unwrap(),
            TypeId::iterable(&db, TypeId::mixed(&db), string),
        );
    }

    #[test]
    fn shapes_drop_the_array_prefix_and_mark_optional_fields() {
        let db = TestDatabase::default();
        use crate::representation::{ShapeField, ShapeKey};
        assert_eq!(
            lower_norm_text(&db, &scope(), "{id: int, name?: string}").unwrap(),
            TypeId::shape(
                &db,
                vec![
                    ShapeField {
                        key: ShapeKey::String("id".to_owned()),
                        optional: false,
                        value: TypeId::int(&db),
                    },
                    ShapeField {
                        key: ShapeKey::String("name".to_owned()),
                        optional: true,
                        value: TypeId::string(&db),
                    },
                ],
            ),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "{0: string, 1?: string}").unwrap(),
            TypeId::shape(
                &db,
                vec![
                    ShapeField {
                        key: ShapeKey::Integer(0),
                        optional: false,
                        value: TypeId::string(&db),
                    },
                    ShapeField {
                        key: ShapeKey::Integer(1),
                        optional: true,
                        value: TypeId::string(&db),
                    },
                ],
            ),
        );
    }

    #[test]
    fn unions_intersections_and_nullable_compose() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "int|string").unwrap(),
            TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "Countable&Traversable").unwrap(),
            TypeId::intersection(
                &db,
                [
                    TypeId::class(&db, "countable", vec![]),
                    TypeId::class(&db, "traversable", vec![]),
                ],
            ),
        );
        // `?` binds tighter than `|` (norm open question 2, answered):
        // `?A|B` is `(A|null)|B`.
        assert_eq!(
            lower_norm_text(&db, &scope(), "?int|string").unwrap(),
            TypeId::union(
                &db,
                [TypeId::int(&db), TypeId::null(&db), TypeId::string(&db)],
            ),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "(A&B)|C").unwrap(),
            TypeId::union(
                &db,
                [
                    TypeId::intersection(
                        &db,
                        [
                            TypeId::class(&db, "a", vec![]),
                            TypeId::class(&db, "b", vec![]),
                        ],
                    ),
                    TypeId::class(&db, "c", vec![]),
                ],
            ),
        );
    }

    #[test]
    fn class_references_carry_generic_arguments() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "Collection<User>").unwrap(),
            TypeId::class(
                &db,
                "collection",
                vec![TypeId::class(&db, "user", vec![])],
            ),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), r"Doctrine\Common\Collections\Collection")
                .unwrap(),
            TypeId::class(&db, r"doctrine\common\collections\collection", vec![]),
        );
    }

    #[test]
    fn enum_cases_class_strings_and_projections_lower() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "Status::Active").unwrap(),
            TypeId::enum_case(&db, "status", "Active"),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "class-string").unwrap(),
            TypeId::class_string(&db, None),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "class-string<User>").unwrap(),
            TypeId::class_string(&db, Some(TypeId::class(&db, "user", vec![]))),
        );
        let subject = TypeId::class(&db, "config", vec![]);
        assert_eq!(
            lower_norm_text(&db, &scope(), "key-of<Config>").unwrap(),
            TypeId::key_of(&db, subject),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "value-of<Config>").unwrap(),
            TypeId::value_of(&db, subject),
        );
    }

    #[test]
    fn callables_lower_with_optional_and_variadic_markers() {
        let db = TestDatabase::default();
        use crate::representation::CallableParameter;
        assert_eq!(
            lower_norm_text(&db, &scope(), "callable(int, string=, bool...): void")
                .unwrap(),
            TypeId::callable(
                &db,
                vec![
                    CallableParameter {
                        parameter_type: TypeId::int(&db),
                        optional: false,
                        variadic: false,
                        by_reference: false,
                    },
                    CallableParameter {
                        parameter_type: TypeId::string(&db),
                        optional: true,
                        variadic: false,
                        by_reference: false,
                    },
                    CallableParameter {
                        parameter_type: TypeId::bool(&db),
                        optional: false,
                        variadic: true,
                        by_reference: false,
                    },
                ],
                TypeId::void(&db),
            ),
        );
        // An omitted return is `mixed`.
        assert_eq!(
            lower_norm_text(&db, &scope(), "callable(int)").unwrap(),
            TypeId::callable(
                &db,
                vec![CallableParameter {
                    parameter_type: TypeId::int(&db),
                    optional: false,
                    variadic: false,
                    by_reference: false,
                }],
                TypeId::mixed(&db),
            ),
        );
    }

    #[test]
    fn templates_resolve_against_the_scope_keywords_first() {
        let db = TestDatabase::default();
        let templates = vec![
            NormTemplate { name: "TKey".to_owned(), bound: None },
            NormTemplate {
                name: "TValue".to_owned(),
                bound: Some(TypeId::object(&db)),
            },
        ];
        let scope = NormScope { key: "array_keys", templates: &templates };
        assert_eq!(
            lower_norm_text(&db, &scope, "list<TKey>").unwrap(),
            TypeId::list(
                &db,
                TypeId::template(&db, "array_keys", "TKey", TypeId::mixed(&db)),
            ),
        );
        assert_eq!(
            lower_norm_text(&db, &scope, "TValue").unwrap(),
            TypeId::template(&db, "array_keys", "TValue", TypeId::object(&db)),
        );
        // A keyword shadows a template of the same spelling: names are
        // matched keywords-first.
        let shadowing = vec![NormTemplate { name: "int".to_owned(), bound: None }];
        let scope = NormScope { key: "x", templates: &shadowing };
        assert_eq!(
            lower_norm_text(&db, &scope, "int").unwrap(),
            TypeId::int(&db),
        );
    }

    #[test]
    fn placeholders_lower() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "static").unwrap(),
            TypeId::static_placeholder(&db),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "self").unwrap(),
            TypeId::self_placeholder(&db),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "parent").unwrap(),
            TypeId::parent_placeholder(&db),
        );
    }

    #[test]
    fn whitespace_is_insignificant() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), " array< int , string > ").unwrap(),
            TypeId::array(&db, TypeId::int(&db), TypeId::string(&db)),
        );
    }

    #[test]
    fn everything_outside_the_subset_answers_none_never_a_panic() {
        let db = TestDatabase::default();
        for text in [
            "",
            "(T is int ? A : B)", // conditionals: excluded from v0
            "int|",
            "array<int,",
            "{id int}",
            "int<1..5", // unterminated
            "callable(int",
            "list<string> extra",
            "'unterminated",
            "T[]",      // rule 6: the suffix form does not exist
            "?",
            "\u{0}\u{1}\u{2}",
            "int<<>>",
            "42.4.2",
        ] {
            assert!(
                lower_norm_text(&db, &scope(), text).is_none(),
                "expected None for {text:?}",
            );
        }
    }
}
```

Add `mod norm;` to `crates/celerrate_types/src/lib.rs` next to the
other private module declarations.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types norm`
Expected: FAIL to compile — `NormScope`, `NormTemplate`,
`lower_norm_text` not defined.

- [ ] **Step 3: Write the implementation**

Above the test module in `norm.rs`:

```rust
use crate::representation::{CallableParameter, ShapeField, ShapeKey, TypeId};

/// A template declared by the refinement entry under lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormTemplate<'db> {
    pub name: String,
    pub bound: Option<TypeId<'db>>,
}

/// The lowering context: the scope key template types intern under,
/// and the templates in scope (keywords shadow them).
pub(crate) struct NormScope<'db, 'a> {
    pub key: &'a str,
    pub templates: &'a [NormTemplate<'db>],
}

/// Lowers one norm type expression. `None` on anything outside the
/// v0 subset (decision 13), tolerant of arbitrary bytes.
pub(crate) fn lower_norm_text<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    text: &str,
) -> Option<TypeId<'db>> {
    let tokens = lex(text)?;
    let mut cursor = Cursor { tokens: &tokens, position: 0 };
    let lowered = union_type(db, scope, &mut cursor)?;
    cursor.at_end().then_some(lowered)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Integer(i64),
    Float(f64),
    Text(String),
    Question,
    Pipe,
    Ampersand,
    Comma,
    Colon,
    DoubleColon,
    LessThan,
    GreaterThan,
    OpenParenthesis,
    CloseParenthesis,
    OpenBrace,
    CloseBrace,
    Equals,
    Ellipsis,
    DotDot,
}

fn lex(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(&character) = characters.peek() {
        match character {
            character if character.is_whitespace() => {
                characters.next();
            }
            '?' => {
                characters.next();
                tokens.push(Token::Question);
            }
            '|' => {
                characters.next();
                tokens.push(Token::Pipe);
            }
            '&' => {
                characters.next();
                tokens.push(Token::Ampersand);
            }
            ',' => {
                characters.next();
                tokens.push(Token::Comma);
            }
            '<' => {
                characters.next();
                tokens.push(Token::LessThan);
            }
            '>' => {
                characters.next();
                tokens.push(Token::GreaterThan);
            }
            '(' => {
                characters.next();
                tokens.push(Token::OpenParenthesis);
            }
            ')' => {
                characters.next();
                tokens.push(Token::CloseParenthesis);
            }
            '{' => {
                characters.next();
                tokens.push(Token::OpenBrace);
            }
            '}' => {
                characters.next();
                tokens.push(Token::CloseBrace);
            }
            '=' => {
                characters.next();
                tokens.push(Token::Equals);
            }
            ':' => {
                characters.next();
                if characters.peek() == Some(&':') {
                    characters.next();
                    tokens.push(Token::DoubleColon);
                } else {
                    tokens.push(Token::Colon);
                }
            }
            '.' => {
                characters.next();
                match characters.peek() {
                    Some('.') => {
                        characters.next();
                        // Three dots are the variadic marker, two the
                        // range separator.
                        if characters.peek() == Some(&'.') {
                            characters.next();
                            tokens.push(Token::Ellipsis);
                        } else {
                            tokens.push(Token::DotDot);
                        }
                    }
                    _ => return None,
                }
            }
            '\'' => {
                characters.next();
                let mut value = String::new();
                loop {
                    match characters.next() {
                        Some('\'') => break,
                        Some(next) => value.push(next),
                        None => return None,
                    }
                }
                tokens.push(Token::Text(value));
            }
            '-' | '0'..='9' => tokens.push(lex_number(&mut characters)?),
            character if is_name_start(character) => {
                tokens.push(Token::Name(lex_name(&mut characters)));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

fn is_name_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_' || character == '\\'
}

fn is_name_continuation(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '\\'
}

/// One (possibly qualified) name. A hyphen continues the name only
/// when a letter follows: `non-empty-string` and `key-of` lex whole,
/// while `int<1..-5>` leaves the minus to the number lexer.
fn lex_name(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut name = String::new();
    while let Some(&character) = characters.peek() {
        if is_name_continuation(character) {
            name.push(character);
            characters.next();
        } else if character == '-' {
            let mut lookahead = characters.clone();
            lookahead.next();
            match lookahead.peek() {
                Some(next) if next.is_ascii_alphabetic() => {
                    name.push('-');
                    characters.next();
                }
                _ => break,
            }
        } else {
            break;
        }
    }
    name
}

/// An integer or float literal, optionally negative. Digits followed
/// by `..` stay an integer (the range separator is not a fraction).
fn lex_number(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Option<Token> {
    let mut digits = String::new();
    if characters.peek() == Some(&'-') {
        digits.push('-');
        characters.next();
    }
    while let Some(&character) = characters.peek() {
        if character.is_ascii_digit() {
            digits.push(character);
            characters.next();
        } else {
            break;
        }
    }
    if digits.is_empty() || digits == "-" {
        return None;
    }
    if characters.peek() == Some(&'.') {
        let mut lookahead = characters.clone();
        lookahead.next();
        if lookahead.peek().is_some_and(char::is_ascii_digit) {
            digits.push('.');
            characters.next();
            while let Some(&character) = characters.peek() {
                if character.is_ascii_digit() {
                    digits.push(character);
                    characters.next();
                } else {
                    break;
                }
            }
            return digits.parse().ok().map(Token::Float);
        }
    }
    digits.parse().ok().map(Token::Integer)
}

struct Cursor<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.position)?;
        self.position += 1;
        Some(token)
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn at_end(&self) -> bool {
        self.position == self.tokens.len()
    }
}

fn union_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    let mut constituents = vec![intersection_type(db, scope, cursor)?];
    while cursor.eat(&Token::Pipe) {
        constituents.push(intersection_type(db, scope, cursor)?);
    }
    Some(match constituents.as_slice() {
        [single] => *single,
        _ => TypeId::union(db, constituents),
    })
}

fn intersection_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    let mut intersectands = vec![atom_type(db, scope, cursor)?];
    while cursor.eat(&Token::Ampersand) {
        intersectands.push(atom_type(db, scope, cursor)?);
    }
    Some(match intersectands.as_slice() {
        [single] => *single,
        _ => TypeId::intersection(db, intersectands),
    })
}

fn atom_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    match cursor.advance()? {
        // `?T` binds tighter than `|` and `&` (decision 13).
        Token::Question => {
            let inner = atom_type(db, scope, cursor)?;
            Some(TypeId::union(db, [inner, TypeId::null(db)]))
        }
        Token::OpenParenthesis => {
            let inner = union_type(db, scope, cursor)?;
            cursor.eat(&Token::CloseParenthesis).then_some(inner)
        }
        Token::OpenBrace => shape_type(db, scope, cursor),
        Token::Integer(value) => Some(TypeId::int_literal(db, *value)),
        Token::Float(value) => Some(TypeId::float_literal(db, *value)),
        Token::Text(value) => Some(TypeId::string_literal(db, value)),
        Token::Name(name) => named_type(db, scope, cursor, name),
        _ => None,
    }
}

fn named_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    name: &str,
) -> Option<TypeId<'db>> {
    // Keywords first (they shadow templates and class names).
    match name {
        "mixed" => return Some(TypeId::mixed(db)),
        "never" => return Some(TypeId::never(db)),
        "void" => return Some(TypeId::void(db)),
        "null" => return Some(TypeId::null(db)),
        "object" => return Some(TypeId::object(db)),
        "resource" => return Some(TypeId::resource(db)),
        "bool" => return Some(TypeId::bool(db)),
        "true" => return Some(TypeId::bool_literal(db, true)),
        "false" => return Some(TypeId::bool_literal(db, false)),
        "float" => return Some(TypeId::float(db)),
        "string" => return Some(TypeId::string(db)),
        "non-empty-string" => return Some(TypeId::non_empty_string(db)),
        "numeric-string" => return Some(TypeId::numeric_string(db)),
        "literal-string" => return Some(TypeId::literal_string_type(db)),
        "array-key" => {
            return Some(TypeId::union(db, [TypeId::int(db), TypeId::string(db)]));
        }
        "static" => return Some(TypeId::static_placeholder(db)),
        "self" => return Some(TypeId::self_placeholder(db)),
        "parent" => return Some(TypeId::parent_placeholder(db)),
        "int" => return int_type(db, cursor),
        "array" => return array_type(db, scope, cursor, false),
        "non-empty-array" => return array_type(db, scope, cursor, true),
        "list" => return list_type(db, scope, cursor, false),
        "non-empty-list" => return list_type(db, scope, cursor, true),
        "iterable" => return iterable_type(db, scope, cursor),
        "class-string" => return class_string_type(db, scope, cursor),
        "key-of" => return projection_type(db, scope, cursor, TypeId::key_of),
        "value-of" => return projection_type(db, scope, cursor, TypeId::value_of),
        "callable" => return callable_type(db, scope, cursor),
        _ => {}
    }
    // `Enum::Case` before template and class references.
    if cursor.eat(&Token::DoubleColon) {
        let Some(Token::Name(case)) = cursor.advance() else {
            return None;
        };
        return Some(TypeId::enum_case(db, name, case));
    }
    // Templates in scope, then class references.
    if let Some(template) = scope
        .templates
        .iter()
        .find(|template| template.name == name)
    {
        return Some(TypeId::template(
            db,
            scope.key,
            &template.name,
            template.bound.unwrap_or_else(|| TypeId::mixed(db)),
        ));
    }
    let arguments = match generic_arguments(db, scope, cursor) {
        Some(arguments) => arguments?,
        None => vec![],
    };
    Some(TypeId::class(db, name, arguments))
}

/// `Some(inner)` when a `<...>` argument list is present (inner is
/// `None` on a malformed list), `None` when absent.
#[allow(clippy::type_complexity)]
fn generic_arguments<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<Option<Vec<TypeId<'db>>>> {
    if !cursor.eat(&Token::LessThan) {
        return None;
    }
    let mut arguments = Vec::new();
    loop {
        match union_type(db, scope, cursor) {
            Some(argument) => arguments.push(argument),
            None => return Some(None),
        }
        if cursor.eat(&Token::GreaterThan) {
            return Some(Some(arguments));
        }
        if !cursor.eat(&Token::Comma) {
            return Some(None);
        }
    }
}

fn int_type<'db>(
    db: &'db dyn salsa::Database,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    if !cursor.eat(&Token::LessThan) {
        return Some(TypeId::int(db));
    }
    let minimum = match cursor.peek() {
        Some(Token::Integer(value)) => {
            let value = *value;
            cursor.advance();
            Some(value)
        }
        _ => None,
    };
    if !cursor.eat(&Token::DotDot) {
        return None;
    }
    let maximum = match cursor.peek() {
        Some(Token::Integer(value)) => {
            let value = *value;
            cursor.advance();
            Some(value)
        }
        _ => None,
    };
    if minimum.is_none() && maximum.is_none() {
        return None;
    }
    cursor
        .eat(&Token::GreaterThan)
        .then(|| TypeId::int_range(db, minimum, maximum))
}

fn array_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    non_empty: bool,
) -> Option<TypeId<'db>> {
    let array_key = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
    let (key, value) = match generic_arguments(db, scope, cursor) {
        None => (array_key, TypeId::mixed(db)),
        Some(arguments) => match arguments?.as_slice() {
            [value] => (array_key, *value),
            [key, value] => (*key, *value),
            _ => return None,
        },
    };
    Some(if non_empty {
        TypeId::non_empty_array(db, key, value)
    } else {
        TypeId::array(db, key, value)
    })
}

fn list_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    non_empty: bool,
) -> Option<TypeId<'db>> {
    let value = match generic_arguments(db, scope, cursor) {
        None => TypeId::mixed(db),
        Some(arguments) => match arguments?.as_slice() {
            [value] => *value,
            _ => return None,
        },
    };
    Some(if non_empty {
        TypeId::non_empty_list(db, value)
    } else {
        TypeId::list(db, value)
    })
}

fn iterable_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    let (key, value) = match generic_arguments(db, scope, cursor) {
        None => (TypeId::mixed(db), TypeId::mixed(db)),
        Some(arguments) => match arguments?.as_slice() {
            // Iterable keys are unconstrained: the array-key default
            // is only correct for arrays (decision 13).
            [value] => (TypeId::mixed(db), *value),
            [key, value] => (*key, *value),
            _ => return None,
        },
    };
    Some(TypeId::iterable(db, key, value))
}

fn class_string_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    match generic_arguments(db, scope, cursor) {
        None => Some(TypeId::class_string(db, None)),
        Some(arguments) => match arguments?.as_slice() {
            [argument] => Some(TypeId::class_string(db, Some(*argument))),
            _ => None,
        },
    }
}

fn projection_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    construct: fn(&'db dyn salsa::Database, TypeId<'db>) -> TypeId<'db>,
) -> Option<TypeId<'db>> {
    match generic_arguments(db, scope, cursor)? {
        Some(arguments) => match arguments.as_slice() {
            [subject] => Some(construct(db, *subject)),
            _ => None,
        },
        None => None,
    }
}

fn callable_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    if !cursor.eat(&Token::OpenParenthesis) {
        // A bare `callable` carries no signature.
        return Some(TypeId::callable(db, vec![], TypeId::mixed(db)));
    }
    let mut parameters = Vec::new();
    if !cursor.eat(&Token::CloseParenthesis) {
        loop {
            let parameter_type = union_type(db, scope, cursor)?;
            let optional = cursor.eat(&Token::Equals);
            let variadic = cursor.eat(&Token::Ellipsis);
            parameters.push(CallableParameter {
                parameter_type,
                optional,
                variadic,
                by_reference: false,
            });
            if cursor.eat(&Token::CloseParenthesis) {
                break;
            }
            if !cursor.eat(&Token::Comma) {
                return None;
            }
        }
    }
    let return_type = if cursor.eat(&Token::Colon) {
        union_type(db, scope, cursor)?
    } else {
        TypeId::mixed(db)
    };
    Some(TypeId::callable(db, parameters, return_type))
}

fn shape_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    let mut fields = Vec::new();
    if cursor.eat(&Token::CloseBrace) {
        return Some(TypeId::shape(db, fields));
    }
    loop {
        let key = match cursor.advance()? {
            Token::Name(name) => ShapeKey::String(name.clone()),
            Token::Integer(value) => ShapeKey::Integer(*value),
            Token::Text(value) => ShapeKey::String(value.clone()),
            _ => return None,
        };
        let optional = cursor.eat(&Token::Question);
        if !cursor.eat(&Token::Colon) {
            return None;
        }
        let value = union_type(db, scope, cursor)?;
        fields.push(ShapeField { key, optional, value });
        if cursor.eat(&Token::CloseBrace) {
            return Some(TypeId::shape(db, fields));
        }
        if !cursor.eat(&Token::Comma) {
            return None;
        }
    }
}
```

Note on `lex_number`: `char::is_ascii_digit` takes `&char` through
the `is_some_and` adapter — if clippy objects to the method-path
form, use the closure `|character| character.is_ascii_digit()`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types norm`
Expected: PASS (14 tests). If a constructed-`TypeId` equality in a
test disagrees with a canonicalization the constructors perform,
adjust the test's expectation to the canonical construction — never
the parser (the constructors are the source of truth).

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_types/src/norm.rs crates/celerrate_types/src/lib.rs
git commit -m "✨ feat(types): the norm lowering for refinement texts"
```

---

### Task 2: The refinements payload and the blob's third section

**Files:**
- Create: `crates/celerrate_stubs/src/refinements.rs`
- Modify: `crates/celerrate_stubs/src/blob.rs`
- Modify: `crates/celerrate_stubs/src/index.rs`
- Modify: `crates/celerrate_stubs/src/lib.rs`
- Test: in-module `#[cfg(test)]` in `refinements.rs` and the
  existing `blob.rs` test module

**Interfaces:**
- Consumes: `write_string`/`Reader` (blob.rs private helpers),
  `StubBlobError`, `StubIndex`.
- Produces (later tasks rely on these exact names):
  - In `refinements.rs`, all `pub` and re-exported from `lib.rs`:
    `RefinedTemplate { pub name: String, pub bound: Option<String> }`,
    `RefinedSignature { pub templates: Vec<RefinedTemplate>, pub
    parameters: Vec<(String, String)>, pub return_type:
    Option<String> }`,
    `RefinedAncestor { pub name: String, pub arguments: Vec<String> }`,
    `RefinedClass { pub templates: Vec<RefinedTemplate>, pub
    ancestors: Vec<RefinedAncestor>, pub methods: Vec<(String,
    RefinedSignature)> }`,
    `StubRefinements { functions, classes }` with
    `StubRefinements::new(functions, classes)` (sorts both lists by
    key) and `StubRefinements::empty()`.
  - On `StubIndex`: `pub fn set_refinements(&mut self, refinements:
    StubRefinements)` (`new` keeps its three parameters so every
    existing caller compiles unchanged), and the accessors
    `pub fn function_refinement(&self, key: &str) ->
    Option<&RefinedSignature>` and
    `pub fn class_refinement(&self, key: &str) ->
    Option<&RefinedClass>` (binary search over the sorted lists),
    plus `pub fn refinements(&self) -> &StubRefinements` (the
    totality test of task 4 iterates it).
  - `BLOB_FORMAT_VERSION == 2`; `encode` writes three sections;
    `decode` reads `SECTION_OVERLAYS` (absent → empty refinements).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_stubs/src/refinements.rs`:

```rust
//! The Celerrate refinements overlay (design section 7): enriched
//! stub signatures written in the internal norm, compiled into the
//! blob's third section at build time and consulted upstairs by
//! `celerrate_types` at the stub-signature fold. Texts are opaque
//! strings here — this crate sits below the lattice and never
//! lowers them (decision 3: the compiler validates existence, the
//! types crate validates lowering totality).

/// One declared template: `TKey`, or `T of Foo` (the bound is a norm
/// text, lowered upstairs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefinedTemplate {
    pub name: String,
    pub bound: Option<String>,
}

/// A per-element signature override: only the named parameters and
/// (when present) the return are replaced; everything else keeps the
/// base stub's delta fold. Version-agnostic by decision 6.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefinedSignature {
    pub templates: Vec<RefinedTemplate>,
    /// Parameter name (without `$`) to norm text.
    pub parameters: Vec<(String, String)>,
    pub return_type: Option<String>,
}

/// One generic ancestor fixed by a class refinement:
/// `implements Iterator<TKey, TValue>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefinedAncestor {
    pub name: String,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefinedClass {
    pub templates: Vec<RefinedTemplate>,
    pub ancestors: Vec<RefinedAncestor>,
    /// Method name (folded) to signature refinement.
    pub methods: Vec<(String, RefinedSignature)>,
}

/// The whole overlay, keyed by folded symbol keys, both lists sorted
/// so lookups binary-search and the blob encoding is deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StubRefinements {
    pub functions: Vec<(String, RefinedSignature)>,
    pub classes: Vec<(String, RefinedClass)>,
}

impl StubRefinements {
    pub fn new(
        mut functions: Vec<(String, RefinedSignature)>,
        mut classes: Vec<(String, RefinedClass)>,
    ) -> Self {
        functions.sort_by(|left, right| left.0.cmp(&right.0));
        classes.sort_by(|left, right| left.0.cmp(&right.0));
        Self { functions, classes }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.classes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StubRefinements {
        StubRefinements::new(
            vec![(
                "array_keys".to_owned(),
                RefinedSignature {
                    templates: vec![
                        RefinedTemplate { name: "TKey".to_owned(), bound: None },
                        RefinedTemplate {
                            name: "TValue".to_owned(),
                            bound: Some("object".to_owned()),
                        },
                    ],
                    parameters: vec![(
                        "array".to_owned(),
                        "array<TKey, TValue>".to_owned(),
                    )],
                    return_type: Some("list<TKey>".to_owned()),
                },
            )],
            vec![(
                "arrayiterator".to_owned(),
                RefinedClass {
                    templates: vec![
                        RefinedTemplate { name: "TKey".to_owned(), bound: None },
                        RefinedTemplate { name: "TValue".to_owned(), bound: None },
                    ],
                    ancestors: vec![RefinedAncestor {
                        name: "iterator".to_owned(),
                        arguments: vec!["TKey".to_owned(), "TValue".to_owned()],
                    }],
                    methods: vec![(
                        "current".to_owned(),
                        RefinedSignature {
                            templates: vec![],
                            parameters: vec![],
                            return_type: Some("TValue".to_owned()),
                        },
                    )],
                },
            )],
        )
    }

    #[test]
    fn construction_sorts_by_key() {
        let refinements = StubRefinements::new(
            vec![
                ("b".to_owned(), RefinedSignature::default()),
                ("a".to_owned(), RefinedSignature::default()),
            ],
            vec![],
        );
        let keys: Vec<&str> = refinements
            .functions
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, ["a", "b"]);
    }

    #[test]
    fn the_payload_round_trips_through_its_encoding() {
        let refinements = sample();
        let mut bytes = Vec::new();
        super::encode_refinements(&refinements, &mut bytes);
        let decoded = super::decode_refinements(&bytes).unwrap();
        assert_eq!(decoded, refinements);
    }

    #[test]
    fn a_truncated_payload_reports_malformed_never_panics() {
        let refinements = sample();
        let mut bytes = Vec::new();
        super::encode_refinements(&refinements, &mut bytes);
        for length in 0..bytes.len() {
            let truncated = bytes.get(..length).unwrap_or_default();
            assert!(super::decode_refinements(truncated).is_err());
        }
    }
}
```

(The test module needs `#![allow(clippy::unwrap_used)]` at its top,
matching the crate's test-module convention.)

In `crates/celerrate_stubs/src/blob.rs`'s test module, add:

```rust
#[test]
fn the_format_version_is_two_and_the_table_carries_three_sections() {
    let blob = encode(&sample_index());
    assert_eq!(blob.get(8..12), Some(2u32.to_le_bytes().as_slice()));
    assert_eq!(blob.get(20..24), Some(3u32.to_le_bytes().as_slice()));
}

#[test]
fn refinements_round_trip_through_the_blob() {
    let mut index = sample_index();
    index.set_refinements(crate::refinements::StubRefinements::new(
        vec![(
            "array_keys".to_owned(),
            crate::refinements::RefinedSignature {
                templates: vec![],
                parameters: vec![],
                return_type: Some("list<int>".to_owned()),
            },
        )],
        vec![],
    ));
    let decoded = decode(&encode(&index)).unwrap();
    assert_eq!(
        decoded
            .function_refinement("array_keys")
            .and_then(|refinement| refinement.return_type.as_deref()),
        Some("list<int>"),
    );
}

#[test]
fn a_blob_without_the_overlays_section_decodes_with_empty_refinements() {
    // Build a two-section blob by hand (version 2, the pre-overlay
    // layout): the tolerance rule the signatures section already has.
    // Reuse the exact construction of
    // `a_blob_without_the_signature_section_decodes_with_empty_payloads`,
    // extended with the signatures section, and assert:
    // decoded.refinements().is_empty()
}
```

(`sample_index()` is whatever small-index helper the existing blob
tests use — reuse it; if none exists, `StubIndex::from_symbols(vec![
...one function symbol...])`. `set_refinements` is a new `StubIndex`
method, step 3.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_stubs`
Expected: FAIL to compile — `refinements` module, `encode_refinements`,
`decode_refinements`, `set_refinements`, `function_refinement` not
defined; then the version pin `the_blob_starts_with_magic_and_format_version`
still expects 1.

- [ ] **Step 3: Write the implementation**

1. `crates/celerrate_stubs/src/lib.rs`: add `mod refinements;` and
   re-export:

```rust
pub use refinements::{
    RefinedAncestor, RefinedClass, RefinedSignature, RefinedTemplate,
    StubRefinements,
};
```

2. `refinements.rs` — the encoding, below the structs (self-contained
   length-prefixed layout, little-endian, matching the blob's idiom):

```rust
pub(crate) fn encode_refinements(refinements: &StubRefinements, bytes: &mut Vec<u8>) {
    write_u32(bytes, refinements.functions.len());
    for (key, signature) in &refinements.functions {
        write_text(bytes, key);
        encode_signature(signature, bytes);
    }
    write_u32(bytes, refinements.classes.len());
    for (key, class) in &refinements.classes {
        write_text(bytes, key);
        encode_templates(&class.templates, bytes);
        write_u32(bytes, class.ancestors.len());
        for ancestor in &class.ancestors {
            write_text(bytes, &ancestor.name);
            write_u32(bytes, ancestor.arguments.len());
            for argument in &ancestor.arguments {
                write_text(bytes, argument);
            }
        }
        write_u32(bytes, class.methods.len());
        for (name, signature) in &class.methods {
            write_text(bytes, name);
            encode_signature(signature, bytes);
        }
    }
}

fn encode_signature(signature: &RefinedSignature, bytes: &mut Vec<u8>) {
    encode_templates(&signature.templates, bytes);
    write_u32(bytes, signature.parameters.len());
    for (name, text) in &signature.parameters {
        write_text(bytes, name);
        write_text(bytes, text);
    }
    write_optional_text(bytes, signature.return_type.as_deref());
}

fn encode_templates(templates: &[RefinedTemplate], bytes: &mut Vec<u8>) {
    write_u32(bytes, templates.len());
    for template in templates {
        write_text(bytes, &template.name);
        write_optional_text(bytes, template.bound.as_deref());
    }
}

fn write_u32(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u32).to_le_bytes());
}

fn write_text(bytes: &mut Vec<u8>, text: &str) {
    write_u32(bytes, text.len());
    bytes.extend_from_slice(text.as_bytes());
}

fn write_optional_text(bytes: &mut Vec<u8>, text: Option<&str>) {
    match text {
        Some(text) => {
            bytes.push(1);
            write_text(bytes, text);
        }
        None => bytes.push(0),
    }
}
```

The decoder mirrors it over a checked cursor (reuse `blob.rs`'s
`Reader` by making it `pub(crate)`, or duplicate the three needed
reads locally — prefer making `Reader` `pub(crate)`; it is already
the crate's checked-read idiom):

```rust
use crate::blob::{Reader, StubBlobError};

pub(crate) fn decode_refinements(
    bytes: &[u8],
) -> Result<StubRefinements, StubBlobError> {
    let mut reader = Reader::new(bytes);
    let function_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut functions = Vec::new();
    for _ in 0..function_count {
        let key = reader.string().ok_or(StubBlobError::MalformedSection)?;
        functions.push((key, decode_signature(&mut reader)?));
    }
    let class_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut classes = Vec::new();
    for _ in 0..class_count {
        let key = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let templates = decode_templates(&mut reader)?;
        let ancestor_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let mut ancestors = Vec::new();
        for _ in 0..ancestor_count {
            let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
            let argument_count =
                reader.u32().ok_or(StubBlobError::MalformedSection)?;
            let mut arguments = Vec::new();
            for _ in 0..argument_count {
                arguments
                    .push(reader.string().ok_or(StubBlobError::MalformedSection)?);
            }
            ancestors.push(RefinedAncestor { name, arguments });
        }
        let method_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let mut methods = Vec::new();
        for _ in 0..method_count {
            let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
            methods.push((name, decode_signature(&mut reader)?));
        }
        classes.push((key, RefinedClass { templates, ancestors, methods }));
    }
    Ok(StubRefinements::new(functions, classes))
}

fn decode_signature(reader: &mut Reader<'_>) -> Result<RefinedSignature, StubBlobError> {
    let templates = decode_templates(reader)?;
    let parameter_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut parameters = Vec::new();
    for _ in 0..parameter_count {
        let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let text = reader.string().ok_or(StubBlobError::MalformedSection)?;
        parameters.push((name, text));
    }
    let return_type = decode_optional_text(reader)?;
    Ok(RefinedSignature { templates, parameters, return_type })
}

fn decode_templates(
    reader: &mut Reader<'_>,
) -> Result<Vec<RefinedTemplate>, StubBlobError> {
    let count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut templates = Vec::new();
    for _ in 0..count {
        let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let bound = decode_optional_text(reader)?;
        templates.push(RefinedTemplate { name, bound });
    }
    Ok(templates)
}

fn decode_optional_text(
    reader: &mut Reader<'_>,
) -> Result<Option<String>, StubBlobError> {
    match reader.u8().ok_or(StubBlobError::MalformedSection)? {
        0 => Ok(None),
        1 => Ok(Some(reader.string().ok_or(StubBlobError::MalformedSection)?)),
        _ => Err(StubBlobError::MalformedSection),
    }
}
```

Adjust visibility as needed: `Reader`, its methods used here, and
`StubBlobError` become `pub(crate)` in `blob.rs` (the error is
already `pub` via `lib.rs` — check and keep the existing surface).

Note on the truncation test: `decode_refinements(&[])` on an empty
input must be an error — an empty *section payload* is malformed
(the counts are always present); the "no overlays section at all"
tolerance lives in `decode`, not here.

3. `index.rs`: the field and accessors.

```rust
pub struct StubIndex {
    symbols: Vec<StubSymbol>,
    functions: Vec<(String, StubSignature)>,
    classes: Vec<(String, StubClassSurface)>,
    refinements: StubRefinements,
}
```

`new` keeps its three parameters (every existing caller compiles
unchanged) and initializes `refinements: StubRefinements::empty()`;
a builder-style setter attaches the overlay:

```rust
pub fn set_refinements(&mut self, refinements: StubRefinements) {
    self.refinements = refinements;
}

pub fn refinements(&self) -> &StubRefinements {
    &self.refinements
}

pub fn function_refinement(&self, key: &str) -> Option<&RefinedSignature> {
    self.refinements
        .functions
        .binary_search_by(|(name, _)| name.as_str().cmp(key))
        .ok()
        .and_then(|position| self.refinements.functions.get(position))
        .map(|(_, signature)| signature)
}

pub fn class_refinement(&self, key: &str) -> Option<&RefinedClass> {
    self.refinements
        .classes
        .binary_search_by(|(name, _)| name.as_str().cmp(key))
        .ok()
        .and_then(|position| self.refinements.classes.get(position))
        .map(|(_, class)| class)
}
```

(If `StubIndex` derives `PartialEq`/`Eq` or is compared anywhere,
the new field participates — that is correct: a refinements change
IS an index change.)

4. `blob.rs`:
   - `pub const BLOB_FORMAT_VERSION: u32 = 2;` Reword the constant's
     doc comment in the same edit: "bumped only on incompatible
     layout changes" is now false; say the bump to 2 marks the
     overlays section going live, the schema bump the design's
     section 9 mandates for this sub-project.
   - `encode`: `table_entries = 3`, a third table entry for
     `SECTION_OVERLAYS` after the signatures entry, the encoded
     refinements appended after the signatures payload (encode via
     `crate::refinements::encode_refinements(index.refinements(), &mut bytes)`
     into its own `Vec<u8>` first, like the other sections).
   - `decode`: when the section table carries `SECTION_OVERLAYS`,
     decode it and `set_refinements` on the built index; when absent,
     leave the empty default (the tolerance test above).
   - Update the pinned tests: every hand-built blob in
     `unknown_sections_are_skipped_for_forward_compatibility`,
     `a_blob_without_the_signature_section_decodes_with_empty_payloads`,
     `a_blob_without_a_symbol_table_reports_it`, and
     `an_unknown_format_version_is_rejected_before_anything_else_is_read`
     writes version `2` in its header (their offsets stay
     self-computed); `the_blob_starts_with_magic_and_format_version`
     asserts `2`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_stubs`
Expected: PASS, including the untouched round-trip and checksum
tests. The embedded-blob tests (`the_embedded_blob_decodes`,
`the_committed_blob_matches_a_recompilation_of_the_pinned_snapshot`)
now FAIL — the committed `stubs.bin` is still version 1. That is
expected mid-task: task 3 recompiles and commits the blob. To keep
this commit green, recompile now with an empty overlay:

Run: `cargo xtask compile-stubs`
Expected: `crates/celerrate_stubs/src/stubs.bin` regenerated as a
version-2 blob with an empty overlays section; the embedded tests
pass again.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): the refinements payload and the blob's third section"
```

---

### Task 3: The refinements source file compiles into the blob

**Files:**
- Create: `crates/celerrate_stubs/src/compiler/refinement_source.rs`
- Create: `crates/celerrate_stubs/refinements.celerrate`
- Modify: `crates/celerrate_stubs/src/compiler/mod.rs` (or wherever
  the `compiler` module tree is declared — follow `extract.rs`'s
  declaration site)
- Modify: `crates/celerrate_stubs/src/bin/stub-compiler.rs`
- Modify: `xtask/src/stubs.rs`
- Modify: `crates/celerrate_stubs/src/stubs.bin` (recompiled)
- Test: in-module `#[cfg(test)]` in `refinement_source.rs`

**Interfaces:**
- Consumes: `StubRefinements`/`RefinedSignature`/`RefinedClass`/
  `RefinedTemplate`/`RefinedAncestor` (task 2), the compiler's
  accumulated `functions: Vec<(String, StubSignature)>` and
  `classes: Vec<(String, StubClassSurface)>`.
- Produces:
  - `pub fn parse_refinement_source(text: &str) ->
    Result<StubRefinements, String>` (feature `compiler`) — the
    error carries a line number and message.
  - `pub fn validate_refinements(refinements: &StubRefinements,
    functions: &[(String, StubSignature)], classes: &[(String,
    StubClassSurface)]) -> Result<(), String>` — existence checks
    (decision 3).
  - The `stub-compiler` binary accepts
    `--refinements <path>` and attaches the parsed overlay before
    encoding; `cargo xtask compile-stubs` passes
    `crates/celerrate_stubs/refinements.celerrate`.
  - The committed `refinements.celerrate` seed: `array_keys`,
    `array_values`, `class ArrayIterator` (tasks 4 and 5 test
    against these entries through the embedded blob).

**The source format** (documented in the module doc and the file
header; keys fold at parse time — lowercase, no leading backslash —
so blob keys match lookup keys; parameter names stay verbatim):

```
# Comments start with '#'; blank lines separate entries.
function array_keys<TKey, TValue>(array<TKey, TValue> $array): list<TKey>

class ArrayIterator<TKey, TValue> implements Iterator<TKey, TValue> {
    method __construct(array<TKey, TValue> $array)
    method current(): TValue
    method key(): TKey
}
```

- One `function` entry per line: `function NAME[<templates>](
  [TYPE $name[, ...]] )[: TYPE]`. Parameters listed refine only
  those names; the return is optional (omitted → the base fold
  stays).
- A `class` entry opens with `class NAME[<templates>]
  [extends A<...>[, ...]] [implements B<...>[, ...]] {`, carries
  `method` lines in the function shape (keyword `method`, no
  templates of their own colliding with the class's — a collision is
  a parse error), and closes with `}`. `extends`/`implements` both
  lower into `RefinedAncestor` (the distinction is the stub graph's,
  not the overlay's); an ancestor may be transitive (decision 8's
  injection rule).
- A template list is `<T, U of Bound>`; `of` introduces a bound
  (norm text, everything to the next top-level comma or `>`).
- Type texts are opaque here: split on **top-level** commas only
  (tracking `<`/`(`/`{` depth); the norm parser upstairs is the
  judge of their content.

- [ ] **Step 1: Write the failing tests**

In `refinement_source.rs`'s `#[cfg(test)]` module:

```rust
#[test]
fn a_function_entry_parses_with_templates_parameters_and_return() {
    let parsed = parse_refinement_source(
        "function array_keys<TKey, TValue>(array<TKey, TValue> $array): list<TKey>\n",
    )
    .unwrap();
    let (key, signature) = parsed.functions.first().unwrap();
    assert_eq!(key, "array_keys");
    assert_eq!(
        signature.templates,
        vec![
            RefinedTemplate { name: "TKey".to_owned(), bound: None },
            RefinedTemplate { name: "TValue".to_owned(), bound: None },
        ],
    );
    assert_eq!(
        signature.parameters,
        vec![("array".to_owned(), "array<TKey, TValue>".to_owned())],
    );
    assert_eq!(signature.return_type.as_deref(), Some("list<TKey>"));
}

#[test]
fn a_bound_reads_after_of_and_commas_nest() {
    let parsed = parse_refinement_source(
        "function pick<T of Countable&Traversable>(array<int, T> $items): T\n",
    )
    .unwrap();
    let (_, signature) = parsed.functions.first().unwrap();
    assert_eq!(
        signature.templates,
        vec![RefinedTemplate {
            name: "T".to_owned(),
            bound: Some("Countable&Traversable".to_owned()),
        }],
    );
}

#[test]
fn a_class_entry_parses_ancestors_and_methods() {
    let parsed = parse_refinement_source(
        "class ArrayIterator<TKey, TValue> implements Iterator<TKey, TValue> {\n\
         \tmethod current(): TValue\n\
         \tmethod key(): TKey\n\
         }\n",
    )
    .unwrap();
    let (key, class) = parsed.classes.first().unwrap();
    assert_eq!(key, "arrayiterator");
    assert_eq!(
        class.ancestors,
        vec![RefinedAncestor {
            name: "iterator".to_owned(),
            arguments: vec!["TKey".to_owned(), "TValue".to_owned()],
        }],
    );
    assert_eq!(class.methods.len(), 2);
    let (name, current) = class.methods.first().unwrap();
    assert_eq!(name, "current");
    assert_eq!(current.return_type.as_deref(), Some("TValue"));
}

#[test]
fn names_fold_and_comments_are_skipped() {
    let parsed = parse_refinement_source(
        "# the seed\nfunction Array_Keys(): list<int>\n",
    )
    .unwrap();
    assert_eq!(parsed.functions.first().unwrap().0, "array_keys");
}

#[test]
fn malformed_lines_fail_with_the_line_number() {
    for (text, line) in [
        ("function\n", 1),
        ("# fine\nfunction broken(\n", 2),
        ("class Foo {\nmethod\n}\n", 2),
        ("class Foo {\n", 1),               // unterminated block
        ("method orphan(): int\n", 1),      // method outside a class
        ("class Foo<T> {\nmethod m<T>(): T\n}\n", 2), // template collision
    ] {
        let error = parse_refinement_source(text).unwrap_err();
        assert!(
            error.starts_with(&format!("line {line}")),
            "for {text:?}: {error}",
        );
    }
}

#[test]
fn validation_names_the_missing_target() {
    let refinements = StubRefinements::new(
        vec![("missing_function".to_owned(), RefinedSignature::default())],
        vec![],
    );
    let error = validate_refinements(&refinements, &[], &[]).unwrap_err();
    assert!(error.contains("missing_function"), "{error}");
}

#[test]
fn validation_checks_parameters_methods_and_classes() {
    let functions = vec![(
        "array_keys".to_owned(),
        StubSignature {
            parameters: vec![StubParameter {
                name: "array".to_owned(),
                type_text: VersionedTypeText::default(),
                optional: false,
                by_reference: false,
                variadic: false,
                availability: StubAvailability::ALWAYS,
            }],
            return_type: VersionedTypeText::default(),
            by_reference: false,
        },
    )];
    // A refined parameter name absent from the base signature fails.
    let refinements = StubRefinements::new(
        vec![(
            "array_keys".to_owned(),
            RefinedSignature {
                templates: vec![],
                parameters: vec![("wrong".to_owned(), "int".to_owned())],
                return_type: None,
            },
        )],
        vec![],
    );
    let error = validate_refinements(&refinements, &functions, &[]).unwrap_err();
    assert!(error.contains("wrong"), "{error}");
}
```

(Adjust `VersionedTypeText::default()` if the type does not derive
`Default`: construct `VersionedTypeText { default: None, overrides:
vec![] }`.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_stubs --features compiler`
Expected: FAIL to compile — the module does not exist.

- [ ] **Step 3: Write the parser and validation**

The parser is line-oriented with one depth-aware splitter as its
workhorse:

```rust
/// Splits on top-level commas, tracking `<`/`(`/`{` depth. Norm
/// texts are opaque here; only the bracket depth matters.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (offset, character) in text.char_indices() {
        match character {
            '<' | '(' | '{' => depth += 1,
            '>' | ')' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(text.get(start..offset).unwrap_or_default());
                start = offset + 1;
            }
            _ => {}
        }
    }
    parts.push(text.get(start..).unwrap_or_default());
    parts
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn folded(name: &str) -> String {
    name.trim_start_matches('\\').to_lowercase()
}
```

(If the crate already exposes a fold helper for symbol keys, reuse
it instead of the local `folded` — one folding convention, one
implementation.)

Parsing skeleton (line loop, `enum Context { TopLevel, Class {...}
}`), each helper returning `Result<_, String>` where errors are
`format!("line {number}: {message}")`:

- `function` lines: strip the keyword; the head is everything to the
  first `(` at top level — split it into name and template list
  (`name<...>`); the parameter list is the text to the **matching**
  `)` (depth-aware scan); an optional `: TYPE` tail follows. Each
  parameter part (via `split_top_level`) splits at its `$`:
  the type text before (trimmed, required non-empty), the name after
  (trimmed of `&`, `.`, whitespace). Template parts split at ` of `
  (first occurrence): name, optional bound.
- `class` lines: strip the keyword; the head before `extends` /
  `implements` / `{` gives name and templates; each clause's list
  via `split_top_level`, each entry `Name<arguments...>` parsed into
  `RefinedAncestor { name: folded(name), arguments }` (a bare `Name`
  carries no arguments — legal, it refines nothing but is accepted);
  the line must end with `{`.
- `method` lines (inside a class): the function shape with the
  `method` keyword; a template list on a method whose names collide
  with the class's templates is an error (decision: shared scope,
  task 4); the parsed signature lands in `methods` with the folded
  name.
- `}` closes the class; end-of-input inside a class is an error
  reported at the class's opening line.

Validation:

```rust
pub fn validate_refinements(
    refinements: &StubRefinements,
    functions: &[(String, StubSignature)],
    classes: &[(String, StubClassSurface)],
) -> Result<(), String> {
    for (key, signature) in &refinements.functions {
        let Some((_, base)) = functions
            .iter()
            .find(|(name, _)| folded(name) == *key)
        else {
            return Err(format!("refined function {key} is not in the snapshot"));
        };
        validate_parameters(key, signature, &base.parameters)?;
    }
    for (key, class) in &refinements.classes {
        let Some((_, surface)) = classes
            .iter()
            .find(|(name, _)| folded(name) == *key)
        else {
            return Err(format!("refined class {key} is not in the snapshot"));
        };
        for (method_name, signature) in &class.methods {
            let Some(base) = surface.members.iter().find(|member| {
                member.kind == StubMemberKind::Method
                    && folded(&member.name) == *method_name
            }) else {
                return Err(format!(
                    "refined method {key}::{method_name} is not in the snapshot",
                ));
            };
            let Some(base_signature) = &base.signature else {
                return Err(format!(
                    "refined method {key}::{method_name} has no base signature",
                ));
            };
            validate_parameters(method_name, signature, &base_signature.parameters)?;
        }
    }
    Ok(())
}

fn validate_parameters(
    target: &str,
    signature: &RefinedSignature,
    base: &[StubParameter],
) -> Result<(), String> {
    for (name, _) in &signature.parameters {
        if !base.iter().any(|parameter| parameter.name == *name) {
            return Err(format!(
                "refined parameter ${name} does not exist on {target}",
            ));
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Wire the compiler binary and xtask**

`stub-compiler.rs`: extend the argument parsing —
`stub-compiler <snapshot> <blob> [--refinements <path>] [--check]`.
When the flag is present: read the file (an unreadable path is a
hard error), `parse_refinement_source`, `validate_refinements`
against the accumulated `functions`/`classes`, then
`index.set_refinements(parsed)` before `encode(&index)`. Any error
prints to stderr and exits non-zero — a broken refinement never
produces a blob.

`xtask/src/stubs.rs`: the `cargo run ... --bin stub-compiler --`
invocation gains
`--refinements crates/celerrate_stubs/refinements.celerrate`
(both in the compile and the `--check` paths — the freshness gate
must hash the refinements too, which it now does by construction:
the blob bytes change with the file).

Create `crates/celerrate_stubs/refinements.celerrate` (the seed —
task 11 grows it):

```
# The Celerrate refinements overlay (design section 7): enriched
# stub signatures in the internal norm, compiled into stubs.bin by
# `cargo xtask compile-stubs`. Validated at compile time (existence)
# and by the lowering-totality test in celerrate_types (grammar).

function array_keys<TKey, TValue>(array<TKey, TValue> $array): list<TKey>
function array_values<TKey, TValue>(array<TKey, TValue> $array): list<TValue>

class ArrayIterator<TKey, TValue> implements Iterator<TKey, TValue> {
    method __construct(array<TKey, TValue> $array)
    method current(): TValue
    method key(): TKey
}
```

- [ ] **Step 5: Recompile the blob, run everything, commit**

Run: `cargo xtask compile-stubs`
Expected: `stubs.bin` regenerated with the three entries.

Run: `cargo test --workspace && cargo test -p celerrate_stubs --features compiler && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings && cargo fmt --all && cargo xtask compile-stubs --check`
Expected: green; the freshness check passes on the just-committed
blob.

```bash
git add crates/celerrate_stubs xtask/src/stubs.rs
git commit -m "✨ feat(stubs): the refinements source file compiles into the blob"
```

---

### Task 4: Refined signatures answer at the declared tier

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`
- Test: `declared.rs`'s existing `#[cfg(test)]` module and
  `crates/celerrate_types/src/inference.rs`'s test module

**Interfaces:**
- Consumes: `lower_norm_text`/`NormScope`/`NormTemplate` (task 1),
  `StubIndex::function_refinement`/`refinements` (task 2), the
  embedded seed entries (task 3), the existing
  `refine`/`value_type_across_range`/`parameter_type_across_range`/
  `resolve_stub_signature`/`resolve_stub_member_signature`/
  `declared_stub_parameter`, the plan-6 solver path (`solver_pairs`/
  `solve`/`finalize_return` already run wherever a call result
  `contains_symbolic`).
- Produces: **no new public names** — `declared_function_signature`
  and `declared_member_signature` keep their exact signatures; their
  answers gain precision for refined stub elements. Internal
  contract for task 5: the private helpers
  `norm_templates(db, scope_key, templates: &[RefinedTemplate]) ->
  Vec<NormTemplate>` and
  `refined_element(db, files, stubs, configuration, scope, text:
  Option<&str>, native: Option<TypeId>) -> (Option<TypeId>, Trust)`
  live in `declared.rs` as `pub(crate)`.

- [ ] **Step 1: Write the failing tests**

In `declared.rs`'s test module (reuse its existing fixture idiom for
building a database with a synthetic stub index; if the module so
far only builds `StubIndex::from_symbols`, extend the helper to
accept functions and refinements):

```rust
/// A database around one synthetic stub function plus a refinement.
/// Reuse the module's fixture struct if it has one; otherwise this
/// self-contained shape (the inference.rs fixture's fields, no
/// source files needed).
struct Fixture {
    db: TestDatabase,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

fn refined_stub_fixture(
    key: &str,
    base_return: &str,
    refinement: RefinedSignature,
) -> Fixture {
    let db = TestDatabase::default();
    let symbols = vec![StubSymbol {
        name: key.to_owned(),
        kind: StubSymbolKind::Function,
        availability: StubAvailability::ALWAYS,
    }];
    let functions = vec![(
        key.to_owned(),
        StubSignature {
            parameters: vec![StubParameter {
                name: "array".to_owned(),
                type_text: VersionedTypeText {
                    default: Some("array".to_owned()),
                    overrides: vec![],
                },
                optional: false,
                by_reference: false,
                variadic: false,
                availability: StubAvailability::ALWAYS,
            }],
            return_type: VersionedTypeText {
                default: Some(base_return.to_owned()),
                overrides: vec![],
            },
            by_reference: false,
        },
    )];
    let mut index = StubIndex::new(symbols, functions, vec![]);
    index.set_refinements(StubRefinements::new(
        vec![(key.to_owned(), refinement)],
        vec![],
    ));
    let files = AnalyzedFileSet::new(&db, vec![]);
    let stubs = StubIndexInput::builder(index)
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

fn array_keys_refinement() -> RefinedSignature {
    RefinedSignature {
        templates: vec![
            RefinedTemplate { name: "TKey".to_owned(), bound: None },
            RefinedTemplate { name: "TValue".to_owned(), bound: None },
        ],
        parameters: vec![(
            "array".to_owned(),
            "array<TKey, TValue>".to_owned(),
        )],
        return_type: Some("list<TKey>".to_owned()),
    }
}

#[test]
fn a_refined_return_lowers_under_the_function_scope_and_is_trusted() {
    let f = refined_stub_fixture("array_keys", "array", array_keys_refinement());
    let signature = declared_function_signature(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "array_keys".to_owned()),
    )
    .unwrap();
    assert_eq!(
        signature.value_type,
        TypeId::list(
            &f.db,
            TypeId::template(&f.db, "array_keys", "TKey", TypeId::mixed(&f.db)),
        ),
    );
    // `list<TKey> <: array` holds: a template candidate proves
    // through its `mixed` bound against the fold's `mixed` value,
    // and every list is an array (the Holds arm, decision 5).
    assert_eq!(signature.value_trust, Trust::Refined);
}

#[test]
fn a_refined_parameter_replaces_the_fold_and_unrefined_elements_keep_it() {
    let f = refined_stub_fixture("array_keys", "array", array_keys_refinement());
    let signature = declared_function_signature(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "array_keys".to_owned()),
    )
    .unwrap();
    let parameter = signature.parameters.first().unwrap();
    assert_eq!(
        parameter.parameter_type,
        Some(TypeId::array(
            &f.db,
            TypeId::template(&f.db, "array_keys", "TKey", TypeId::mixed(&f.db)),
            TypeId::template(&f.db, "array_keys", "TValue", TypeId::mixed(&f.db)),
        )),
    );
    // `TKey <: int|string` cannot be decided through the `mixed`
    // bound: the genuine CannotProve arm (decision 5).
    assert_eq!(parameter.trust, Trust::RefinedUnproven);
}

#[test]
fn a_failing_refinement_is_rejected_and_the_native_fold_wins() {
    // A refined `int` against a native `string`: Proof::Fails — the
    // curation-typo containment (decision 5).
    let f = refined_stub_fixture(
        "getcwd",
        "string",
        RefinedSignature {
            templates: vec![],
            parameters: vec![],
            return_type: Some("int".to_owned()),
        },
    );
    let signature = declared_function_signature(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "getcwd".to_owned()),
    )
    .unwrap();
    assert_eq!(signature.value_type, TypeId::string(&f.db));
    assert_eq!(signature.value_trust, Trust::RejectedAnnotation);
}

#[test]
fn every_embedded_refinement_text_lowers() {
    // Decision 3's totality gate: a typo in refinements.celerrate is
    // a test failure here, never a silent fallback.
    let db = TestDatabase::default();
    let index = celerrate_stubs::embedded_stub_index().unwrap();
    for (key, signature) in &index.refinements().functions {
        assert_signature_lowers(&db, key, signature, &[]);
    }
    for (key, class) in &index.refinements().classes {
        let class_templates = norm_templates(&db, key, &class.templates);
        let scope = NormScope { key, templates: &class_templates };
        for template in &class.templates {
            if let Some(bound) = &template.bound {
                let empty = NormScope { key, templates: &[] };
                assert!(
                    lower_norm_text(&db, &empty, bound).is_some(),
                    "bound of {key}::{}", template.name,
                );
            }
        }
        for ancestor in &class.ancestors {
            for argument in &ancestor.arguments {
                assert!(
                    lower_norm_text(&db, &scope, argument).is_some(),
                    "ancestor argument {argument} of {key}",
                );
            }
        }
        for (name, signature) in &class.methods {
            assert_signature_lowers(&db, &format!("{key}::{name}"), signature, &class_templates);
        }
    }
}
```

(`assert_signature_lowers` is a small test helper: build the scope
from the outer templates plus the signature's own via
`norm_templates`, assert every parameter text and the return text
lower to `Some`.)

In `inference.rs`'s test module, the end-to-end proof that the
plan-6 solver picks the templates up (uses the embedded stubs —
add a fixture variant whose `StubIndexInput` wraps
`celerrate_stubs::embedded_stub_index()` instead of the empty
index):

```rust
#[test]
fn a_refined_stub_function_solves_its_templates_at_the_call_site() {
    let f = fixture_with_embedded_stubs(&[r#"<?php
function consume() { return array_keys(['a' => 1, 'b' => 2]); }
"#]);
    let inferred = inferred_function_return(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "consume".to_owned()),
    );
    // TKey solved from the shape argument against
    // `array<TKey, TValue>`.
    assert_eq!(inferred.display(&f.db), "list<'a'|'b'>");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types declared && cargo test -p celerrate_types inference`
Expected: the new tests FAIL — the refined return still answers the
native `array` fold (`declared_function_signature` never consults
refinements yet).

- [ ] **Step 3: Implement the consultation**

In `declared.rs`:

1. The shared helpers (both `pub(crate)`, task 5 reuses them):

```rust
/// Lowers a refinement's template declarations. Bounds lower under
/// an empty scope (a bound cannot reference a sibling template);
/// a bound that fails to lower falls to `mixed` — the totality test
/// keeps this the dead branch it should be.
pub(crate) fn norm_templates<'db>(
    db: &'db dyn salsa::Database,
    scope_key: &str,
    templates: &[celerrate_stubs::RefinedTemplate],
) -> Vec<crate::norm::NormTemplate<'db>> {
    let empty = crate::norm::NormScope { key: scope_key, templates: &[] };
    templates
        .iter()
        .map(|template| crate::norm::NormTemplate {
            name: template.name.clone(),
            bound: template
                .bound
                .as_deref()
                .and_then(|bound| crate::norm::lower_norm_text(db, &empty, bound)),
        })
        .collect()
}

/// One refined element: lower the text, then the section-3 trust
/// rule against the native fold. `native == None` (the silenced
/// empty-intersection parameter) trusts against `mixed`, so a
/// refinement can rescue a silenced parameter.
pub(crate) fn refined_element<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    scope: &crate::norm::NormScope<'db, '_>,
    text: Option<&str>,
    native: Option<TypeId<'db>>,
) -> (Option<TypeId<'db>>, Trust) {
    let Some(lowered) = text
        .and_then(|text| crate::norm::lower_norm_text(db, scope, text))
    else {
        return (native, Trust::NativeOnly);
    };
    let native_for_trust = native.unwrap_or_else(|| TypeId::mixed(db));
    let (chosen, trust) = refine(
        db, files, stubs, configuration, native_for_trust, Some(lowered),
    );
    (Some(chosen), trust)
}
```

2. `declared_function_signature`'s stub path: fetch
   `stubs.index(db).function_refinement(key)` and pass it through —
   `resolve_stub_signature` gains two parameters:

```rust
fn resolve_stub_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    scope_key: &str,
    signature: &StubSignature,
    refinement: Option<&celerrate_stubs::RefinedSignature>,
) -> DeclaredSignature<'db> {
```

   Inside: build the scope once —

```rust
let templates = refinement
    .map(|refinement| norm_templates(db, scope_key, &refinement.templates))
    .unwrap_or_default();
let scope = crate::norm::NormScope { key: scope_key, templates: &templates };
```

   The return: compute the native fold as today
   (`value_type_across_range`), then

```rust
let refined_return = refinement.and_then(|refinement| refinement.return_type.as_deref());
let (value_type, value_trust) = match refined_element(
    db, files, stubs, configuration, &scope, refined_return, Some(native_return),
) {
    (Some(chosen), trust) => (chosen, trust),
    (None, trust) => (native_return, trust),
};
```

   Parameters: `declared_stub_parameter` gains
   `scope: &crate::norm::NormScope<'db, '_>` and
   `refined_text: Option<&str>` (the caller finds the parameter's
   entry by name in `refinement.parameters`); inside, the across-range
   fold computes as today into `native: Option<TypeId>`, then
   `refined_element(..., refined_text, native)` decides
   `parameter_type` and `trust` (unrefined: the exact current
   behavior falls out of the `None` text arm).

3. `resolve_stub_member_signature` mirrors it: the caller
   (`declared_member_signature`'s `MemberResolution::Stub { member,
   owner }` arm) fetches `stubs.index(db).class_refinement(&owner)`,
   finds the method's entry by folded member name (methods only —
   property and constant refinements are out of scope, `None` for
   those kinds), and builds the scope from the **class templates
   plus the method's own** (one scope key: the owner class key —
   the parse-time collision check of task 3 makes this safe):

```rust
let class_refinement = stubs.index(db).class_refinement(owner);
let method_refinement = class_refinement.and_then(|class| {
    class
        .methods
        .iter()
        .find(|(name, _)| *name == member_key)
        .map(|(_, signature)| signature)
});
let mut templates = class_refinement
    .map(|class| norm_templates(db, owner, &class.templates))
    .unwrap_or_default();
if let Some(refinement) = method_refinement {
    templates.extend(norm_templates(db, owner, &refinement.templates));
}
```

4. Adjust every `resolve_stub_signature` call site for the new
   parameters (the key is in hand at each one).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS. If the end-to-end display disagrees on shape-key
solving (`list<'a'|'b'>` versus a widened key form), check what the
plan-6 solver actually binds for a shape argument against
`array<TKey, TValue>` and fix the expectation to the solver's
documented rule — never weaken the solver.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): refined stub signatures answer at the declared tier"
```

---

### Task 5: Stub-class refinements thread templates and ancestors

**Files:**
- Modify: `crates/celerrate_types/src/inheritance.rs` (plan 6's
  module)
- Modify: `crates/celerrate_types/src/flow.rs` (only if constructor
  inference reads `class_annotations(...).templates` directly — see
  step 3)
- Test: `inheritance.rs`'s test module and `inference.rs`'s test
  module

**Interfaces:**
- Consumes: `class_templates`, `ancestor_arguments`, and the
  ancestry-walk composition (plan 6, `inheritance.rs`),
  `ParsedTemplate`/`ParsedAncestor` (plan 6's payload types),
  `norm_templates`/`refined_element` (task 4, `pub(crate)`),
  `StubIndex::class_refinement` (task 2), the embedded
  `ArrayIterator` seed (task 3), `lookup_member`'s stub origin,
  iteration typing's threaded-ancestors step (plan 6 task 10),
  constructor inference (plan 6 task 9).
- Produces: **no new public names** — the plan-6 queries answer for
  refined stub classes too. Behavior contract for later tasks and
  plan 8:
  - `class_templates(<stub class with refinement>)` answers the
    refined templates in declaration order (bounds lowered under an
    empty scope, scope key = the class key).
  - `ancestor_arguments(C)`: during the walk, a visited **stub**
    owner with a class refinement contributes its refined ancestors
    exactly as a source owner's `@extends`/`@implements` would —
    texts lowered under the owner's template scope, substituted by
    the owner's composed substitution, first-edge-wins preserved.
    The refined ancestor may be transitive (its name need not be a
    direct parent); it enters the per-ancestor table under the same
    first-wins rule. Stub owners without a refinement contribute
    nothing, as today.

- [ ] **Step 1: Write the failing tests**

In `inheritance.rs`'s test module (mirror its plan-6 fixture idiom;
these use a fixture whose stub input carries the task-4 style
synthetic index — build one local helper with the `ArrayIterator`
refinement of the seed, plus the `Iterator` interface class surface
so linearization has the edge):

```rust
#[test]
fn a_refined_stub_class_answers_its_templates() {
    let f = stub_refinement_fixture();
    let templates = class_templates(
        &f.db, f.files, f.stubs, f.configuration,
        ClassQuery::new(&f.db, "arrayiterator".to_owned()),
    );
    let names: Vec<&str> = templates
        .iter()
        .map(|template| template.name.as_str())
        .collect();
    assert_eq!(names, ["TKey", "TValue"]);
}

#[test]
fn a_refined_stub_ancestor_threads_through_a_source_subclass() {
    // Source code extends the refined stub class with fixed
    // arguments; the composition must reach Iterator.
    let f = stub_refinement_fixture_with_source(&[r#"<?php
namespace App;
/** @extends \ArrayIterator<int, \App\Post> */
class RecentPosts extends \ArrayIterator {}
class Post {}
"#]);
    let arguments = ancestor_arguments(
        &f.db, f.files, f.stubs, f.configuration,
        ClassQuery::new(&f.db, "app\\recentposts".to_owned()),
    );
    // The iterator entry carries the substituted arguments:
    // TKey := int, TValue := app\post.
    let iterator = arguments
        .iter()
        .find(|entry| entry.ancestor == "iterator")
        .expect("iterator threaded");
    assert_eq!(
        iterator.arguments,
        vec![
            TypeId::int(&f.db),
            TypeId::class(&f.db, "app\\post", vec![]),
        ],
    );
}

#[test]
fn a_stub_class_without_a_refinement_still_contributes_nothing() {
    // The plan-6 boundary stays the default: only curation opens it.
    // SplStack carries no refinement in this fixture; extending it
    // threads nothing.
    let f = stub_refinement_fixture_with_source(&[r#"<?php
namespace App;
class Stack extends \SplStack {}
"#]);
    let arguments = ancestor_arguments(
        &f.db, f.files, f.stubs, f.configuration,
        ClassQuery::new(&f.db, "app\\stack".to_owned()),
    );
    assert!(
        arguments.iter().all(|entry| entry.arguments.is_empty()),
        "uncurated stub ancestors contribute no arguments: {arguments:?}",
    );
}
```

(Adapt the `arguments.iter().find(...)` access to `ancestor_arguments`'
actual return shape — plan 6 defines it; the assertion content is
the contract.)

In `inference.rs`'s test module (embedded stubs, end to end):

```rust
#[test]
fn a_refined_stub_constructor_solves_the_class_templates() {
    let f = fixture_with_embedded_stubs(&[r#"<?php
function consume() { return new \ArrayIterator(['a' => 1]); }
"#]);
    let inferred = inferred_function_return(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "consume".to_owned()),
    );
    // The refined __construct's `array<TKey, TValue>` bound both.
    // Display renders folded class keys; fix the expectation to the
    // canonical rendering if it differs, never the solving.
    assert_eq!(inferred.display(&f.db), "arrayiterator<'a', 1>");
}

#[test]
fn iteration_over_a_refined_stub_iterator_types_key_and_value() {
    let f = fixture_with_embedded_stubs(&[r#"<?php
function consume() {
    foreach (new \ArrayIterator(['a' => 1, 'b' => 2]) as $key => $value) {
        return [$key, $value];
    }
    return null;
}
"#]);
    let inferred = inferred_function_return(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "consume".to_owned()),
    );
    // The threaded Iterator<TKey, TValue> arguments reached
    // iteration typing's protocol chain (if iteration typing answers
    // through the refined `current()`/`key()` returns instead, that
    // is equally correct — the displays are the contract, not the
    // path). The list literal display: a shape of the two subjects,
    // unioned with the fall-through null.
    let display = inferred.display(&f.db);
    assert!(display.contains("'a'|'b'"), "{display}");
    assert!(display.contains("1|2"), "{display}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types inheritance && cargo test -p celerrate_types inference`
Expected: FAIL — stub classes answer empty templates and contribute
no ancestor arguments.

- [ ] **Step 3: Implement the stub branch**

In `inheritance.rs`:

1. `class_templates`: where the query resolves the class's own
   docblock (the source path), add the stub arm — when the class key
   resolves to a stub (no source declaration), consult
   `stubs.index(db).class_refinement(key)`; map each
   `RefinedTemplate` into the query's template payload
   (`ParsedTemplate` or the module's equivalent), lowering bounds via
   `norm_templates(db, key, ...)`. Declaration order preserved.
2. The `ancestor_arguments` walk: at the point where a visited
   owner's own annotations contribute ancestor arguments (the
   plan-6 decision-7 composition), add the stub arm — a stub owner
   with a class refinement contributes each `RefinedAncestor` as an
   entry: ancestor key = the refined name (already folded),
   arguments = each text lowered via
   `lower_norm_text(db, &owner_scope, text)` (owner scope = the
   owner's refined templates), then substituted by the owner's
   composed substitution, entering the table first-wins. A text that
   fails to lower contributes `mixed` for that position (the
   totality test makes this dead).
3. Constructor inference's template source: if plan 6 wired `new
   Foo(...)` template solving to `class_annotations(...).templates`
   (source docblocks only), point it at `class_templates` instead —
   the query that now covers both channels. If it already reads
   `class_templates`, nothing to do.
4. Reword the plan-6 debt line in `inheritance.rs`'s module doc:
   stub ancestors now contribute **curated** generic arguments; the
   uncurated remainder still degrades to the protocol-member
   fallback.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types`
Expected: PASS, including plan 6's existing inheritance fixtures
(the stub arm must not disturb the source path — the
`a_stub_class_without_a_refinement_still_contributes_nothing` probe
pins the boundary).

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): stub-class refinements thread templates and ancestors"
```

---

### Task 6: The provider crate registers and claims

**Files:**
- Create: `crates/celerrate_stdlib_provider/Cargo.toml`
- Create: `crates/celerrate_stdlib_provider/src/lib.rs`
- Create: `crates/celerrate_stdlib_provider/src/array_functions.rs`
- Modify: `crates/celerrate_cli/src/plugins.rs`
- Modify: `xtask/src/dependency_shape.rs`
- Test: in-module tests in the new crate, `plugins.rs`'s test
  module, `dependency_shape.rs`'s test module

**Interfaces:**
- Consumes: `celerrate_plugin` only (normal dependencies):
  `DynamicTypeProvider`, `Invocation`, `SymbolClaim`, `TypeId`,
  `PluginDescriptor`, `PluginIdentity`, `PLUGIN_API_VERSION`,
  `salsa`. Dev-dependencies for end-to-end tests mirror the bridge's
  (`celerrate_db`, `celerrate_project`, `celerrate_semantics`,
  `celerrate_source`, `celerrate_stubs`, `celerrate_types`, `salsa`).
- Produces (later tasks rely on these exact names):
  - `pub struct StdlibProvider;` with `pub fn new() -> Self`.
  - `pub fn descriptor() -> celerrate_plugin::PluginDescriptor`
    (name `stdlib-provider`, version `env!("CARGO_PKG_VERSION")`,
    empty configuration).
  - `const CLAIMED_FUNCTIONS: &[&str]` — sorted; this task ships
    `["current", "end", "reset"]`; tasks 7–9 grow it.
  - The dispatch skeleton in `lib.rs`:
    `fn function_return<'db>(db, key: &str, arguments:
    &[TypeId<'db>]) -> Option<TypeId<'db>>` matching on the key and
    delegating to the family modules.
  - In `plugins.rs`: the claim-conflict **rebuild loop** replacing
    the recorded-only branch.

- [ ] **Step 1: Write the failing tests**

`crates/celerrate_stdlib_provider/Cargo.toml` (the bridge's template):

```toml
[package]
name = "celerrate_stdlib_provider"
description = "First-party plugin computing the stdlib signatures no declarative stub can express"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_plugin = { path = "../celerrate_plugin" }

# Test-only, exempt from the dependency-shape rule: the end-to-end
# tests need the whole seam to prove answers flow through the call
# boundary, not just the handlers' unit shapes.
[dev-dependencies]
celerrate_db = { path = "../celerrate_db" }
celerrate_project = { path = "../celerrate_project" }
celerrate_semantics = { path = "../celerrate_semantics" }
celerrate_source = { path = "../celerrate_source" }
celerrate_stubs = { path = "../celerrate_stubs" }
celerrate_types = { path = "../celerrate_types" }
salsa.workspace = true

[lints]
workspace = true
```

`src/lib.rs` — start with the doc, the struct, the descriptor, and
the test module:

```rust
//! The stdlib type provider (design section 7): a first-party
//! plugin computing the computation-dependent stdlib signatures no
//! declarative stub can express — the declarative long tail lives
//! in the refinements overlay instead. Stateless and pure: every
//! answer is a function of the `Invocation` alone (argument values
//! travel as literal types); `None` falls through to the declared
//! tier. Claims are exact folded function keys. Depends only on
//! `celerrate_plugin` (enforced by `cargo xtask dependency-shape`).

mod array_functions;

use celerrate_plugin::{
    DynamicTypeProvider, Invocation, SymbolClaim, TypeId, salsa,
};

/// Sorted; `claims()` maps it verbatim. Grown by tasks 7–9 and
/// curation, never speculatively.
const CLAIMED_FUNCTIONS: &[&str] = &["current", "end", "reset"];

#[derive(Debug, Clone, Copy, Default)]
pub struct StdlibProvider;

impl StdlibProvider {
    pub fn new() -> Self {
        Self
    }
}

pub fn descriptor() -> celerrate_plugin::PluginDescriptor {
    celerrate_plugin::PluginDescriptor {
        identity: celerrate_plugin::PluginIdentity {
            name: "stdlib-provider".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            configuration: String::new(),
        },
        api_version: celerrate_plugin::PLUGIN_API_VERSION,
    }
}

impl DynamicTypeProvider for StdlibProvider {
    fn claims(&self) -> Vec<SymbolClaim> {
        CLAIMED_FUNCTIONS
            .iter()
            .map(|key| SymbolClaim::Function { key: (*key).to_owned() })
            .collect()
    }

    fn return_type<'db>(
        &self,
        db: &'db dyn salsa::Database,
        invocation: &Invocation<'db>,
    ) -> Option<TypeId<'db>> {
        let SymbolClaim::Function { key } = &invocation.claim else {
            return None;
        };
        function_return(db, key, &invocation.argument_types)
    }
}

fn function_return<'db>(
    db: &'db dyn salsa::Database,
    key: &str,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    match key {
        "current" | "end" | "reset" => {
            array_functions::pointer_value(db, arguments)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::{DynamicTypeProvider, Invocation, SymbolClaim, TypeId};

    use super::StdlibProvider;

    pub(crate) fn function_invocation<'db>(
        key: &str,
        arguments: Vec<TypeId<'db>>,
    ) -> Invocation<'db> {
        Invocation {
            claim: SymbolClaim::Function { key: key.to_owned() },
            receiver_type: None,
            argument_types: arguments,
        }
    }

    #[test]
    fn the_descriptor_names_the_plugin_and_the_api_version() {
        let descriptor = super::descriptor();
        assert_eq!(descriptor.identity.name, "stdlib-provider");
        assert_eq!(descriptor.api_version, celerrate_plugin::PLUGIN_API_VERSION);
    }

    #[test]
    fn claims_are_sorted_and_distinct() {
        let mut sorted = super::CLAIMED_FUNCTIONS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), super::CLAIMED_FUNCTIONS);
    }

    #[test]
    fn current_projects_the_value_type_with_the_false_miss() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        let subject = TypeId::array(&db, TypeId::int(&db), TypeId::string(&db));
        let answer = provider
            .return_type(&db, &function_invocation("current", vec![subject]))
            .unwrap();
        assert_eq!(
            answer,
            TypeId::union(
                &db,
                [TypeId::string(&db), TypeId::bool_literal(&db, false)],
            ),
        );
    }

    #[test]
    fn current_over_a_shape_unions_the_field_values() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        let subject = TypeId::shape(
            &db,
            vec![
                celerrate_plugin::ShapeField {
                    key: celerrate_plugin::ShapeKey::String("a".to_owned()),
                    optional: false,
                    value: TypeId::int_literal(&db, 1),
                },
                celerrate_plugin::ShapeField {
                    key: celerrate_plugin::ShapeKey::String("b".to_owned()),
                    optional: false,
                    value: TypeId::string_literal(&db, "x"),
                },
            ],
        );
        let answer = provider
            .return_type(&db, &function_invocation("current", vec![subject]))
            .unwrap();
        assert_eq!(
            answer,
            TypeId::union(
                &db,
                [
                    TypeId::int_literal(&db, 1),
                    TypeId::string_literal(&db, "x"),
                    TypeId::bool_literal(&db, false),
                ],
            ),
        );
    }

    #[test]
    fn an_unknown_subject_answers_none_and_falls_through() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        assert!(
            provider
                .return_type(
                    &db,
                    &function_invocation("current", vec![TypeId::mixed(&db)]),
                )
                .is_none(),
        );
        assert!(
            provider
                .return_type(&db, &function_invocation("current", vec![]))
                .is_none(),
        );
    }
}
```

In `plugins.rs`'s test module:

```rust
#[test]
fn the_stdlib_provider_registers_with_its_claims() {
    let database = AnalysisDatabase::default();
    let registered = register_plugins(&database);
    assert!(registered.excluded.is_empty());
    let registry =
        celerrate_types::DynamicTypeProviderRegistry::try_get(&database)
            .expect("set");
    let registrations = registry.registrations(&database);
    let provider = registrations
        .iter()
        .find(|registration| registration.identity.name == "stdlib-provider")
        .expect("registered");
    assert!(!provider.provider.claims().is_empty());
}

#[test]
fn a_claim_conflict_excludes_the_later_registrant_and_rebuilds() {
    // Unit-level: two fake registrations claiming the same function;
    // the admitted set keeps the first, the exclusion names the
    // second, and validate_claims passes on the rebuilt vector.
    let (admitted, excluded) = admit_dynamic_providers(vec![
        fake_registration("first", &["current"]),
        fake_registration("second", &["current"]),
    ]);
    assert_eq!(admitted.len(), 1);
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded.first().unwrap().name, "second");
    assert!(celerrate_types::validate_claims(&admitted).is_ok());
}
```

(`fake_registration` builds a `DynamicTypeProviderRegistration` with
a minimal in-test provider claiming the given keys — mirror the
`FakeProvider` in `dynamic_type_provider.rs`'s unit tests.)

In `dependency_shape.rs`, extend the const and its passing test's
expectations:

```rust
const PLUGIN_CRATES: &[&str] = &["celerrate_phpdoc_bridge", "celerrate_stdlib_provider"];
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_stdlib_provider -p celerrate_cli -p xtask`
Expected: the new crate FAILS to compile until `array_functions`
exists; `plugins.rs` tests fail on the missing registration and the
missing `admit_dynamic_providers`.

- [ ] **Step 3: Implement**

1. `src/array_functions.rs`:

```rust
//! The array family. Every handler is a pure projection over the
//! invocation's argument types; `None` falls through to the
//! declared tier (conservative silence).

use celerrate_plugin::{TypeId, salsa};

/// `current`/`reset`/`end`: the value projection with the `false`
/// miss. Arrays and lists answer their value type; shapes union
/// their field values; anything else is `None`.
pub(crate) fn pointer_value<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let subject = arguments.first()?;
    let value = array_value_of(db, *subject)?;
    Some(TypeId::union(db, [value, TypeId::bool_literal(db, false)]))
}

/// The value type of an array-like subject, `None` when unknown.
pub(crate) fn array_value_of<'db>(
    db: &'db dyn salsa::Database,
    subject: TypeId<'db>,
) -> Option<TypeId<'db>> {
    if let Some(value) = subject.array_value(db) {
        return Some(value);
    }
    let fields = subject.shape_fields(db)?;
    if fields.is_empty() {
        return None;
    }
    Some(TypeId::union(db, fields.into_iter().map(|field| field.value)))
}
```

2. `plugins.rs`: after the bridge's registration, register the
   provider —

```rust
match admission(&celerrate_stdlib_provider::descriptor()) {
    Ok(()) => {
        dynamic_providers.push(celerrate_types::DynamicTypeProviderRegistration {
            identity: celerrate_stdlib_provider::descriptor().identity,
            provider: std::sync::Arc::new(celerrate_stdlib_provider::StdlibProvider::new()),
        });
    }
    Err(reason) => excluded.push(ExcludedPlugin {
        name: celerrate_stdlib_provider::descriptor().identity.name,
        reason,
    }),
}
```

   and replace the recorded-only conflict branch with the rebuild
   loop, factored for the unit test:

```rust
/// Overlapping claims exclude the later registrant and the set is
/// rebuilt until it validates — the plan-7 gap the registration
/// comment recorded, closed.
fn admit_dynamic_providers(
    mut registrations: Vec<celerrate_types::DynamicTypeProviderRegistration>,
) -> (
    Vec<celerrate_types::DynamicTypeProviderRegistration>,
    Vec<ExcludedPlugin>,
) {
    let mut excluded = Vec::new();
    while let Err(conflict) = celerrate_types::validate_claims(&registrations) {
        excluded.push(ExcludedPlugin {
            name: conflict.second.clone(),
            reason: format!(
                "claim conflict with {} on {:?}",
                conflict.first, conflict.claim,
            ),
        });
        registrations
            .retain(|registration| registration.identity.name != conflict.second);
    }
    (registrations, excluded)
}
```

   `register_plugins` routes `dynamic_providers` through it before
   setting the registry. `celerrate_cli/Cargo.toml` gains the
   dependency `celerrate_stdlib_provider = { path =
   "../celerrate_stdlib_provider" }` (the CLI is the composition
   root; it may depend on everything).

3. `dependency_shape.rs`: the const above; update the test fixture
   metadata so both plugin crates appear (the "not found" guard
   protects the new name from a silent rename).

4. End-to-end (in the provider crate's `tests/` or `lib.rs` test
   module with dev-dependencies): a `celerrate_types` fixture with
   the provider registered — mirror `fixpoint.rs`'s
   registry-building idiom — and a body
   `function f() { return current([1, 'a']); }`; assert the inferred
   return displays `1|'a'|false` and `edge_counts.provider_edges == 1`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_stdlib_provider -p celerrate_cli -p xtask && cargo xtask dependency-shape`
Expected: PASS; the shape check accepts the new crate.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_stdlib_provider crates/celerrate_cli xtask/src/dependency_shape.rs Cargo.lock
git commit -m "✨ feat(stdlib): the provider crate registers and claims the pointer family"
```

---

### Task 7: `array_map` and `array_filter`

**Files:**
- Modify: `crates/celerrate_stdlib_provider/src/array_functions.rs`
- Modify: `crates/celerrate_stdlib_provider/src/lib.rs` (claims and
  dispatch)
- Test: the crate's test modules

**Interfaces:**
- Consumes: `TypeId` interrogation (`callable_return`, `array_key`,
  `array_value`, `is_list`, `shape_fields`, `is_null`,
  `constituents`), `pointer_value`/`array_value_of` (task 6).
- Produces: `CLAIMED_FUNCTIONS` grows to
  `["array_filter", "array_map", "current", "end", "reset"]`;
  handlers `array_map(db, arguments)` and
  `array_filter(db, arguments)` in `array_functions.rs`.

**The semantics, fixed** (each rule is one test):

`array_map(callable, array, ...)`:
- Callback `null` literal with exactly two arguments: the answer is
  the array argument unchanged (`array_map(null, $a)` is `$a`).
- Two arguments, callback carries a callable type with return `R`:
  a list argument answers `list<R>`; an array argument with key `K`
  answers `array<K, R>`; a shape argument answers
  `array<K', R>` where `K'` is the union of the shape's keys
  (integer keys as int literals, string keys as string literals).
- More than two arguments (the zip form) with a callable callback:
  `list<R>` (PHP reindexes).
- A callback without a callable type (a `'strtoupper'` string, a
  `mixed`): `None` — the declared tier's answer stands.

`array_filter(array, callback?, mode?)`:
- The key: a list argument answers `array<int<0..>, V>` — filtering
  keeps keys, so list contiguity is lost, but the keys stay
  non-negative integers. An array argument keeps its key type; a
  shape argument answers keys as the shape-key union.
- The value: with one argument (no callback), falsy constituents
  drop — `null` and `false` literals are removed from a union value
  type (`0`/`''`/`'0'` literals too when present as literals); a
  non-union value stays. With a callback (two or three arguments),
  the value type passes through unchanged (the predicate is opaque).
- Non-empty never survives: the result is always the plain
  `array<K, V>` form.
- An unknown subject: `None`.

- [ ] **Step 1: Write the failing tests**

In `array_functions.rs`'s test module (construct invocations with
the task-6 `function_invocation` helper, moved to a shared
`#[cfg(test)] pub(crate) mod test_support` in `lib.rs` if not
already):

```rust
#[test]
fn array_map_with_a_null_callback_answers_the_array_unchanged() {
    let db = TestDatabase::default();
    let subject = TypeId::list(&db, TypeId::int(&db));
    let answer = super::array_map(
        &db,
        &[TypeId::null(&db), subject],
    )
    .unwrap();
    assert_eq!(answer, subject);
}

#[test]
fn array_map_composes_the_callable_return_over_a_list() {
    let db = TestDatabase::default();
    let callback = TypeId::callable(&db, vec![], TypeId::string(&db));
    let subject = TypeId::list(&db, TypeId::int(&db));
    assert_eq!(
        super::array_map(&db, &[callback, subject]).unwrap(),
        TypeId::list(&db, TypeId::string(&db)),
    );
}

#[test]
fn array_map_keeps_the_key_type_over_an_array() {
    let db = TestDatabase::default();
    let callback = TypeId::callable(&db, vec![], TypeId::bool(&db));
    let subject = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
    assert_eq!(
        super::array_map(&db, &[callback, subject]).unwrap(),
        TypeId::array(&db, TypeId::string(&db), TypeId::bool(&db)),
    );
}

#[test]
fn array_map_over_the_zip_form_answers_a_list() {
    let db = TestDatabase::default();
    let callback = TypeId::callable(&db, vec![], TypeId::int(&db));
    let first = TypeId::list(&db, TypeId::int(&db));
    let second = TypeId::list(&db, TypeId::string(&db));
    assert_eq!(
        super::array_map(&db, &[callback, first, second]).unwrap(),
        TypeId::list(&db, TypeId::int(&db)),
    );
}

#[test]
fn array_map_without_a_callable_type_stays_silent() {
    let db = TestDatabase::default();
    let subject = TypeId::list(&db, TypeId::int(&db));
    assert!(
        super::array_map(&db, &[TypeId::string(&db), subject]).is_none(),
    );
    assert!(super::array_map(&db, &[TypeId::mixed(&db)]).is_none());
}

#[test]
fn array_filter_without_a_callback_drops_falsy_constituents() {
    let db = TestDatabase::default();
    let value = TypeId::union(
        &db,
        [
            TypeId::string(&db),
            TypeId::null(&db),
            TypeId::bool_literal(&db, false),
        ],
    );
    let subject = TypeId::array(&db, TypeId::string(&db), value);
    assert_eq!(
        super::array_filter(&db, &[subject]).unwrap(),
        TypeId::array(&db, TypeId::string(&db), TypeId::string(&db)),
    );
}

#[test]
fn array_filter_over_a_list_loses_contiguity_but_keeps_int_keys() {
    let db = TestDatabase::default();
    let subject = TypeId::list(&db, TypeId::int(&db));
    assert_eq!(
        super::array_filter(&db, &[subject]).unwrap(),
        TypeId::array(&db, TypeId::int_range(&db, Some(0), None), TypeId::int(&db)),
    );
}

#[test]
fn array_filter_with_a_callback_passes_the_value_through() {
    let db = TestDatabase::default();
    let value = TypeId::union(&db, [TypeId::string(&db), TypeId::null(&db)]);
    let subject = TypeId::array(&db, TypeId::string(&db), value);
    let callback = TypeId::callable(&db, vec![], TypeId::bool(&db));
    assert_eq!(
        super::array_filter(&db, &[subject, callback]).unwrap(),
        TypeId::array(&db, TypeId::string(&db), value),
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_stdlib_provider`
Expected: FAIL to compile — the handlers do not exist.

- [ ] **Step 3: Implement**

```rust
pub(crate) fn array_map<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let callback = arguments.first()?;
    let subjects = arguments.get(1..)?;
    let first_subject = subjects.first()?;
    if callback.is_null(db) {
        return match subjects {
            [only] => Some(*only),
            _ => None,
        };
    }
    let mapped = callback.callable_return(db)?;
    if subjects.len() > 1 {
        return Some(TypeId::list(db, mapped));
    }
    if first_subject.is_list(db) {
        return Some(TypeId::list(db, mapped));
    }
    Some(TypeId::array(db, array_key_of(db, *first_subject)?, mapped))
}

pub(crate) fn array_filter<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let subject = arguments.first()?;
    let value = array_value_of(db, *subject)?;
    let value = if arguments.len() == 1 {
        without_falsy(db, value)
    } else {
        value
    };
    let key = if subject.is_list(db) {
        TypeId::int_range(db, Some(0), None)
    } else {
        array_key_of(db, *subject)?
    };
    Some(TypeId::array(db, key, value))
}

/// The key type of an array-like subject: arrays answer theirs,
/// shapes the union of their key literals, `None` when unknown.
pub(crate) fn array_key_of<'db>(
    db: &'db dyn salsa::Database,
    subject: TypeId<'db>,
) -> Option<TypeId<'db>> {
    if let Some(key) = subject.array_key(db) {
        return Some(key);
    }
    let fields = subject.shape_fields(db)?;
    if fields.is_empty() {
        return None;
    }
    Some(TypeId::union(
        db,
        fields.into_iter().map(|field| match field.key {
            ShapeKey::Integer(value) => TypeId::int_literal(db, value),
            ShapeKey::String(value) => TypeId::string_literal(db, &value),
        }),
    ))
}

/// Removes the falsy constituents a bare `array_filter` discards:
/// `null`, `false`, `0`, `0.0`, `''`, `'0'`. A constituent set that
/// empties entirely stays unchanged (the conservative floor — an
/// always-falsy value is the caller's bug, not ours to `never`).
fn without_falsy<'db>(
    db: &'db dyn salsa::Database,
    value: TypeId<'db>,
) -> TypeId<'db> {
    let kept: Vec<TypeId<'db>> = value
        .constituents(db)
        .into_iter()
        .filter(|constituent| !is_falsy_literal(db, *constituent))
        .collect();
    if kept.is_empty() {
        value
    } else {
        TypeId::union(db, kept)
    }
}

fn is_falsy_literal<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.is_null(db)
        || of.bool_literal_value(db) == Some(false)
        || of.int_literal_value(db) == Some(0)
        || of.float_literal_value(db) == Some(0.0)
        || matches!(of.string_literal_value(db).as_deref(), Some("") | Some("0"))
}
```

(`ShapeKey` comes through the facade: `celerrate_plugin::ShapeKey`.)
Extend `lib.rs`'s dispatch:

```rust
"array_filter" => array_functions::array_filter(db, arguments),
"array_map" => array_functions::array_map(db, arguments),
```

and `CLAIMED_FUNCTIONS` to
`&["array_filter", "array_map", "current", "end", "reset"]`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_stdlib_provider`
Expected: PASS (the claims-sorted test still holds).

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_stdlib_provider
git commit -m "✨ feat(stdlib): array_map and array_filter compute from their arguments"
```

---

### Task 8: `explode` and `json_decode`

**Files:**
- Create: `crates/celerrate_stdlib_provider/src/string_functions.rs`
- Create: `crates/celerrate_stdlib_provider/src/json_functions.rs`
- Modify: `crates/celerrate_stdlib_provider/src/lib.rs`
- Test: the new modules' test modules

**Interfaces:**
- Consumes: `TypeId` interrogation (`int_literal_value`,
  `bool_literal_value`), constructors.
- Produces: `CLAIMED_FUNCTIONS` grows to `["array_filter",
  "array_map", "current", "end", "explode", "json_decode", "reset"]`;
  handlers `explode(db, arguments)` and
  `json_decode(db, arguments)`.

**The semantics, fixed** (decision 12 for `json_decode`):

`explode(separator, subject, limit?)`:
- No limit argument, or an integer-literal limit `>= 1`:
  `non-empty-list<string>` (PHP answers at least `[$subject]`).
- An integer-literal limit `< 1`: `list<string>` (a negative limit
  can empty the result; a zero limit is treated as 1 by PHP but the
  distinction buys nothing — the plain list is sound).
- A non-literal limit: `list<string>`.
- The answer never depends on the separator's value (an empty
  separator makes `explode` return `false` before PHP 8.0 and throw
  after — both outside the checked range's honesty; the stub's
  declared union already covers the historical `false`).

`json_decode(json, associative?, depth?, flags?)` (decision 12,
amended — a non-`null` `associative` overrides the flags argument in
BOTH directions, per PHP's `ext/json/json.c` "for BC reasons"
override):
- The scalar tail is always
  `bool|float|int|string|null`.
- Associative literal `true`: the array branch —
  `array<int|string, mixed>` unioned with the tail — regardless of
  the flags argument.
- Associative literal `false`: the object branch — `stdClass`
  unioned with the tail — regardless of the flags argument.
- The `null` literal, or absent (`?bool $associative = null` since
  PHP 7.4: an explicit `null` behaves exactly like an absent
  argument): the flags argument (index 3) decides — an integer
  literal with bit `JSON_OBJECT_AS_ARRAY = 1` set selects the array
  branch, without it selects the object branch, and a non-literal
  flags argument leaves the answer undecided (both branches).
- Associative present but neither a bool literal nor `null`: both
  branches unioned with the tail, regardless of flags (it may be
  `false` at runtime; answering the array branch alone would be
  unsound).
- `null` never leaves the union (`"null"` decodes to `null`;
  `JSON_THROW_ON_ERROR` changes the error path, not the `null`
  value).

- [ ] **Step 1: Write the failing tests**

```rust
// string_functions.rs
#[test]
fn explode_without_a_limit_answers_a_non_empty_list() {
    let db = TestDatabase::default();
    assert_eq!(
        super::explode(&db, &[TypeId::string(&db), TypeId::string(&db)]).unwrap(),
        TypeId::non_empty_list(&db, TypeId::string(&db)),
    );
}

#[test]
fn explode_with_a_positive_literal_limit_stays_non_empty() {
    let db = TestDatabase::default();
    assert_eq!(
        super::explode(
            &db,
            &[
                TypeId::string(&db),
                TypeId::string(&db),
                TypeId::int_literal(&db, 3),
            ],
        )
        .unwrap(),
        TypeId::non_empty_list(&db, TypeId::string(&db)),
    );
}

#[test]
fn explode_with_a_negative_or_unknown_limit_answers_a_plain_list() {
    let db = TestDatabase::default();
    for limit in [TypeId::int_literal(&db, -1), TypeId::int(&db)] {
        assert_eq!(
            super::explode(
                &db,
                &[TypeId::string(&db), TypeId::string(&db), limit],
            )
            .unwrap(),
            TypeId::list(&db, TypeId::string(&db)),
        );
    }
}

// json_functions.rs
#[test]
fn json_decode_defaults_to_the_object_branch() {
    let db = TestDatabase::default();
    let answer = super::json_decode(&db, &[TypeId::string(&db)]).unwrap();
    assert_eq!(answer, super::object_branch(&db));
}

#[test]
fn an_associative_true_literal_selects_the_array_branch() {
    let db = TestDatabase::default();
    let answer = super::json_decode(
        &db,
        &[TypeId::string(&db), TypeId::bool_literal(&db, true)],
    )
    .unwrap();
    assert_eq!(answer, super::array_branch(&db));
}

#[test]
fn an_associative_false_literal_overrides_the_object_as_array_flag() {
    // `ext/json/json.c` carries this override as an explicit "for BC
    // reasons" comment: a non-null `associative` beats the flag in
    // both directions, so the flag being set does not win here
    // (decision 12, amended).
    let db = TestDatabase::default();
    let answer = super::json_decode(
        &db,
        &[
            TypeId::string(&db),
            TypeId::bool_literal(&db, false),
            TypeId::int_literal(&db, 512),
            TypeId::int_literal(&db, super::JSON_OBJECT_AS_ARRAY),
        ],
    )
    .unwrap();
    assert_eq!(answer, super::object_branch(&db));
}

#[test]
fn a_null_associative_falls_back_to_the_object_as_array_flag_when_set() {
    // `associative` is `null`, so — unlike the test above — the flag
    // is the decider.
    let db = TestDatabase::default();
    let answer = super::json_decode(
        &db,
        &[
            TypeId::string(&db),
            TypeId::null(&db),
            TypeId::int_literal(&db, 512),
            TypeId::int_literal(&db, super::JSON_OBJECT_AS_ARRAY),
        ],
    )
    .unwrap();
    assert_eq!(answer, super::array_branch(&db));
}

#[test]
fn an_undecided_associative_argument_answers_both_branches() {
    let db = TestDatabase::default();
    let answer = super::json_decode(
        &db,
        &[TypeId::string(&db), TypeId::bool(&db)],
    )
    .unwrap();
    assert_eq!(answer, super::both_branches(&db));
}

#[test]
fn an_explicit_null_associative_behaves_like_an_absent_one() {
    // `?bool $associative = null` since PHP 7.4: an explicit `null`
    // is exactly the absent argument (decision 12).
    let db = TestDatabase::default();
    let answer = super::json_decode(
        &db,
        &[TypeId::string(&db), TypeId::null(&db)],
    )
    .unwrap();
    assert_eq!(answer, super::object_branch(&db));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_stdlib_provider`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
// string_functions.rs
pub(crate) fn explode<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    if arguments.len() < 2 {
        return None;
    }
    let non_empty = match arguments.get(2) {
        None => true,
        Some(limit) => match limit.int_literal_value(db) {
            Some(value) => value >= 1,
            None => false,
        },
    };
    Some(if non_empty {
        TypeId::non_empty_list(db, TypeId::string(db))
    } else {
        TypeId::list(db, TypeId::string(db))
    })
}

// json_functions.rs
/// PHP's decode-side flag selecting the array branch.
pub(crate) const JSON_OBJECT_AS_ARRAY: i64 = 1;

/// `json_decode(json, associative?, depth?, flags?)`: decision 12
/// (amended). The scalar tail (`bool|float|int|string|null`) is
/// always present. PHP's `ext/json/json.c` overrides the
/// `JSON_OBJECT_AS_ARRAY` flag with a non-`null` `$associative` in
/// BOTH directions ("for BC reasons"): a `true` associative literal
/// selects the array branch and a `false` associative literal selects
/// the object branch, regardless of the flags argument. The flags
/// argument decides only when `associative` is the `null` literal
/// (the `?bool $associative = null` default since PHP 7.4) or absent:
/// an integer-literal flags argument with the `JSON_OBJECT_AS_ARRAY`
/// bit set selects the array branch, without it selects the object
/// branch, and a non-literal flags argument leaves the answer
/// undecided (both branches). Any other associative reading (present,
/// neither a bool literal nor `null`) also answers both branches,
/// regardless of flags — it may be `false` at runtime.
pub(crate) fn json_decode<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    if arguments.is_empty() {
        return None;
    }
    let flags = arguments.get(3);
    let flag_selects_array = flags
        .and_then(|flags| flags.int_literal_value(db))
        .is_some_and(|value| value & JSON_OBJECT_AS_ARRAY != 0);
    let flags_undecided = flags.is_some_and(|flags| flags.int_literal_value(db).is_none());
    Some(match arguments.get(1) {
        None => flags_branch(db, flag_selects_array, flags_undecided),
        Some(associative) => match associative.bool_literal_value(db) {
            // A non-`null` associative overrides the flag in both
            // directions (PHP's BC-reasons override), regardless of
            // what the flags argument says.
            Some(true) => array_branch(db),
            Some(false) => object_branch(db),
            // An explicit `null` behaves exactly like an absent
            // argument (`?bool $associative = null` since PHP 7.4):
            // the flags argument decides.
            None if associative.is_null(db) => {
                flags_branch(db, flag_selects_array, flags_undecided)
            }
            _ => both_branches(db),
        },
    })
}

/// The answer when `associative` is `null` or absent: the flags
/// argument is the sole decider.
fn flags_branch<'db>(
    db: &'db dyn salsa::Database,
    flag_selects_array: bool,
    flags_undecided: bool,
) -> TypeId<'db> {
    if flags_undecided {
        both_branches(db)
    } else if flag_selects_array {
        array_branch(db)
    } else {
        object_branch(db)
    }
}

fn scalar_tail<'db>(db: &'db dyn salsa::Database) -> [TypeId<'db>; 5] {
    [
        TypeId::bool(db),
        TypeId::float(db),
        TypeId::int(db),
        TypeId::string(db),
        TypeId::null(db),
    ]
}

pub(crate) fn array_branch<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    let array = TypeId::array(
        db,
        TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
        TypeId::mixed(db),
    );
    TypeId::union(db, scalar_tail(db).into_iter().chain([array]))
}

pub(crate) fn object_branch<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    let object = TypeId::class(db, "stdclass", vec![]);
    TypeId::union(db, scalar_tail(db).into_iter().chain([object]))
}

pub(crate) fn both_branches<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    TypeId::union(db, [array_branch(db), object_branch(db)])
}
```

Dispatch and claims in `lib.rs`:

```rust
"explode" => string_functions::explode(db, arguments),
"json_decode" => json_functions::json_decode(db, arguments),
```

`CLAIMED_FUNCTIONS: &["array_filter", "array_map", "current", "end",
"explode", "json_decode", "reset"]`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_stdlib_provider`
Expected: PASS.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_stdlib_provider
git commit -m "✨ feat(stdlib): explode and json_decode answer from their arguments"
```

---

### Task 9: The by-reference channel and `preg_match`

**Files:**
- Modify: `crates/celerrate_types/src/dynamic_type_provider.rs`
- Modify: `crates/celerrate_types/src/flow.rs`
- Create: `crates/celerrate_stdlib_provider/src/pattern_functions.rs`
- Modify: `crates/celerrate_stdlib_provider/src/lib.rs`
- Modify: `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`
- Test: `dynamic_type_provider.rs`, `pattern_functions.rs`, and
  `inference.rs` test modules

**Interfaces:**
- Consumes: `Invocation`, `provider_return`'s registry loop shape,
  `apply_by_reference` and the free-function call arm
  (`flow.rs:1755-1813` pre-plan-6 numbering), `subject_of`,
  `Environment::bind`, `capped_child`.
- Produces:
  - On the trait (facade-visible through the existing re-export):

```rust
/// By-reference parameter refinements for a claimed invocation:
/// (positional parameter index, the type the argument holds after
/// the call). The default contributes nothing. Same purity and
/// monotonicity contract as `return_type`; contributions are
/// widened at the consumption boundary. Positional only — the
/// consumer skips labeled arguments and stops at a spread.
fn by_reference_types<'db>(
    &self,
    db: &'db dyn salsa::Database,
    invocation: &Invocation<'db>,
) -> Vec<(usize, TypeId<'db>)> {
    let _ = (db, invocation);
    Vec::new()
}
```

  - On `Walker` (flow.rs): `fn provider_by_reference(&mut self,
    claim: SymbolClaim, receiver_type: Option<TypeId<'db>>,
    argument_types: &[TypeId<'db>]) -> Vec<(usize, TypeId<'db>)>`
    and `fn apply_provider_by_reference(&mut self, contributions:
    &[(usize, TypeId<'db>)], arguments: &[CallArgument],
    environment: &mut Environment<'db>)`.
  - `CLAIMED_FUNCTIONS` grows `preg_match`; the handlers
    `preg_match_return(db)` and
    `preg_match_matches(db, arguments)` in `pattern_functions.rs`;
    the group scanner `pattern_groups(pattern: &str) ->
    Option<Vec<PatternGroup>>` with
    `enum PatternGroup { Numbered, Named(String) }`.

- [ ] **Step 1: Write the failing tests**

In `dynamic_type_provider.rs`'s test module — the default method:

```rust
#[test]
fn the_by_reference_channel_defaults_to_empty() {
    let db = TestDatabase::default();
    let provider = FakeProvider { claimed: vec![] };
    let invocation = Invocation {
        claim: SymbolClaim::Function { key: "any".to_owned() },
        receiver_type: None,
        argument_types: vec![],
    };
    assert!(provider.by_reference_types(&db, &invocation).is_empty());
}
```

In `pattern_functions.rs` — the scanner and the shapes:

```rust
#[test]
fn the_scanner_counts_capturing_groups() {
    assert_eq!(
        pattern_groups("/(a)(b)/").unwrap(),
        vec![PatternGroup::Numbered, PatternGroup::Numbered],
    );
}

#[test]
fn non_capturing_and_lookaround_groups_do_not_count() {
    assert_eq!(pattern_groups("/(?:a)(?=b)(?!c)(?<=d)(?<!e)/").unwrap(), vec![]);
}

#[test]
fn named_groups_carry_their_names_in_all_three_spellings() {
    assert_eq!(
        pattern_groups("/(?P<year>\\d+)-(?<month>\\d+)-(?'day'\\d+)/").unwrap(),
        vec![
            PatternGroup::Named("year".to_owned()),
            PatternGroup::Named("month".to_owned()),
            PatternGroup::Named("day".to_owned()),
        ],
    );
}

#[test]
fn escapes_and_character_classes_hide_their_parentheses() {
    assert_eq!(pattern_groups("/\\((a)[)(]/").unwrap(), vec![PatternGroup::Numbered]);
}

#[test]
fn bracket_style_delimiters_pair() {
    assert_eq!(
        pattern_groups("{(a)}").unwrap(),
        vec![PatternGroup::Numbered],
    );
}

#[test]
fn a_degenerate_pattern_answers_none_never_panics() {
    for pattern in ["", "/", "x", "((((", "/\\", "[", "/(?P<"] {
        // None or a best-effort group list — the only hard
        // requirement is: no panic, and None on patterns without a
        // readable body.
        let _ = pattern_groups(pattern);
    }
    assert!(pattern_groups("").is_none());
    assert!(pattern_groups("x").is_none());
}

#[test]
fn matches_shape_is_all_optional_with_both_key_spellings() {
    let db = TestDatabase::default();
    let pattern = TypeId::string_literal(&db, "/(?<year>\\d+)-(\\d+)/");
    let answer = super::preg_match_matches(&db, &[pattern, TypeId::string(&db)])
        .unwrap();
    let fields = answer.shape_fields(&db).unwrap();
    // {0?: string, year?: string, 1?: string, 2?: string}: group 0,
    // the named group under both its name and its number, the
    // second group under its number. Every field optional, every
    // value string.
    assert!(fields.iter().all(|field| field.optional));
    assert!(
        fields
            .iter()
            .all(|field| field.value == TypeId::string(&db)),
    );
    let keys: Vec<ShapeKey> = fields.iter().map(|field| field.key.clone()).collect();
    assert!(keys.contains(&ShapeKey::Integer(0)));
    assert!(keys.contains(&ShapeKey::Integer(1)));
    assert!(keys.contains(&ShapeKey::Integer(2)));
    assert!(keys.contains(&ShapeKey::String("year".to_owned())));
}

#[test]
fn a_flags_argument_or_unknown_pattern_falls_back_conservatively() {
    let db = TestDatabase::default();
    let int_or_string = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
    // Unknown pattern: values are still strings.
    assert_eq!(
        super::preg_match_matches(&db, &[TypeId::string(&db), TypeId::string(&db)])
            .unwrap(),
        TypeId::array(&db, int_or_string, TypeId::string(&db)),
    );
    // A non-zero-literal flags argument: values are opaque.
    let pattern = TypeId::string_literal(&db, "/(a)/");
    assert_eq!(
        super::preg_match_matches(
            &db,
            &[
                pattern,
                TypeId::string(&db),
                TypeId::mixed(&db), // the $matches slot
                TypeId::int(&db),   // unknown flags
            ],
        )
        .unwrap(),
        TypeId::array(&db, int_or_string, TypeId::mixed(&db)),
    );
}

#[test]
fn the_return_is_zero_one_or_false() {
    let db = TestDatabase::default();
    assert_eq!(
        super::preg_match_return(&db),
        TypeId::union(
            &db,
            [
                TypeId::int_literal(&db, 0),
                TypeId::int_literal(&db, 1),
                TypeId::bool_literal(&db, false),
            ],
        ),
    );
}
```

In `inference.rs`'s test module — the flow wiring end to end (the
fixture registers `StdlibProvider` the way `fixpoint.rs` registers
its fake; embedded stubs so `preg_match` exists and its `$matches`
parameter is by-reference):

```rust
#[test]
fn preg_match_refines_matches_through_the_by_reference_channel() {
    let f = fixture_with_embedded_stubs_and_stdlib_provider(&[r#"<?php
function consume(string $subject) {
    if (preg_match('/(?<year>\d+)/', $subject, $matches) === 1) {
        return $matches;
    }
    return null;
}
"#]);
    let inferred = inferred_function_return(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "consume".to_owned()),
    );
    // The all-optional shape, not the declared write-back's plain
    // array: the provider overrode the general rule. Adjust the
    // shape spelling to display.rs's rendering if it differs.
    let display = inferred.display(&f.db);
    assert!(display.contains("year"), "{display}");
    assert!(!display.contains("array<"), "{display}");
}

#[test]
fn a_spread_argument_stops_the_by_reference_application() {
    // No panic, no binding: the contribution indices have no
    // positional argument to land on.
    let f = fixture_with_embedded_stubs_and_stdlib_provider(&[r#"<?php
function consume(array $arguments): void {
    preg_match(...$arguments);
}
"#]);
    let inferred = inferred_body_types(
        &f.db, f.files, f.stubs, f.configuration,
        FileId::new(0), body_query(&f, 0),
        InferenceContext::new(&f.db, None),
    );
    // Completion without a panic is the contract; the body still
    // types every expression. The recording assertion lives in
    // task 10, where `stub_calls` exists.
    assert!(!inferred.expression_types.is_empty());
}
```

(The spread test's load-bearing assertion is completion without a
panic; task 10 re-covers the same body with the recording assertion
once `stub_calls` exists.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types -p celerrate_stdlib_provider`
Expected: the trait-default test FAILS to compile (no method); the
scanner tests FAIL to compile (no module).

- [ ] **Step 3: Implement**

1. The trait method (exactly the Interfaces block's code) appended
   to `DynamicTypeProvider` in `dynamic_type_provider.rs`. The
   module doc gains the channel's sentence; the trait stays
   object-safe (`&self`, no generics beyond the `'db` lifetime the
   existing method already has).

2. `flow.rs` — mirror `provider_return`'s registry loop:

```rust
/// The by-reference sibling of `provider_return`: first claiming
/// registration wins; every contribution is widened at the
/// consumption boundary. Applied after `apply_by_reference`, so a
/// provider refines (overrides) the declared write-back.
fn provider_by_reference(
    &mut self,
    claim: crate::dynamic_type_provider::SymbolClaim,
    receiver_type: Option<TypeId<'db>>,
    argument_types: &[TypeId<'db>],
) -> Vec<(usize, TypeId<'db>)> {
    let db = self.db();
    let Some(registry) =
        crate::dynamic_type_provider::DynamicTypeProviderRegistry::try_get(db)
    else {
        return Vec::new();
    };
    for registration in registry.registrations(db) {
        if !registration.provider.claims().contains(&claim) {
            continue;
        }
        let invocation = crate::dynamic_type_provider::Invocation {
            claim: claim.clone(),
            receiver_type,
            argument_types: argument_types.to_vec(),
        };
        let contributions = registration.provider.by_reference_types(db, &invocation);
        if !contributions.is_empty() {
            return contributions
                .into_iter()
                .map(|(index, of)| (index, crate::widening::capped_child(db, of)))
                .collect();
        }
    }
    Vec::new()
}

/// Binds provider by-reference contributions onto their positional
/// arguments' subjects. Labeled arguments are skipped (the channel
/// is positional); a spread ends the mapping, like
/// `apply_by_reference`.
fn apply_provider_by_reference(
    &mut self,
    contributions: &[(usize, TypeId<'db>)],
    arguments: &[celerrate_semantics::CallArgument],
    environment: &mut Environment<'db>,
) {
    for (index, of) in contributions {
        let Some(argument) = arguments.get(*index) else {
            continue;
        };
        if arguments
            .iter()
            .take(*index + 1)
            .any(|argument| argument.spread)
            || argument.label.is_some()
        {
            continue;
        }
        if let Some(subject) = subject_of(self.context.ir, argument.value) {
            environment.bind(subject, *of);
        }
    }
}
```

   At the free-function call arm (where `apply_by_reference` already
   runs with the declared signature), after it:

```rust
let contributions =
    self.provider_by_reference(claim.clone(), None, &argument_types);
self.apply_provider_by_reference(&contributions, &arguments, environment);
```

   (Build `claim` once at the top of the arm and clone into both
   provider calls. The method-call sites stay unwired — a recorded
   debt, decision 10.)

3. `pattern_functions.rs`:

```rust
//! `preg_match`: the return literals and the pattern-derived
//! `$matches` shape (decision 11). The group scanner is lexical —
//! delimiters, escapes, character classes, non-capturing markers,
//! the three named-group spellings — not a regex parser:
//! alternation-aware optionality is a recorded debt, and a
//! conditional group (`(?(1)a|b)`) counts one spurious group.

use celerrate_plugin::{ShapeField, ShapeKey, TypeId, salsa};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatternGroup {
    Numbered,
    Named(String),
}

pub(crate) fn preg_match_return<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    TypeId::union(
        db,
        [
            TypeId::int_literal(db, 0),
            TypeId::int_literal(db, 1),
            TypeId::bool_literal(db, false),
        ],
    )
}

pub(crate) fn preg_match_matches<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    if arguments.len() < 2 {
        return None;
    }
    let int_or_string = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
    let flags = arguments.get(3);
    let flags_decided_zero = match flags {
        None => true,
        Some(flags) => flags.int_literal_value(db) == Some(0),
    };
    if !flags_decided_zero {
        // PREG_OFFSET_CAPTURE and friends change the value shape.
        return Some(TypeId::array(db, int_or_string, TypeId::mixed(db)));
    }
    let groups = arguments
        .first()
        .and_then(|pattern| pattern.string_literal_value(db))
        .and_then(|pattern| pattern_groups(&pattern));
    let Some(groups) = groups else {
        return Some(TypeId::array(db, int_or_string, TypeId::string(db)));
    };
    let mut fields = vec![ShapeField {
        key: ShapeKey::Integer(0),
        optional: true,
        value: TypeId::string(db),
    }];
    for (position, group) in groups.iter().enumerate() {
        let number = position as i64 + 1;
        if let PatternGroup::Named(name) = group {
            fields.push(ShapeField {
                key: ShapeKey::String(name.clone()),
                optional: true,
                value: TypeId::string(db),
            });
        }
        fields.push(ShapeField {
            key: ShapeKey::Integer(number),
            optional: true,
            value: TypeId::string(db),
        });
    }
    Some(TypeId::shape(db, fields))
}

/// The capturing groups of a PCRE pattern, in order. `None` when
/// the pattern has no readable delimited body.
pub(crate) fn pattern_groups(pattern: &str) -> Option<Vec<PatternGroup>> {
    let mut characters = pattern.chars();
    let opening = characters.next()?;
    let closing = match opening {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        delimiter if !delimiter.is_alphanumeric()
            && delimiter != '\\'
            && !delimiter.is_whitespace() => delimiter,
        _ => return None,
    };
    let rest: String = characters.collect();
    let body_end = rest.rfind(closing)?;
    let body = rest.get(..body_end)?;
    let mut groups = Vec::new();
    let mut cursor = body.chars().peekable();
    while let Some(character) = cursor.next() {
        match character {
            '\\' => {
                cursor.next();
            }
            '[' => {
                // A character class: escapes still hide, `]` ends it.
                while let Some(inner) = cursor.next() {
                    match inner {
                        '\\' => {
                            cursor.next();
                        }
                        ']' => break,
                        _ => {}
                    }
                }
            }
            '(' => {
                if cursor.peek() != Some(&'?') {
                    groups.push(PatternGroup::Numbered);
                    continue;
                }
                cursor.next(); // the '?'
                match cursor.peek() {
                    Some('P') => {
                        cursor.next();
                        if cursor.peek() == Some(&'<') {
                            cursor.next();
                            groups.push(named_group(&mut cursor, '>'));
                        }
                    }
                    Some('<') => {
                        cursor.next();
                        // `(?<name>` captures; `(?<=` / `(?<!` do not.
                        match cursor.peek() {
                            Some('=') | Some('!') => {}
                            _ => groups.push(named_group(&mut cursor, '>')),
                        }
                    }
                    Some('\'') => {
                        cursor.next();
                        groups.push(named_group(&mut cursor, '\''));
                    }
                    _ => {} // (?:, (?=, (?!, modifiers: non-capturing
                }
            }
            _ => {}
        }
    }
    Some(groups)
}

fn named_group(
    cursor: &mut std::iter::Peekable<std::str::Chars<'_>>,
    terminator: char,
) -> PatternGroup {
    let mut name = String::new();
    while let Some(&character) = cursor.peek() {
        cursor.next();
        if character == terminator {
            break;
        }
        name.push(character);
    }
    PatternGroup::Named(name)
}
```

4. `lib.rs`: claims gain `"preg_match"` (keep the const sorted);
   dispatch:

```rust
"preg_match" => Some(pattern_functions::preg_match_return(db)),
```

   and the by-reference override:

```rust
fn by_reference_types<'db>(
    &self,
    db: &'db dyn salsa::Database,
    invocation: &Invocation<'db>,
) -> Vec<(usize, TypeId<'db>)> {
    let SymbolClaim::Function { key } = &invocation.claim else {
        return Vec::new();
    };
    match key.as_str() {
        "preg_match" => {
            pattern_functions::preg_match_matches(db, &invocation.argument_types)
                .map(|matches| vec![(2, matches)])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}
```

5. The WASM sketch
   (`.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`):
   add one line to the guest-export families — the
   dynamic-type-provider guest exports now count `return_type` **and
   `by_reference_types`**; no new acceptance case (the contributions
   are plain (index, handle) pairs, call-scoped like every handle).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types -p celerrate_stdlib_provider`
Expected: PASS. The `$matches` end-to-end test may flush out the
call-arm ordering (the contribution must bind after
`apply_by_reference` and before `apply_call_assertions` reads the
environment) — the fixed order is: kill property bindings → declared
write-back → provider write-back → assertions.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_types crates/celerrate_stdlib_provider .claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md
git commit -m "✨ feat(stdlib): preg_match refines its matches through the by-reference channel"
```

---

### Task 10: The mixed-rate instrument

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs`
- Modify: `crates/celerrate_types/src/flow.rs`
- Modify: `crates/celerrate_types/src/lib.rs`
- Modify: `crates/celerrate_cli/src/arguments.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (dispatch)
- Create: `crates/celerrate_cli/src/mixed_rate.rs`
- Create: `xtask/src/mixed_rate.rs`
- Modify: `xtask/src/main.rs`, `xtask/src/lib.rs`
- Modify: `.github/workflows/corpus.yml`
- Create: `xtask/mixed-rate-baseline.txt` (blessed at the end)
- Test: `inference.rs`'s test module, `mixed_rate.rs` unit tests,
  the xtask gate itself

**Interfaces:**
- Consumes: `InferredBody`, the free-function call arm and
  `resolved_function_key` (flow.rs), `stub_symbol_table`
  (`celerrate_semantics`), the plan-6 `ground-truth` subcommand's
  session/enumeration idiom (`celerrate_cli/src/ground_truth.rs` —
  reuse its body iteration exactly, never a parallel loader),
  `xtask::corpus::prepare()` and `xtask::release_binary()`.
- Produces:
  - `pub struct StubCallRecord { pub callee: String, pub mixed:
    bool }` (derives `Debug, Clone, PartialEq, Eq`), re-exported
    from `celerrate_types`.
  - `InferredBody.stub_calls: Vec<StubCallRecord>` — one record per
    stub-function call expression in the body (task-14 recording
    rule).
  - `Command::MixedRate { path: PathBuf }`, hidden
    (`#[command(hide = true)]`).
  - The report format (task 12 and plan 9b rely on it):
    line 1 `expressions <total>\tmixed <count>`, then
    `<callee>\t<mixed>\t<total>` per callee, sorted by callee,
    trailing newline.
  - `cargo xtask mixed-rate [--bless]` gating
    `xtask/mixed-rate-baseline.txt` byte-for-byte.

- [ ] **Step 1: Write the failing tests**

In `inference.rs`'s test module:

```rust
#[test]
fn stub_function_calls_are_recorded_with_their_mixed_verdict() {
    // Embedded stubs: `array_keys` is refined (non-mixed answer),
    // `getenv` is not (its declared union is honest but this body
    // discards it into a mixed context — pick any stub function
    // whose call answers mixed here; `unserialize` is a safe one).
    let f = fixture_with_embedded_stubs(&[r#"<?php
function consume(): void {
    $keys = array_keys(['a' => 1]);
    $value = unserialize('x');
}
"#]);
    let inferred = inferred_body_types(
        &f.db, f.files, f.stubs, f.configuration,
        FileId::new(0), body_query(&f, 0),
        InferenceContext::new(&f.db, None),
    );
    let callees: Vec<(&str, bool)> = inferred
        .stub_calls
        .iter()
        .map(|record| (record.callee.as_str(), record.mixed))
        .collect();
    assert!(callees.contains(&("array_keys", false)));
    assert!(callees.iter().any(|(callee, _)| *callee == "unserialize"));
}

#[test]
fn source_function_calls_are_not_recorded() {
    let f = fixture(&[r#"<?php
function helper(): int { return 1; }
function consume(): void { $x = helper(); }
"#]);
    let inferred = inferred_body_types(
        &f.db, f.files, f.stubs, f.configuration,
        FileId::new(0), body_query(&f, 1),
        InferenceContext::new(&f.db, None),
    );
    assert!(inferred.stub_calls.is_empty());
}

#[test]
fn a_spread_call_to_a_stub_function_is_still_recorded() {
    // The task-9 spread body, re-covered with the recording
    // assertion now that `stub_calls` exists: the by-reference
    // application stops at the spread, the record does not.
    let f = fixture_with_embedded_stubs(&[r#"<?php
function consume(array $arguments): void {
    preg_match(...$arguments);
}
"#]);
    let inferred = inferred_body_types(
        &f.db, f.files, f.stubs, f.configuration,
        FileId::new(0), body_query(&f, 0),
        InferenceContext::new(&f.db, None),
    );
    assert_eq!(inferred.stub_calls.len(), 1);
}
```

(If `unserialize`'s call happens to answer non-mixed, any stub
function without a refinement whose declared return folds to `mixed`
serves; the assertion that matters is presence with the verdict
matching `TypeId::is_mixed` of the recorded call expression.)

In `crates/celerrate_cli/src/mixed_rate.rs` — unit-test the pure
aggregation before wiring it:

```rust
#[test]
fn the_report_sorts_by_callee_and_sums_per_callee() {
    let report = render_report(
        7,
        3,
        &[
            StubCallRecord { callee: "b".to_owned(), mixed: true },
            StubCallRecord { callee: "a".to_owned(), mixed: false },
            StubCallRecord { callee: "b".to_owned(), mixed: false },
        ],
    );
    assert_eq!(
        report,
        "expressions 7\tmixed 3\n\
         a\t0\t1\n\
         b\t1\t2\n",
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types stub_calls && cargo test -p celerrate_cli mixed_rate`
Expected: FAIL to compile — no field, no module.

- [ ] **Step 3: Implement**

1. `inference.rs`:

```rust
/// One stub-function call and whether its expression stayed
/// `mixed` — the residual instrument stub curation measures its
/// exit with (design sections 7 and 9). Recorded by the walker at
/// the call boundary; nothing re-derives callee resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubCallRecord {
    /// The folded function key.
    pub callee: String,
    pub mixed: bool,
}
```

   `InferredBody` gains `pub stub_calls: Vec<StubCallRecord>`;
   every construction site initializes it (the walker's result
   assembly carries the accumulated vector; cycle-initial and
   bailout constructions carry `Vec::new()`).

2. `flow.rs`, in the free-function call arm, after the call's
   result type `of` is known (provider or `function_call_result`)
   and before the arm records it:

```rust
if !source_exists
    && celerrate_semantics::stub_symbol_table(
        db, self.context.stubs, self.context.configuration,
    )
    .lookup(celerrate_semantics::SymbolSpace::Function, &key)
    .is_some()
{
    self.stub_calls.push(crate::inference::StubCallRecord {
        callee: key.clone(),
        mixed: of.is_mixed(db),
    });
}
```

   (`self.stub_calls` is a new `Vec` on `Walker`, drained into
   `InferredBody` where `edge_counts` already lands. Adapt the
   `lookup` call to the table's real accessor shape.)

3. `celerrate_types/src/lib.rs`: re-export `StubCallRecord` next to
   `InferredBody`.

4. `arguments.rs`: the hidden variant, mirroring plan 6's
   `GroundTruth`:

```rust
/// Internal: the residual mixed-rate counters over a project
/// (design sections 7 and 9). Plan 9b publishes the number; this
/// stays hidden until then.
#[command(hide = true)]
MixedRate { path: PathBuf },
```

5. `mixed_rate.rs`: reuse `ground_truth.rs`'s session entry and body
   enumeration verbatim (same loader, same iteration); for every
   body, `inferred_body_types(...)`: add
   `inferred.expression_types.len()` to the total, count
   `expression_types.iter().filter(|of| of.is_mixed(db))` into the
   mixed sum, and extend a `Vec<StubCallRecord>` with
   `inferred.stub_calls.clone()`. Render:

```rust
pub(crate) fn render_report(
    expressions: usize,
    mixed: usize,
    calls: &[celerrate_types::StubCallRecord],
) -> String {
    use std::collections::BTreeMap;
    let mut per_callee: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for record in calls {
        let entry = per_callee.entry(record.callee.as_str()).or_insert((0, 0));
        entry.1 += 1;
        if record.mixed {
            entry.0 += 1;
        }
    }
    let mut report = format!("expressions {expressions}\tmixed {mixed}\n");
    for (callee, (mixed_calls, total)) in per_callee {
        report.push_str(&format!("{callee}\t{mixed_calls}\t{total}\n"));
    }
    report
}
```

   The command prints the report to stdout and exits 0 (analysis
   errors follow `ground-truth`'s handling).

6. `xtask/src/mixed_rate.rs`: mirror `corpus.rs`'s
   `check_snapshot` — `prepare()` the corpus, delete its
   `.celerrate` directory (cold), run
   `<release binary> mixed-rate <corpus>` capturing stdout (exit
   codes 0/1 tolerated), then byte-compare against
   `xtask/mixed-rate-baseline.txt`; on `--bless` write it; on
   divergence write `target/corpus/actual-mixed-rate.txt`, show the
   `git --no-pager diff --no-index` between them, and fail asking
   for review and re-bless. Dispatch in `main.rs`:
   `"mixed-rate"` → `mixed_rate::check(false)`,
   with `--bless` → `check(true)`.

7. `.github/workflows/corpus.yml`: a `mixed-rate` job cloned from
   the plan-6 `ground-truth` job (same corpus cache key, runs
   `cargo xtask mixed-rate`).

- [ ] **Step 4: Run the tests, bless the initial baseline**

Run: `cargo test --workspace`
Expected: PASS.

Run: `cargo xtask fetch-corpus && cargo xtask mixed-rate --bless`
Expected: `xtask/mixed-rate-baseline.txt` written — the
pre-curation numbers (the provider families already active). Then:

Run: `cargo xtask mixed-rate`
Expected: green against the fresh baseline.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

```bash
git add crates/celerrate_types crates/celerrate_cli xtask .github/workflows/corpus.yml
git commit -m "✨ feat(cli): the mixed-rate instrument and its corpus baseline"
```

---

### Task 11: Curation — the measured exit, and the norm revision

**Files:**
- Modify: `crates/celerrate_stubs/refinements.celerrate`
- Modify: `crates/celerrate_stubs/src/stubs.bin` (recompiled)
- Modify: `xtask/mixed-rate-baseline.txt` (re-blessed)
- Modify: `.claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md`
- Test: the existing gates (`compile-stubs --check`, the totality
  test, `mixed-rate`, `corpus`, `ground-truth`)

**Interfaces:**
- Consumes: everything tasks 1–10 built; the task-10 baseline (the
  per-callee table is the work list).
- Produces: the curated `refinements.celerrate`; the re-blessed
  baseline; the revised norm draft (section 5's questions answered);
  the decision-15 exit evidence in the commit message.

- [ ] **Step 1: Read the work list**

Run: `cargo xtask mixed-rate` (against the task-10 baseline) and
open `xtask/mixed-rate-baseline.txt`. Sort the callee table by
`<total>` descending: that is the curation queue. For each callee
with `<mixed>` > 0, decide the channel by decision 1: declarative →
a refinement entry; computation-dependent → a provider handler
(only if the corpus genuinely demands one beyond tasks 6–9 — new
handlers here follow the task-7 shape: semantics fixed as rules,
one test per rule, claims kept sorted).

- [ ] **Step 2: Write the refinement entries**

The candidate block below covers the stdlib's declarative long tail
as it appears in Symfony-family code. **Add an entry only if its
callee appears in the corpus table** (an unmeasured entry is
speculation — YAGNI applies to signatures too); prune or extend by
the measurement:

```
function array_combine<TKey, TValue>(array<mixed, TKey> $keys, array<mixed, TValue> $values): array<TKey, TValue>
function array_fill_keys<TKey, TValue>(array<mixed, TKey> $keys, TValue $value): array<TKey, TValue>
function array_flip<TKey, TValue>(array<TKey, TValue> $array): array<TValue, TKey>
function array_merge<TValue>(array<mixed, TValue> ...$arrays): array<int|string, TValue>
function array_reverse<TKey, TValue>(array<TKey, TValue> $array): array<TKey, TValue>
function array_search<TKey, TValue>(TValue $needle, array<TKey, TValue> $haystack): TKey|false
function array_slice<TKey, TValue>(array<TKey, TValue> $array): array<TKey, TValue>
function array_unique<TKey, TValue>(array<TKey, TValue> $array): array<TKey, TValue>
function count(): int<0..>
function implode(): string
function in_array(): bool
function iterator_to_array<TKey, TValue>(iterable<TKey, TValue> $iterator): array<TKey, TValue>
function str_replace(): string|list<string>
function strlen(): int<0..>
```

Notes the executor must keep:

- Parameter-less entries (`count(): int<0..>`) refine the return
  only — legal, the parameters keep their base fold.
- `array_merge`'s integer-key renumbering makes `array<int|string,
  TValue>` the honest key; do not tighten it.
- `str_replace` answers `string` when the subject is a string and
  `list<string>` when it is an array — declaratively that is the
  union; if the corpus table shows it hot, move it to the provider
  in this task instead (subject-type dispatch is computation).
- Class entries: extend the `ArrayIterator` pattern to the stub
  iterators the corpus table names (candidates: `ArrayObject`,
  `SplStack`, `SplQueue`, `SplObjectStorage`, `Generator` needs
  nothing — its class arguments are native). Each gets templates,
  the `Iterator`/`IteratorAggregate` ancestor with its arguments,
  and the element-bearing methods (`current`, `key`,
  `getIterator` where applicable).

Then:

Run: `cargo xtask compile-stubs && cargo test -p celerrate_types every_embedded_refinement_text_lowers`
Expected: the blob recompiles; the totality test proves every new
text lowers (fix typos here, not in the field).

- [ ] **Step 3: Re-measure and record**

Run: `cargo xtask mixed-rate`
Expected: FAIL — the counters moved (downward for the curated
callees). Review the diff it prints: every change must be a
reduction in `<mixed>` columns or in the global mixed count; any
increase is a regression to investigate before blessing.

Run: `cargo xtask mixed-rate --bless && cargo xtask corpus && cargo xtask ground-truth`
Expected: baseline re-blessed; the corpus snapshot unchanged
(nothing in this plan renders diagnostics); ground-truth green
against its baseline — if inference precision improvements moved
ground-truth records, review them under plan 6's classification
rules and re-bless with classifications preserved.

**The decision-15 exit check**: in the re-blessed baseline, the ten
most-called callees (by `<total>`) each show `<mixed> == 0`, or the
exception is written into the closing debt ledger (task 12) with
its reason (for example: an argument that is itself `mixed` at
every call site — no signature can fix a caller's opacity).

- [ ] **Step 4: Revise the norm draft**

In `.claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md`:

1. Section 5 ("Open questions for curation") becomes "Answers from
   curation (plan 7)":
   - Per-version multi-signature form: **deferred** — refinements
     are version-agnostic (decision 6); the base stubs keep the
     deltas. Revisit only if a curated signature ever needs a
     version split.
   - `?T` inside unions: `?` binds tighter than `|` and `&`;
     `?A|B` is `(A|null)|B` (decision 13, pinned by the norm
     parser's tests).
   - Intersections in shape field types: allowed directly
     (`{handler: Countable&Traversable}`) — the field value is a
     full type expression.
   - Union-arity/depth-cap spelling: none — caps are lattice
     behavior, not notation.
   - Trust-rule cannot-prove syntax: none — trust is a judgment on
     the annotation's relation to the native type, not something an
     annotation declares about itself.
2. Add a "First consumer" note under section 1: the refinements
   overlay (`crates/celerrate_stubs/refinements.celerrate`), with
   the file's grammar summarized in three lines (function entries,
   class blocks, `of` bounds) and a pointer to
   `refinement_source.rs` as the authoritative parser.
3. Record the v0 grammar additions the parser ships beyond the
   draft's mapping table, so the draft and the shipped parser cannot
   diverge: the `array-key` keyword (sugar for `int|string`; an
   accepted tension with design rule 1's "no synonyms", kept because
   curated signatures read it everywhere), single-argument
   `array<V>` = `array<int|string, V>`, and single-argument
   `iterable<V>` = `iterable<mixed, V>` (iterable keys are
   unconstrained).
4. Amendment-history line at the top, dated, one sentence.

- [ ] **Step 5: Commit with the numbers**

```bash
git add crates/celerrate_stubs xtask/mixed-rate-baseline.txt .claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md
git commit -m "✨ feat(stubs): the curated refinement set drives the corpus residual down

Corpus mixed-rate before curation: expressions <N> mixed <M>;
after: expressions <N> mixed <M'>. Top-ten stub callees at zero
mixed calls: <yes / the listed exceptions and why>."
```

(Fill the real numbers from the two baselines — the before is in
git history from task 10.)

---

### Task 12: Closure — determinism, invalidation, and the debt ledger

**Files:**
- Modify: `crates/celerrate_types/tests/fixpoint.rs` (or the
  module tests, matching where the plan-6 determinism fixtures
  landed)
- Modify: `crates/celerrate_types/tests/invalidation_scope.rs`
- Modify: rustdoc seams for the debt ledger (listed in step 3)
- Test: this task is tests

**Interfaces:**
- Consumes: everything this plan built; `TestDatabase`'s
  `take_executed`/`executions_of` and the existing fixture idioms.
- Produces: pins only, plus the debt ledger recorded as rustdoc
  where each debt lives.

- [ ] **Step 1: Write the determinism pins**

In the fixpoint/determinism suite, with the real `StdlibProvider`
registered (the task-6 registration idiom):

```rust
const DETERMINISM_SOURCE: &str = r#"<?php
function consume(string $json, string $subject): void {
    $mapped = array_map(fn (int $n): string => (string) $n, [1, 2]);
    $decoded = json_decode($json, true);
    if (preg_match('/(?<year>\d+)/', $subject, $matches) === 1) {
        $inside = $matches;
    }
}
"#;

#[test]
fn provider_answers_are_identical_across_fresh_databases() {
    // Interner handles may differ across databases; displays must
    // not.
    let render = || {
        let f = fixture_with_embedded_stubs_and_stdlib_provider(&[DETERMINISM_SOURCE]);
        let inferred = inferred_body_types(
            &f.db, f.files, f.stubs, f.configuration,
            FileId::new(0), body_query(&f, 0),
            InferenceContext::new(&f.db, None),
        );
        inferred
            .expression_types
            .iter()
            .map(|of| of.display(&f.db))
            .collect::<Vec<String>>()
    };
    assert_eq!(render(), render());
}

#[test]
fn a_claim_never_reached_leaves_the_body_on_the_declared_tier() {
    // Every handler answers None for these argument shapes: the
    // declared tier's answer stands — the fall-through contract.
    let f = fixture_with_embedded_stubs_and_stdlib_provider(&[r#"<?php
function consume(mixed $anything): void {
    $mapped = array_map($anything, $anything);
    $slice = current($anything);
}
"#]);
    let inferred = inferred_body_types(
        &f.db, f.files, f.stubs, f.configuration,
        FileId::new(0), body_query(&f, 0),
        InferenceContext::new(&f.db, None),
    );
    assert_eq!(inferred.edge_counts.provider_edges, 0);
}
```

- [ ] **Step 2: Write the invalidation probes**

In `invalidation_scope.rs`, mirroring its established
edit-and-count shape:

```rust
#[test]
fn an_argument_literal_edit_reruns_only_the_editing_body() {
    // A provider answer is a pure function of the invocation, so an
    // argument-literal edit moves nothing but the editing body.
    let before = r#"<?php
function decoding(string $json) { return json_decode($json, true); }
function bystander(string $text) { return strlen($text); }
"#;
    let after = before.replace(", true)", ", false)");
    let mut f = fixture_with_embedded_stubs_and_stdlib_provider(&[before]);
    for index in [0, 1] {
        let _ = inferred_body_types(
            &f.db, f.files, f.stubs, f.configuration,
            FileId::new(0), body_query(&f, index),
            InferenceContext::new(&f.db, None),
        );
    }
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    for index in [0, 1] {
        let _ = inferred_body_types(
            &f.db, f.files, f.stubs, f.configuration,
            FileId::new(0), body_query(&f, index),
            InferenceContext::new(&f.db, None),
        );
    }
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        1,
        "only the editing body re-infers: {log:?}",
    );
}

#[test]
fn a_provider_answer_change_propagates_like_any_inferred_return() {
    // The flags edit changes the callee's inferred return; the
    // caller re-infers on demand — provider answers ride the
    // existing invalidation paths, no special casing.
    let before = r#"<?php
function decoding(string $json) { return json_decode($json, true); }
function caller(string $json) { return decoding($json); }
"#;
    let after = before.replace(", true)", ", false)");
    let mut f = fixture_with_embedded_stubs_and_stdlib_provider(&[before]);
    let first = inferred_function_return(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "caller".to_owned()),
    );
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    let second = inferred_function_return(
        &f.db, f.files, f.stubs, f.configuration,
        FunctionQuery::new(&f.db, "caller".to_owned()),
    );
    assert_ne!(
        first.display(&f.db),
        second.display(&f.db),
        "the array branch became the object branch through the caller",
    );
}
```

- [ ] **Step 3: The re-export audit and the debt ledger**

1. Surface audit: `celerrate_types` adds exactly `StubCallRecord`
   to its public surface; `norm` and its types stay `pub(crate)`;
   `celerrate_stubs` adds the five refinement types and the
   `StubIndex` accessors; `celerrate_plugin` re-exports are
   **unchanged** (the trait method travels with the trait).
   `cargo xtask dependency-shape` green.
2. Record the debts as rustdoc at their seams, one line each,
   naming the owner:
   - the by-reference channel is wired for free functions only;
     method-call symmetry waits for a method claimant
     (`flow.rs`, next to `apply_provider_by_reference`).
   - the pattern scanner is lexical: alternation-aware group
     optionality, the spurious group a conditional group
     (`(?(1)a|b)`) counts, and `PREG_OFFSET_CAPTURE` value shapes
     are conservative (`pattern_functions.rs` module doc).
   - the mixed-rate instrument records free-function stub calls
     only; stub method calls move the global counter but never the
     per-callee table (`flow.rs`, next to the recording arm;
     decision 14).
   - `array_map` with a callable-string callback (`'strtoupper'`)
     answers `None` (`array_functions.rs`).
   - norm v0 excludes conditional types; refinements are
     version-agnostic (`norm.rs` module doc; the draft records the
     same).
   - stub classes outside the curated set still contribute no
     generic ancestors (`inheritance.rs`, updating the task-5
     wording with the curated-list reality).
   - by-reference stdlib mutators (`array_shift`, `usort`, `sort`)
     stay on the declared write-back; curation revisits when the
     corpus table demands (`refinements.celerrate` header comment).
   - any decision-15 exceptions from task 11, verbatim.
3. Sweep the forward-pointing "(plan 7)" comments this plan
   fulfilled and reword each to describe shipped behavior:
   `crates/celerrate_cli/src/plugins.rs` (the rebuild note, closed
   in task 6), `xtask/src/dependency_shape.rs:6-7` (the const now
   lists both crates), the plan-6 ledger lines in
   `inheritance.rs` (task 5) and `declared.rs` if any remain, and
   `crates/celerrate_types/src/inference.rs`'s
   `InterproceduralEdgeCounts` doc if it still names this plan.

- [ ] **Step 4: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings && cargo fmt --all && cargo xtask dependency-shape && cargo xtask compile-stubs --check && cargo xtask corpus && cargo xtask ground-truth && cargo xtask mixed-rate`
Expected: everything green.

```bash
git add crates xtask
git commit -m "✅ test(types): provider determinism and invalidation pinned"
```

---

## Execution notes

- Tasks run strictly in order — the declared tier (4) needs the
  payload (2–3) and the lowering (1); the inheritance channel (5)
  needs 4's helpers; the provider tasks (6–9) are independent of
  4–5 but the instrument (10) and curation (11) need everything.
  Do not parallelize: 4, 5, 9, and 10 all touch `flow.rs`/
  `declared.rs` seams.
- **This plan builds on plan 6's shipped names** (`class_templates`,
  `ancestor_arguments`, `solver_pairs`/`solve`/`finalize_return`,
  the four call tiers, `ground_truth.rs`). Where a plan-6 name
  shipped slightly differently, follow the shipped code — the
  Interfaces blocks state the contracts, the repository states the
  spellings. Plan 6 must be merged before this plan starts.
- Display assertions: when an expected string disagrees with
  `display.rs`'s rendering or a constructor's canonicalization, fix
  the expectation, never the code (the plan-5 rule, reconducted).
- The corpus snapshot (`cargo xtask corpus`) must stay green
  throughout: nothing here emits a diagnostic; any snapshot change
  is a bug in the execution. The ground-truth baseline may move
  (inference gets more precise) — review and re-bless with
  classifications preserved, per plan 6's rules.
- `refinements.celerrate` edits always travel with a recompiled
  `stubs.bin` in the same commit (`cargo xtask compile-stubs`);
  CI's `compile-stubs --check` catches a forgotten recompile.
- The provider handlers construct types through `celerrate_plugin`'s
  re-exported `TypeId` only. If a handler needs an interrogation
  method the facade does not re-export, extend the facade's
  re-export list (it re-exports `TypeId` itself, so methods come
  with it — this note matters only if a needed helper type like
  `ShapeKey` is missing; add it to the existing `pub use
  celerrate_types::{...}` line).
- After the final task, do not extend the README or CHANGELOG: the
  preview's product surface is plan 9c's. The `mixed-rate`
  subcommand stays hidden and undocumented, like `ground-truth`.
