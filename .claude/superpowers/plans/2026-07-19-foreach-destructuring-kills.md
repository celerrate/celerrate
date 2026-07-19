# Foreach List-Destructuring Kills and Binds — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `foreach` list-destructuring binds kill stale call-result fingerprints and bind each destructured target to its element type, by routing the foreach value bind through the existing `assign_target` machinery (issue #75).

**Architecture:** One behavioral change in `celerrate_types`: the `Foreach` arm of the flow walker stops guarding its value bind on `subject_of` (which returns `None` for destructuring patterns) and instead calls `assign_target`, which already recurses into `Array` patterns, kills each leaf target's fingerprints, and binds it via `index_type`. Everything else is pinning tests and documentation.

**Tech Stack:** Rust, salsa, the existing `celerrate_types` test helpers (`family_verdicts` for verdict tests, `fixture`/`caller_return_display` for inference tests).

**Design:** `.claude/superpowers/specs/2026-07-19-foreach-destructuring-kills-design.md`

## Global Constraints

- Zero panic: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Test modules may locally `#[allow]` these lints (the existing test modules already do).
- TDD: failing test first for the behavioral change (Task 1). Tasks 2 and 3 add pinning tests expected to pass against Task 1's machinery — if one fails, stop and report; that contradicts the design.
- All code, comments, and documentation in English; full words, no abbreviated names.
- Work on the existing branch `fix-75-foreach-destructuring-kills` (the design spec is already committed there).
- Commits: gitmoji + Conventional Commits, repository-configured identity, no Claude attribution.
- Corpus stance (design section 3): verify then accept — every Symfony corpus delta is hand-inspected; a true positive updates the snapshot and is documented in the pull request; a single false positive is blocking.

---

### Task 1: The headline kill — foreach value binds route through `assign_target`

