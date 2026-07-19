# Inference-Warming Enforcement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the warm-before-demand precondition of `inferred_body_types_unguarded` structural instead of prose-guarded (issue #63), per `.claude/superpowers/specs/2026-07-19-inference-warming-enforcement-design.md`.

**Architecture:** Two moves on branch `fix-63-inference-warming-enforcement`: (1) the public `inferred_body_types` wrapper drops its `context` parameter and constructs the `None` context itself, making the unwarmed `Some`-context external call unrepresentable; (2) the unguarded tracked query moves into a sealed nested module whose single entry takes a zero-sized `Warmed` proof token. Production call sites of the unguarded query are exactly three (the public wrapper at `inference.rs:393` and the two recovery-carrying return queries at `inference.rs:681` and `inference.rs:837`); everything at line 844+ is the test module. **No `cycle_fn` on the body query** — plan #51's fixed decision 1 rejected that route and this plan honors the rejection.

**Tech Stack:** Rust 1.94, salsa 0.27 (tracked queries, cycle recovery — configuration untouched).

## Global Constraints

- Zero panic lints at deny; `unsafe_code` forbidden; test modules may locally `#[allow]`.
- TDD; API-narrowing tasks are compiler-driven (the failing state is the build).
- Zero behavioral change: every analysis result byte-identical; corpus gates must show zero delta.
- Commits: gitmoji + Conventional Commits.
- Determinism rules of the engine unchanged (no new queries, no removed recovery).

---

### Task 1: The public wrapper stops accepting a context

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs:372-394` (the public wrapper and its rustdoc)
- Modify (mechanical, drop the argument): `crates/celerrate_types/src/checks/mod.rs:281,364`; `crates/celerrate_types/src/checks/test_support.rs:226`; `crates/celerrate_cli/src/mixed_rate.rs:115`; `crates/celerrate_cli/src/cache/mod.rs:363,404`; `crates/celerrate_types/tests/by_reference.rs:123`; `crates/celerrate_types/tests/fixpoint.rs:659,696`; `crates/celerrate_types/tests/invalidation_scope.rs:2070,2084`; `crates/celerrate_stdlib_provider/tests/end_to_end.rs:79`

**Interfaces:**
- Consumes: existing `warm_the_cycle_safe_entry_point` and `inferred_body_types_unguarded`.
- Produces: `pub fn inferred_body_types<'db>(db, files, stubs, configuration, file, body) -> &'db Option<InferredBody<'db>>` — six parameters, no `context`. Task 2 rewrites its body again; the signature fixed here is final.

- [ ] **Step 1: Narrow the signature**

Replace the wrapper (currently `inference.rs:383-394`) with:

```rust
/// The inference of one body: `None` when the identity carries no body
/// in `file`. This public name is a cycle-safe wrapper over the
/// module-private tracked query (issue #51): it first warms the owner's
/// recovery-carrying return query, completing any fixpoint the body
/// participates in, then demands the raw query — which either hits its
/// memo or recomputes with every recursive edge answered from the
/// completed return memo. Either way, salsa's `Panic` strategy is
/// never reachable from here.
///
/// The wrapper owns its `InferenceContext` (always the `None` shape):
/// a trait-body (`Some`) context is an engine-internal currency of the
/// return queries, and exposing it here would reopen the unwarmed
/// entry issue #63 closed. A future external trait-body consumer adds
/// a deliberate new entry point — and its symmetric warming with it.
pub fn inferred_body_types<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> &'db Option<InferredBody<'db>> {
    warm_the_cycle_safe_entry_point(db, files, stubs, configuration, file, body);
    inferred_body_types_unguarded(
        db,
        files,
        stubs,
        configuration,
        file,
        body,
        InferenceContext::new(db, None),
    )
}
```

- [ ] **Step 2: Observe the compile failures**

Run: `cargo check --workspace --all-targets`
Expected: FAIL at every call site listed under Files (one extra argument).

- [ ] **Step 3: Drop the argument at every call site**

Each listed site currently passes `InferenceContext::new(db, None)` (or a
local binding of it) as the last argument; delete that argument. Where
the site's only use of `InferenceContext` was this argument, remove the
now-unused import/binding (the `checks/mod.rs:277-280` comment block
explaining the `None` choice moves onto nothing — delete it; the wrapper
rustdoc now owns that explanation).

- [ ] **Step 4: Run the affected suites**

