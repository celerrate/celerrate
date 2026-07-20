# Plugin-Seal Negative Proof Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A trybuild compile-fail suite proving the plugin-boundary seal from the facade side, and a derived (not hardcoded) plugin-crate set in `xtask dependency-shape` (issue #67), per `.claude/superpowers/specs/2026-07-19-plugin-seal-negative-proof-design.md`.

**Architecture:** On branch `fix-67-plugin-seal-negative-proof`: `trybuild` becomes `celerrate_plugin`'s first dev-dependency with five pinned compile-fail cases; `xtask/src/dependency_shape.rs` derives the governed set from `cargo metadata` (non-dev dependents of `celerrate_plugin`, minus a composition-root allowlist) with two sanity guards.

**Tech Stack:** Rust 1.94 (pinned by `rust-toolchain.toml`, which keeps `.stderr` files stable), trybuild 1.x (MIT OR Apache-2.0 — passes `deny.toml`), serde_json over `cargo metadata` (as today).

## Global Constraints

- Zero panic lints at deny; `unsafe_code` forbidden; test modules may locally `#[allow]`.
- TDD: for the trybuild suite, the "failing first" state is a case that COMPILES before the suite exists is meaningless — instead each case is verified to fail compilation for the pinned reason; for the xtask change, failing unit tests come first.
- Commits: gitmoji + Conventional Commits.
- No production-code change anywhere: corpus gates trivially zero-delta (still run per policy).

---

### Task 1: The trybuild compile-fail suite

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`: add `trybuild = "1"`)
- Modify: `crates/celerrate_plugin/Cargo.toml` (add `[dev-dependencies] trybuild.workspace = true`)
- Create: `crates/celerrate_plugin/tests/seal.rs`
- Create: `crates/celerrate_plugin/tests/seal/annotation_site_database.rs` (+ `.stderr`)
- Create: `crates/celerrate_plugin/tests/seal/annotation_site_new.rs` (+ `.stderr`)
- Create: `crates/celerrate_plugin/tests/seal/type_context_new.rs` (+ `.stderr`)
- Create: `crates/celerrate_plugin/tests/seal/invocation_site_new.rs` (+ `.stderr`)
- Create: `crates/celerrate_plugin/tests/seal/salsa_reexport.rs` (+ `.stderr`)

**Interfaces:**
- Consumes: the facade surface of `crates/celerrate_plugin/src/lib.rs` (re-exports `AnnotationSite`, `TypeContext`, `InvocationSite`; no `salsa`).
- Produces: a `cargo test -p celerrate_plugin` suite that fails the build of any future edit reopening the boundary.

- [ ] **Step 1: Wire trybuild**

Workspace `Cargo.toml`, `[workspace.dependencies]` (alphabetical order
as the section keeps): `trybuild = "1"`. Then in
`crates/celerrate_plugin/Cargo.toml`:

```toml
[dev-dependencies]
trybuild = { workspace = true }
```

- [ ] **Step 2: Write the harness**

`crates/celerrate_plugin/tests/seal.rs`:

```rust
//! The negative proof of the plugin-boundary seal (issues #61, #67):
//! from the facade side, the sealed constructors are not nameable, the
//! database is not reachable, and `salsa` is not re-exported. Each case
//! is one file whose compiler error is pinned; an edit that reopens the
//! boundary makes a case compile, which fails this suite.