**Files:**
- Modify: `crates/celerrate_types/src/flow/walk.rs:176-180` (the `Foreach` arm's value-bind block)
- Modify: `crates/celerrate_types/src/flow/assignment.rs:165` (visibility of `assign_target`)
- Test: `crates/celerrate_types/src/checks/nullability.rs` (append after `a_foreach_rebind_kills_the_call_result_narrowing`, which ends around line 545)

**Interfaces:**
- Consumes: `assign_target(&mut self, target: ExpressionId, value_type: TypeId<'db>, environment: &mut Environment<'db>)` in `flow/assignment.rs` (currently private), `family_verdicts(source: &str) -> Vec<TypedVerdictKind>` and `TypedVerdictKind::NullDereference { member, receiver }` from the nullability test module.
- Produces: `assign_target` becomes `pub(super)` (visible to the sibling `flow::walk` module); the `Foreach` arm's value bind kills and binds every destructured leaf target. Tasks 2 and 3 rely on this behavior, not on any new name.

- [ ] **Step 1: Write the failing verdict test**

In `crates/celerrate_types/src/checks/nullability.rs`, directly after the test `a_foreach_rebind_kills_the_call_result_narrowing`, add:

```rust
    #[test]
    fn a_foreach_destructuring_rebind_kills_the_call_result_narrowing() {
        // List-destructuring binds are value changes like plain loop
        // variables (issue #75): the pattern's leaf targets are
        // rebound on every pass, so fingerprints naming them are
        // stale inside the body. `f` rebinds a local the fingerprint
        // names as an argument through the short syntax; `g` does
        // the same through the keyed form. Both dereferences are
        // silenced today because the pattern is neither bound nor
        // killed.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function f(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        foreach ([[1, 2]] as [$id, $x]) {
            $repo->find($id)->title;
        }
    }
}
function g(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        foreach ([1 => [1, 2]] as $k => [$id, $x]) {
            $repo->find($id)->title;
        }
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::NullDereference {
                    member: "title".to_owned(),
                    receiver: "Post|null".to_owned(),
                },
                TypedVerdictKind::NullDereference {
                    member: "title".to_owned(),
                    receiver: "Post|null".to_owned(),
                },
            ],
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p celerrate_types a_foreach_destructuring_rebind_kills_the_call_result_narrowing
```

Expected: FAIL — the assertion reports `left == right` with an empty `left` (both dereferences are silenced by the surviving fingerprints).

- [ ] **Step 3: Raise `assign_target` to `pub(super)`**

In `crates/celerrate_types/src/flow/assignment.rs`, line 165, change:

```rust
    fn assign_target(
```

to:

```rust
    pub(super) fn assign_target(
```

(`assignment` and `walk` are sibling modules under `flow`; `pub(super)` makes the method visible in `flow` and therefore in `flow::walk`.)

- [ ] **Step 4: Route the foreach value bind through `assign_target`**

In `crates/celerrate_types/src/flow/walk.rs`, the `Foreach` arm, replace:

```rust
                    walker.expression(value, env);
                    if let Some(subject) = subject_of(walker.context.ir, value) {
                        env.kill_call_results_for_subject(&subject);
                        env.bind(subject, value_type);
                    }
```

with:

```rust
                    walker.expression(value, env);
                    // The value bind is an assignment (issue #75): a
                    // destructuring pattern rebinds every leaf
                    // target, so `assign_target` recurses into it,
                    // kills each target's stale call-result
                    // fingerprints, and binds it to its element
                    // type. A plain variable takes the same path it
                    // always did (kill, then bind). The
                    // `expression` call above recorded the pattern
                    // subtree — including pattern keys, which
                    // `assign_target` reads back through `recorded`.
                    walker.assign_target(value, value_type, env);
```

The key-bind block above it and the `by_reference` stance are untouched.

- [ ] **Step 5: Run the test to verify it passes**

```bash
cargo test -p celerrate_types a_foreach_destructuring_rebind_kills_the_call_result_narrowing
```

Expected: PASS.

- [ ] **Step 6: Run the crate suite to catch regressions**

```bash
cargo test -p celerrate_types
```

Expected: PASS. The existing foreach tests (`a_foreach_rebind_kills_the_call_result_narrowing`, `foreach_over_an_array_literal_types_key_and_value`, the iteration-protocol tests) must be untouched: the plain-variable path performs the same kill and bind as before.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types/src/flow/walk.rs crates/celerrate_types/src/flow/assignment.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): stale fingerprints die at foreach destructuring binds (#75)"
```

---

### Task 2: Pattern-coverage verdict tests (keyed pattern, nesting, `list()`, survival)

**Files:**
- Test: `crates/celerrate_types/src/checks/nullability.rs` (append after Task 1's test)

**Interfaces:**
- Consumes: the Task 1 behavior (`assign_target` routing), `family_verdicts`, `TypedVerdictKind::NullDereference`.
- Produces: pinned coverage only; no new names.

These tests are expected to pass immediately: they pin coverage the `assign_target` recursion already provides (design section 2, "coverage for free"). If any fails, stop and report — that contradicts the design (for example, `list(...)` not lowering to `BodyExpression::Array`).

- [ ] **Step 1: Write the four pinning tests**

In `crates/celerrate_types/src/checks/nullability.rs`, directly after `a_foreach_destructuring_rebind_kills_the_call_result_narrowing`, add:

```rust
    #[test]
    fn destructuring_kill_coverage_spans_keyed_nested_and_list_patterns() {
        // The `assign_target` recursion covers every pattern form
        // uniformly (issue #75): explicit string keys (`h`),
        // nesting (`i`), and the classic `list()` syntax (`j`),
        // which lowers to the same `Array` node as the short form.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function h(Repo $repo, int $v): void {
    if ($repo->find($v)) {
        foreach ([['k' => 1]] as ['k' => $v]) {
            $repo->find($v)->title;
        }
    }
}
function i(Repo $repo, int $a): void {
    if ($repo->find($a)) {
        foreach ([[[1], 2]] as [[$a], $b]) {
            $repo->find($a)->title;
        }
    }
}
function j(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        foreach ([[1, 2]] as list($id, $x)) {
            $repo->find($id)->title;
        }
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::NullDereference {
                    member: "title".to_owned(),
                    receiver: "Post|null".to_owned(),
                },
                TypedVerdictKind::NullDereference {
                    member: "title".to_owned(),
                    receiver: "Post|null".to_owned(),
                },
                TypedVerdictKind::NullDereference {
                    member: "title".to_owned(),
                    receiver: "Post|null".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn a_destructuring_kill_is_scoped_to_the_pattern_targets() {
        // The kill sweeps the pattern's own targets, not every
        // local: `$id` does not appear in the pattern, so its
        // guarded fingerprint survives the loop and keeps
        // silencing the dereference.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function k(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        foreach ([[1]] as [$x]) {
            $repo->find($id)->title;
        }
    }
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }
```

- [ ] **Step 2: Run the two tests**

```bash
cargo test -p celerrate_types destructuring_kill
```

Expected: both PASS (`destructuring_kill_coverage_spans_keyed_nested_and_list_patterns`, `a_destructuring_kill_is_scoped_to_the_pattern_targets`). A failure is a finding: stop and report before proceeding.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_types/src/checks/nullability.rs
git commit -m "✅ test(types): pin pattern coverage for the destructuring kills (#75)"
```

---

### Task 3: Element-typing inference test

**Files:**
- Test: `crates/celerrate_types/src/inference.rs` (append after `foreach_over_an_array_literal_types_key_and_value`, which ends around line 3604)

**Interfaces:**
- Consumes: the Task 1 behavior; the inference test helpers `fixture(&[&str]) -> Fixture` and `caller_return_display(&Fixture, &str) -> String` already used by the neighboring foreach tests.
- Produces: pinned coverage only; no new names.

- [ ] **Step 1: Write the element-typing test**

In `crates/celerrate_types/src/inference.rs`, directly after `foreach_over_an_array_literal_types_key_and_value`, add:

```rust
    #[test]
    fn foreach_destructuring_types_the_pattern_targets() {
        // Destructured loop variables carry the element types
        // `assign_target` derives from the iteration value through
        // `index_type` (issue #75): positional patterns read the
        // shape's positional fields, keyed patterns read the named
        // field. Before the fix the targets kept their stale
        // pre-loop bindings. Displays follow `structural_order`'s
        // sorted literal unions, exactly like the array-literal
        // foreach test above.
        let f = fixture(&[r#"<?php
namespace App;
function positional() {
    foreach ([[1, 2]] as [$a, $b]) { return $b; }
    return 0;
}
function keyed() {
    foreach ([['k' => 5]] as ['k' => $v]) { return $v; }
    return 0;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\positional"), "0|2");
        assert_eq!(caller_return_display(&f, "app\\keyed"), "0|5");
    }
```

- [ ] **Step 2: Run the test**

```bash
cargo test -p celerrate_types foreach_destructuring_types_the_pattern_targets
```

Expected: PASS with exactly `"0|2"` and `"0|5"`. If the displayed union differs, verify the inferred type is genuinely the element type (shape field `1` is the literal `2`; field `'k'` is the literal `5`, each unioned with the fallback `0`) before adjusting the pinned string; a `mixed` or a stale type in the display is a bug, not a string to appease.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_types/src/inference.rs
git commit -m "✅ test(types): destructured loop targets carry element types (#75)"
```

---

### Task 4: CHANGELOG entry

**Files:**
- Modify: `CHANGELOG.md` (the `### Fixed` section under `## [Unreleased]`, which currently opens with the #72 call-result hardening entry around line 32)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: documentation only.

- [ ] **Step 1: Add the entry**

In `CHANGELOG.md`, insert as the first bullet under the existing `### Fixed` heading of the `[Unreleased]` section (before the #72 entry, leaving it untouched — design section 5):

```markdown
- `foreach` list-destructuring binds are now value-change sites
  (#75): the value pattern routes through the assignment machinery,
  so every destructured target kills its stale call-result
  fingerprints and is bound to its element type — short and
  `list()` syntaxes, keyed and nested patterns, and index targets
  alike. Each missed kill could only silence a genuine
  possibly-null dereference (CEL0034), never fabricate one;
  destructured loop variables previously kept their stale pre-loop
  types.
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the foreach destructuring fix (#75)"
```

---

### Task 5: Workspace verification, the corpus gate, and the pull request

**Files:** none created or modified by verification itself. A corpus snapshot delta is handled under the verify-then-accept stance (Global Constraints): a hand-verified true positive updates the snapshot in its own commit; a false positive is blocking.

- [ ] **Step 1: Full workspace suite**

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 2: Clippy and formatting**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Expected: both clean.

- [ ] **Step 3: The Symfony corpus gate (verify then accept)**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
```

Expected: the snapshot check passes with no changes. If it reports deltas: inspect every one by hand. Each delta must be a genuine possibly-null dereference newly surfaced by the kill or by the element typing (a true positive). If all deltas are verified true positives, update the snapshot as the tooling directs, commit it separately with

```bash
git commit -m "✅ test(corpus): accept the destructuring true positives (#75)"
```

and list each delta with its verification in the pull request body. If any delta is a false positive, STOP: do not update the snapshot, report the finding — it is blocking (Global Constraints).

- [ ] **Step 4: Push and open the pull request**

```bash
git push -u origin fix-75-foreach-destructuring-kills
gh pr create --title "🐛 fix(types): foreach destructuring kills and binds (#75)" --body "Closes #75.

- The \`Foreach\` arm's value bind routes through \`assign_target\` (now \`pub(super)\`): destructuring patterns kill each leaf target's stale call-result fingerprints and bind it to its element type, uniformly with plain assignment — short and \`list()\` syntaxes, keyed and nested patterns, index targets.
- Strictly false-negative debt: each missed kill could only silence a genuine CEL0034, never fabricate one. Destructured loop variables previously kept their stale pre-loop types.
- Pinned by verdict tests (headline kill, pattern coverage, survival contrast) and an element-typing inference test.
- Symfony corpus: <no new diagnostics | the verified true positives listed below>.

Design: \`.claude/superpowers/specs/2026-07-19-foreach-destructuring-kills-design.md\`
Plan: \`.claude/superpowers/plans/2026-07-19-foreach-destructuring-kills.md\`"
```

Expected: pull request created against `main`. Replace the corpus line with the actual outcome from Step 3 before submitting.
