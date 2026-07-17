# The Celerrate Norm (Internal Draft)

Date: 2026-07-14
Status: Internal draft. No public documentation, no migration tooling,
no stability promise. Freezes in v1.x, informed by real-world feedback.
Parent spec: `.claude/superpowers/specs/2026-07-14-type-engine-design.md`
(sections 3 and 7).

Amendment history:
- 2026-07-17 (plan 7, task 11 curation): section 5's open questions
  answered from curation; a first-consumer grammar note and the v0
  parser's grammar additions recorded so the draft cannot drift from
  the shipped `refinement_source.rs` parser.

## 1. Purpose and posture

The norm is the annotation syntax Celerrate will natively own, designed
against the lattice so both stay honest (the parent spec's timing
decision). It reads as PHPStan where PHPStan is already clean, and
departs only where the departure buys regularity. First consumer: the
"Celerrate refinements" stub overlay (parent spec section 7, plan 7),
which writes enriched stdlib signatures in the norm and thereby exercises
every spelling this draft claims. The bridge that will eventually read
the norm from real docblocks (`celerrate_phpdoc_bridge`, parent spec
section 5) does not exist yet at the time of writing; the norm's only
consumer today is the mapping table below, checked by hand against the
finished `celerrate_types` lattice (`crates/celerrate_types/src/construction.rs`
and `crates/celerrate_types/src/display.rs`).

**First consumer (the refinements overlay).** The overlay file
`crates/celerrate_stubs/refinements.celerrate` is the norm's first and
only writer today. Its grammar is three shapes: function entries
(`function name<T, U>(type $param): type`, refining any subset of the
base parameters plus the return); class blocks
(`class Name<T, U> implements Ancestor<T, U> { method m(type $p): type }`,
carrying templates, ancestor arguments, and element-bearing methods);
and `of` template bounds (`<T of SomeBound>`, elided to `mixed` when
bare). The authoritative parser is
`crates/celerrate_stubs/src/compiler/refinement_source.rs`; every type
expression it accepts lowers through `lower_norm_text`
(`crates/celerrate_types/src/norm.rs`), gated by the totality test
`every_embedded_refinement_text_lowers`.

## 2. Design rules

1. One spelling per lattice constructor; no synonyms.
2. Every norm expression lowers losslessly to the lattice; nothing in
   the norm exists that the lattice cannot represent.
3. Ranges use `..` (`int<1..>`, `int<..5>`, `int<1..5>`); `min`/`max`
   keywords do not exist in the norm.
4. Nullable shorthand `?T` is `T|null`, usable anywhere a type appears.
5. Shapes drop the `array` prefix: `{id: int, name?: string}`.
6. Lists are `list<T>`; `T[]` does not exist in the norm.

## 3. The mapping table

The table covers every `pub fn` constructor on `TypeId` in
`construction.rs` that builds a new lattice member (functions returning
`Self`). Query and interrogation methods (`is_mixed`, `int_bounds`,
`class_name`, `without_null`, and so on) do not spell new syntax and are
out of scope; `display` is the renderer the "PHPStan equivalent" column
is checked against, not a constructor. All 37 public constructors are
covered by the table below.

| Lattice constructor | Norm | PHPStan equivalent | Divergence |
| --- | --- | --- | --- |
| `mixed` / `never` / `void` / `null` | same | same | none |
| `object` | same | same | none |
| `resource` | same | same | none |
| `bool`, `bool_literal` (`true`, `false`) | same | same | none |
| `int`, `int_literal` (`42`), `int_range` (`int<1..>`, `int<..5>`, `int<1..5>`) | `..` ranges | `int<1, max>` | rule 3 |
| `float`, `float_literal` | same | same | none |
| `string`, `non_empty_string`, `numeric_string`, `literal_string_type`, `string_literal` (`'active'`) | same | same | none |
| `class_string` (`class-string`, `class-string<T>`) | same | same | none |
| `array`, `non_empty_array` (`array<K, V>`, `non-empty-array<K, V>`) | same | same | none |
| `list`, `non_empty_list` (`list<T>`, `non-empty-list<T>`) | same | same | rule 6 |
| `shape` | `{id: int, name?: string}` | `array{id: int, name?: string}` | rule 5 |
| `iterable` (`iterable<K, V>`) | same (desugars to `array<K, V>\|Traversable<K, V>` at construction) | same | none |
| `class` (class and enum types, generic arguments) | `User`, `Collection<User>` | same | none |
| `enum_case` | `Status::Active` | same | none |
| `callable` | `callable(int, string=, bool...): void` | same | none |
| `template` | `T of Foo` (bound elided when `mixed`: bare `T`) | `@template T of Foo` | none |
| `key_of`, `value_of` | `key-of<T>`, `value-of<T>` | same | none |
| `conditional` | `(T is int ? A : B)` | same | none |
| `static_placeholder`, `self_placeholder`, `parent_placeholder` | `static`, `self`, `parent` | same | none |
| `union` (general) | `A\|B` | same | none, except: no subsumption elimination is a lattice semantics decision, not a norm spelling; `int\|int<1, 5>` keeps both constituents in both dialects |
| `intersection` (general) | `A&B` | same | none |
| nullable shorthand (not its own constructor; lowers through `union` with `null`) | `?T` | `T\|null` also accepted by PHPStan | rule 4 |

