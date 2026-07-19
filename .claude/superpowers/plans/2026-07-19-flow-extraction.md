# Flow Module Extraction Implementation Plan (issue #39)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `crates/celerrate_types/src/flow.rs` (~3,911 lines) into a `flow/` directory of cohesive submodules, with a guard test pinning the `member_boundary_type` substitution funnel — a pure refactor, no behaviour change.

**Architecture:** The `Walker` struct, `Environment`, `FlowContext`, `walk_body`, and the context helpers stay in `flow/mod.rs`; each cohesive method cluster becomes an `impl Walker` block in its own submodule (`walk`, `boundary`, `calls`, `instantiation`, `iteration`, `branching`, `callables`, `assignment`). Cross-submodule method calls get `pub(super)` visibility, driven by the compiler. A new integration test enumerates every `substitution::substitute` call site in the crate against an explicit allowlist.

**Tech Stack:** Rust, Cargo workspace, salsa. No new dependencies.

**Spec:** `.claude/superpowers/specs/2026-07-19-flow-extraction-design.md`

## Global Constraints

- **Pure refactor**: no behaviour change, no new or changed diagnostics, no inference-result change. Every move is a translation verifiable with `git diff --color-moved`.
- **Public surface frozen**: `crate::flow` keeps exporting `walk_body`, `FlowContext`, and `resolved_function_key` (the free-function form) identically. `crates/celerrate_types/src/lib.rs` does not change. `inference.rs` and `checks/arguments.rs` keep their imports untouched.
- **Zero panic, mechanically enforced**: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide. Test files may open with `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` (see `crates/celerrate_types/tests/fixpoint.rs` for the precedent).
- **Visibility ceiling**: extracted methods become `pub(super)` only when another `flow/` submodule calls them; nothing becomes `pub(crate)` that was not already. Let the compiler drive: after each move, `cargo check -p celerrate_types` reports exactly which private methods are called cross-module.
- **Commits**: gitmoji + Conventional Commits, one commit per task, repository-configured identity, no Claude attribution.
- **Gates per task**: `cargo check -p celerrate_types`, `cargo test -p celerrate_types`, `cargo fmt --all`. Full workspace gates run in the final task.
- **Line anchors below are from the pre-refactor `flow.rs`** (3,911 lines, commit `6e63adb`). They shift as tasks complete — always locate code by method name, use anchors only for orientation.

---

### Task 1: Move `flow.rs` to `flow/mod.rs` and record the baseline

**Files:**
- Move: `crates/celerrate_types/src/flow.rs` → `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Produces: the `flow/` directory every later task adds submodules to. Module resolution is unchanged: `lib.rs`'s `mod flow;` finds `flow/mod.rs`.

- [ ] **Step 1: Verify the baseline is green**

Run: `cargo test -p celerrate_types && cargo clippy -p celerrate_types --all-targets -- -D warnings`
Expected: all tests pass, clippy clean. If not, stop — the refactor starts from green.

- [ ] **Step 2: Move the file**

```bash
mkdir crates/celerrate_types/src/flow
git mv crates/celerrate_types/src/flow.rs crates/celerrate_types/src/flow/mod.rs
```

- [ ] **Step 3: Verify nothing changed**

Run: `cargo test -p celerrate_types`
Expected: PASS, identical to Step 1.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "♻️ refactor(types): move flow.rs to flow/mod.rs (#39)"
```

---

### Task 2: Extract `flow/iteration.rs` — the iteration protocol