#[test]
fn the_seal_holds() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/seal/*.rs");
}
```

- [ ] **Step 3: Write the five cases**

`tests/seal/annotation_site_database.rs`:

```rust
//! The database escape hatch is engine-internal: a facade consumer
//! cannot reach a salsa handle through an annotation site.
fn misuse<'db, 'site>(site: &celerrate_plugin::AnnotationSite<'db, 'site>) {
    let _ = site.database();
}

fn main() {}
```

`tests/seal/annotation_site_new.rs`:

```rust
//! Sites are constructed by the engine at dispatch, never by a plugin.
fn main() {
    let _ = celerrate_plugin::AnnotationSite::new;
}
```

`tests/seal/type_context_new.rs`:

```rust
//! The type facade is entered through `AnnotationSite::types()` or
//! `InvocationSite::types()`, never constructed directly.
fn main() {
    let _ = celerrate_plugin::TypeContext::new;
}
```

`tests/seal/invocation_site_new.rs`:

```rust
//! Invocation sites are constructed by the engine at dispatch.
fn main() {
    let _ = celerrate_plugin::InvocationSite::new;
}
```

`tests/seal/salsa_reexport.rs`:

```rust
//! The facade re-exports vocabulary, never the database crate.
use celerrate_plugin::salsa;

fn main() {
    let _ = salsa;
}
```

If a path in a case does not match the facade's actual re-export shape
(for example a generic parameter count), fix the case to reference the
real path — the requirement is that each case fails on exactly the
sealed item (a private-method/private-fn error for the first four,
an unresolved import for the fifth), never on an unrelated error like a
wrong lifetime arity.

- [ ] **Step 4: Generate and pin the expected errors**

```bash
TRYBUILD=overwrite cargo test -p celerrate_plugin --test seal
```

Then inspect each generated `.stderr`: the first four must carry E0624
(private associated function / method), the fifth E0432 (unresolved
import). If any case fails for a different reason, fix the case (not
the seal) and regenerate.

- [ ] **Step 5: Run the suite in pin mode**

Run: `cargo test -p celerrate_plugin --test seal`
Expected: PASS (all five cases fail compilation with the pinned
stderr).

- [ ] **Step 6: Prove the proof (throwaway)**

Temporarily flip `TypeContext::new` to `pub` in
`crates/celerrate_types/src/type_context.rs:22`, run the suite, observe
`type_context_new.rs` now compiles and the suite FAILS. Revert the
flip (`git checkout -- crates/celerrate_types`), re-run, PASS. This
step verifies the guard actually guards; nothing from it is committed.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/celerrate_plugin
git commit -m "✅ test(plugin): compile-fail proof of the boundary seal (#67)"
```

---

### Task 2: Derive the plugin-crate set in `dependency_shape`

**Files:**
- Modify: `xtask/src/dependency_shape.rs`

**Interfaces:**
- Consumes: the `cargo metadata --no-deps` JSON it already parses.
- Produces: `check(metadata)` with the same external contract (CI calls `cargo xtask dependency-shape` unchanged), governing a derived set.

- [ ] **Step 1: Write the failing tests**

In the existing test module (which feeds synthetic `serde_json`
metadata — mirror its fixture helpers):

```rust
#[test]
fn a_new_facade_dependent_is_governed_without_an_xtask_edit() {
    // A workspace member depending on celerrate_plugin AND another
    // workspace crate: today it would silently escape (not in the
    // hardcoded list); derived, it must fail the shape check.
    let metadata = metadata_with_packages(vec![
        plugin_package("celerrate_phpdoc_bridge", vec![normal("celerrate_plugin")]),
        plugin_package("celerrate_stdlib_provider", vec![normal("celerrate_plugin")]),
        plugin_package(
            "celerrate_future_provider",
            vec![normal("celerrate_plugin"), normal("celerrate_types")],
        ),
    ]);
    let error = check(&metadata).unwrap_err().to_string();
    assert!(error.contains("celerrate_future_provider"));
    assert!(error.contains("celerrate_types"));
}

#[test]
fn a_member_without_a_facade_dependency_is_not_governed() {
    let metadata = metadata_with_packages(vec![
        plugin_package("celerrate_phpdoc_bridge", vec![normal("celerrate_plugin")]),
        plugin_package("celerrate_stdlib_provider", vec![normal("celerrate_plugin")]),
        plugin_package("celerrate_syntax", vec![normal("celerrate_source")]),
    ]);
    assert!(check(&metadata).is_ok());
}

#[test]
fn the_composition_root_is_not_governed() {
    let metadata = metadata_with_packages(vec![
        plugin_package("celerrate_phpdoc_bridge", vec![normal("celerrate_plugin")]),
        plugin_package("celerrate_stdlib_provider", vec![normal("celerrate_plugin")]),
        plugin_package(
            "celerrate_cli",
            vec![normal("celerrate_plugin"), normal("celerrate_types")],
        ),
    ]);
    assert!(check(&metadata).is_ok());
}

#[test]
fn a_missing_known_plugin_crate_fails_the_sanity_guard() {
    let metadata = metadata_with_packages(vec![plugin_package(
        "celerrate_phpdoc_bridge",
        vec![normal("celerrate_plugin")],
    )]);
    let error = check(&metadata).unwrap_err().to_string();
    assert!(error.contains("celerrate_stdlib_provider"));
}
```

Reuse or extend the module's existing fixture builders for
`metadata_with_packages` / `plugin_package` / `normal` (dependency with
`"kind": null`); add a `dev(name)` builder (`"kind": "dev"`) where the
existing dev-exemption test needs porting.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p xtask dependency_shape`
Expected: the first and fourth tests FAIL against the hardcoded-list
implementation (the future provider escapes; the sanity message
differs).

- [ ] **Step 3: Implement the derivation**

Replace the constants and the selection logic:

```rust
/// The composition roots: they depend on the facade to register
/// implementations, and legitimately on everything else — the one
/// literal left, failing loud (a facade dependent that is not listed
/// here is governed as a plugin crate).
const COMPOSITION_ROOTS: &[&str] = &["celerrate_cli"];
/// The first-party plugin crates the derived set must always contain:
/// the sanity guard against renames and derivation bugs alike.
const KNOWN_PLUGIN_CRATES: &[&str] = &["celerrate_phpdoc_bridge", "celerrate_stdlib_provider"];
const ALLOWED_DEPENDENCY: &str = "celerrate_plugin";
const FORBIDDEN_EXTERNAL_DEPENDENCIES: &[&str] = &["salsa"];
```

In `check`, a first pass derives the governed set: every package with a
dependency named `celerrate_plugin` whose `kind` is not `"dev"`, and
whose own name is not in `COMPOSITION_ROOTS`. A second pass (the
existing loop body, reusing its two rules verbatim) governs exactly
that set. After derivation, the guards:

```rust
    for expected in KNOWN_PLUGIN_CRATES {
        if !governed.contains(*expected) {
            return Err(format!(
                "dependency shape: plugin crate {expected} was not derived from the workspace \
                 (renamed, or the derivation broke)"
            )
            .into());
        }
    }
```

(the non-empty guard is implied: the set contains the two known
crates or the error above fired). Update the module rustdoc: the set
is derived — a plugin crate is governed the moment it depends on the
facade, which is the one thing a plugin cannot avoid doing.

- [ ] **Step 4: Run the xtask suite and the real check**

Run: `cargo test -p xtask`
Expected: PASS (new tests and the ported existing ones).

Run: `cargo xtask dependency-shape`
Expected: exits 0 against the real workspace (derives exactly the two
known crates; `celerrate_cli` excluded as a root).

- [ ] **Step 5: Commit**

```bash
git add xtask/src/dependency_shape.rs
git commit -m "✨ feat(xtask): derive the governed plugin-crate set from the workspace (#67)"
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
cargo xtask dependency-shape
```

Expected: all clean (`cargo deny check` newly covers trybuild's
subtree).

- [ ] **Step 2: Corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: zero delta (no production code changed).

- [ ] **Step 3: Changelog and PR**

Unreleased entry: the plugin-boundary seal carries a compile-fail proof
and the dependency-shape check derives its governed set (#67).

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the seal negative proof and derived crate set (#67)"
git push -u origin fix-67-plugin-seal-negative-proof
gh pr create --title "✅ test(plugin, xtask): negative-proof the seal, derive the plugin-crate set (#67)" --body "Implements .claude/superpowers/specs/2026-07-19-plugin-seal-negative-proof-design.md: five pinned trybuild compile-fail cases prove the #61/#65 seal from the facade side, and dependency-shape derives its governed set from cargo metadata (non-dev facade dependents minus composition roots). Closes #67."
```
