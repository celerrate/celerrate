# Inference-Warming Enforcement — Design

Date: 2026-07-19
Status: Approved (issue #63)

## Problem

`inferred_body_types_unguarded` (`crates/celerrate_types/src/inference.rs`)
carries no `cycle_fn`: re-entering it on an active claim panics under
salsa's `Panic` strategy. Issue #51 made the reachable callers safe by
routing them through `warm_the_cycle_safe_entry_point`, but the
precondition — warm before you demand — is a module-private convention
plus a rustdoc note. The rustdoc itself names the open edge: a future
external caller passing a `Some` (trait-body) `InferenceContext` must
"extend the warming symmetrically before it ships". In a codebase whose
zero-panic rule is otherwise mechanically enforced, a latent
panic-by-construction defended by prose is a defect.

## A recorded rejection this design honors

Issue #63's preferred direction — give `inferred_body_types` its own
`cycle_fn`/`cycle_initial` so the unguarded variant disappears — was
already evaluated and rejected by plan #51 (fixed decision 1): the
design names the small return projection "the fixpoint's currency"; a
fixpoint over the whole `InferredBody` struct would let mid-iteration
provisional tables become observable to the three check families, and
plan 9a's decision 9 (the live-`never` provisional-mismatch rule in
`validated_stored_return`) reasons about provisionality at the return
level only. That rejection stands. This design therefore takes the
issue's directions 2 and 3 — make the precondition structural — instead
of re-litigating direction 1.

## Design

Two moves, one per exposure surface.

### 1. The public wrapper stops accepting a context (the external edge)

Every caller outside `inference.rs` — `checks/mod.rs`,
`checks/test_support.rs`, `celerrate_cli`'s `mixed_rate.rs` and
`cache/mod.rs`, and the integration tests — passes
`InferenceContext::new(db, None)`, verified by enumeration. The public
`inferred_body_types` therefore loses its `context` parameter and
constructs the `None` context itself, immediately after warming. The
"future external caller passing `Some`" edge stops being a prose warning
and becomes unrepresentable: the public API can no longer express the
call whose warming nobody wrote. When trait-body inference grows an
external consumer, it must add a new deliberate entry point — and the
symmetric warming with it, forced by review rather than remembered by
luck. All external call sites update mechanically (drop the argument).

### 2. The unguarded query moves behind a sealed module (the internal edge)

Inside `inference.rs`, the tracked query moves into a nested private
module (`mod sealed` or equivalent) that exports a single plain-function
entry taking a zero-sized `Warmed` proof token:

- `warm_the_cycle_safe_entry_point` returns `Warmed` — warming is the
  ordinary way to mint the proof.
- A second constructor, `Warmed::from_inside_the_fixpoint()`, exists for
  the two recovery-carrying return queries (function return, method
  return), which are themselves the cycle-safe heads the warming
  completes: they may legally demand the body query mid-fixpoint.

The tracked query itself is not nameable outside the sealed module, so
"call without a proof" is a compile error, not a review catch. The
salsa constraint that motivated the wrapper shape: tracked-query
arguments must be salsa structs, so the token rides on the plain
wrapper function, not on the tracked query's signature.

Honesty about enforcement strength: `Warmed::from_inside_the_fixpoint()`
remains nameable anywhere in `inference.rs`, so the seal converts a
silent omission into a deliberate, greppable act — the same standard the
plugin-boundary seal (#61) meets — rather than into an impossibility.
The rustdoc on the constructor states the only two legal call sites.

## What does not change

- No new fixpoint, no `cycle_fn` on the body query (the plan #51
  rejection above).
- No behavioral change: every existing analysis result, corpus snapshot,
  and mixed-rate baseline must be byte-identical.
- The two return queries keep their existing recovery.

## Testing

- The existing #51 regression suite (fixpoint tests, cache-seeding
  harness) must pass unchanged — it pins the behavior this refactor
  preserves.
- A compile-time check (compile-fail test or a doc-tested negative
  example) pinning that `inferred_body_types_unguarded` is not nameable
  from outside the sealed module, if a `trybuild` harness exists by
  then (issue #67 introduces one); otherwise the visibility is asserted
  by `cargo check` construction and the #67 harness picks it up when it
  lands.
- Corpus gates: `cargo xtask corpus` and `cargo xtask mixed-rate` after
  `cargo xtask fetch-corpus`, zero delta expected.

## Out of scope

- Trait-body (`Some`-context) external inference: explicitly not built;
  this design makes it unrepresentable rather than half-supported.
- Any change to warming's cost or coverage: warming demands the same
  return queries it does today.