**Files:**
- Create: `crates/celerrate_types/src/flow/iteration.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Consumes: `Walker` (defined in `flow/mod.rs`; its private fields are visible to submodules because field visibility extends to descendants of the defining module).
- Produces: `Walker::iteration_types` as `pub(super)` (called from the `foreach` arm in the future `flow/walk.rs`; until Task 9 the caller sits in `mod.rs`, which also requires `pub(super)`).

- [ ] **Step 1: Create the submodule with the moved block**

Create `crates/celerrate_types/src/flow/iteration.rs`:

```rust
//! The iteration protocol (design section 6): what `foreach` yields —
//! array shapes and lists by their parts, `iterable<K, V>` and
//! `Traversable` by their arguments, `Iterator`/`IteratorAggregate`
//! implementors by their `current()`/`key()` signatures.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}
```

Then cut the three methods from `flow/mod.rs` — `iteration_types` (anchor 1568), `implements_iteration_protocol` (anchor 1656), `class_iteration_types` (anchor 1708), i.e. the contiguous block from `fn iteration_types` up to (excluding) `fn statements` (anchor 1774) — and paste them verbatim inside the `impl` block. Do not reword anything inside the methods.

Note on imports: `use super::*;` re-imports everything `mod.rs` already imports, which keeps a mechanical move mechanical. If clippy flags unused imports through it, switch to explicit `use` lines copied from `mod.rs` and prune to what the compiler asks for.

- [ ] **Step 2: Declare the submodule**

In `flow/mod.rs`, after the existing `use` block, add:

```rust
mod iteration;
```

(Later tasks each add theirs; keep the list alphabetical: `assignment`, `boundary`, `branching`, `callables`, `calls`, `instantiation`, `iteration`, `walk`.)

- [ ] **Step 3: Let the compiler set visibility**

Run: `cargo check -p celerrate_types`
Expected: errors of the form `method 'iteration_types' is private`. For each, mark the method `pub(super)` in `iteration.rs`. Expected outcome: `iteration_types` becomes `pub(super)`; `implements_iteration_protocol` and `class_iteration_types` stay private (only `iteration_types` calls them). Re-run until clean, fixing any unused-import warnings in `mod.rs`.

- [ ] **Step 4: Verify the move is pure and green**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS.

Run: `git add -A && git diff --cached --color-moved=dimmed-zebra -- crates/celerrate_types/src/flow/`
Expected: every method body shows as moved (dimmed), the only non-moved lines are the module rustdoc, `use super::*;`, the `impl` wrapper, `mod iteration;`, and `pub(super)` markers.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract the iteration protocol into flow/iteration.rs (#39)"
```

---

### Task 3: Extract `flow/instantiation.rs` — class-generic delivery

**Files:**
- Create: `crates/celerrate_types/src/flow/instantiation.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Produces: `Walker::constructor_solved_class`, `Walker::bind_inline_variables` as `pub(super)` (callers live in the `new` arm and body-entry seeding that end up in `flow/walk.rs`); `inline_variables` likely stays private (called by `bind_inline_variables`).

- [ ] **Step 1: Create the submodule with the moved block**

Create `crates/celerrate_types/src/flow/instantiation.rs`:

```rust
//! Class-generic delivery: `new C(...)` solving class templates from
//! constructor arguments (`constructor_solved_class`), and inline
//! `@var` docblocks binding declared types into the environment.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}
```

Cut from `flow/mod.rs` the contiguous block `constructor_solved_class` (anchor 1000), `inline_variables` (anchor 1072), `bind_inline_variables` (anchor 1101) — from `fn constructor_solved_class` up to (excluding) `fn apply_call_assertions` (anchor 1122) — and paste it verbatim inside the `impl` block.

- [ ] **Step 2: Declare `mod instantiation;` in `flow/mod.rs`** (alphabetical order).

- [ ] **Step 3: Compiler-driven visibility**

Run: `cargo check -p celerrate_types`
Mark `pub(super)` exactly the methods the errors name. Re-run until clean.

- [ ] **Step 4: Verify**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS.
Run: `git add -A && git diff --cached --color-moved=dimmed-zebra -- crates/celerrate_types/src/flow/`
Expected: pure translation, as in Task 2.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract class-generic delivery into flow/instantiation.rs (#39)"
```

---

