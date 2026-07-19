# Foreach List-Destructuring Kills and Binds — Design

Date: 2026-07-19
Status: Approved (brainstorming output for issue #75)
Issue: https://github.com/celerrate/celerrate/issues/75
Parent design: `.claude/superpowers/specs/2026-07-19-call-result-hardening-design.md`
(issue #72, PR #74)

## 1. Context and scope

Issue #75 is the follow-up debt PR #74's whole-branch review surfaced:
`foreach` **list-destructuring** binds are an unswept value-change site
for call-result fingerprints. In the `Foreach` arm of
`crates/celerrate_types/src/flow/walk.rs`, the value-bind path guards on
`subject_of(value)`, which returns `None` for an `Array` pattern
(`crates/celerrate_types/src/narrowing.rs`, the `_ => None`
fallthrough). The destructured locals of

```php
foreach ($rows as [$id, $x]) { ... }
foreach ($rows as $k => [$id, $x]) { ... }
```

are therefore neither bound nor killed: a call-result fingerprint
naming `$id` from before the loop survives the rebind and keeps
narrowing inside the body, silencing genuine possibly-null dereference
reports (CEL0034). The debt is strictly false-negative-only — a
surviving stale fingerprint can only suppress a report, never fabricate
one — and pre-existing: `foreach` has never tracked destructured loop
variables, so this is not a regression from #72, whose plan enumerated
five other sites.

Out of scope: the fingerprint grammar (unchanged), the `foreach` key
path (already killed and bound since #72), the `by_reference` stance
(decision 12 of the type-engine design: a by-reference value binds like
a plain value, no write-back), and any new diagnostic family.

## 2. The fix: the foreach value bind becomes an assignment

Plain assignment already handles destructuring correctly:
`assign_target` (`crates/celerrate_types/src/flow/assignment.rs`)
recurses into its `Array` arm — explicit and positional keys, element
types computed through `index_type`, kill plus bind on every leaf
target, index-write targets rebinding their base array. The `foreach`
path lacks that recursion, and writing a bespoke one would duplicate
it.

The fix reuses it. In the `Foreach` arm's loop closure:

- Keep the existing `walker.expression(value, env)` call. It records
  the pattern subtree — including pattern keys, which
  `assign_target`'s `Array` arm reads back through `recorded` — the
  same precondition the `Assignment` arm establishes before calling
  into the assignment machinery.
- Replace the `subject_of(value)` guard, the kill, and the bind with a
  single call: `walker.assign_target(value, value_type, env)`.
- Raise `assign_target`'s visibility from private to `pub(super)` so
  the sibling `flow::walk` module can call it.

What this yields, uniformly with plain assignment:

- **Kills.** Every destructured leaf target's call-result fingerprints
  die on each iteration pass, exactly when the value genuinely
  changes — the missing sweep the issue names.
- **Binds.** Every destructured leaf target is bound to its element
  type, derived from the iteration value type by `index_type` (explicit
  keys and positional indexes alike). Today those locals keep their
  stale pre-loop bindings; after the fix they carry the loop's actual
  element types.
- **Coverage for free.** `list($a, $b)` and `[$a, $b]` lower to the
  same `BodyExpression::Array` node
  (`crates/celerrate_semantics/src/body.rs`), so both syntaxes, keyed
  patterns (`['k' => $v]`), arbitrary nesting, and index targets
  (`foreach ($rows as $arr[$i])`) are covered by the existing
  recursion.
- **No behavior change for the plain-variable case.** For a simple
  local, `assign_target` falls through to its default arm: the same
  kill plus bind the `Foreach` arm performs today.

The key path and the `by_reference` handling are untouched.

## 3. The accepted precision gain and the corpus stance

Binding destructured loop variables to their real element types is a
precision improvement beyond the kill the issue asks for. It can
surface **new, genuine** diagnostics on the corpus — code that
dereferences a possibly-null destructured element was invisible while
the local sat at its stale pre-loop type.

Recorded stance (decided during brainstorming): **verify then accept.**
Every Symfony-corpus snapshot delta is inspected by hand. A verified
true positive updates the snapshot and is documented in the pull
request. A single false positive is blocking and gets fixed before
merge — the anti-false-positive policy applied literally. The
fallback of degrading to a kill-only sweep (no type binds) was
considered and rejected: it would leave the destructured locals with
their stale types and add a pattern walk parallel to `assign_target`.

## 4. Tests

Test-first, in `crates/celerrate_types/src/checks/nullability.rs`
unless noted. The verdict tests are shaped to avoid the observability
trap the issue names: the killed local appears as an **argument** of a
call on a still-typed receiver, or the fingerprint's base is rebound to
a typed value — so the lost narrowing is observable as a report, not
as silence.

1. **The headline kill.** A fingerprint established before the loop
   names `$id`; the loop rebinds `$id` through `[$id, $x]`; the body
   dereferences the call result. Today silent; must report CEL0034.
2. **Keyed form.** The same shape through
   `foreach ($rows as $k => [$id, $x])`.
3. **Keyed pattern.** A `['k' => $v]` pattern killing and binding
   `$v`.
4. **Nesting.** A nested pattern (`[[$a], $b]`) reaching the inner
   leaf.
5. **`list()` equivalence.** The classic syntax behaves identically to
   the short syntax (one representative case).
6. **Survival contrast.** A fingerprint naming a local that does not
   appear in the pattern survives the loop — the kill is scoped to the
   pattern's targets, not a blanket sweep.
7. **Element typing.** An inference-level test (alongside the existing
   foreach typing tests) asserting destructured locals carry the
   element types derived from the iteration value type.

## 5. Documentation

- New CHANGELOG entry under Unreleased, Fixed: `foreach`
  list-destructuring now kills stale call-result fingerprints and
  binds destructured locals to their element types.
- The #72 entry stays untouched: its enumeration documents its own
  scope, and the shipped wording lists exactly the sites it swept.
- The "die at every value-change site" phrase exists only in
  `CHANGELOG.md` (verified: no rustdoc carries it), so no code
  documentation needs rescoping.

## 6. Acceptance

- `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo fmt --all --check` clean.
- The Symfony corpus snapshot under the verify-then-accept stance of
  section 3: unchanged, or changed only by hand-verified true
  positives documented in the pull request.
- The issue's acceptance test shape (section 4, test 1) reports the
  previously silenced null dereference.
