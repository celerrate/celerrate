# The WASM-Level Plugin Interface (Sketch)

Date: 2026-07-15
Status: Internal draft — the acceptance artifact of type-engine plan 4c
Parent: `.claude/superpowers/specs/2026-07-14-type-engine-design.md`,
section 4

Nothing here is implemented in this sub-project. The WASM host ships
with sub-project 6 (framework providers); this sketch exists because
the four boundary cases below are what break naive designs, and they
shape the *native* trait signatures frozen now — the acceptance
property is that every native trait projects onto this sketch without
reshaping.

## 1. The projection model

- Each extension trait projects onto a flat guest export table. The
  native traits are dyn-compatible by design — no generic methods,
  construction through builders, interrogation through query methods —
  so each trait method maps one-to-one onto one guest export.
- Every value crossing the boundary is plain data (strings, integers,
  booleans) or an opaque handle (`TypeId`, `SymbolId`, the call-scoped
  site handles). No borrowed internals, no retained database
  references, no closures.
- Guests construct types through host builder calls and interrogate
  through host query calls; the internal representation never crosses,
  which is what keeps the lattice free to evolve behind the interner.

## 2. Case 1 — guest statelessness

- A guest contribution runs **instance-per-call**: the host
  instantiates the module fresh per call, or resumes it from a
  pristine pre-initialized snapshot, which is observationally the
  same. No guest linear memory survives from one contribution to the
  next.
- **Guest-side memoization is forbidden**, and the reason is
  structural, not stylistic: every host callback a guest makes is a
  salsa read that records a dependency. A guest cache that answers
  from its own memory skips the callback, salsa records no dependency,
  and invalidation silently breaks — the worst failure class this
  engine has, because nothing crashes.
- Cross-call guest state would also make contributions depend on call
  order, which under parallel fan-out is thread timing: it would
  poison the persistent cache and the byte-identical harness at once.
  Instance-per-call makes the guest a pure function by construction
  rather than by review.

## 3. Case 2 — cancellation

- `salsa::Cancelled` cannot unwind through a guest frame: WASM frames
  are not unwind-transparent to the host's panic mechanism.
- The contract: a host callback that observes a pending cancellation
  **converts it to a guest trap**. The trap collapses the guest frame;
  the host catches the trap at the call boundary, recognizes the
  cancellation cause, and **re-raises `Cancelled`** to salsa.
- A guest can therefore never observe, swallow, or outlive a
  cancellation, and `--watch`'s clean-unwind invariant (no provisional
  value served or persisted) holds through guest code unchanged.

## 4. Case 3 — fuel across re-entrancy

- Fuel is accounted **per outermost guest call**. A
  host→guest→host→guest nesting draws every guest instruction from
  the same outermost budget.
- **Host-callback time burns no guest fuel**: the clock stops at the
  boundary in both directions. A guest is charged for its own
  instructions only.
- Consequence, and the reason the rule exists: "budget exceeded" is a
  pure function of the call's input — never of host load, cache
  temperature, or thread timing — so a fuel exhaustion is
  deterministic and reproducible.
- Exhaustion is a trap: the contribution is dropped, the run is
  reported degraded (the parent's crash semantics), and no panic
  surfaces. A provider never controls termination — the same posture
  the fixpoint discipline takes with the iteration budget.

## 5. Case 4 — handle lifetime

- Handles are **call-scoped**: the host-side handle table is created
  when a guest call begins and invalidated when it returns. A guest
  caching a `TypeId` across calls holds nothing.
- Using a stale or forged handle is a trap into the same degradation
  path as fuel exhaustion: contribution dropped, run degraded, no
  panic, no undefined behavior.
- The native tier already honors the shape this forces:
  `AnnotationSite` and `Invocation` are borrowed per call and never
  stored, and `TypeId` values never escape the process (persistence is
  structural, design section 3).

## 6. The v0 host interface families

Enumerated so sub-project 6 extends rather than reshapes:

1. **Type construction** (builders): `mixed`/`null`/`never`, scalar
   and literal types, a class type carrying generic arguments, union,
   intersection, array/list/shape built field by field, a callable
   signature built parameter by parameter, a template reference by
   name.
2. **Type interrogation**: kind probes, nullability, constituent count
   and constituent-at-index, the class name of a class type, the
   generic argument at an index, the signature of a callable.
3. **Argument value access**: argument count, the literal string,
   integer, or boolean value of argument N when it is literal (the
   stdlib provider reads regex sources and `json_decode` flags, not
   just `TypeId`s), spread presence, named-argument lookup by name.
4. **Symbol lookup**: class existence, member existence with kind and
   flags, function existence, claim-key normalization.

## 7. Projection of each native trait

| Native trait (owner) | Guest exports | Host families needed |
|---|---|---|
| `TypeSyntax` (`celerrate_types`) | `can_parse(text) -> bool`, `parse_docblock(site, text) -> annotations`, `parse_type_expression(site, text) -> type?` | construction, symbol lookup |
| `DynamicTypeProvider` (`celerrate_types`) | `claims() -> list`, `return_type(invocation) -> type?`, `by_reference_types(invocation) -> list<(index, handle)>` | all four |
| `VirtualSymbolProvider` (`celerrate_semantics`) | `virtual_members(text) -> list` | none — plain data out |
| `CommentDirectiveProvider` (`celerrate_semantics`) | `directives(kind, text) -> list` | none — plain data out |

Two of the four traits cross the boundary with plain data only — the
cheapest possible guests — and the two type-aware traits need no
signature change to project. That is the acceptance property this
sketch exists to demonstrate.

## 8. Acceptance checklist

- [x] Guest statelessness: instance-per-call fixed; guest-side
      memoization forbidden, with the salsa-dependency reason recorded.
- [x] Cancellation: trap conversion plus host re-raise fixed; a guest
      frame never outlives a `Cancelled`.
- [x] Fuel: per-outermost-call accounting fixed; host callbacks burn
      no guest fuel; exhaustion is a pure function of the input.
- [x] Handle lifetime: call-scoped tables; stale handles trap into the
      degradation path.
- [x] The v0 families are enumerated: type construction, type
      interrogation, argument value access, symbol lookup.
- [x] Every native trait projects onto the sketch without reshaping.