Run: `cargo test -p celerrate_types -p celerrate_cli -p celerrate_stdlib_provider`
Expected: PASS — behavior identical, every existing test green.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types crates/celerrate_cli crates/celerrate_stdlib_provider
git commit -m "♻️ refactor(types): the public inference entry owns its None context (#63)"
```

---

### Task 2: Seal the unguarded query behind a proof token

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs` — the unguarded query (`196-320` region), the warming function (`326-370`), the wrapper body (Task 1's), the two return-query call sites (`681`, `837`), and the test module's direct unguarded calls (all at line 844+)

**Interfaces:**
- Consumes: Task 1's wrapper shape.
- Produces, inside `inference.rs` only (nothing crosses the crate boundary):
  - `struct Warmed(());` — zero-sized proof, private field.
  - `warm_the_cycle_safe_entry_point(...) -> Warmed` (same six parameters as today, now returns the proof; the no-op ownerless path returns it too — no cycle exists there, per issue #51's analysis, so the proof is honest).
  - `Warmed::from_inside_the_fixpoint() -> Warmed` — documented as legal in exactly two places: the bodies of `inferred_function_return` and `inferred_method_return`, which ARE the recovery-carrying heads.
  - `sealed::demand(proof: Warmed, db, files, stubs, configuration, file, body, context) -> &Option<InferredBody>` — the only path to the tracked query.

- [ ] **Step 1: Write the sealed module**

Move the tracked query into a nested module at its current location; the
query body itself is unchanged (cut and paste, indented):

```rust
/// The warm-before-demand proof (issue #63). Minted by
/// [`warm_the_cycle_safe_entry_point`] — the ordinary route — or by
/// [`Warmed::from_inside_the_fixpoint`], legal in exactly two places:
/// the two recovery-carrying return queries, which are themselves the
/// cycle-safe heads the warming completes. The private field keeps the
/// value unconstructible by pattern or literal.
pub(self) struct Warmed(());

impl Warmed {
    /// See the type-level contract: only `inferred_function_return`
    /// and `inferred_method_return` may call this. Anywhere else, call
    /// `warm_the_cycle_safe_entry_point` and use its returned proof.
    fn from_inside_the_fixpoint() -> Self {
        Self(())
    }
}

/// The seal (issue #63): the tracked query lives here so that no code
/// outside this module can name it; the only export demands the
/// [`Warmed`] proof. "Call without warming" is thereby a compile
/// error, not a rustdoc plea. The proof rides on this plain wrapper
/// rather than the tracked signature because tracked-query arguments
/// must be salsa structs.
mod sealed {
    use super::*;

    pub(super) fn demand<'db>(
        _proof: Warmed,
        db: &'db dyn salsa::Database,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
        file: SourceFile,
        body: BodyQuery<'db>,
        context: InferenceContext<'db>,
    ) -> &'db Option<InferredBody<'db>> {
        inferred_body_types_unguarded(db, files, stubs, configuration, file, body, context)
    }

    #[salsa::tracked(returns(ref))]
    #[allow(clippy::too_many_arguments)]
    fn inferred_body_types_unguarded<'db>(/* existing signature, unchanged */)
        -> Option<InferredBody<'db>> {
        // existing body, unchanged
    }
}
```

Adjust the moved body's paths if it referenced siblings via `self::` (it
uses plain names; `use super::*` covers them). Update the existing
rustdoc of the unguarded query: the "module-private on purpose" note
becomes "sealed on purpose", and the enumeration of legal callers moves
to `Warmed`'s rustdoc.

- [ ] **Step 2: Thread the proof through the three production callers**

`warm_the_cycle_safe_entry_point` (currently `-> ()`, `inference.rs:326`)
returns `Warmed`: add `Warmed(())` as the final expression of every arm
(the match at `inference.rs:304-369` currently ends each arm with `()`).
The wrapper body becomes:

```rust
    let proof = warm_the_cycle_safe_entry_point(db, files, stubs, configuration, file, body);
    sealed::demand(
        proof,
        db,
        files,
        stubs,
        configuration,
        file,
        body,
        InferenceContext::new(db, None),
    )
```

The two return-query call sites (`inference.rs:681` and `:837`) become
`sealed::demand(Warmed::from_inside_the_fixpoint(), ...)` with their
existing argument lists otherwise unchanged.

- [ ] **Step 3: Observe the compile failures in the test module**

Run: `cargo check -p celerrate_types --all-targets`
Expected: FAIL at every direct test-module call of the formerly nameable
query (all at `inference.rs:844+`).

- [ ] **Step 4: Migrate the test callers**

Rule, per call site: a test passing `InferenceContext::new(db, None)`
switches to the public wrapper `inferred_body_types` (dropping the
context argument); a test deliberately exercising a `Some` context or
the raw entry mints `Warmed::from_inside_the_fixpoint()` with a
one-line comment naming why the fixture is cycle-free (test modules are
inside `inference.rs`, so the constructor is reachable there — the seal
constrains production paths, tests document their exemption).

- [ ] **Step 5: Run the suites**

Run: `cargo test -p celerrate_types`
Then: `cargo test --workspace`
Expected: PASS everywhere; the #51 regression suite (fixpoint tests,
`cache_seeding.rs` in `celerrate_cli`) green and unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types
git commit -m "🔒 fix(types): the unguarded inference query demands a warming proof (#63)"
```

---

### Task 3: Verification and PR

**Files:** `CHANGELOG.md`.

- [ ] **Step 1: Full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

Expected: all clean.

- [ ] **Step 2: Corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: zero delta against the committed snapshot and baseline (pure
refactor). Any delta is a bug in this branch; stop and investigate.

- [ ] **Step 3: Changelog and PR**

Add the Unreleased entry (internal hardening: the inference warming
precondition is now compile-checked, #63), then:

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the inference-warming enforcement (#63)"
git push -u origin fix-63-inference-warming-enforcement
gh pr create --title "🔒 fix(types): make the inference-warming precondition structural (#63)" --body "Implements .claude/superpowers/specs/2026-07-19-inference-warming-enforcement-design.md: the public entry owns its None context, and the unguarded query is sealed behind a Warmed proof token. Honors plan #51's recorded rejection of a cycle_fn on the body query. Closes #63."
```
