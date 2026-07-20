# Type-Grammar Overlap Oracle and Norm-Subset Tightening — Design

Date: 2026-07-19
Status: Approved (issues #62 and #48)

## The issue's premise, re-examined

Issue #62 describes the PHPStan/Psalm type grammar as "implemented
twice" (`celerrate_types/src/norm.rs` vs the bridge) and asks for one
shared compositional entry point through `AnnotationSite`, on the
grounds that the site exposes only atoms. Two findings from
re-examination against the current code change that framing:

1. **The compositional entry point already exists.** Issue #62 predates
   the plugin-boundary sealing (#61, PR #65). Since then,
   `AnnotationSite::types()` returns `TypeContext<'db>` — roughly
   thirty-seven compositional constructors mirroring the internal
   builders (union, array, list, shape, callable, class, class-string,
   key-of, conditional, ...), proven equivalent to the raw builders by
   an in-crate test. The bridge builds every type through it. The
   "extended, never bypassed" violation the issue cites was real when
   filed and has been repaired by #65: construction goes through the
   seam. What remains bridge-owned is *parsing*, which is exactly the
   bridge's chartered job (design section 4: the bridge is "one plugin
   and one lexer" for the PHPStan/Psalm dialects).
2. **The two parsers parse two different languages, by design.**
   `norm.rs` parses the Celerrate norm (draft spec
   `2026-07-14-celerrate-norm-draft.md`): ranges are `int<1..5>`, `T[]`
   does not exist, one spelling per constructor. The bridge parses the
   PHPStan/Psalm dialects: `int<1, max>`, `T[]`, conditionals, variance
   keywords, the `positive-int`/`scalar`/`int-mask` name table, callable
   templates. A stub-declared `int<1..5>` and a docblock `int<1..5>` are
   *supposed* to behave differently — the second is not valid PHPStan.
   Merging the parsers, or lowering docblocks through the norm parser,
   would either mistranslate dialect spellings or leak the norm through
   the plugin seam — and the norm is deliberately not a public surface
   in v0.1 (design section 4; `norm.rs`'s own rustdoc: "never crosses
   the facade").

What survives of #62 is real and narrower: **the overlap zone** —
unions, intersections, `?T`, generics application, shapes, callables,
projections, `class-string`, keyword atoms — is implemented twice with
no shared oracle, and it has already drifted once: `Foo::BAR` lowers to
an enum-case type via the norm and to `mixed` via the bridge. Silent
semantic divergence in the overlap is exactly the correctness risk the
issue names; the fix is an oracle plus repairs, not a parser merge.

## Design

### 1. A cross-provenance equivalence corpus (the oracle)

A shared table of overlap-zone spellings, each asserted to lower to the
**identical `TypeId`** through both provenances: the norm path (as a
stub refinement) and the bridge path (as a docblock annotation), in one
database. One table, both harnesses iterate it; a spelling that either
side cannot parse is an explicit table entry (`norm: rejects` /
`bridge: rejects`), so deliberate dialect differences are *documented
by the table* rather than invisible to it. Drift in the overlap becomes
a failing test the day it is introduced. The table lives with
`celerrate_types`' tests and covers, at minimum: every keyword atom,
literals, unions/intersections/nullable compositions, `array`/`list`/
`iterable`/`non-empty-*` generics and their single-argument sugars,
`array-key`, shapes (including optional keys), callables (optional and
variadic markers), `class-string` bare and parameterized,
`key-of`/`value-of`, enum-case references, and template references in
scope.

### 2. Repair the known divergence

The bridge's `Foo::BAR` lowering (currently `mixed`) aligns with the
norm's enum-case semantics where the reference names an enum case the
engine can see, through `TypeContext` — extending the seam if the
enum-case constructor is missing from it (the "extended, never
bypassed" rule, applied in its intended direction). Any further
divergences the new corpus surfaces are repaired in the same change,
each on whichever side is wrong against the documented semantics.

**Implementation outcome (narrow branch taken).** The graceful repair
above proved unsound and was not shipped: the bridge has no symbol
table at lowering time, so lowering every `Foo::BAR` to an enum-case
type makes a resolvable non-enum `Foo::BAR` (an ordinary class
constant) fabricate unknown-member (`CEL0030`/`CEL0031`) and
argument-type false positives that `mixed` correctly suppresses (the
norm avoids this only because it lowers curated stub refinements, never
arbitrary user docblocks). Per the plan's sanctioned fallback, the
bridge keeps `mixed`, and the oracle pins the enum-case divergence with
an explicit inequality assertion. Sound unification — deferred,
symbol-aware const-fetch resolution, or enum-ness-checking downstream
consumers — is tracked in #86.

### 3. Tighten the norm parser to its documented subset (#48)

`lower_norm_text` accepts five form families wider than decision 13's
documented v0 subset, all untested: bare `array`/`list`/`iterable`
(and `non-empty-*`), the empty shape `{}`, quoted shape keys,
hyphenated class names lexed by the name-continuation rule, and stacked
`??T`. The norm's design principle is one spelling per constructor;
undocumented accepted spellings are future compatibility debt in a
grammar that intends to freeze publicly in v1.x. Resolution:

- **Tighten to the documented subset**: the five families are rejected
  (`None`) unless the refinement corpus actually uses them — verified
  mechanically, because `celerrate_stubs`' totality test
  (`every_embedded_refinement_text_lowers`) fails if a tightening
  strands an embedded refinement. A stranded spelling is either
  rewritten in `refinements.celerrate` to the documented form, or — if
  the documented subset genuinely lacks the needed form — the subset
  documentation is amended first and the form gains a positive test.
- The three documented conveniences (draft §3.1: `array-key` sugar,
  single-argument `array<V>`, single-argument `iterable<V>`) each gain
  the positive pin they currently lack.
- Rejection tests pin each newly rejected family, alongside the
  existing outside-the-subset rejection test.

The hyphen rule note: name continuation across `-` exists to lex
`non-empty-string` and friends; tightening constrains where a
hyphenated name may *lower* (known hyphenated keywords only), not the
lexer itself.

## Explicitly rejected

- **Wholesale parser unification** (the issue's original fix
  direction): two languages, two parsers; the overlap is guarded by the
  oracle instead. Revisit when the norm freezes publicly and
  `migrate --to-celerrate-types` needs a translation table — that
  sub-project can reuse the equivalence corpus as its ground truth.
- **Exposing the norm parser through `AnnotationSite`**: contradicts
  the v0.1 stance that the norm is not a public surface.

When the implementation lands, the issue receives the reframing as a
closing comment: what #65 already fixed, what this change fixes, and
what is deliberately not done.

## Testing

- The equivalence corpus is the deliverable test; it must fail on the
  enum-case divergence before the repair and pass after (TDD order).
- Norm-subset tightening: rejection and convenience pins as above; the
  stubs totality test guards the corpus impact.
- Corpus gates (`cargo xtask corpus`, `cargo xtask mixed-rate`): the
  enum-case repair may move typed diagnostics on the Symfony corpus; any
  delta is hand-inspected under verify-then-accept, and a mixed-rate
  precision gain is re-blessed per policy. The norm tightening must
  show zero corpus delta (refinement texts are covered by the totality
  test before the corpus ever runs).

## Out of scope

- New grammar capability on either side (no new dialect spellings, no
  norm extensions).
- The norm's public freeze, `migrate --to-celerrate-types`, and any
  norm documentation surface: v1.x-era work.
