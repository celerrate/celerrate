# Call-Result Narrowing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CEL0034 reports unguarded possibly-null call-result dereferences by teaching the narrowing floor call-result fingerprints, narrowing the blanket call-receiver silence to the genuinely untrackable shapes (issue #54).

**Architecture:** `NarrowingSubject` gains a `CallResult` variant (a canonical fingerprint: `$this`/local base, case-folded method, stable arguments). `subject_of` produces it, so every existing condition form narrows it with no per-form logic. `flow.rs`'s method-call arm consults the environment by fingerprint ("environment wins", the same idiom the property-fetch arm already uses); the check's skip becomes "no fingerprint". Spec: `.claude/superpowers/specs/2026-07-19-call-result-narrowing-design.md`.

**Tech Stack:** Rust, salsa (no new queries), the existing `family_verdicts` test fixture.

## Global Constraints

- Zero panic in production code: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`; test modules may locally `#[allow]` (the existing per-module allows already cover the touched test modules).
- TDD: every behavior change starts with a failing test.
- Determinism: no new salsa queries; all fingerprint data is structural and ordered (`BTreeMap`-keyed environment).
- The purity assumption's direction: a missed kill silences (false negative — acceptable); an over-applied kill reports guarded code (false positive — forbidden). Kill only on genuine value changes, never on narrowing refinements (`branch_environments`, `apply_call_assertions`, `bind_inline_variables` are refinements — do NOT kill there).
- English everywhere; commits are gitmoji + Conventional Commits, repository-configured identity, no AI attribution.
- Work happens on branch `fix-54-call-result-narrowing` (already created, carries the spec commit).

---

### Task 1: Fingerprint vocabulary and the `subject_of` extension

**Files:**
- Modify: `crates/celerrate_types/src/narrowing.rs` (types + `subject_of` + tests)
- Modify: `crates/celerrate_types/src/flow.rs:401-422` (`subject_type` exhaustive match gains the new arm)

