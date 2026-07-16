# Issue #36 — Minimal Stub Surface by Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close issue #36: `celerrate_types`' shared test fixtures default to `StubIndex::from_symbols(vec![])`, so no test in the crate can observe stub-dependent behaviour by default — the recorded root of two of PR #35's three most interesting findings.

**Architecture:** One shared builder, `minimal_stub_index()`, lives in the crate's only shared test-support module (`inheritance/test_support.rs`) and carries a minimal, realistic PHP builtin surface (the iteration protocol chain, `ArrayObject`/`ArrayIterator`, `Exception`/`Throwable`, `stdClass`, `Countable`, `ArrayAccess`, `Stringable`). Every per-module `fixture()` in `src/` switches its empty index to this builder. Because a source declaration always beats a stub for the same folded key (`resolve_ancestor` checks `lookup_class_declaration` first — `celerrate_semantics/src/linearize.rs:853`), tests that define protocol interfaces in their own sources keep their answers; the migration risk is confined to tests that name a surface symbol *without* defining it, which previously resolved to nothing. Each module migrates as its own reviewed task with an explicit intent-verification protocol — a test that goes green for a new reason is the defect this issue exists to prevent.

**Tech Stack:** Rust, salsa fixtures, `celerrate_stubs` (`StubIndex::new`, `StubSymbol`, `StubClassSurface`, `StubMember`).

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`; test modules may locally `#[allow]` (every touched module already does).
- **TDD**: failing test → minimal implementation → refactor. Task 1's pin must be watched failing before `minimal_stub_index()` exists.
- **Intent re-verification, not re-running**: for every test whose sources mention a surface symbol, the migrating task answers *why* the test still passes, in writing (the triage table in each task). A silently repurposed test is a failure of this plan even when the suite is green.
- **Source beats stub**: never "fix" a triaged test by renaming its source-declared `Traversable`/`Iterator`/`Exception` interfaces — source declarations winning over stubs is the production semantic and exactly what the migration relies on.
- **Isolated-empty stays available**: a test whose documented intent *requires* an unresolvable name keeps an empty index through the module-local `fixture_with_empty_stubs` variant added on demand — never by shrinking the shared surface.
- **Determinism**: no wall clock, no randomness, no environment reads inside queries.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`.

## Fixed decisions

1. **The default carries stubs; the isolated variant is the exception.** Adjudicated by the human partner on 2026-07-16 against the alternative (documenting the isolated-variant pattern as the norm): the empty default is the recorded root cause of defects surviving two review passes, so the default is what gets fixed.
2. **The surface is minimal and grows on demand.** Exactly the symbols below, chosen because they are the shapes PR #35's findings needed and the commonest PHP builtins in test sources. No functions, no constants (YAGNI — no current test needs `strlen`; add it the day a test does).
3. **Scope is `src/` unit fixtures only.** The seven module fixtures (`inference`, `declared`, `judgments`, `substitution`, `inheritance`, `type_syntax`, `narrowing`). The `tests/` integration suites (`fixpoint.rs`, `invalidation_scope.rs`) keep their hand-rolled empty-index fixtures: they pin fixpoint budgets and salsa execution counts, where a stub surface adds resolution noise without observing any stub behaviour, and as a separate compilation unit they cannot reach `pub(crate)` support anyway. Recorded in the final task's documentation sweep.
4. **Specific-payload variants stay.** `fixture_with_stub_payload` (`declared.rs`), `fixture_with_stub_classes` (`judgments.rs`), and the hand-built index of `a_stub_ancestor_carries_a_genuine_protocol_implementor_transitively` (`inference.rs`) keep building their own indexes — their intent is a *particular* payload, not the default surface. Their doc comments that assert "the fixture's stub index is empty" are updated to name the new default.

### The surface

| Symbol | Kind | Surface (parents; members) |
|---|---|---|
| `Traversable` | Interface | — |
| `Iterator` | Interface | parents: `Traversable` |
| `IteratorAggregate` | Interface | parents: `Traversable` |
| `ArrayAccess` | Interface | — |
| `Countable` | Interface | — |
| `Stringable` | Interface | — |
| `Throwable` | Interface | — |
| `stdClass` | Class | — |
| `Exception` | Class | parents: `Throwable`; `getMessage(): string` |
| `ArrayObject` | Class | parents: `IteratorAggregate`, `ArrayAccess`, `Countable`; `getIterator(): ArrayIterator` |
| `ArrayIterator` | Class | parents: `Iterator`, `ArrayAccess`, `Countable`; `current(): mixed`, `key(): mixed`, `next(): void`, `rewind(): void`, `valid(): bool` |

`ArrayObject → IteratorAggregate` is deliberately a *surface parent*, not a direct edge any test source declares: it reproduces the transitive stub-frontier shape (`linearize.rs`'s walk into `stub_ancestors`) that hid PR #35's iteration-gate bug — the default fixture must be able to express that shape from now on.

## File structure

- Modify: `crates/celerrate_types/src/inheritance/test_support.rs` — add `minimal_stub_index()`; module doc gains the surface.
- Modify: `crates/celerrate_types/src/inference.rs` — fixture switch, pin test, triage, comment updates (~127 tests).
- Modify: `crates/celerrate_types/src/declared.rs` — fixture switch, triage (~46 tests).
- Modify: `crates/celerrate_types/src/judgments.rs`, `narrowing.rs`, `type_syntax.rs`, `inheritance.rs`, `substitution.rs` — fixture switches, triage (~53 tests).
- No production (non-test) code changes anywhere. No `Cargo.toml` changes.

---

### Task 1: The shared builder `minimal_stub_index()`

**Files:**
- Modify: `crates/celerrate_types/src/inheritance/test_support.rs`
- Test: same file (a `#[cfg(test)]` module colocated with the builder is not needed; the pin lives in `inference.rs`'s test module, the crate's established home for cross-cutting fixture pins)
- Modify (pin only): `crates/celerrate_types/src/inference.rs`

