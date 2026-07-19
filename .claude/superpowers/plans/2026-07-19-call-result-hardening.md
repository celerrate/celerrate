# Call-Result Narrowing Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close issue #72's remainder: sweep the five unswept
value-change sites so stale call-result fingerprints die where a
local's value genuinely changes, fold the repeated kill pattern into
one helper, scope the over-claiming purity wording to positive
guards, and pay the verdict-test debt.

**Architecture:** All changes live in `celerrate_types`, inside the
flow walker (`src/flow/`) and the narrowing vocabulary
(`src/narrowing.rs`). Every behavioral change removes a stale
`CallResult` binding from the environment at a value-change site;
removal can only surface reports on genuinely unguarded code (the
false-negative-only direction). Tests are CEL0034 verdict tests in
`src/checks/nullability.rs`, matching the existing kill-site tests.

**Tech Stack:** Rust 1.94 (edition 2024, let-chains available),
salsa 0.27, the existing `family_verdicts` test support.

**Design source:**
`.claude/superpowers/specs/2026-07-19-call-result-hardening-design.md`

**Branch:** `fix-72-call-result-hardening` (already created, carries
the spec commit).

## Global Constraints

- Zero panic: clippy denies `unwrap_used`, `expect_used`,
  `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is
  forbidden. Test modules may locally `#[allow]` these lints (the
  nullability test module already does).
- Everything in English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits. No Claude attribution
  anywhere.
- The observability trap (from the design work): a fingerprint whose
  *base* local degrades to `mixed` at the same site is silent for
  CEL0034 with or without the kill (a call on a `mixed` receiver
  types `mixed`, which never contains null). Verdict tests must
  therefore target fingerprints where the killed local is an
  **argument** of a call on a still-typed receiver, or rebind the
  base to a **typed** value. Each task's test below is already
  shaped that way — do not "simplify" a test into the unobservable
  shape.
- Run tests from the workspace root. The verdict tests live in
  `crates/celerrate_types/src/checks/nullability.rs`; run them with
  `cargo test --package celerrate_types nullability`.

---

### Task 1: The shared kill helper

Behavior-preserving refactor: one method replaces the seven copies of
the three-line kill pattern. No new test — the existing kill verdict
tests (`a_base_value_change_kills_the_call_result_narrowing`,
`an_argument_value_change_kills_the_call_result_narrowing`) cover the
sites; they must stay green.

**Files:**
- Modify: `crates/celerrate_types/src/flow/mod.rs` (add method after
  `kill_call_results_involving`, around line 123)
- Modify: `crates/celerrate_types/src/flow/calls.rs:142-144`
- Modify: `crates/celerrate_types/src/flow/assignment.rs` (four
  sites: by-reference target ~145, by-reference value ~151, index
  write base ~203, plain assign target ~211)
- Modify: `crates/celerrate_types/src/flow/walk.rs` (`Global` arm
  ~72-74, `Unset` arm ~98-100)

**Interfaces:**
- Produces: `Environment::kill_call_results_for_subject(&mut self,
  subject: &NarrowingSubject)` — used by every later task.

- [ ] **Step 1: Add the helper to `Environment` in `flow/mod.rs`**

Immediately after `kill_call_results_involving` (after its closing
brace, line 123):

```rust
    /// The subject-shaped face of the kill rule: a `Local` subject's
    /// value changed, so every fingerprint mentioning it (as base or
    /// argument) is stale. Non-`Local` subjects kill nothing — a
    /// property or call-result subject is not a fingerprint base or
    /// argument.
    pub(crate) fn kill_call_results_for_subject(&mut self, subject: &NarrowingSubject) {
        if let NarrowingSubject::Local { name } = subject {
            self.kill_call_results_involving(name);
        }
    }
```

- [ ] **Step 2: Replace the seven call sites**

Each site currently reads (variable names differ per site):

```rust
if let NarrowingSubject::Local { name } = &subject {
    environment.kill_call_results_involving(name);
}
```

Replace each with the single line:

```rust
environment.kill_call_results_for_subject(&subject);
```

The seven sites and their subject variable:

1. `flow/calls.rs` `apply_by_reference` (~142): subject `subject`.
2. `flow/assignment.rs` by-reference target (~145): subject
   `subject`.
3. `flow/assignment.rs` by-reference value (~151): subject `subject`.
4. `flow/assignment.rs` `assign_target` index-write arm (~203):
   subject `base` — the line becomes
   `environment.kill_call_results_for_subject(&base);`.