**Interfaces:**
- Consumes: `BodyExpression::{Call, MemberAccess, Variable, Literal}`, `MemberReference::Named`, `CallArgument { label: Option<String>, spread: bool, value: ExpressionId }` (all from `celerrate_semantics`).
- Produces: `NarrowingSubject::CallResult { base: CallBase, method: String, arguments: Vec<ArgumentFingerprint> }`, `CallBase::{This, Local { name }}`, `ArgumentFingerprint { label: Option<String>, value: ArgumentValue }`, `ArgumentValue::{Literal { text }, Local { name }, This}` — all `pub(crate)`, all deriving `Debug, Clone, PartialEq, Eq, PartialOrd, Ord` (the environment's `BTreeMap` requires `Ord`). Later tasks match on these exact names.

- [ ] **Step 1: Write the failing tests**

In `narrowing.rs`'s existing `mod tests`, using the existing `first_expression` helper (it lowers `<?php function f() { <one statement> }` and returns `(BodyIr, ExpressionId)`):

```rust
    #[test]
    fn call_results_on_stable_bases_fingerprint() {
        use super::{ArgumentFingerprint, ArgumentValue, CallBase};

        let (ir, expression) = first_expression("<?php function f() { $e->getCommand(); }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::CallResult {
                base: CallBase::Local {
                    name: "e".to_owned()
                },
                method: "getcommand".to_owned(),
                arguments: vec![],
            }),
        );

        // `$this` is the most stable base of all (never reassignable).
        let (ir, expression) = first_expression("<?php function f() { $this->user(); }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::CallResult {
                base: CallBase::This,
                method: "user".to_owned(),
                arguments: vec![],
            }),
        );

        // Method names fold case (PHP method names are case-insensitive).
        let (ir, expression) = first_expression("<?php function f() { $e->GetCommand(); }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::CallResult {
                base: CallBase::Local {
                    name: "e".to_owned()
                },
                method: "getcommand".to_owned(),
                arguments: vec![],
            }),
        );

        // Stable arguments: literals by canonical text, locals, `$this`;
        // named-argument labels are part of the identity.
        let (ir, expression) = first_expression("<?php function f() { $r->find(1, name: $n); }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::CallResult {
                base: CallBase::Local {
                    name: "r".to_owned()
                },
                method: "find".to_owned(),
                arguments: vec![
                    ArgumentFingerprint {
                        label: None,
                        value: ArgumentValue::Literal {
                            text: "1".to_owned()
                        },
                    },
                    ArgumentFingerprint {
                        label: Some("name".to_owned()),
                        value: ArgumentValue::Local {
                            name: "n".to_owned()
                        },
                    },
                ],
            }),
        );
    }

    #[test]
    fn unstable_call_shapes_refuse_a_fingerprint() {
        // A property-fetch argument is not stable.
        let (ir, expression) = first_expression("<?php function f() { $r->find($this->id); }");
        assert_eq!(subject_of(&ir, expression), None);
        // A nested call argument is not stable.
        let (ir, expression) = first_expression("<?php function f() { $r->find(g()); }");
        assert_eq!(subject_of(&ir, expression), None);
        // A spread refuses the whole fingerprint.
        let (ir, expression) = first_expression("<?php function f() { $r->find(...$a); }");
        assert_eq!(subject_of(&ir, expression), None);
        // A null-safe call is never a subject (the chain rule owns it).
        let (ir, expression) = first_expression("<?php function f() { $e?->getCommand(); }");
        assert_eq!(subject_of(&ir, expression), None);
        // A property-rooted receiver is not a stable base (v1 scope).
        let (ir, expression) = first_expression("<?php function f() { $this->repo->find(1); }");
        assert_eq!(subject_of(&ir, expression), None);
        // A free-function call is out of scope for v1.
        let (ir, expression) = first_expression("<?php function f() { config('x'); }");
        assert_eq!(subject_of(&ir, expression), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types --lib narrowing::tests::call_results 2>&1 | tail -5`
Expected: compile error (no `CallResult` variant, no `CallBase`) — a compile failure is the red state here.

- [ ] **Step 3: Add the vocabulary and the `subject_of` arm**

In `narrowing.rs`, next to `NarrowingSubject` (after its existing variants, before the closing brace), add the variant; add the two new enums and the struct below the subject enum; extend `subject_of`.

The variant (inside `pub(crate) enum NarrowingSubject`):

```rust
    /// The result of `$base->method(stable arguments)` — the
    /// call-result fingerprint (issue #54, design
    /// 2026-07-19-call-result-narrowing). Two occurrences of one
    /// fingerprint denote the same value: the purity assumption,
    /// documented engine semantics whose unsoundness can only silence
    /// the nullability family, never make it report.
    CallResult {
        base: CallBase,
        method: String,
        arguments: Vec<ArgumentFingerprint>,
    },
```

The supporting types (below the `NarrowingSubject` enum):

```rust
/// The stable base a call-result fingerprint hangs off: `$this`
/// (never reassignable in PHP) or a local. Property-rooted receivers
/// are deliberately excluded in v1 — their kill discipline would have
/// to reconcile with decision 10, and the silence they keep is
/// today's behavior.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CallBase {
    This,
    Local { name: String },
}

/// One argument in a call fingerprint: its named-argument label (part
/// of the identity — `f(a: 1)` and `f(1)` are distinct) and its
/// stable value form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ArgumentFingerprint {
    pub label: Option<String>,
    pub value: ArgumentValue,
}

/// A stable argument value. Anything outside this grammar (a property
/// fetch, a nested call, a spread) refuses the whole fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ArgumentValue {
    /// A literal by its canonical source text. `1` and `0x1` are
    /// distinct fingerprints — a false-negative direction only.
    Literal { text: String },
    Local { name: String },
    This,
}
```

The `subject_of` arm (a new arm in its `match`, before the final `_ => None`):

```rust
        BodyExpression::Call { callee, arguments } => {
            let BodyExpression::MemberAccess {
                receiver,
                member: MemberReference::Named { name },
                null_safe: false,
            } = ir.expression(*callee)?
            else {
                return None;
            };
            let base = match ir.expression(*receiver)? {
                BodyExpression::Variable { name } if name == "this" => CallBase::This,
                BodyExpression::Variable { name } => CallBase::Local { name: name.clone() },
                _ => return None,
            };
            let fingerprints = arguments
                .iter()
                .map(|argument| {
                    if argument.spread {
                        return None;
                    }
                    Some(ArgumentFingerprint {
                        label: argument.label.clone(),
                        value: argument_value(ir, argument.value)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(NarrowingSubject::CallResult {
                base,
                method: name.to_ascii_lowercase(),
                arguments: fingerprints,
            })
        }
```

The argument helper (a free function next to `subject_of`):

```rust
/// The stable fingerprint of one argument value, or `None` when the
/// expression is outside the stable grammar.
fn argument_value(ir: &BodyIr, id: ExpressionId) -> Option<ArgumentValue> {
    match ir.expression(id)? {
        BodyExpression::Literal { text } => Some(ArgumentValue::Literal { text: text.clone() }),
        BodyExpression::Variable { name } if name == "this" => Some(ArgumentValue::This),
        BodyExpression::Variable { name } => Some(ArgumentValue::Local { name: name.clone() }),
        _ => None,
    }
}
```

- [ ] **Step 4: Fix the one exhaustive match the new variant breaks**

`flow.rs:401-422`, `subject_type`'s `match subject` is exhaustive. Add the arm (an unbound call result reads as `mixed` — silence; the environment-first check above the match already serves bound ones):

```rust
            NarrowingSubject::CallResult { .. } => TypeId::mixed(db),
```

The compiler is the completeness check for this step: `cargo build -p celerrate_types` must list no other non-exhaustive site (`kill_property_bindings` and `eval` use non-exhaustive `matches!` guards; their semantics change in Task 2, not here).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types --lib narrowing 2>&1 | tail -3`
Expected: PASS, including the pre-existing narrowing tests.

Then the no-behavior-change guard — the full crate must stay green (bindings may now be created by condition narrowing, but nothing consults them yet, and the nullability skip is still blanket):

Run: `cargo test -p celerrate_types --lib 2>&1 | tail -3`
Expected: PASS (401 tests as of branch time).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/flow.rs
git commit -m "✨ feat(types): call-result fingerprints as narrowing subjects"
```

---

### Task 2: The environment consult, call survival, and the narrowed skip

**Files:**
- Modify: `crates/celerrate_types/src/flow.rs:808-814` (`kill_property_bindings` preserves `CallResult`)
- Modify: `crates/celerrate_types/src/flow.rs:2596-2604` (the method-call branch's final value consults the environment)
- Modify: `crates/celerrate_types/src/checks/nullability.rs:47-63` (the blanket skip becomes fingerprint-aware)
- Test: `crates/celerrate_types/src/checks/nullability.rs` (`mod tests`)

**Interfaces:**
- Consumes: `NarrowingSubject::CallResult` and `subject_of` from Task 1; `Environment::binding`; the existing `family_verdicts` fixture and `TypedVerdictKind::NullDereference { member, receiver }`.
- Produces: the user-visible behavior later tasks pin down. No new names.

- [ ] **Step 1: Rewrite the test that encoded the blanket silence, and add the new shapes**

In `nullability.rs`'s `mod tests`, **replace** the existing test `a_call_result_receiver_is_not_a_tracked_dereference` (it asserts silence for the exact unguarded shape this issue exists to report) with the headline test, and add the guarded and untrackable shapes:

```rust
    #[test]
    fn an_unguarded_call_result_dereference_reports() {
        // Issue #54's headline: a possibly-null call result
        // dereferenced with no guard at all is a real possible-null
        // dereference. The variable receiver in `g` pins that the
        // report shape is the same one variables already get.
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Event { public function getCommand(): ?Command { return null; } }
function f(Event $e): void {
    $e->getCommand()->getName();
}
function g(?Command $c): void {
    $c->getName();
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
                    member: "getName".to_owned(),
                    receiver: "Command|null".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn a_guarded_repeated_call_stays_silent() {
        // The corpus idiom that motivated the original silence: the
        // `&&` guard narrows the fingerprint, the second occurrence
        // consults the binding. The block-guard form with an
        // intervening unrelated call pins the survival rule — the
        // purity assumption is exactly why an intervening call does
        // not kill the binding.
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Event { public function getCommand(): ?Command { return null; } }
function log_something(): void {}
function f(Event $e): void {
    if ($e->getCommand() && $e->getCommand()->getName()) {}
    if ($e->getCommand()) {
        log_something();
        $e->getCommand()->getName();
    }
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn untrackable_call_receivers_keep_todays_silence() {
        // No fingerprint, no report: a property-rooted receiver base
        // and an unstable (property-fetch) argument are outside the
        // v1 grammar, so the guillotine's silence holds for them.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
class Holder { public Repo $repo; public int $id = 1; public function __construct() { $this->repo = new Repo(); } }
function f(Holder $h): void {
    $h->repo->find(1)->title;
}
class Caller {
    public function __construct(private Repo $repo) {}
    public function f(): void {
        $this->repo->find(1)->title;
    }
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }
```

- [ ] **Step 2: Run the tests to verify the red/green split**

Run: `cargo test -p celerrate_types --lib checks::nullability 2>&1 | tail -8`
Expected: `an_unguarded_call_result_dereference_reports` FAILS (silent today — the blanket skip); `untrackable_call_receivers_keep_todays_silence` PASSES (pin); `a_guarded_repeated_call_stays_silent` PASSES (pin — silent today via the blanket skip, must stay silent after through narrowing).

- [ ] **Step 3: Preserve `CallResult` bindings across calls**

`flow.rs:808-814`. Replace the sweep's guard so call-result bindings survive (v1 bases are `This`/`Local`, both call-stable — the purity assumption's survival rule, spec section 3):

```rust
    fn kill_property_bindings(&mut self, environment: &mut Environment<'db>) {
        for subject in environment.subjects() {
            if !matches!(
                subject,
                NarrowingSubject::Local { .. } | NarrowingSubject::CallResult { .. }
            ) {
                environment.remove(&subject);
            }
        }
    }
```

Also update the method's doc comment: after "Locals survive: they are not addressable through arbitrary aliasing the way `$this`/`self::` state is." append: "Call-result fingerprints survive too: their v1 bases are `$this` and locals (both call-stable), and their validity is the purity assumption itself — an intervening call does not undermine 'this method keeps answering the same value' (design 2026-07-19-call-result-narrowing)."

Check the `eval` arm (`flow.rs:2243-2254`) still clears everything: it removes `Local` subjects explicitly and then calls `kill_property_bindings` — with the new guard, `CallResult` bindings would survive `eval`. Fix the `eval` arm to clear wholesale instead of the two sweeps:

```rust
            BodyExpression::Eval { argument } => {
                self.expression(argument, environment);
                // eval can rewrite every local and every property
                // binding: forget them all (decision 10).
                *environment = {
                    let mut cleared = Environment::new();
                    if !environment.reachable() {
                        cleared.mark_unreachable();
                    }
                    cleared
                };
                TypeId::mixed(db)
            }
```

If `Environment` has no `reachable()` accessor, add one next to `mark_unreachable` (`pub(crate) fn reachable(&self) -> bool { self.reachable }`) — or, when one already exists under another name, use it. `Environment::clear()` (which resets `reachable` to `true`) is not equivalent: an unreachable environment must stay unreachable.

- [ ] **Step 4: The environment consult in the method-call branch**

`flow.rs:2596-2604`, the final value of the `MemberAccess` callee branch. Replace:

```rust
                        match &signature {
                            Some(signature) => self.solved_call_result(
                                of,
                                &signature.parameters,
                                &arguments,
                                &argument_types,
                            ),
                            None => of,
                        }
```

with (the "environment wins over the declaration" idiom the property-fetch arm already uses at `flow.rs:2391-2394`):

```rust
                        let computed = match &signature {
                            Some(signature) => self.solved_call_result(
                                of,
                                &signature.parameters,
                                &arguments,
                                &argument_types,
                            ),
                            None => of,
                        };
                        // A narrowed call-result fingerprint: the
                        // environment wins over the fresh return type
                        // (issue #54; the property-fetch arm's idiom).
                        // The binding survived this call's own
                        // `kill_property_bindings` above by the
                        // survival rule.
                        if let Some(subject) = subject_of(self.context.ir, id)
                            && let Some(bound) = environment.binding(&subject)
                        {
                            bound
                        } else {
                            computed
                        }
```

- [ ] **Step 5: Narrow the check's skip**

`nullability.rs:47-63`. Replace the comment and the skip:

```rust
        // A receiver that is itself a call result: when the call has a
        // stable fingerprint (`subject_of` answers `CallResult`), the
        // narrowing floor tracks guard state on it, and the recorded
        // type — narrowed where a guard held — decides below like any
        // other receiver. A call outside the fingerprint grammar
        // (property-rooted base, unstable argument) is one the floor
        // cannot track, so the guillotine stays silent for it (design
        // 2026-07-19-call-result-narrowing; design section 8: an
        // undecidable receiver is never a guess).
        if matches!(
            context.ir.expression(*receiver),
            Some(BodyExpression::Call { .. })
        ) && crate::narrowing::subject_of(context.ir, *receiver).is_none()
        {
            continue;
        }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types --lib checks::nullability 2>&1 | tail -5`
Expected: PASS — all three new tests and every pre-existing nullability test.

Then the whole crate (other families read recorded expression types; a narrowed call-result type must not disturb them):

Run: `cargo test -p celerrate_types --lib 2>&1 | tail -3`
Expected: PASS. Any failure elsewhere is a real interaction — investigate before proceeding, do not weaken the failing test.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types/src/flow.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): narrow CEL0034's call-result silence to untrackable shapes (#54)"
```

---

### Task 3: Value-change kills

**Files:**
- Modify: `crates/celerrate_types/src/narrowing.rs` (the `call_result_involves_local` helper)
- Modify: `crates/celerrate_types/src/flow.rs` (an `Environment` sweep + the value-change sites)
- Test: `crates/celerrate_types/src/checks/nullability.rs` (`mod tests`)

**Interfaces:**
- Consumes: `NarrowingSubject::CallResult`, `CallBase`, `ArgumentValue` from Task 1.
- Produces: `NarrowingSubject::call_result_involves_local(&self, name: &str) -> bool` (narrowing.rs), `Environment::kill_call_results_involving(&mut self, name: &str)` (flow.rs). No later task consumes them; they are internal to the kill rule.

- [ ] **Step 1: Write the failing tests**

In `nullability.rs`'s `mod tests`:

```rust
    #[test]
    fn a_base_value_change_kills_the_call_result_narrowing() {
        // Reassigning the base local makes the fingerprint stale: the
        // second call is on a different object and re-acquires its
        // fresh `?Command`. A by-reference capture is a value change
        // the callee may perform, so it kills too.
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Event { public function getCommand(): ?Command { return null; } }
function mutate(Event &$e): void {}
function f(Event $e, Event $other): void {
    if ($e->getCommand()) {
        $e = $other;
        $e->getCommand()->getName();
    }
}
function g(Event $e): void {
    if ($e->getCommand()) {
        mutate($e);
        $e->getCommand()->getName();
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
                    member: "getName".to_owned(),
                    receiver: "Command|null".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn an_argument_value_change_kills_the_call_result_narrowing() {
        // The killed local appears as an argument, not the base: the
        // fingerprint names `$id`, so reassigning `$id` stales it.
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
function f(Repo $repo, int $id): void {
    if ($repo->find($id)) {
        $id = 2;
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

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types --lib checks::nullability::tests::a_base_value_change 2>&1 | tail -5`
Expected: FAIL — the stale binding still narrows, so the expected reports are absent. Same for the argument test.

- [ ] **Step 3: The involvement predicate**

In `narrowing.rs`, an inherent impl next to the enum:

```rust
impl NarrowingSubject {
    /// Whether this subject is a call-result fingerprint whose value
    /// could change when local `name` is reassigned — its base or any
    /// argument names it. The kill rule's predicate (design
    /// 2026-07-19-call-result-narrowing): killing only on genuine
    /// value changes, because an over-applied kill re-reports guarded
    /// code (a false positive), while a missed kill only silences.
    pub(crate) fn call_result_involves_local(&self, name: &str) -> bool {
        let NarrowingSubject::CallResult {
            base, arguments, ..
        } = self
        else {
            return false;
        };
        matches!(base, CallBase::Local { name: base_name } if base_name == name)
            || arguments.iter().any(|argument| {
                matches!(
                    &argument.value,
                    ArgumentValue::Local { name: argument_name } if argument_name == name
                )
            })
    }
}
```

- [ ] **Step 4: The environment sweep**

In `flow.rs`, on `impl Environment` next to `remove`:

```rust
    /// The call-result kill rule: local `name`'s value changed, so
    /// every fingerprint mentioning it (as base or argument) is
    /// stale. Deterministic: `retain` walks the `BTreeMap` in order.
    pub(crate) fn kill_call_results_involving(&mut self, name: &str) {
        self.bindings
            .retain(|subject, _| !subject.call_result_involves_local(name));
    }
```

- [ ] **Step 5: Call the sweep at every genuine value-change site**

The pattern at sites that hold a `subject` from `subject_of`:

```rust
                if let NarrowingSubject::Local { name } = &subject {
                    environment.kill_call_results_involving(name);
                }
```

Apply it immediately before the `bind`/`remove` at each of these sites (all in `flow.rs`; line numbers as of branch time):

1. `assign_target`, the final `_` arm (`if let Some(subject) = subject_of(...) { environment.bind(subject, value_type); }`) — insert the pattern between the `if let` and the `bind`.
2. `assign_target`, the `Index` arm (`Some(BodyExpression::Index { subject, index })` — the base rebinding) — same insertion before `environment.bind(base, updated)`, matching on `&base`.
3. `assignment`, the `by_reference` arm (~3641-3651) — both binds (target and value subjects).
4. `apply_by_reference` (~845-852) — before the bind.
5. The `Unset` statement arm (~1836-1841) — before the `remove`.
6. The `Global` statement arm (~1814-1818) — before the bind.
7. The `StaticVariables` statement arm (~1826-1832) — the name is direct: `environment.kill_call_results_involving(&variable.name);` before the bind.

Do **not** touch narrowing-refinement binds: `branch_environments`/condition narrowing, `apply_call_assertions`, `bind_inline_variables` (an inline `@var` is a type assertion, not a value change), and the `??` arm's `when_present`/`when_null` binds. Killing there would re-report guarded code — the forbidden direction.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p celerrate_types --lib 2>&1 | tail -3`
Expected: PASS — the two new tests and the whole crate (Task 2's survival test must still pass: an intervening call is not a value change).

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types/src/narrowing.rs crates/celerrate_types/src/flow.rs crates/celerrate_types/src/checks/nullability.rs
git commit -m "🐛 fix(types): invalidate call-result narrowings on base value changes"
```

---

### Task 4: Guard-form coverage pins

**Files:**
- Test: `crates/celerrate_types/src/checks/nullability.rs` (`mod tests`)

**Interfaces:**
- Consumes: everything from Tasks 1-3. Produces: regression pins only — no new code expected; a failure here is a bug in the generic condition machinery's consumption of the new subject.

- [ ] **Step 1: Write the coverage tests**

```rust
    #[test]
    fn every_guard_form_narrows_a_call_result() {
        // The whole point of extending `subject_of` rather than
        // teaching the check about guards: negation with early
        // return, `!== null`, `instanceof`, and case-folded repeats
        // all flow through the existing condition machinery.
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Event { public function getCommand(): ?Command { return null; } }
function a(Event $e): void {
    if (!$e->getCommand()) {
        return;
    }
    $e->getCommand()->getName();
}
function b(Event $e): void {
    if ($e->getCommand() !== null) {
        $e->getCommand()->getName();
    }
}
function c(Event $e): void {
    if ($e->getCommand() instanceof Command) {
        $e->getCommand()->getName();
    }
}
function d(Event $e): void {
    if ($e->getCommand()) {
        $e->GETCOMMAND()->getName();
    }
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn distinct_fingerprints_do_not_share_a_guard() {
        // Different arguments are different identities; a label makes
        // an identity of its own; a guard on a plain variable does not
        // transfer to a fresh call of the expression that produced it
        // (assumed PHPStan parity, recorded in the design).
        let verdicts = family_verdicts(
            r#"<?php
class Post { public string $title = ''; }
class Repo { public function find(int $id): ?Post { return null; } }
class Command { public function getName(): string { return ''; } }
class Event { public function getCommand(): ?Command { return null; } }
function a(Repo $repo): void {
    if ($repo->find(1)) {
        $repo->find(2)->title;
    }
}
function b(Repo $repo): void {
    if ($repo->find(id: 1)) {
        $repo->find(1)->title;
    }
}
function c(Event $e): void {
    $command = $e->getCommand();
    if ($command) {
        $e->getCommand()->getName();
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
                    member: "getName".to_owned(),
                    receiver: "Command|null".to_owned(),
                },
            ],
        );
    }
```

- [ ] **Step 2: Run them**

Run: `cargo test -p celerrate_types --lib checks::nullability 2>&1 | tail -5`
Expected: PASS on every form. If any form fails, the generic condition machinery is not consuming the new subject on that path — use superpowers:systematic-debugging before changing anything; the fix belongs in the condition path, never in the tests.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_types/src/checks/nullability.rs
git commit -m "✅ test(types): pin guard-form coverage for call-result narrowing"
```

---

### Task 5: Gates and the changelog

**Files:**
- Modify: `CHANGELOG.md` (the `### Fixed` list under `## [Unreleased]`)

**Interfaces:**
- Consumes: the finished behavior. Produces: the releasable branch.

- [ ] **Step 1: The full gates**

Run each; every one must pass before the changelog is written:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --features celerrate_stubs/compiler -- -D warnings
cargo fmt --all -- --check
cargo run -q --release --package xtask -- corpus
cargo run -q --release --package xtask -- ground-truth
```

Expected, respectively: all suites green (the incremental-equivalence harness runs inside the workspace suite); no clippy issues; no formatting drift; **"the corpus report matches the committed snapshot"** (symfony/demo stays at 0 diagnostics — the anti-false-positive gate; a corpus failure means the fingerprint missed a genuinely-guarded real-world shape, and the response is widening the untrackable silence, never weakening the gate); "the ground-truth baseline holds".

- [ ] **Step 2: The changelog entry**

Prepend to the `### Fixed` list under `## [Unreleased]` in `CHANGELOG.md`:

```markdown
- Possibly-null dereference (`CEL0034`) now reports on unguarded
  call-result receivers (`$repo->find($id)->title` with no guard),
  instead of silencing every call-result receiver. The narrowing
  floor tracks call-result fingerprints — `$base->method(stable
  arguments)` on a `$this` or local base — so the guards PHP
  routinely writes (`if ($e->getCommand() &&
  $e->getCommand()->getName())`, every condition form) narrow them,
  and the silence shrinks to the genuinely untrackable shapes
  (property-rooted receivers, unstable arguments). Two occurrences of
  one fingerprint are assumed to denote the same value — documented
  engine semantics whose unsoundness can only silence, never
  fabricate a report. Verified on the Symfony corpus (no new
  diagnostics). (#54)
```

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the call-result narrowing fix (#54)"
```
