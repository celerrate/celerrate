# Call-Result Narrowing — Design

Date: 2026-07-19
Status: Approved (brainstorming output for issue #54)
Issue: #54 — CEL0034 silences all nullable call-result receivers

## 1. Problem

The nullability check (CEL0034) silences **every** dereference whose
receiver is a call expression (`checks/nullability.rs`, the
`matches!(receiver, Call)` skip). The silence protects a real
false-positive shape — `if ($e->getCommand() && $e->getCommand()->getName())`,
where the second call is fresh and the narrowing floor cannot see the
guard — but it is broader than that shape: a genuinely unguarded
possibly-null dereference of a call result
(`$repo->find($id)->title` with no guard at all) is not reported either.

The narrowing floor (`narrowing::subject_of`) tracks three subjects,
all stable bases: `Local`, `ThisProperty`, `StaticProperty`. A call
result is not a subject, so guard state on it is invisible, and the
check has no way to distinguish the guarded shape from the unguarded
one. Issue #54's stated fix direction: give `subject_of` call-result
subject tracking, so the blanket silencing can narrow back to the
genuinely untrackable shapes.

## 2. Decisions

Three decisions were made during brainstorming and are fixed here:

1. **Full syntactic identity (the purity assumption).** Two call
   expressions with the same canonical form — same stable base, same
   method (case-folded), same stable arguments — denote the same
   value for narrowing purposes. PHP does not guarantee this
   (`->fetch()` returns a different value per call); the assumption is
   documented engine semantics, the same stance PHPStan and Psalm
   take. Its unsoundness is one-directional for the nullability
   family: removing `null` from a union can only silence a diagnostic
   (a false negative), never produce one. One theoretical cross-family
   exception is named honestly: an `instanceof`-form narrowing on a
   call result changes the receiver's class set, so a purity violation
   could in principle mis-type a later member check. The shape
   requires a method returning differently-classed objects per call
   under an `instanceof` guard on itself — pathological, shared by
   PHPStan, and covered empirically by the corpus gate.
2. **Scope: method calls on stable bases only.** `$base->method(args)`
   where the base is `$this` or a local, and every argument is stable.
   Static calls (`Foo::bar()`), free-function calls (`config('x')`),
   and recursive chains (`$a->b()->c()` as a base) are explicit future
   extensions, each behind its own corpus verification.
3. **Approach: extend the narrowing floor**, not the check.
   `NarrowingSubject` gains a `CallResult` variant; every existing
   condition form (truthiness, `!== null`, `instanceof`, negation with
   early return, `match`) narrows it through the existing machinery
   with no per-form logic. The alternative — a hand-rolled dominance
   analysis inside `nullability.rs` — would re-implement half the
   narrowing floor and turn every missed guard form into a false
   positive.

## 3. Semantics

### The fingerprint

A `CallResult` subject identifies a call by canonical form:

- **Base**: `This` (`$this->m(...)` — `$this` cannot be reassigned in
  PHP) or `Local { name }` (`$e->m(...)`).
- **Method**: the name case-folded (PHP method names are
  case-insensitive; the same folding as `folded_member_key`).
- **Arguments**: an ordered list of stable argument fingerprints —
  literals (by canonical text), `Local { name }`, `This` — each
  carrying its named-argument label when present (`f(a: 1)` and
  `f(1)` are distinct identities).

Anything outside this grammar refuses a fingerprint: a property-rooted
receiver (`$this->repo->find(...)`), an unstable argument (a property
fetch, a nested call, a spread), a null-safe call (`?->` is never a
base nor a subject, consistent with the existing chain rule).

### Lifetime (kill rules)

A `CallResult` binding dies when:

- its base local, or any local named in its arguments, is reassigned,
  captured by reference (`apply_by_reference`), or `unset`;
- the environment is cleared wholesale (`eval`, a `goto` label —
  existing paths).

It **survives intervening calls**. This is a deliberate divergence
from decision 10 (`kill_property_bindings`: any call kills every
non-`Local` binding), and the justification is the purity assumption
itself: decision 10 protects bindings whose validity depends on object
state not having changed; a `CallResult` binding's validity *is* the
assumption that the method keeps answering the same value, so an
intervening call does not undermine it. Killing on intervening calls
would resurrect the false-positive class on routine code
(`if ($e->getCommand()) { $this->log(); $e->getCommand()->x(); }`).
The v1 base restriction makes this coherent: `This` and `Local` bases
are exactly the bases that decision 10 already lets survive calls.

### The check's silence narrows, it does not disappear

`nullability.rs` replaces the blanket `Call`-receiver skip with:
skip only when `subject_of(receiver)` answers `None`. Trackable call
receivers use their recorded (possibly narrowed) type; untrackable
ones — property-rooted bases, unstable arguments — keep today's
silence. The guillotine's rule is preserved in its exact design
formulation: report only what the floor can track, stay silent on
what it cannot.

## 4. Components

- **`narrowing.rs`** (most of the new code):
  `NarrowingSubject::CallResult { base, method, arguments }`, an
  `ArgumentFingerprint` enum, and the `subject_of` extension for the
  `Call { callee: MemberAccess, .. }` shape. Structural `Ord`/`Eq`
  derives — the environment's `BTreeMap` stays deterministic by
  construction.
- **`flow.rs`** (surgical):
  - the `Call` arm consults the environment by fingerprint before
    computing the callee return; a bound type is recorded for that
    expression node (arguments are still walked for their own types
    and effects);
  - the existing kill sweeps extend mechanically: killing or
    rebinding a `Local` also kills every `CallResult` mentioning that
    local in its base or arguments; by-reference invalidation the
    same;
  - `kill_property_bindings` does **not** touch `CallResult`
    bindings (v1 bases are `This`/`Local`, both call-stable).
- **`checks/nullability.rs`**: the blanket skip becomes the
  `subject_of`-is-`None` test described above.

New code concentrates in `narrowing.rs`; `flow.rs` (3,841 lines,
already flagged for extraction by issue #39) receives only the
integration points.

## 5. Data flow

`if ($e->getCommand())` → `subject_of(condition)` answers the
fingerprint → the existing condition machinery binds the narrowed
type. Later, `$e->getCommand()->getName()`: the `Call` arm finds the
binding, records `Command` (non-null), the check stays silent. With no
guard: the fresh `?Command` is recorded, `contains_null` holds, the
check reports — the issue's headline case.

## 6. Determinism and invalidation

No new salsa queries: narrowing remains an intra-body computation
inside the existing inference queries. No impact on the invalidation
boundary, early cutoff, or the persistent cache. No address-based
hashing; all fingerprint data is structural and ordered.

## 7. Testing

TDD throughout; each shape gets a failing test first
(`family_verdicts` fixtures):

- the corpus idiom (`&&` guard + repeated call) stays silent;
- unguarded `$repo->find($id)->title` **reports** (the headline);
- block guard (`if (...) { ... }`), negation + early return,
  `!== null`, `instanceof` forms all narrow;
- different arguments are different fingerprints (`find(1)` vs
  `find(2)` reports);
- named-argument labels distinguish identities;
- base reassignment kills; by-reference capture kills; an intervening
  unrelated call does **not** kill;
- property-rooted receivers stay silent (today's behavior);
- unstable arguments stay silent;
- the mixed-alias shape (`$cmd = $e->getCommand(); if ($cmd) {
  $e->getCommand()->x(); }`) reports — assumed PHPStan parity,
  documented.

Final gates: `cargo test --workspace`, clippy at deny level, the
symfony/demo corpus at **0 diagnostics**, the ground-truth baseline,
and the incremental-equivalence harness.

Contingency: if the corpus reveals genuinely-guarded shapes the
fingerprint cannot see (false positives), the response is to widen the
untrackable silence, never to weaken the gate.

## 8. Out of scope (future extensions)

- Static-call and free-function fingerprints.
- Recursive chains (a narrowed call result as a base).
- Property-rooted receiver bases (would need a kill discipline
  reconciled with decision 10).
- Cross-family consumption of call-result narrowing (the member and
  argument families read narrowed receiver types wherever the
  recorded expression types already feed them; no additional work is
  planned or needed in v1).
