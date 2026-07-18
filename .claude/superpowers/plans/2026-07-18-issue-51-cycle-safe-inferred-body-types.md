# Issue 51 — Cycle-Safe `inferred_body_types` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close issue #51: a recursive function or method type-checked
before any caller warms its return panics through salsa's
`CycleRecoveryStrategy::Panic`, because `body_typed_verdicts` (and every
other direct caller) reaches `inferred_body_types` without passing
through the cycle-safe `inferred_function_return` /
`inferred_method_return` entry points. After this plan, the panic is
structurally unreachable: the public `inferred_body_types` name becomes
a wrapper that warms the owner's cycle-safe return query first, and the
raw tracked query becomes module-private so no caller inside or outside
the crate can bypass the guard again.

**Architecture:** One file carries the fix
(`crates/celerrate_types/src/inference.rs`): the existing tracked query
is renamed to a module-private `inferred_body_types_unguarded`, and a
plain public function takes over the old name and signature, so not one
call site outside `inference.rs` changes text and every present and
future caller (checks, the mixed-rate instrument, the cache persist
walk, tests) is safe by construction. Warming resolves the body's owner
through the existing `body_owner` projection and demands the matching
return query; the fixpoint completes there (both return queries carry
`cycle_fn`/`cycle_initial`), after which the unguarded call either hits
its memo or recomputes with every recursive edge answered from the
completed memo. The rest of the plan converts the caller-first fixture
workarounds in `cache_seeding.rs` into genuine pins of the fix.

**Tech Stack:** Rust 1.94, salsa 0.27.2 (`cycle_fn`/`cycle_initial`
fixpoint recovery), the existing `checks::tests` fixture harness and
`cache_seeding.rs` integration harness.

**Provenance:** GitHub issue #51; plan 9a's closing memo ("CYCLIC bug —
DEFERRED, HIGH PRIORITY", `.claude/superpowers/plans/2026-07-16-type-engine-9a-cache.md`);
PR #50's "Standing follow-ups". Blocks the v0.0.3 release (plan 9c):
the panic fires on valid recursive PHP, against the project's "no user
input may ever crash the tool" rule.

## Global Constraints

- **Zero panic, mechanically enforced**: Clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
- **TDD**: failing test → minimal implementation → refactor. The
  reproduction tests of task 1 must be observed panicking (RED) before
  the wrapper lands.
- **Strict layering**: the fix stays inside `celerrate_types`; no new
  inter-crate edge (`cargo xtask dependency-shape` must stay clean).
- **Determinism**: no wall-clock, randomness, or environment reads
  inside queries. The wrapper adds only salsa query demands.
- **Error resilience**: no user input may ever crash the tool — the
  invariant this plan restores.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no Claude attribution.

## Fixed decisions (the header the tasks implement)

1. **The fix is entry-point warming, not a `cycle_fn` on
   `inferred_body_types`.** The memo offered both routes. The
   table-level route is rejected: the design names the small return
   projection "the fixpoint's currency" (`inferred_function_return`'s
   own rustdoc), a fixpoint over the whole `InferredBody` struct would
   let mid-iteration provisional tables become observable to the three
   check families, and decision 9 of plan 9a (the live-`never`
   provisional-mismatch rule in `validated_stored_return`) reasons
   about provisionality at the return level only. Warming adds no new
   fixpoint: it routes every entry into an existing one.

2. **Visibility is the enforcement, not convention.** The raw tracked
   query becomes module-private (`fn inferred_body_types_unguarded`,
   no `pub`), so `checks/mod.rs` (same crate, different module) and
   every other caller physically cannot reach it. Only `inference.rs`'s
   own internals — the two return queries, which ARE the cycle-safe
   heads, and the public wrapper — call it. This deliberately goes one
   step beyond the plan-6 "prove the convention by test" precedent
   because here the failure mode is a crash, not an imprecision.

3. **Warming is owner-directed and skips ownerless bodies.** The
   wrapper resolves `body_owner(db, file, body)`:
   - `Function` → demand `inferred_function_return` on the folded
     Function-space key;
   - `Method` with a class key (named and anonymous classes both
     produce `Some` today) → demand `inferred_method_return` on
     `MethodQuery(class_key, folded_member_key(Method, name))`;
   - `Method { class_key: None }` or `None` → no warming. A body the
     member tree does not own cannot be re-entered through the symbol
     index under its own name (a recursive-looking call resolves to
     whatever declaration the index does know, a different body
     identity), so no same-claim cycle exists to guard against.
   The wrapper's warming covers the `InferenceContext::new(db, None)`
   entry every external caller uses today; a future external caller
   passing a `Some` (trait-body) context extends the warming
   symmetrically before it ships — recorded in the wrapper's rustdoc,
   decision-5-of-plan-9a style.