### Task 4: Extract `flow/boundary.rs` — the member boundary cluster

**Files:**
- Create: `crates/celerrate_types/src/flow/boundary.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Produces: `Walker::member_boundary_type` (THE funnel), `member_owner`, `method_signatures`, `scope_keyword_class`, `parent_class_key`, `parent_class_key_of` as `pub(super)` where the compiler demands; `member_value_type` and `declared_present` stay private if only the funnel calls them.

- [ ] **Step 1: Create the submodule with the funnel rustdoc**

Create `crates/celerrate_types/src/flow/boundary.rs` with this module rustdoc — it is the prose half of the guard (Task 10 is the mechanical half):

```rust
//! The member boundary: `member_boundary_type` is the ONE funnel
//! every member read, method call, callable projection, and `new`
//! result passes through (issue #39). It is where a member's declared
//! type crosses from its defining class into the receiver's context:
//! placeholders are substituted there, once, against the receiver's
//! solved arguments. Calling `crate::substitution` from anywhere else
//! in `flow/` needs a justification against this invariant and a
//! deliberate entry in the allowlist of
//! `tests/substitution_funnel_guard.rs`.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}
```

Cut from `flow/mod.rs` the contiguous block from `fn member_value_type` (anchor 525) up to (excluding) `fn scoped_subject` (anchor 759): `member_value_type`, `method_signatures`, `declared_present`, `member_boundary_type` (anchor 593), `member_owner`, `scope_keyword_class`, `parent_class_key`, `parent_class_key_of`. Paste verbatim inside the `impl` block. (`scoped_subject` stays in `mod.rs` — it is a context helper, per the spec.)

- [ ] **Step 2: Declare `mod boundary;` in `flow/mod.rs`** (alphabetical order).

- [ ] **Step 3: Compiler-driven visibility**

Run: `cargo check -p celerrate_types`
Mark `pub(super)` exactly what the errors name. Re-run until clean.

- [ ] **Step 4: Verify**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS.
Run: `git add -A && git diff --cached --color-moved=dimmed-zebra -- crates/celerrate_types/src/flow/`
Expected: pure translation plus the new module rustdoc.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract the member boundary into flow/boundary.rs (#39)"
```

---

### Task 5: Extract `flow/calls.rs` — the call machinery

**Files:**
- Create: `crates/celerrate_types/src/flow/calls.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Consumes: `Walker::member_boundary_type` etc. from `flow/boundary.rs` (already `pub(super)`).
- Produces: the free function `resolved_function_key` re-exported from `mod.rs` so `checks/arguments.rs`'s `use crate::flow::resolved_function_key;` keeps working unchanged; call-result methods (`function_call_result`, `method_call_result_*`, `solved_call_result`, `typed_arguments`, `apply_call_assertions`, ...) as `pub(super)` where the walk calls them.

- [ ] **Step 1: Create the submodule with the two moved blocks**

Create `crates/celerrate_types/src/flow/calls.rs`:

```rust
//! The call boundary: argument typing, by-reference effects, template
//! solving at the call site, provider consultation, and call-result
//! computation for free functions and methods. `solved_call_result`'s
//! direct substitution call is one of the two deliberate sites in
//! `flow/` (the other is the `flow/boundary.rs` funnel) — see
//! `tests/substitution_funnel_guard.rs`.

use super::*;