5. `flow/assignment.rs` `assign_target` default arm (~211): subject
   `subject`.
6. `flow/walk.rs` `Global` arm (~72): subject `subject`.
7. `flow/walk.rs` `Unset` arm (~98): subject `subject`.

`StaticVariables` (`walk.rs` ~85) holds a bare `&variable.name`, not
a subject: it keeps calling `kill_call_results_involving` directly.
Do not touch it.

- [ ] **Step 3: Run the crate tests and clippy**

```bash
cargo test --package celerrate_types
cargo clippy --package celerrate_types --all-targets -- -D warnings
```

Expected: all tests PASS (in particular the two existing kill verdict
tests), clippy clean.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_types/src/flow/mod.rs crates/celerrate_types/src/flow/calls.rs crates/celerrate_types/src/flow/assignment.rs crates/celerrate_types/src/flow/walk.rs
git commit -m "♻️ refactor(types): fold the local-kill pattern into kill_call_results_for_subject (#72)"
```

---

### Task 2: `foreach` key/value rebinds kill

**Files:**
- Modify: `crates/celerrate_types/src/flow/walk.rs` (`Foreach` arm,
  key bind ~170-175 and value bind ~176-179)
- Test: `crates/celerrate_types/src/checks/nullability.rs` (append to
  the existing `tests` module)

**Interfaces:**
- Consumes: `Environment::kill_call_results_for_subject` (Task 1).

- [ ] **Step 1: Write the failing verdict test**

Append to the `tests` module of `checks/nullability.rs`:

```rust
    #[test]
    fn a_foreach_rebind_kills_the_call_result_narrowing() {
        // The loop arm binds the key and value subjects directly,
        // not through `assign_target` (issue #72 item 3): each
        // rebind is a value change, so fingerprints naming the loop
        // variable are stale inside the body. `f` rebinds the
        // fingerprint's base to a typed value; `g` rebinds a local
        // the fingerprint names as an argument (the key bind path).
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Event { public function getCommand(): ?Command { return null; } }
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function f(Event $e, Event $other): void {
    if ($e->getCommand()) {
        foreach ([$other] as $e) {
            $e->getCommand()->getName();
        }
    }
}
function g(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        foreach ([1 => 2] as $id => $value) {
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
                    member: "getName".to_owned(),
                    receiver: "Command|null".to_owned(),
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
cargo test --package celerrate_types a_foreach_rebind_kills_the_call_result_narrowing
```

Expected: FAIL — the verdicts vector is empty (both stale
fingerprints still narrow).

- [ ] **Step 3: Kill at both rebinds**

In the `Foreach` arm's `looped` closure, insert the kill immediately
before each bind:

```rust
                    if let Some(key) = key
                        && let Some(subject) = subject_of(walker.context.ir, key)
                    {
                        walker.expression(key, env);
                        // A loop-variable rebind is a value change
                        // (issue #72): fingerprints naming it are
                        // stale from this pass on.
                        env.kill_call_results_for_subject(&subject);
                        env.bind(subject, key_type);
                    }
                    walker.expression(value, env);
                    if let Some(subject) = subject_of(walker.context.ir, value) {
                        env.kill_call_results_for_subject(&subject);
                        env.bind(subject, value_type);
                    }
```

- [ ] **Step 4: Run the test to verify it passes, then the crate suite**

```bash
cargo test --package celerrate_types a_foreach_rebind_kills_the_call_result_narrowing
cargo test --package celerrate_types
```

Expected: PASS, and no other test regresses.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow/walk.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): stale call-result fingerprints die at foreach rebinds (#72)"
```

---

### Task 3: `catch` variable bind kills

**Files:**
- Modify: `crates/celerrate_types/src/flow/walk.rs` (`Try` arm, catch
  variable bind ~276-283)
- Test: `crates/celerrate_types/src/checks/nullability.rs`

**Interfaces:**
- Consumes: `Environment::kill_call_results_involving` (the catch arm
  holds a bare `String` name, like `StaticVariables`).

- [ ] **Step 1: Write the failing verdict test**

```rust
    #[test]
    fn a_catch_bind_kills_the_call_result_narrowing() {
        // The catch arm binds the caught variable directly (issue
        // #72 item 4): the bind is a value change, and the caught
        // class's own `getCommand` answers `?Command`, so the
        // guarded fingerprint from outside the try is stale.
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Oops { public function getCommand(): ?Command { return null; } }
class Event { public function getCommand(): ?Command { return null; } }
function f(Event $e): void {
    if ($e->getCommand()) {
        try {
        } catch (Oops $e) {
            $e->getCommand()->getName();
        }
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "getName".to_owned(),
                receiver: "Command|null".to_owned(),
            }],
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test --package celerrate_types a_catch_bind_kills_the_call_result_narrowing
```

Expected: FAIL — empty verdicts (the stale fingerprint narrows the
catch-body call to `Command`).

- [ ] **Step 3: Kill before the catch bind**

In the `Try` arm:

```rust
                    if let Some(variable) = &catch.variable {
                        // The catch bind is a value change (issue
                        // #72): fingerprints naming the variable are
                        // stale in the arm.
                        arm.kill_call_results_involving(variable);
                        arm.bind(
                            NarrowingSubject::Local {
                                name: variable.clone(),
                            },
                            caught,
                        );
                    }
```

- [ ] **Step 4: Run the test to verify it passes, then the crate suite**

```bash
cargo test --package celerrate_types a_catch_bind_kills_the_call_result_narrowing
cargo test --package celerrate_types
```

Expected: PASS, no regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow/walk.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): stale call-result fingerprints die at catch binds (#72)"
```

---

### Task 4: `use (&$x)` closure capture kills

**Files:**
- Modify: `crates/celerrate_types/src/flow/walk.rs` (`Closure` arm,
  by-reference branch ~1179-1185)
- Test: `crates/celerrate_types/src/checks/nullability.rs`

**Interfaces:**
- Consumes: `Environment::kill_call_results_involving` (the capture
  holds a bare `String` name).

- [ ] **Step 1: Write the failing verdict test**

The killed local must be an *argument* of the fingerprint: the
by-reference branch degrades the captured local itself to `mixed`,
so a base-named fingerprint is unobservable for CEL0034 either way
(global constraint above). `$repo` stays typed; `$id` is captured.

```rust
    #[test]
    fn a_by_reference_closure_capture_kills_the_call_result_narrowing() {
        // `use (&$id)` aliases the local into the closure for as
        // long as the closure lives (issue #72 item 5): any later
        // closure call may rewrite it, so the fingerprint naming it
        // as an argument is stale from the capture on.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function f(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        $fn = function () use (&$id): void {};
        $repo->find($id)->title;
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "title".to_owned(),
                receiver: "Post|null".to_owned(),
            }],
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test --package celerrate_types a_by_reference_closure_capture_kills_the_call_result_narrowing
```

Expected: FAIL — empty verdicts.

- [ ] **Step 3: Kill on the outer environment in the by-reference branch**

```rust
                    if capture.by_reference {
                        // `use (&$x)`: the local is aliased into the
                        // closure's scope for as long as the closure
                        // lives, unknowable without alias analysis —
                        // degrade both sides now (decision 10), and
                        // kill the fingerprints naming it (issue
                        // #72): any later closure call may rewrite
                        // the alias.
                        inner.bind(subject.clone(), TypeId::mixed(db));
                        environment.kill_call_results_involving(&capture.name);
                        environment.bind(subject, TypeId::mixed(db));
                    } else {
```

The by-value branch is untouched — it only reads the outer
environment into `inner`.

- [ ] **Step 4: Run the test to verify it passes, then the crate suite**

```bash
cargo test --package celerrate_types a_by_reference_closure_capture_kills_the_call_result_narrowing
cargo test --package celerrate_types
```

Expected: PASS, no regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow/walk.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): stale call-result fingerprints die at by-reference captures (#72)"
```

---

### Task 5: `extract()` sweeps local-involving fingerprints

**Files:**
- Modify: `crates/celerrate_types/src/narrowing.rs` (new predicate
  after `call_result_involves_local`, ~line 98)
- Modify: `crates/celerrate_types/src/flow/walk.rs` (the `extract`
  sweep in the named-call arm, ~961-971)
- Test: `crates/celerrate_types/src/checks/nullability.rs`

**Interfaces:**
- Produces: `NarrowingSubject::call_result_involves_any_local(&self)
  -> bool`.

- [ ] **Step 1: Write the failing verdict test**

`extract()` wipes every `Local` binding, so a local-based receiver
degrades to `mixed` and is unobservable (global constraint): the
observable shape is a `$this`-based fingerprint whose *argument* is a
local. The `g` function pins the survival side: a fingerprint
involving no local outlives the sweep, because `extract()` cannot
reassign `$this`.

```rust
    #[test]
    fn extract_kills_local_involving_call_results_but_spares_this_based_ones() {
        // `extract()` may rewrite every local (issue #72 item 6):
        // fingerprints whose base or any argument is a local are
        // stale. A fingerprint involving no local — `$this` base,
        // no arguments — survives: `extract()` cannot reassign
        // `$this`.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Holder {
    public function find(int $id): ?Post { return null; }
    public function helper(): ?Post { return null; }
    public function f(int $id, array $data): void {
        if ($this->find($id)) {
            extract($data);
            $this->find($id)->title;
        }
    }
    public function g(array $data): void {
        if ($this->helper()) {
            extract($data);
            $this->helper()->title;
        }
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "title".to_owned(),
                receiver: "Post|null".to_owned(),
            }],
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test --package celerrate_types extract_kills_local_involving_call_results_but_spares_this_based_ones
```

Expected: FAIL — empty verdicts (`f`'s stale fingerprint still
narrows; `g` is silent as it should be).

- [ ] **Step 3: Add the predicate to `NarrowingSubject`**

In `narrowing.rs`, immediately after `call_result_involves_local`:

```rust
    /// Whether this subject is a call-result fingerprint involving
    /// *any* local — its base or any argument. `extract()`'s sweep
    /// predicate (issue #72): it may rewrite every local, so every
    /// local-involving fingerprint is stale, while a fingerprint of
    /// literals and `$this` alone survives.
    pub(crate) fn call_result_involves_any_local(&self) -> bool {
        let NarrowingSubject::CallResult {
            base, arguments, ..
        } = self
        else {
            return false;
        };
        matches!(base, CallBase::Local { .. })
            || arguments
                .iter()
                .any(|argument| matches!(argument.value, ArgumentValue::Local { .. }))
    }
```

- [ ] **Step 4: Extend the `extract` sweep in `walk.rs`**

```rust
                        // `extract()` rewrites every local from its
                        // array argument's keys: an aggressive sweep
                        // on top of the general kill below — locals,
                        // and every call-result fingerprint naming a
                        // local as base or argument (issue #72). A
                        // fingerprint of literals and `$this` alone
                        // survives: `extract()` cannot reassign
                        // `$this`.
                        let name = text.strip_prefix('\\').unwrap_or(text.as_str());
                        if name.eq_ignore_ascii_case("extract") {
                            for subject in environment.subjects() {
                                if matches!(subject, NarrowingSubject::Local { .. })
                                    || subject.call_result_involves_any_local()
                                {
                                    environment.remove(&subject);
                                }
                            }
                        }
```

- [ ] **Step 5: Run the test to verify it passes, then the crate suite**

```bash
cargo test --package celerrate_types extract_kills_local_involving_call_results_but_spares_this_based_ones
cargo test --package celerrate_types
```

Expected: PASS (exactly one verdict, from `f`), no regressions —
in particular `extract_forgets_every_local` in `inference.rs` stays
green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/flow/walk.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): extract() sweeps local-involving call-result fingerprints (#72)"
```

---

### Task 6: `++`/`--` kill their operand's fingerprints

**Files:**
- Modify: `crates/celerrate_types/src/flow/walk.rs` (`Postfix` arm
  ~397-400, `Unary` arm ~393-396)
- Test: `crates/celerrate_types/src/checks/nullability.rs`

**Interfaces:**
- Consumes: `Environment::kill_call_results_for_subject` (Task 1),
  `subject_of`.

- [ ] **Step 1: Write the failing verdict test**

```rust
    #[test]
    fn increments_kill_the_call_result_narrowing() {
        // `++`/`--` mutate their operand (issue #72 item 7): the
        // fingerprint naming `$id` as an argument is stale after
        // either form. Postfix in `f`, prefix in `g`.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function f(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        $id++;
        $repo->find($id)->title;
    }
}
function g(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        --$id;
        $repo->find($id)->title;
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
cargo test --package celerrate_types increments_kill_the_call_result_narrowing
```

Expected: FAIL — empty verdicts.

- [ ] **Step 3: Kill in both arms**

The `Postfix` arm is always `++` or `--` (the parser produces no
other postfix operator): kill unconditionally. The `Unary` arm
covers every prefix operator (`!`, `-`, `+`, `~`, `++`, `--`): kill
only on `PlusPlus`/`MinusMinus`.

```rust
            BodyExpression::Unary { operator, operand } => {
                let operand_type = self.expression(operand, environment);
                // Prefix `++`/`--` mutate their operand (issue #72):
                // fingerprints naming it are stale. The other unary
                // operators read only.
                if matches!(operator, SyntaxKind::PlusPlus | SyntaxKind::MinusMinus)
                    && let Some(subject) = subject_of(self.context.ir, operand)
                {
                    environment.kill_call_results_for_subject(&subject);
                }
                operators::unary_type(db, operator, operand_type)
            }
            BodyExpression::Postfix { operand, .. } => {
                let operand_type = self.expression(operand, environment);
                // Postfix is always `++`/`--`: a mutation (issue
                // #72), so its operand's fingerprints are stale.
                if let Some(subject) = subject_of(self.context.ir, operand) {
                    environment.kill_call_results_for_subject(&subject);
                }
                operators::postfix_type(db, operand_type)
            }
```

- [ ] **Step 4: Run the test to verify it passes, then the crate suite**

```bash
cargo test --package celerrate_types increments_kill_the_call_result_narrowing
cargo test --package celerrate_types
```

Expected: PASS, no regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/flow/walk.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): increments kill their operand's call-result fingerprints (#72)"
```

---

### Task 7: The pinning and debt tests

Three verdict tests that pin **current** behavior — they are expected
to pass as written. If one fails, stop: that is a discovery, not a
step to force green; report it before proceeding.

**Files:**
- Test: `crates/celerrate_types/src/checks/nullability.rs`

**Interfaces:**
- Consumes: `family_verdicts`, `TypedVerdictKind` (existing test
  support).

- [ ] **Step 1: Write the lazy-initialization pinning test (issue point 2)**

```rust
    #[test]
    fn the_lazy_initialization_idiom_reports_by_the_survival_rule() {
        // The purity assumption's one report-producing consequence
        // (issue #72 item 2): under a negative guard, the null
        // binding survives the intervening call — the fingerprint's
        // whole point — so the re-call inside the branch reads the
        // narrowed `null` and reports. PHPStan-parity, corpus-clean,
        // and now a conscious stance: the "can only silence"
        // guarantee is scoped to positive guards.
        let verdicts = family_verdicts(
            r#"<?php
class User { public function getName(): string { return ''; } }
class Holder {
    public function getUser(): ?User { return null; }
    public function authenticate(): void {}
    public function f(): void {
        if ($this->getUser() === null) {
            $this->authenticate();
            $this->getUser()->getName();
        }
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "getName".to_owned(),
                receiver: "null".to_owned(),
            }],
        );
    }
```

- [ ] **Step 2: Write the `CallBase::This` end-to-end test (issue point 10)**

```rust
    #[test]
    fn a_this_based_fingerprint_narrows_end_to_end() {
        // `$this` is never reassignable, so a `CallBase::This`
        // fingerprint needs no kill discipline on its base — covered
        // until now only at the `subject_of` unit level (issue #72
        // item 10). Guarded `f` is silent; unguarded `g` reports.
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Holder {
    public function command(): ?Command { return null; }
    public function f(): void {
        if ($this->command()) {
            $this->command()->getName();
        }
    }
    public function g(): void {
        $this->command()->getName();
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "getName".to_owned(),
                receiver: "Command|null".to_owned(),
            }],
        );
    }
```

- [ ] **Step 3: Write the `unset` kill verdict test (issue point 11)**

```rust
    #[test]
    fn unset_kills_the_call_result_narrowing() {
        // The `Unset` arm's kill, pinned at the verdict level (issue
        // #72 item 11): `unset($id)` is a value change, so the
        // fingerprint naming `$id` is stale and the re-call reads
        // its fresh `?Post`.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function f(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        unset($id);
        $repo->find($id)->title;
    }
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "title".to_owned(),
                receiver: "Post|null".to_owned(),
            }],
        );
    }
```

- [ ] **Step 4: Run the three tests**

```bash
cargo test --package celerrate_types the_lazy_initialization_idiom_reports_by_the_survival_rule
cargo test --package celerrate_types a_this_based_fingerprint_narrows_end_to_end
cargo test --package celerrate_types unset_kills_the_call_result_narrowing
```

Expected: all three PASS. If the lazy-initialization test's receiver
display is not exactly `"null"`, the run's assertion diff shows the
actual display: verify the verdict is still a `NullDereference` on
`getName` (the stance being pinned), fix the expected string to the
actual display, and note the display in the test comment. Any other
failure is a discovery — stop and report.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/checks/nullability.rs
git commit -m "✅ test(types): pin the survival-rule stance and the unswept verdict debt (#72)"
```

---

### Task 8: Documentation — scope the promise, fix the comments, changelog

**Files:**
- Modify: `crates/celerrate_types/src/narrowing.rs` (the `CallResult`
  variant documentation, ~lines 29-34)
- Modify: `crates/celerrate_types/src/flow/walk.rs` (the `Eval` arm
  comment, ~513-514)
- Modify: `CHANGELOG.md` (the `Unreleased` section)

**Interfaces:** none — documentation only.

- [ ] **Step 1: Scope the `CallResult` variant documentation**

Replace the variant's documentation comment:

```rust
    /// The result of `$base->method(stable arguments)` — the
    /// call-result fingerprint (issue #54, design
    /// 2026-07-19-call-result-narrowing). Two occurrences of one
    /// fingerprint denote the same value: the purity assumption,
    /// documented engine semantics. Under a **positive** guard its
    /// unsoundness can only silence the nullability family; under a
    /// negative guard the surviving `null` binding makes the
    /// lazy-initialization idiom report (PHPStan parity, pinned by
    /// `the_lazy_initialization_idiom_reports_by_the_survival_rule`).
    CallResult {
```

- [ ] **Step 2: Extend the `Eval` arm comment**

```rust
                // eval can rewrite every local, every property
                // binding, and every call-result fingerprint:
                // forget them all (decision 10).
```

- [ ] **Step 3: Add the changelog entry**

In `CHANGELOG.md` under `## [Unreleased]` → `### Fixed`, add as the
first bullet:

```markdown
- Call-result narrowing hardening (#72): stale call-result
  fingerprints now die at every value-change site — `foreach`
  key/value rebinds, `catch` binds, by-reference closure captures
  (`use (&$x)`), `extract()` (which now sweeps every fingerprint
  naming a local, sparing pure `$this`-based ones), and `++`/`--`.
  Each missed kill could only silence a genuine possibly-null
  dereference; none could fabricate one. The purity assumption's
  documented guarantee is also corrected: "can only silence, never
  fabricate" holds for positive guards — under a negative guard the
  surviving `null` binding makes the lazy-initialization idiom
  report (PHPStan parity, now pinned by a test).
```

- [ ] **Step 4: Run clippy and the crate tests**

```bash
cargo test --package celerrate_types
cargo clippy --package celerrate_types --all-targets -- -D warnings
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/flow/walk.rs CHANGELOG.md
git commit -m "📝 docs(types): scope the purity guarantee to positive guards (#72)"
```

---

### Task 9: Workspace verification and the corpus gate

**Files:** none created or modified (verification only; a corpus
snapshot change would be a finding, not a commit).

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

- [ ] **Step 3: The Symfony corpus gate**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
```

Expected: the snapshot check passes with **no changes** — the swept
sites only remove narrowings, and removal surfaces reports only on
genuinely unguarded code, which the corpus does not contain. If the
corpus shows new diagnostics, stop: inspect each one — a true
positive newly surfaced contradicts the design's corpus-clean claim
and must be reported before merging.

- [ ] **Step 4: Push and open the pull request**

```bash
git push -u origin fix-72-call-result-hardening
gh pr create --title "🐛 fix(types): call-result narrowing hardening (#72)" --body "Closes #72 (the post-PR-#71 remainder: item 1 shipped in f89591d).

- One kill helper (\`kill_call_results_for_subject\`) replaces the repeated three-line pattern.
- Five value-change sites now kill stale fingerprints: \`foreach\` rebinds, \`catch\` binds, \`use (&\$x)\` captures, \`extract()\` (new any-local sweep predicate, \`\$this\`-pure fingerprints survive), \`++\`/\`--\`.
- The purity guarantee is scoped to positive guards; the lazy-initialization stance, the \`CallBase::This\` end-to-end path, and the \`unset\` kill are pinned by tests.
- Symfony corpus: no new diagnostics.

Design: \`.claude/superpowers/specs/2026-07-19-call-result-hardening-design.md\`
Plan: \`.claude/superpowers/plans/2026-07-19-call-result-hardening.md\`"
```

Expected: pull request created against `main`.