4. **Execution-count adjudications are equally strict, never weaker.**
   The wrapper demands a return query at sites that previously did
   not, so suites pinning exact salsa execution counts
   (`crates/celerrate_types/tests/invalidation_scope.rs`,
   `crates/celerrate_semantics/tests/invalidation_scope.rs`,
   `cache_seeding.rs`'s counter assertions) may shift. Every retarget
   follows the plan-9a task-4 precedent: the new assertion must pin a
   property at least as strong, and the test's comment records the
   adjudication. A count change that cannot be argued equally strict
   escalates to the human instead of landing.

5. **The caller-first fixtures flip to callee-first.** The orderings
   that worked around the bug (`useBoth` first in
   `a_cyclic_cluster_recomputes_and_stays_deterministic`, `useIt`
   first in `a_mutually_recursive_typed_callee_validates_correctly_warm`)
   invert, so both tests would panic on a regression instead of
   silently leaning on the workaround. Their long workaround comments
   rewrite to describe the fix. `a_never_returning_cycle_participant_never_validates_warm`
   changes comments only (its two-file topology never reached the
   hazard; its de-vacuuming was the `Outcome::Clean` assertion, which
   stays).

6. **Corpus and baselines: verify, and re-bless only with a reviewed
   diff.** `symfony/demo` currently passes byte-identically, so no
   corpus body trips the panic path and the expected outcome of every
   gate (`cargo xtask corpus`, `cargo xtask mixed-rate`) is
   byte-identical output. If either diverges, every changed line must
   be explained by the fix (an internal-error render or a missing
   verdict on a recursive body becoming a real answer) before
   re-blessing; an unexplained diff escalates.

## File structure

- Modify: `crates/celerrate_types/src/inference.rs` — the rename, the
  private raw query, the warming helper, the public wrapper (task 1).
- Modify: `crates/celerrate_types/src/checks/mod.rs` — two new
  reproduction tests in the existing `tests` module (task 1).
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs` — one new
  integration pin, two fixture flips, three comment rewrites (task 2).
- Possibly modify: `crates/celerrate_types/tests/invalidation_scope.rs`,
  `crates/celerrate_semantics/tests/invalidation_scope.rs` — only if
  execution counts shift, per fixed decision 4 (task 3).
- Modify: `.claude/superpowers/plans/2026-07-16-type-engine-9a-cache.md`
  — one amendment line on the closing memo's CYCLIC bullet (task 3).

---

### Task 1: The wrapper — reproduction, fix, and unit pins

**Files:**
- Modify: `crates/celerrate_types/src/inference.rs` (the tracked query
  at line ~199, its internal callers inside `inferred_function_return`
  ~line 598 and `inferred_method_return` ~line 700)
- Test: `crates/celerrate_types/src/checks/mod.rs` (the existing
  `mod tests`, which provides `fixture(&[...])`, `handle_of(&fixture, N)`,
  and fields `db`/`files`/`stubs`/`configuration`)

**Interfaces:**
- Consumes: `body_owner(db, file, body) -> &Option<BodyOwner>`
  (`inference.rs:142`), `inferred_function_return(db, files, stubs,
  configuration, FunctionQuery)`, `inferred_method_return(db, files,
  stubs, configuration, MethodQuery)`, `folded_symbol_key`,
  `fully_qualified_name`, `folded_member_key` (already imported at
  `inference.rs:20-21`), `crate::declared::FunctionQuery` (pre-folded
  Function-space key), `MethodQuery` (`inference.rs:619`, pre-folded
  class key + `folded_member_key(Method, name)`).
- Produces: `pub fn inferred_body_types(...)` with the **byte-identical
  public signature and return type** it has today
  (`&'db Option<InferredBody<'db>>`) — every external caller
  (`checks/mod.rs:281`, `checks/mod.rs:364`,
  `checks/test_support.rs:226`, `celerrate_cli/src/mixed_rate.rs:95`,
  `celerrate_cli/src/cache/mod.rs:363` and `:404`) compiles unchanged
  and becomes cycle-safe with no edit.

- [ ] **Step 1: Write the two failing reproduction tests**

In `crates/celerrate_types/src/checks/mod.rs`'s `tests` module, next to
`the_checks_record_the_receivers_they_consult`:

```rust
/// Issue #51's exact reproduction: a self-recursive free function
/// declared with NO caller ahead of it. `typed_file_verdicts` walks it
/// through `body_typed_verdicts`, whose `inferred_body_types` demand
/// used to re-enter its own still-active claim when the body's
/// recursive call resolved back through `inferred_function_return` —
/// salsa's `Panic` strategy, a crash on valid recursive PHP. The
/// public `inferred_body_types` now warms the cycle-safe return query
/// first, so this must answer (no verdicts: the body is clean) rather
/// than panic.
#[test]
fn a_recursive_function_type_checks_without_a_caller() {
    let fixture = fixture(&[r#"<?php
function down(int $n) {
    if ($n <= 0) { return 0; }
    return down($n - 1);
}
"#]);
    let result = typed_file_verdicts(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        handle_of(&fixture, 0),
    );
    assert!(
        result.verdicts.is_empty(),
        "a clean self-recursive function raises no typed verdict: {:?}",
        result.verdicts,
    );
}

/// The method-recursion twin, entering the cycle through
/// `inferred_method_return` instead: `$this->down(...)` inside the
/// method's own body, again with no caller anywhere.
#[test]
fn a_recursive_method_type_checks_without_a_caller() {
    let fixture = fixture(&[r#"<?php
class Walker {
    public function down(int $n) {
        if ($n <= 0) { return 0; }
        return $this->down($n - 1);
    }
}
"#]);
    let result = typed_file_verdicts(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        handle_of(&fixture, 0),
    );
    assert!(
        result.verdicts.is_empty(),
        "a clean self-recursive method raises no typed verdict: {:?}",
        result.verdicts,
    );
}
```

- [ ] **Step 2: Run the tests and observe the panic (RED)**

Run: `cargo test -p celerrate_types a_recursive_function_type_checks_without_a_caller a_recursive_method_type_checks_without_a_caller`
Expected: both FAIL by panicking (salsa cycle panic mentioning the
`Panic` recovery strategy or a dependency-graph cycle), NOT by a failed
assertion. If either test passes here, STOP: the reproduction is wrong
and the plan's premise needs re-verification against the memo's repro.

- [ ] **Step 3: Rename the raw query and drop its visibility**

In `crates/celerrate_types/src/inference.rs`, change the tracked query
at ~line 199 from `pub fn inferred_body_types` to a module-private
name, keeping the `#[salsa::tracked(returns(ref))]` attribute and the
whole body untouched:

```rust
/// The unguarded tracked query behind [`inferred_body_types`]: entering
/// it while its own claim is active panics (salsa's `Panic` strategy —
/// this query carries no `cycle_fn`). Module-private on purpose
/// (issue #51): the only legal callers are the two cycle-safe return
/// queries, which ARE the recovery-carrying heads, and the public
/// wrapper, which warms one of them first. Do not re-export.
#[salsa::tracked(returns(ref))]
#[allow(clippy::too_many_arguments)]
fn inferred_body_types_unguarded<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
    context: InferenceContext<'db>,
) -> Option<InferredBody<'db>> {
    // body unchanged
}
```

Then retarget the internal call sites inside `inferred_function_return`
and `inferred_method_return` (and any other caller inside
`inference.rs` — find them with
`grep -n "inferred_body_types(" crates/celerrate_types/src/inference.rs`)
to `inferred_body_types_unguarded`. They must NOT go through the
wrapper: they are the cycle heads themselves, and warming from inside
them would demand their own query recursively for nothing.

- [ ] **Step 4: Write the warming helper and the public wrapper**

Below the renamed query, in the same file:

```rust
/// Completes any inference fixpoint `body` participates in by demanding
/// its owner's cycle-safe return query — the only entry points carrying
/// `cycle_fn`/`cycle_initial`. Ownerless bodies (and the impossible
/// keyless-method case) need no warming: a body the member tree does
/// not own cannot be re-entered through the symbol index under its own
/// name, so no same-claim cycle exists (issue #51).
fn warm_the_cycle_safe_entry_point<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) {
    match body_owner(db, file, body) {
        Some(BodyOwner::Function(function)) => {
            let key = folded_symbol_key(
                SymbolSpace::Function,
                &fully_qualified_name(&function.namespace, &function.name),
            );
            inferred_function_return(
                db,
                files,
                stubs,
                configuration,
                crate::declared::FunctionQuery::new(db, key),
            );
        }
        Some(BodyOwner::Method {
            class_key: Some(class_key),
            member,
            ..
        }) => {
            inferred_method_return(
                db,
                files,
                stubs,
                configuration,
                MethodQuery::new(
                    db,
                    class_key.clone(),
                    folded_member_key(MemberKind::Method, &member.name),
                ),
            );
        }
        Some(BodyOwner::Method {
            class_key: None, ..
        })
        | None => {}
    }
}

/// The inference of one body: `None` when the identity carries no body
/// in `file`. This public name is a cycle-safe wrapper over the
/// module-private tracked query (issue #51): it first warms the owner's
/// recovery-carrying return query, completing any fixpoint the body
/// participates in, then demands the raw query — which either hits its
/// memo or recomputes with every recursive edge answered from the
/// completed return memo. Either way, salsa's `Panic` strategy is
/// never reachable from here. The warming covers the
/// `InferenceContext::new(db, None)` entry every present caller uses;
/// a future external caller passing a `Some` (trait-body) context
/// extends the warming symmetrically before it ships.
pub fn inferred_body_types<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
    context: InferenceContext<'db>,
) -> &'db Option<InferredBody<'db>> {
    warm_the_cycle_safe_entry_point(db, files, stubs, configuration, file, body);
    inferred_body_types_unguarded(db, files, stubs, configuration, file, body, context)
}
```

Check the exact `MemberKind` import is already in scope (it is used at
`inference.rs`'s `MemberResolution` handling); add it to the
`celerrate_semantics` import list if not.

- [ ] **Step 5: Run the reproduction tests (GREEN)**

Run: `cargo test -p celerrate_types a_recursive_function_type_checks_without_a_caller a_recursive_method_type_checks_without_a_caller`
Expected: PASS, no panic.

- [ ] **Step 6: Run the crate suites**

Run: `cargo test -p celerrate_types && cargo test -p celerrate_cli`
Expected: green. If an execution-count assertion in
`invalidation_scope.rs` or `cache_seeding.rs` fails, do NOT fix it in
this task: record the exact failing assertion and counts in the report
for task 3's adjudication (fixed decision 4), and only then decide with
the human whether to proceed to commit with the failure quarantined or
to fold the adjudication in here. Never weaken an assertion to get to
green.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types/src/inference.rs crates/celerrate_types/src/checks/mod.rs
git commit -m "🐛 fix(types): route every inferred_body_types entry through cycle recovery"
```

---

### Task 2: The integration pins — fixture flips in `cache_seeding.rs`

**Files:**
- Test: `crates/celerrate_cli/tests/cache_seeding.rs`
  (`a_cyclic_cluster_recomputes_and_stays_deterministic` ~line 1200,
  `a_never_returning_cycle_participant_never_validates_warm` ~line 1287,
  `a_mutually_recursive_typed_callee_validates_correctly_warm` ~line 1600)

**Interfaces:**
- Consumes: the file's existing `project`, `run_check`, `Outcome`
  helpers; task 1's fix (this whole task is meaningless before it).
- Produces: nothing new for later tasks; three tests that panic on a
  regression of task 1.

- [ ] **Step 1: Add the CLI-level regression pin**

Next to the two cyclic tests (after
`a_cyclic_cluster_recomputes_and_stays_deterministic`):

```rust
/// Issue #51's reproduction at the CLI boundary: one file, one
/// self-recursive function, NO caller anywhere — the exact topology
/// that used to panic through `body_typed_verdicts`'s direct
/// `inferred_body_types` walk before the cycle-safe wrapper
/// (`celerrate_types::inference`, issue #51) landed. Cold, warm, and
/// fresh must all answer — never an internal error — and agree
/// byte-for-byte.
#[test]
fn a_callerless_recursive_function_never_panics() {
    let source = "<?php
        function down(int $n) {
            if ($n <= 0) { return 0; }
            return down($n - 1);
        }
    ";
    let root = project(&[("recursive.php", source)]);
    let (cold_outcome, cold_output) = run_check(root.path());
    assert_ne!(
        cold_outcome,
        Outcome::InternalError,
        "a callerless recursive function must never crash the typed \
         checks: {cold_output}",
    );

    let (warm_outcome, warm_output) = run_check(root.path());
    assert_ne!(warm_outcome, Outcome::InternalError, "{warm_output}");
    assert_eq!(
        cold_output, warm_output,
        "warm rendering equals cold over the unchanged recursive file",
    );

    let fresh_root = project(&[("recursive.php", source)]);
    let (_, fresh_output) = run_check(fresh_root.path());
    assert_eq!(warm_output, fresh_output, "warm rendering equals fresh");
}
```

This test is expected GREEN immediately (task 1 already fixed the
path); it is a regression pin, stated as such — its RED direction was
observed in task 1's step 2.

- [ ] **Step 2: Flip `a_cyclic_cluster_recomputes_and_stays_deterministic` to callee-first**

Reorder the fixture so the recursive pair comes FIRST — the ordering
that panicked before task 1:

```rust
    let source = "<?php declare(strict_types=1);
        function evenOrOdd($n) {
            if ($n === 0) { return 'even'; }
            return oddOrEven($n - 1);
        }
        function oddOrEven($n) {
            if ($n === 0) { return 'odd'; }
            return evenOrOdd($n - 1);
        }
        function takesInt(int $n): void {}
        function useBoth() { takesInt(evenOrOdd(3)); takesInt(oddOrEven(3)); }
    ";
```

Replace the test's long leading comment (lines ~1201-1242): the first
paragraph (the caller-first workaround rationale) becomes a short
statement that the recursive pair is now deliberately declared FIRST —
the topology that used to panic through the direct `inferred_body_types`
walk before the cycle-safe wrapper (issue #51) — so this test now pins
the fix as well as decision 9's stance. Keep the second paragraph (the
CEL0035 fixture rationale: strict-types, the weak-mode silence, the
non-vacuous assertion order) unchanged — it is still load-bearing.
Assertions stay exactly as they are.

- [ ] **Step 3: Flip `a_mutually_recursive_typed_callee_validates_correctly_warm` to callee-first**

Same inversion: move `function useIt(?Node $node): void { ... }` to
AFTER `findOdd`'s declaration in the fixture string (the `class Node`
declaration stays first; `findEven` and `findOdd` keep their bodies
verbatim). Rewrite the two ordering paragraphs of its doc comment
(lines ~1546-1584): the "useIt is declared FIRST ... load-bearing"
paragraph and the "originally declared its recursive pair BEFORE any
caller ... de-vacuumed with this same caller-first ordering technique"
paragraph collapse into a short paragraph stating the callee-first
ordering is now deliberate, pinning issue #51's fix, and that the
defect the old comment called "a tracked follow-up" is closed by the
`celerrate_types::inference` wrapper. Keep the CEL0034-versus-CEL0035
family rationale and the `validate_typed` asymmetry paragraph
unchanged. Assertions stay exactly as they are.

- [ ] **Step 4: Update `a_never_returning_cycle_participant_never_validates_warm`'s comment**

Fixture and assertions unchanged (its two-file topology never reached
the hazard; `Outcome::Clean` stays the non-vacuous observable). In its
doc comment (lines ~1288-1310), replace the parenthetical "(a
pre-existing `celerrate_types` defect, tracked as a follow-up, NOT
fixed here)" and the surrounding hazard description with one sentence:
the single-file hazard is closed by the cycle-safe
`inferred_body_types` wrapper (issue #51), and the cross-file fan-out
description stays as the reason this fixture never needed it.

- [ ] **Step 5: Run the four tests**

Run: `cargo test -p celerrate_cli --test cache_seeding -- a_callerless_recursive_function_never_panics a_cyclic_cluster_recomputes_and_stays_deterministic a_never_returning_cycle_participant_never_validates_warm a_mutually_recursive_typed_callee_validates_correctly_warm`
Expected: 4 passed.

- [ ] **Step 6: Prove the flip discriminates (temporary revert probe)**

Temporarily stash task 1's `inference.rs` change
(`git stash push crates/celerrate_types/src/inference.rs`), re-run the
step-5 command, and confirm the two flipped tests plus the new pin now
FAIL (panic or `InternalError`), then restore (`git stash pop`) and
re-run to green. Record both outputs in the task report — this is the
RED evidence for fixtures whose tests never went red in sequence.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_cli/tests/cache_seeding.rs
git commit -m "✅ test(cache): the cyclic fixtures pin the cycle-safe walk callee-first"
```

---

### Task 3: Adjudication, corpus verification, and closure

**Files:**
- Possibly modify: `crates/celerrate_types/tests/invalidation_scope.rs`,
  `crates/celerrate_semantics/tests/invalidation_scope.rs`,
  `crates/celerrate_cli/tests/cache_seeding.rs` (execution-count
  assertions only, per fixed decision 4)
- Modify: `.claude/superpowers/plans/2026-07-16-type-engine-9a-cache.md`
  (the closing memo's CYCLIC bullet)

**Interfaces:**
- Consumes: tasks 1 and 2 committed; fixed decisions 4 and 6.
- Produces: the closed issue (via the merge), the amended 9a memo.

- [ ] **Step 1: Run the invalidation and counter suites**

Run: `cargo test -p celerrate_types --test invalidation_scope && cargo test -p celerrate_semantics --test invalidation_scope && cargo test -p celerrate_cli --test cache_seeding`
Expected: green. For every failure: it must be an execution-count
assertion shifted by the wrapper's new return-query demand. Retarget it
per fixed decision 4 (equally strict, adjudication written into the
test's comment, plan-9a task-4 precedent). Any failure that is NOT a
count shift: STOP and escalate — the wrapper changed behavior, which
this plan forbids.

- [ ] **Step 2: Verify the corpus and the instrument baselines**

Run: `cargo xtask corpus && cargo xtask mixed-rate`
Expected: both report a byte-identical match (fixed decision 6:
`symfony/demo` never tripped the panic path, so nothing may change).
On a diff: explain every changed line by the fix's mechanism before
`--bless`, or escalate.

- [ ] **Step 3: Amend the 9a closing memo**

In `.claude/superpowers/plans/2026-07-16-type-engine-9a-cache.md`,
inside the closing memo's "(b) CYCLIC bug" paragraph, append one
sentence at its end:

```markdown
**Amendment (2026-07-18): FIXED by the cycle-safe `inferred_body_types`
wrapper — plan `2026-07-18-issue-51-cycle-safe-inferred-body-types.md`,
issue #51; the two workaround fixtures were flipped back to
callee-first and now pin the fix.**
```

- [ ] **Step 4: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo fmt --all -- --check && cargo deny check && cargo xtask dependency-shape`
Expected: all green, `fmt --check` exit 0, no new inter-crate edges.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "📝 docs(plans): record the issue-51 fix in the 9a closing memo"
```

(If step 1 produced retargets, commit them here too with their own
message: `✅ test(types): retarget the execution counts the cycle-safe warm shifted`.)

- [ ] **Step 6: Closure note**

The branch's PR description carries `Closes #51` so the merge closes
the issue. Release note ownership: plan 9c's CHANGELOG task picks the
fix up from the 9a memo amendment; nothing to write here.

---

## Self-review notes (performed at authoring time)

- **Signature fidelity**: the wrapper's signature and `returns(ref)`
  return type were copied from the query at `inference.rs:199-209`;
  the six external call sites were enumerated by grep and all use the
  public name, so task 1 compiles without touching them. The
  `FunctionQuery`/`MethodQuery` construction idioms are copied verbatim
  from `inference.rs:214-217` and `flow.rs:1445-1449`.
- **Known cost, accepted**: every external `inferred_body_types` call
  now also demands the owner's return query. On the analysis path this
  is work the walk performed anyway (the return projection over the
  same body); on the persist path (`cache/mod.rs`) the demand is
  memoized from the analysis that just ran. The only genuinely new
  work is `body_owner` + a memo lookup per call — noise. Plan 9b's
  benchmark runs after this plan and will price it honestly.
- **Known risk, guarded**: execution-count pins may shift (task 3 step
  1 owns the adjudication; fixed decision 4 forbids silent weakening).
  The plan deliberately quarantines those retargets away from task 1's
  fix commit so the review can reject one without the other.
- **What this plan does not do**: no `cycle_fn` on
  `inferred_body_types` (fixed decision 1's rationale), no trait-body
  (`Some`-context) warming (no external caller exists; rustdoc records
  the symmetric-extension obligation), no touch of decision 9's
  provisional-mismatch rule (the never-cluster test pins it unchanged).