// moved free function `resolved_function_key` goes here

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}
```

Cut from `flow/mod.rs`, in three pieces, pasting verbatim:

1. The **free function** `pub(crate) fn resolved_function_key(...)` (anchor 313, between `walk_body` and `struct PendingAssertion` — it is top-level, not a `Walker` method). Paste it above the `impl` block. It keeps `pub(crate)`.
2. The contiguous method block from `fn typed_arguments` (anchor 799) up to (excluding) `fn constructor_solved_class` (anchor 1000): `typed_arguments`, `kill_property_bindings`, `apply_by_reference`, `solver_pairs`, `solved_call_result`.
3. The contiguous method block from `fn apply_call_assertions` (anchor 1122) up to (excluding) `fn iteration_types`' former position — after Tasks 2–3 this block now ends at (excluding) `fn statements`: `apply_call_assertions`, `resolved_function_key` (the private method wrapper, anchor 1203), `provider_return`, `provider_by_reference`, `apply_provider_by_reference`, `function_call_result`, `method_call_result_for_keys_with_provider`, `method_call_result_with_provider`, `method_call_result_for_keys`.

- [ ] **Step 2: Declare and re-export in `flow/mod.rs`**

```rust
mod calls;

pub(crate) use calls::resolved_function_key;
```

(`mod calls;` in alphabetical order; the `pub(crate) use` sits right after the `mod` declarations.)

- [ ] **Step 3: Compiler-driven visibility**

Run: `cargo check -p celerrate_types`
Mark `pub(super)` exactly what the errors name. Verify `checks/arguments.rs` compiles without modification — if it errors, the re-export in Step 2 is wrong; fix the re-export, never the consumer.

- [ ] **Step 4: Verify**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS.
Run: `git add -A && git diff --cached --color-moved=dimmed-zebra -- crates/celerrate_types/src/flow/`
Expected: pure translation.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract the call machinery into flow/calls.rs (#39)"
```

---

### Task 6: Extract `flow/branching.rs` — condition splitting and narrowing glue

**Files:**
- Create: `crates/celerrate_types/src/flow/branching.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Produces: `Walker::branch_environments` as `pub(super)` (the walk's `if`/`while`/ternary arms call it); the leaf helpers (`split_on_subject`, `narrowed_to`, `removed_type`, `instanceof_target`, `type_check_facts`) stay private unless the compiler says otherwise.

- [ ] **Step 1: Create the submodule with the moved block**

Create `crates/celerrate_types/src/flow/branching.rs`:

```rust
//! Condition splitting: one condition expression in, a
//! (true-environment, false-environment) pair out — instanceof,
//! null checks, truthiness, comparisons against literals, type-check
//! functions, and the call-assertion facts drained from
//! `pending_condition_facts`.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}
```

Cut from `flow/mod.rs` the contiguous block from `fn branch_environments` (anchor 3001) up to (excluding) `fn nested_returns` (anchor 3318): `branch_environments`, `split_on_subject`, `narrowed_to`, `removed_type`, `instanceof_target`, `type_check_facts`. Paste verbatim.

- [ ] **Step 2: Declare `mod branching;` in `flow/mod.rs`** (alphabetical order).

- [ ] **Step 3: Compiler-driven visibility**

Run: `cargo check -p celerrate_types` — mark `pub(super)` as demanded, re-run until clean.

- [ ] **Step 4: Verify**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS. Then the `--color-moved` check as in prior tasks.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract condition splitting into flow/branching.rs (#39)"
```

---

### Task 7: Extract `flow/callables.rs` — closures and callable projection

**Files:**
- Create: `crates/celerrate_types/src/flow/callables.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Produces: `Walker::closure_type`, `projected_callable`, and siblings as `pub(super)` where the walk calls them.

- [ ] **Step 1: Create the submodule with the moved block**

Create `crates/celerrate_types/src/flow/callables.rs`:

```rust
//! Closures and first-class callables: closure/arrow-function typing
//! (including nested-return collection and written-parameter
//! seeding) and the projection of `f(...)` / `$o->m(...)` /
//! `C::m(...)` into callable signatures.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}
```

Cut from `flow/mod.rs` the contiguous block from `fn nested_returns` (anchor 3318) up to (excluding) `fn string_parts` (anchor 3571): `nested_returns`, `closure_type`, `seed_written_parameters`, `projected_callable`, `projected_callable_of_function`, `projected_callable_of_method`, `projected_callable_of_keys`. Paste verbatim.

- [ ] **Step 2: Declare `mod callables;` in `flow/mod.rs`** (alphabetical order).

- [ ] **Step 3: Compiler-driven visibility**

Run: `cargo check -p celerrate_types` — mark `pub(super)` as demanded, re-run until clean.

- [ ] **Step 4: Verify**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS. Then the `--color-moved` check.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract callable projection into flow/callables.rs (#39)"
```