**Interfaces:**
- Produces: `pub(crate) fn minimal_stub_index() -> celerrate_stubs::StubIndex` in `crate::inheritance::test_support`. Later tasks call exactly this from each module's `fixture()`.
- Consumes: `celerrate_stubs::{StubAvailability, StubClassSurface, StubIndex, StubMember, StubMemberKind, StubSignature, StubSymbol, StubSymbolKind, StubVisibility, VersionedTypeText}` (the exact set `a_stub_ancestor_carries_a_genuine_protocol_implementor_transitively` already imports at `inference.rs:3426-3429`).

- [ ] **Step 1: Write the failing pin test**

In `inference.rs`'s test module, next to `a_stub_ancestor_carries_a_genuine_protocol_implementor_transitively` (which documents the hand-built idiom this pin reuses):

```rust
/// Issue #36's contract: the crate's default fixture surface can
/// express stub-dependent shapes. This pins the builder itself by
/// hand-building a `Fixture` around it (the module's own `fixture()`
/// switches to the builder in the same task); `getIterator()` through
/// a bare `extends \ArrayObject` discriminates — an empty index
/// answers `mixed`, the surface answers `arrayiterator`.
#[test]
fn the_minimal_stub_surface_expresses_the_transitive_protocol_shape() {
    let db = TestDatabase::default();
    let source = r#"<?php
class Users extends \ArrayObject {}
function caller(Users $users) { return $users->getIterator(); }
"#;
    let handles = vec![SourceFile::new(
        &db,
        FileId::new(0),
        source.as_bytes().to_vec(),
    )];
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(
        crate::inheritance::test_support::minimal_stub_index(),
    )
    .durability(salsa::Durability::HIGH)
    .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let f = Fixture {
        db,
        handles,
        files,
        stubs,
        configuration,
    };
    assert_eq!(caller_return_display(&f, "caller"), "arrayiterator");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --package celerrate_types the_minimal_stub_surface_expresses_the_transitive_protocol_shape`
Expected: compile error — `minimal_stub_index` not found in `test_support`. (A compile-fail RED is the correct failure here: the builder does not exist.)

- [ ] **Step 3: Implement the builder**

In `test_support.rs` (imports join the existing `use` block):

