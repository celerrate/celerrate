# Call-Result Narrowing Hardening — Design

Date: 2026-07-19
Status: Approved (brainstorming output for issue #72)
Issue: https://github.com/celerrate/celerrate/issues/72
Parent design: `.claude/superpowers/specs/2026-07-19-call-result-narrowing.md`
(issue #54, PR #71)

## 1. Context and scope

Issue #72 collects the follow-up debt from the call-result narrowing
work (#54). The whole-branch review behind it found no false-positive
path: everything below degrades toward silence (the accepted
direction) or is documentation and test debt.

Point 1 of the issue — truthiness guards narrowing from `mixed`
instead of the call's recorded type — **already merged** in commit
`f89591d` (PR #71): both the default arm and the `Empty` arm of
`branch_environments` substitute the just-recorded condition type for
an unbound `CallResult` subject, and both carry inline comments
citing issue #72. It is out of scope here.

This design covers everything else, in one plan:

- the five unswept value-change sites (issue points 3–7),
- the shared kill helper (point 8),
- the over-claiming one-directionality wording and the stale `eval`
  comment (points 2 and 9),
- the verdict-test debt (points 10 and 11).

**Transversal principle.** Every behavioral change in this design
*removes undue silence*: a stale fingerprint binding kept alive past
a genuine value change suppresses a nullability report that should
fire. Killing the binding restores the report. None of the changes
can fabricate a false positive, because every new kill site targets a
local whose value genuinely changed (loop rebind, catch bind,
by-reference capture, `extract`, increment). The acceptance gate is
the Symfony corpus: no new diagnostics.

Out of scope: the fingerprint grammar (unchanged), property-rooted
receivers (the parent design's recorded v1 exclusion), any new
diagnostic family, and the guard-type precision fix (point 1, already
shipped).

## 2. Foundation: the shared kill helper (point 8)

The three-line pattern

```rust
if let NarrowingSubject::Local { name } = &subject {
    environment.kill_call_results_involving(name);
}
```

repeats across the existing kill sites (`flow/calls.rs`,
`flow/assignment.rs` four times, `flow/walk.rs` in the `Global` and
`Unset` arms). Add one method on `Environment`:

```rust
/// The subject-shaped face of the kill rule: a `Local` subject's
/// value changed, so every fingerprint mentioning it is stale.
/// Non-`Local` subjects kill nothing — a property or call-result
/// subject is not a fingerprint base or argument.
pub(crate) fn kill_call_results_for_subject(&mut self, subject: &NarrowingSubject) {
    if let NarrowingSubject::Local { name } = subject {
        self.kill_call_results_involving(name);
    }
}
```

Refactor the existing sites to call it. This lands first so the new
sweep sites of section 3 use it from birth. `StaticVariables`
(`walk.rs`), which already holds a bare `name`, keeps calling
`kill_call_results_involving` directly.

## 3. Behavior: the five unswept value-change sites (points 3–7)

All five verified unswept in the current tree. Each is
false-negative-only debt: a stale narrowing silences a report that
should fire.

1. **`foreach` key/value binds** (`flow/walk.rs`, `Foreach` arm). The
   loop arm binds the key and value subjects directly, not through
   `assign_target`, so fingerprints naming the loop variable survive
   the rebind. Call `kill_call_results_for_subject` immediately
   before each bind. Placement: at the top of each iteration pass,
   before the body — fingerprints established *inside* the current
   iteration are killed only when the next pass rebinds, which is
   exactly when the value changes.
2. **`catch` variable bind** (`flow/walk.rs`, `Try` arm). The catch
   arm binds the caught variable directly. Kill fingerprints naming
   it on the arm environment before the bind.
3. **Closure `use (&$x)` capture** (`flow/walk.rs`, `Closure` arm,
   by-reference branch). The outer environment binds the capture to
   `mixed` (decision 10 of the parent design: aliased, unknowable)
   but leaves fingerprints naming it alive. Kill on the **outer**
   environment. The by-value branch changes nothing — it only reads
   the outer environment into `inner`.
4. **`extract()`** (`flow/walk.rs`, named-call arm). The existing
   sweep removes `Local` bindings but leaves `CallResult`
   fingerprints whose base or arguments name those locals. Add a
   predicate on `NarrowingSubject`:

   ```rust
   /// Whether this subject is a call-result fingerprint involving
   /// *any* local — `extract()`'s sweep predicate: it may rewrite
   /// every local, so every local-involving fingerprint is stale.
   pub(crate) fn call_result_involves_any_local(&self) -> bool
   ```

   (the any-local mirror of `call_result_involves_local`) and extend
   the sweep to remove those fingerprints too. A `CallBase::This`
   fingerprint whose arguments are all literals or `$this` correctly
   survives: `extract()` cannot reassign `$this`.
5. **`++`/`--`** (`flow/walk.rs`). The `Postfix` arm is always `++`
   or `--` (the parser produces no other postfix operator): kill on
   `subject_of(operand)` unconditionally. The `Unary` arm covers all
   prefix operators: kill only when the operator is
   `SyntaxKind::PlusPlus` or `SyntaxKind::MinusMinus`.

Each site lands test-first: a failing verdict test showing the stale
fingerprint silencing a CEL0034 report, then the kill.

## 4. Documentation: scoping the promise (points 2 and 9)

1. **The one-directionality wording over-claims.** The `CallResult`
   variant documentation (`narrowing.rs`) and the `0.0.3`-era
   changelog entry say the purity assumption "can only silence, never
   fabricate a report". That is true for **positive** guards. Under a
   **negative** guard, negative-branch narrowing plus the survival
   rule makes the lazy-initialization idiom report:

   ```php
   if ($this->getUser() === null) {
       $this->authenticate();          // actually re-populates
       $this->getUser()->getName();    // reports CEL0034
   }
   ```

   This is PHPStan-parity behavior and corpus-clean, but it is a
   report the purity assumption *produces*, not one it silences.
   Rewordings: scope the guarantee to positive guards in the variant
   documentation, and in the changelog **add a new Unreleased entry**
   recording the corrected claim (the shipped `0.0.3` entry is
   history and stays untouched). The stance itself is pinned by a
   test (section 5).
2. **The `eval` comment forgets what the code forgets.** The `Eval`
   arm's comment says "eval can rewrite every local and every
   property binding" — but replacing the environment with
   `Environment::new()` also wipes call-result fingerprints, which is
   correct and must be said. Extend the comment.

## 5. Tests (points 2, 10, 11)

1. **Pin the lazy-initialization stance.** A verdict test asserting
   that the negative-guard idiom of section 4 **does report**
   CEL0034, with a comment naming it the recorded, PHPStan-parity
   consequence of the purity assumption. The stance becomes
   conscious, not accidental.
2. **End-to-end `CallBase::This` coverage.** A verdict test guarding
   and dereferencing `$this->user()` (fingerprint base `This`),
   today covered only at the `subject_of` unit level.
3. **Kill-site verdict coverage.** Only reassignment and
   by-reference-call kills have verdict tests today. Add one for
   `unset($e)` (the plausible real-world shape named in the issue).
   The five new sites of section 3 each arrive with their own
   failing-first verdict test, closing the gap site by site.

## 6. Acceptance

- `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo fmt --all --check` clean.
- The Symfony corpus reports **no new diagnostics** (the changes only
  remove narrowings, and removal can only surface reports on
  genuinely unguarded code, which the corpus does not contain — the
  gate verifies exactly that claim).
- Changelog entry under Unreleased describing the swept sites and the
  scoped wording.