---

### Task 8: Extract `flow/assignment.rs` — literals, assignment, and the free helpers

**Files:**
- Create: `crates/celerrate_types/src/flow/assignment.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Produces: `Walker::string_parts`, `array_literal`, `assignment` as `pub(super)` for the walk; the free helper `widen_if_literal` as `pub(super)` (also called from the `??` coalesce arm of `expression_value`, which lands in `flow/walk.rs`); `compound_base`, `updated_array`, `shape_join` stay private to `assignment.rs`.

- [ ] **Step 1: Create the submodule with the moved blocks**

Create `crates/celerrate_types/src/flow/assignment.rs`:

```rust
//! Literals and assignment: interpolated strings, array literals
//! (shape versus list versus map), simple and compound assignment,
//! and assignment targets (locals, properties, array writes) — plus
//! the pure helpers they reduce to.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}

// moved free functions go here
```

Cut from `flow/mod.rs`, pasting verbatim:

1. The contiguous method block from `fn string_parts` (anchor 3571) through the end of `fn assign_target` (anchor 3727, the last method of the `impl`): `string_parts`, `array_literal`, `assignment`, `assign_target`.
2. The four trailing free functions (anchors 3786–3911): `widen_if_literal`, `compound_base`, `updated_array`, `shape_join`. Paste them below the `impl` block.

- [ ] **Step 2: Declare `mod assignment;` in `flow/mod.rs`** (alphabetical order).

- [ ] **Step 3: Compiler-driven visibility**

Run: `cargo check -p celerrate_types`
Mark `pub(super)` as demanded — expect it on `string_parts`, `array_literal`, `assignment`, and the free `widen_if_literal` (its other caller is still in `mod.rs` until Task 9). Re-run until clean.

- [ ] **Step 4: Verify**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS. Then the `--color-moved` check.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract literals and assignment into flow/assignment.rs (#39)"
```

---

### Task 9: Extract `flow/walk.rs` — the statement/expression walk

**Files:**
- Create: `crates/celerrate_types/src/flow/walk.rs`
- Modify: `crates/celerrate_types/src/flow/mod.rs`

**Interfaces:**
- Consumes: everything the prior tasks exported as `pub(super)`.
- Produces: `Walker::statements` as `pub(super)` (`walk_body` in `mod.rs` calls it); `statement`, `looped`, `expression`, `expression_value` stay private to `walk.rs` unless the compiler says otherwise (`closure_type` in `callables.rs` recurses into the walk, so expect `pub(super)` on `statements` and `expression` at least).

- [ ] **Step 1: Create the submodule with the moved block**

Create `crates/celerrate_types/src/flow/walk.rs`:

```rust
//! The walk itself: statements in order, expressions by form. Every
//! arm delegates to a cluster module — the member boundary, the call
//! machinery, iteration, branching, callables, assignment — so this
//! file is the traversal, not the typing rules. `expression_value`'s
//! match moves here whole, deliberately unsplit (spec decision 2).

use super::*;

impl<'db> Walker<'db, '_, '_> {
    // moved methods go here
}
```

Cut from `flow/mod.rs` the contiguous block from `fn statements` (anchor 1774) up to (excluding) `fn branch_environments`' former position — after Tasks 2–8 this is the block from `fn statements` through the end of `fn expression_value` (anchor 2126, ~875 lines): `statements`, `statement`, `looped`, `expression`, `expression_value`. Paste verbatim. Do not split the `expression_value` match.

- [ ] **Step 2: Declare `mod walk;` in `flow/mod.rs`** (alphabetical order).