```rust
use celerrate_stubs::{
    StubAvailability, StubClassSurface, StubIndex, StubMember, StubMemberKind, StubSignature,
    StubSymbol, StubSymbolKind, StubVisibility, VersionedTypeText,
};

/// Issue #36: the minimal, realistic builtin surface every module
/// `fixture()` carries by default, so stub-dependent shapes (a
/// transitive protocol implementor, a stub member's declared return)
/// are expressible without a hand-built index. Minimal and
/// grow-on-demand: exactly the symbols PR #35's findings needed plus
/// the commonest builtins in test sources — no functions, no
/// constants, until a test demands one. A test whose documented
/// intent requires an unresolvable name uses its module's
/// `fixture_with_empty_stubs` variant instead; the surface never
/// shrinks to accommodate one test.
///
/// `ArrayObject → IteratorAggregate` is deliberately a surface
/// parent (the transitive stub frontier `linearize.rs` folds into
/// `stub_ancestors`), not an edge any test source declares: it is
/// the exact shape that hid PR #35's iteration-gate bug.
pub(crate) fn minimal_stub_index() -> StubIndex {
    fn class_like(name: &str, kind: StubSymbolKind) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability: StubAvailability::ALWAYS,
        }
    }
    fn method(name: &str, return_type: &str) -> StubMember {
        StubMember {
            kind: StubMemberKind::Method,
            name: name.to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            signature: Some(StubSignature {
                parameters: vec![],
                return_type: VersionedTypeText::from_text(Some(return_type.to_owned())),
                by_reference: false,
            }),
            type_text: VersionedTypeText::default(),
            value_text: None,
        }
    }
    fn parents(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }
    StubIndex::new(
        vec![
            class_like("ArrayAccess", StubSymbolKind::Interface),
            class_like("ArrayIterator", StubSymbolKind::Class),
            class_like("ArrayObject", StubSymbolKind::Class),
            class_like("Countable", StubSymbolKind::Interface),
            class_like("Exception", StubSymbolKind::Class),
            class_like("Iterator", StubSymbolKind::Interface),
            class_like("IteratorAggregate", StubSymbolKind::Interface),
            class_like("Stringable", StubSymbolKind::Interface),
            class_like("Throwable", StubSymbolKind::Interface),
            class_like("Traversable", StubSymbolKind::Interface),
            class_like("stdClass", StubSymbolKind::Class),
        ],
        vec![],
        vec![
            (
                "ArrayIterator".to_owned(),
                StubClassSurface {
                    parents: parents(&["Iterator", "ArrayAccess", "Countable"]),
                    members: vec![
                        method("current", "mixed"),
                        method("key", "mixed"),
                        method("next", "void"),
                        method("rewind", "void"),
                        method("valid", "bool"),
                    ],
                },
            ),
            (
                "ArrayObject".to_owned(),
                StubClassSurface {
                    parents: parents(&["IteratorAggregate", "ArrayAccess", "Countable"]),
                    members: vec![method("getIterator", "ArrayIterator")],
                },
            ),
            (
                "Exception".to_owned(),
                StubClassSurface {
                    parents: parents(&["Throwable"]),
                    members: vec![method("getMessage", "string")],
                },
            ),
            (
                "Iterator".to_owned(),
                StubClassSurface {
                    parents: parents(&["Traversable"]),
                    members: vec![],
                },
            ),
            (
                "IteratorAggregate".to_owned(),
                StubClassSurface {
                    parents: parents(&["Traversable"]),
                    members: vec![],
                },
            ),
        ],
    )
}
```

Extend the module doc's opening paragraph (the same one issue #40's fix already extended) with one sentence: "Issue #36 adds `minimal_stub_index()`, the default stub surface every module fixture carries."

- [ ] **Step 4: Run the pin to verify it passes**

Run: `cargo test --package celerrate_types the_minimal_stub_surface_expresses_the_transitive_protocol_shape`
Expected: PASS. Also run the full crate (`cargo test --package celerrate_types`): everything else still green — nothing consumes the builder yet.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/inheritance/test_support.rs crates/celerrate_types/src/inference.rs
git commit -m "✅ test(types): add the minimal default stub surface builder"
```

---

### Task 2: Migrate `inference.rs` (~127 tests)

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs` (`fixture()` at ~line 568; doc comments at ~3393-3414)

**Interfaces:**
- Consumes: `crate::inheritance::test_support::minimal_stub_index()` (Task 1).
- Produces: the migration + triage protocol below, which Tasks 3 and 4 repeat verbatim per module.

- [ ] **Step 1: Switch the fixture**

In `fixture()`, replace:

```rust
let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
```

with:

```rust
let stubs =
    StubIndexInput::builder(crate::inheritance::test_support::minimal_stub_index())
```

If the module's `StubIndex` import becomes unused, remove it.

- [ ] **Step 2: Run the module suite and record the delta**

Run: `cargo test --package celerrate_types --lib inference 2>&1 | tail -20`
Expected: some failures are *possible* — each one is a test whose sources name a surface symbol that previously resolved to nothing. List every failure verbatim before touching anything.

- [ ] **Step 3: Build the triage candidate list**

Candidates are every test in the module whose PHP source mentions a surface name, failing or green:

```bash
grep -n "ArrayObject\|ArrayIterator\|IteratorAggregate\|Traversable\|Iterator\b\|Countable\|ArrayAccess\|stdClass\|Exception\|Throwable\|Stringable\|getIterator" crates/celerrate_types/src/inference.rs
```

- [ ] **Step 4: Adjudicate every candidate with the intent table**

For each candidate test, record one row in the task's report (this table is the review artifact — a bare "suite green" is not acceptance):

| Test | Mentions | Source-declares it? | Verdict |
|---|---|---|---|

Verdict is exactly one of:
- **`source-wins`** — the test declares the symbol in its own source; the source declaration beats the stub (`resolve_ancestor` order), the assertion pins the same mechanism as before. No change.
- **`intent-preserved`** — the symbol now resolves through the stub and the answer is unchanged (or changed to a *more precise* answer the test's documented intent still covers). Update the expected value only if the test's own comment justifies the new answer; extend the comment to say why.
- **`needs-isolation`** — the test's documented intent requires the name to be unresolvable (for example, a test pinning conservative `mixed` for an unknown ancestor). Add the module-local variant once, and switch only these tests to it:

```rust
/// `fixture` with an empty stub index, for the tests whose documented
/// intent requires a name to resolve to nothing (issue #36 made the
/// default carry `minimal_stub_index()`); never the default again.
fn fixture_with_empty_stubs(sources: &[&str]) -> Fixture {
    // identical to `fixture` except:
    // StubIndexInput::builder(StubIndex::from_symbols(vec![]))
}
```

(Write the variant as a full copy of the module's `fixture` body with only the stubs line differing — each module's `Fixture` struct differs slightly, so there is no shared shortcut.)

- [ ] **Step 5: Update the stale comments**

`a_stub_ancestor_carries_a_genuine_protocol_implementor_transitively`'s doc comment says "`fixture`'s own stub index is empty, so this crate needs its own isolated stub index". Replace that sentence with: "`fixture`'s default surface (issue #36) already carries `ArrayObject`, but this test pins a *particular* transitive payload (`getIterator(): Cursor` against source-declared protocol interfaces), so it keeps building its own index by hand." Search the module for other occurrences of "stub index is empty" / "empty stub" and reconcile each the same way.

- [ ] **Step 6: Run the full crate suite**

Run: `cargo test --package celerrate_types`
Expected: PASS, with every candidate adjudicated in the table.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types/src/inference.rs
git commit -m "✅ test(types): inference fixtures carry the default stub surface"
```

(Include the triage table in the commit body.)

---

### Task 3: Migrate `declared.rs` (~46 tests)

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs` (`fixture()` at ~line 1359)

**Interfaces:**
- Consumes: `minimal_stub_index()` (Task 1) and Task 2's protocol.

- [ ] **Step 1: Switch the fixture** — in `declared.rs`'s `fixture()`, replace `StubIndexInput::builder(StubIndex::from_symbols(vec![]))` with `StubIndexInput::builder(crate::inheritance::test_support::minimal_stub_index())`; remove the `StubIndex` import if it becomes unused. `fixture_with_stub_payload` and `fixture_with_stub_payload_in_range` (lines ~2068, ~2081) keep their own indexes untouched (fixed decision 4); update any of their comments that assert the default is empty.
- [ ] **Step 2: Run the module suite and record the delta** — `cargo test --package celerrate_types --lib declared`. List every failure verbatim before touching anything.
- [ ] **Step 3: Build the candidate list** — every test whose PHP source mentions a surface name, failing or green:

```bash
grep -n "ArrayObject\|ArrayIterator\|IteratorAggregate\|Traversable\|Iterator\b\|Countable\|ArrayAccess\|stdClass\|Exception\|Throwable\|Stringable\|getIterator" crates/celerrate_types/src/declared.rs
```

- [ ] **Step 4: Adjudicate with the intent table** — one row per candidate (`| Test | Mentions | Source-declares it? | Verdict |`), verdict exactly one of `source-wins` (the test declares the symbol in its own source; source beats stub, no change), `intent-preserved` (the stub now resolves; the answer is unchanged or more precise and the test's own comment justifies it — extend the comment to say why), or `needs-isolation` (the documented intent requires the name unresolvable; add the module-local `fixture_with_empty_stubs` variant — a full copy of `fixture` with only the stubs line reading `StubIndex::from_symbols(vec![])` — and switch only these tests to it).
- [ ] **Step 5: Run the full crate suite** — `cargo test --package celerrate_types`, PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/declared.rs
git commit -m "✅ test(types): declared fixtures carry the default stub surface"
```

---

### Task 4: Migrate the five small modules (~53 tests)

**Files:**
- Modify: `crates/celerrate_types/src/judgments.rs` (fixture ~line 814), `narrowing.rs` (~382), `type_syntax.rs` (~275), `inheritance.rs` (~227), `substitution.rs` (~289 — note its `fixture()` takes no sources)

**Interfaces:**
- Consumes: `minimal_stub_index()` (Task 1) and Task 2's protocol.

- [ ] **Step 1: Switch all five fixtures** — in each module's `fixture()`, replace `StubIndexInput::builder(StubIndex::from_symbols(vec![]))` with `StubIndexInput::builder(crate::inheritance::test_support::minimal_stub_index())`; remove any `StubIndex` import that becomes unused. `judgments.rs`'s `fixture_with_stub_classes` (~line 845) keeps its own index (fixed decision 4); reconcile its comments.
- [ ] **Step 2: Run each module suite and record deltas** — `cargo test --package celerrate_types --lib judgments` (then `narrowing`, `type_syntax`, `inheritance`, `substitution`). List every failure verbatim before touching anything.
- [ ] **Step 3: Build the candidate list per module** — every test whose PHP source mentions a surface name, failing or green:

```bash
grep -n "ArrayObject\|ArrayIterator\|IteratorAggregate\|Traversable\|Iterator\b\|Countable\|ArrayAccess\|stdClass\|Exception\|Throwable\|Stringable\|getIterator" crates/celerrate_types/src/judgments.rs crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/type_syntax.rs crates/celerrate_types/src/inheritance.rs crates/celerrate_types/src/substitution.rs
```

- [ ] **Step 4: Adjudicate with one intent table per module** — one row per candidate (`| Test | Mentions | Source-declares it? | Verdict |`), verdict exactly one of `source-wins` (the test declares the symbol in its own source; source beats stub, no change), `intent-preserved` (the stub now resolves; the answer is unchanged or more precise and the test's own comment justifies it — extend the comment to say why), or `needs-isolation` (the documented intent requires the name unresolvable; add the module-local `fixture_with_empty_stubs` variant — a full copy of that module's `fixture` with only the stubs line reading `StubIndex::from_symbols(vec![])` — and switch only these tests to it).
- [ ] **Step 5: Run the full crate suite** — `cargo test --package celerrate_types`, PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/judgments.rs crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/type_syntax.rs crates/celerrate_types/src/inheritance.rs crates/celerrate_types/src/substitution.rs
git commit -m "✅ test(types): remaining module fixtures carry the default stub surface"
```

---

### Task 5: Documentation sweep, full gate, close #36

**Files:**
- Modify: `crates/celerrate_types/tests/fixpoint.rs`, `crates/celerrate_types/tests/invalidation_scope.rs` (comments only)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Record the integration-suite scope decision**

Above each `tests/`-dir fixture's `StubIndex::from_symbols(vec![])` line, add:

```rust
// Deliberately empty (issue #36's fixed decision 3): this suite pins
// fixpoint budgets / salsa execution counts, where a stub surface adds
// resolution noise without observing stub behaviour, and a separate
// compilation unit cannot reach `pub(crate)` test support anyway.
```

(Adjust "fixpoint budgets / salsa execution counts" to the suite it sits in.)

- [ ] **Step 2: Sweep for stale claims**

```bash
grep -rn "empty stub\|stub index is empty\|from_symbols(vec!\[\])" crates/celerrate_types
```

Every remaining hit must be either a `fixture_with_empty_stubs` variant, a `tests/`-dir fixture with the Step 1 comment, or a specific-payload builder — nothing may still claim the *default* is empty.

- [ ] **Step 3: Full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo xtask dependency-shape`
Expected: all green; zero `Cargo.toml` diffs; zero production (non-test) code diffs (`git diff main --stat` shows only test modules, test_support, and comments).

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_types/tests/fixpoint.rs crates/celerrate_types/tests/invalidation_scope.rs
git commit -m "📝 docs(types): record the integration suites' deliberate empty stubs"
```

- [ ] **Step 5: Push and open the PR**

PR body: the surface table, the aggregated intent tables (source-wins / intent-preserved / needs-isolation counts per module), and `Closes #36`. The PR is the "task with its own review" the issue's deferral demanded.