### 3.1. v0 grammar additions the parser ships (recorded by curation)

Three conveniences the shipped v0 parser (`norm.rs`) accepts that the
mapping table above does not name, recorded here so the draft and the
parser cannot diverge:

- `array-key` keyword: sugar for `int|string`. This is an accepted
  tension with design rule 1 ("no synonyms"), kept because curated
  signatures read it everywhere; it lowers to exactly `int|string`.
- Single-argument `array<V>`: sugar for `array<int|string, V>` (the
  key defaults to the array-key union). `non-empty-array<V>` follows
  the same rule.
- Single-argument `iterable<V>`: sugar for `iterable<mixed, V>` —
  iterable keys are unconstrained (unlike arrays, whose keys are
  `int|string`), so the key default is `mixed`, not the array-key
  union.

## 4. The tag set (sketch, revised by curation)

`@param`, `@return`, `@var`, `@template`, `@extends`, `@implements`,
`@use`: the standard names, unprefixed; the norm is recognized by
context (a Celerrate-flavored expression parses under the norm grammar
first when the bridge gains it, a later sub-project's concern).

State of the lattice at writing time: no tag reader exists yet. The
body IR (`celerrate_semantics::body`, plan `type-engine-1b`) carries
recognized annotation content verbatim (any docblock a tag reader may
consume, plus suppression-directive comments) so that an edit to it
invalidates body consumers correctly, but it does not itself parse
individual tags into structured data. The parent spec's bridge
(`celerrate_phpdoc_bridge`, section 5) is the crate that will eventually
own tag extraction and dialect precedence, and it is unbuilt as of this
draft. The tag list above is therefore aspirational: it names the
PHPStan-standard tags the bridge's own coverage list already commits to
reading (`@param`, `@return`, `@var`, plus the inheritance-time generic
tags `@extends`, `@implements`, `@use`, plus `@template` for type
variables), under the norm's one-spelling-per-construct rule (design
rule 1) applied to tag names rather than type expressions. It excludes
tags with no lattice-constructor counterpart in this plan (`@throws`,
`@property`, `@method`), which belong to the bridge's virtual-member and
exception-tracking machinery, not to the type lattice this draft maps.

## 5. Answers from curation (plan 7)

Curation (plan 7, task 11) exercised the norm across the stub overlay
and answered each of section 5's open questions:

- **Per-version multi-signature form: deferred.** Refinements are
  version-agnostic (decision 6); the base stubs keep the per-version
  deltas. Revisit only if a curated signature ever needs a version
  split, which none has.
- **`?T` inside unions: no extra grammar.** `?` binds tighter than
  `|` and `&`, so `?A|B` is `(A|null)|B` (decision 13, pinned by the
  norm parser's `unions_intersections_and_nullable_compose` test). No
  parenthesization rule is needed for the input grammar.
- **Intersections in shape field types: allowed directly.** A shape
  field value is a full type expression, so
  `{handler: Countable&Traversable}` parses with no special
  parenthesization.
- **Union-arity / depth-cap spelling: none.** The caps are lattice
  widening behavior (`crates/celerrate_types/src/widening.rs`), not
  notation; the norm has nothing to spell for them.
- **Trust-rule cannot-prove syntax: none.** Trust is a judgment on the
  annotation's relation to the native type, not something an annotation
  declares about itself, so it stays invisible to the norm's grammar.