- [ ] **Step 3: Compiler-driven visibility**

Run: `cargo check -p celerrate_types` — mark `pub(super)` as demanded, both in `walk.rs` and back in `mod.rs` (context helpers like `record`, `recorded`, `subject_type`, `this_type`, `receiver_parts`, `scoped_subject` are now called from `walk.rs`; methods defined in `mod.rs` are visible to submodules already **only if** their visibility reaches them — private methods defined in `mod.rs` ARE visible in child modules, so most context helpers need no change; only cross-submodule calls between siblings need `pub(super)`). Re-run until clean, then clean up now-unused imports in `mod.rs`.

- [ ] **Step 4: Verify**

Run: `cargo test -p celerrate_types && cargo fmt --all`
Expected: PASS. Then the `--color-moved` check.

Also verify the final shape:

```bash
wc -l crates/celerrate_types/src/flow/*.rs
```

Expected: `mod.rs` around 800 lines, `walk.rs` around 1,250, no file above ~1,300 — approximate, the partition matters, not the counts.

- [ ] **Step 5: Commit**

```bash
git commit -m "♻️ refactor(types): extract the walk into flow/walk.rs (#39)"
```

---

### Task 10: The substitution funnel guard test

**Files:**
- Create: `crates/celerrate_types/tests/substitution_funnel_guard.rs`

**Interfaces:**
- Consumes: nothing from the crate's API — it reads the crate's own sources under `CARGO_MANIFEST_DIR`.
- Produces: the mechanical half of the funnel invariant; `flow/boundary.rs`'s rustdoc (Task 4) is the prose half and points here.

- [ ] **Step 1: Write the guard test**

Create `crates/celerrate_types/tests/substitution_funnel_guard.rs`:

```rust
//! Negative-proof guard for the substitution funnel (issue #39):
//! `Walker::member_boundary_type` in `src/flow/boundary.rs` is the
//! one funnel every member read, method call, callable projection,
//! and `new` result passes through. The call sites outside it are
//! deliberate and enumerated below — the PR #35 whole-branch review
//! verified this list by hand; this test keeps it verified by
//! machine. An unlisted site is not necessarily a bug, but it is
//! necessarily a decision: justify it against the invariant in
//! `src/flow/boundary.rs`'s rustdoc, then add it here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

/// Production call sites of `substitution::substitute`, by file
/// relative to `src/`, with their exact count. `substitution.rs`
/// itself is exempt (its own recursion and its test module).
const ALLOWED_CALL_SITES: &[(&str, usize)] = &[
    ("declared.rs", 2),        // written-type substitution at declaration reading
    ("flow/boundary.rs", 1),   // member_boundary_type — THE funnel
    ("flow/calls.rs", 1),      // solved_call_result — call-site template solving
    ("inheritance.rs", 2),     // parent-argument substitution along linearization
    ("solver.rs", 2),          // template-map application and bound checking
];

#[test]
fn substitution_stays_funneled_through_member_boundary_type() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeMap::new();
    collect_call_sites(&source_root, &source_root, &mut found);

    let expected: BTreeMap<String, usize> = ALLOWED_CALL_SITES
        .iter()
        .map(|(file, count)| ((*file).to_owned(), *count))
        .collect();

    assert_eq!(
        found, expected,
        "substitution call sites changed. `member_boundary_type` in \
         src/flow/boundary.rs is the one funnel every member read, \
         method call, callable projection, and `new` result passes \
         through (issue #39). A new `substitute` call site must be \
         justified against that invariant and, if legitimate, added \
         to ALLOWED_CALL_SITES with its count."
    );
}

/// Counts non-comment lines containing a `substitute(` call per file
/// under `src/`, excluding `substitution.rs` (the defining module)
/// and `fn substitute` definitions. Textual on purpose: the guard
/// must catch call sites regardless of import style
/// (`crate::substitution::substitute(...)` or a `use`d bare
/// `substitute(...)`).
fn collect_call_sites(root: &Path, directory: &Path, found: &mut BTreeMap<String, usize>) {
    for entry in std::fs::read_dir(directory).expect("source directory is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() {
            collect_call_sites(root, &path, found);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("file is under src")
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "substitution.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("source file is readable");
        let count = text
            .lines()
            .map(str::trim_start)
            .filter(|line| !line.starts_with("//"))
            .filter(|line| line.contains("substitute("))
            .filter(|line| !line.contains("fn substitute"))
            .count();
        if count > 0 {
            found.insert(relative, count);
        }
    }
}
```

- [ ] **Step 2: Run it — it must pass against the extracted layout**

Run: `cargo test -p celerrate_types --test substitution_funnel_guard`
Expected: PASS. If it fails, the diff in the assertion message tells you which file drifted — reconcile against the real call sites (`grep -rn "substitute(" crates/celerrate_types/src`), do NOT silently widen the allowlist.

- [ ] **Step 3: Prove it can fail (red proof)**

Temporarily add to `crates/celerrate_types/src/operators.rs` (any non-exempt file):

```rust
const RED_PROOF: &str = "substitute(";
```

Run: `cargo test -p celerrate_types --test substitution_funnel_guard`
Expected: FAIL, with `operators.rs` appearing in the found-but-not-allowed diff.

Then delete the line and re-run: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_types/tests/substitution_funnel_guard.rs
git commit -m "✅ test(types): guard the member-boundary substitution funnel (#39)"
```

---

### Task 11: Full gates, whole-branch move review, and the PR

**Files:**
- None created; verification and delivery only.

- [ ] **Step 1: Full workspace gates**

Run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

Expected: all four clean.

- [ ] **Step 2: Whole-branch purity review**

Run: `git diff main...HEAD --color-moved=dimmed-zebra -- crates/celerrate_types/src/`
Expected: every method body reads as moved (dimmed). The only bright (new) lines across the whole branch: module rustdocs, `use super::*;` lines, `impl` wrappers, `mod` declarations, the `pub(crate) use calls::resolved_function_key;` re-export, `pub(super)` markers, and the guard test file. Any other bright line in `src/` is a transcription error — fix it before the PR.

Also confirm the frozen public surface:

```bash
grep -rn "use crate::flow" crates/celerrate_types/src/inference.rs crates/celerrate_types/src/checks/arguments.rs
```

Expected: both imports byte-identical to `main` (no consumer changed).

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin HEAD
gh pr create \
  --title "♻️ refactor(types): split flow.rs into cohesive flow/ submodules" \
  --body "$(cat <<'EOF'
Closes #39.

Pure refactor, no behaviour change: `flow.rs` (3,911 lines) becomes a `flow/` directory — `mod.rs` (Environment, Walker, walk_body, context helpers) plus eight cohesive submodules (`walk`, `boundary`, `calls`, `instantiation`, `iteration`, `branching`, `callables`, `assignment`). Cross-submodule methods are `pub(super)`; the crate-facing surface (`walk_body`, `FlowContext`, `resolved_function_key`) is unchanged and no consumer file moved.

The funnel constraint the issue asks to preserve is now held mechanically: `tests/substitution_funnel_guard.rs` enumerates every `substitution::substitute` call site in the crate against an explicit allowlist (the funnel in `flow/boundary.rs`, plus the seven deliberate sites the PR #35 review audited), and `flow/boundary.rs`'s rustdoc states the invariant.

Spec: `.claude/superpowers/specs/2026-07-19-flow-extraction-design.md`
Plan: `.claude/superpowers/plans/2026-07-19-flow-extraction.md`

Review tip: read the diff with `--color-moved=dimmed-zebra` — every method body should render as moved, one commit per extracted cluster.
EOF
)"
```

Expected: PR opens against `main`, CI runs the same gates as Step 1.
