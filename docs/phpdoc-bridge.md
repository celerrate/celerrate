# The PHPDoc bridge

Celerrate ships one first-party annotation plugin, enabled by
default: `phpdoc-bridge`. It translates the inherited PHPDoc
convention family (standard PHPDoc plus the PHPStan dialect, with
Psalm synonyms) into Celerrate's internal types. This page is the
published form of the tables that govern it; the rustdoc of
`celerrate_phpdoc_bridge` is the authoritative source and this page
mirrors it at each release.

Two ground rules:

- **No docblock diagnostics.** A malformed annotation is silently
  ignored, per construct: one unsupported construct inside a docblock
  never discards the docblock.
- **Loss is a widening, never a guess.** Every construct lowers to a
  lattice value or a documented sound widening (a supertype), so a
  widening can silence a diagnostic but never mis-report working
  code.

## What is read

- **Standard PHPDoc**, complete: `@param`, `@return`, `@var`,
  `@throws`, `@property` (and `-read`/`-write`), `@method`. The last
  two declare virtual members: a member declared by `@property` or
  `@method` counts as existing for the unknown-member diagnostics.
- **The PHPStan dialect**, measured against the test corpus of
  `phpstan/phpdoc-parser` at a pinned version: 225 of 241 pinned
  inputs parse (93 %; the corpus deliberately includes invalid
  inputs, which count as rejected). The statement lives in
  `crates/celerrate_phpdoc_bridge/tests/phpstan_corpus/verdicts.txt`.
- **Psalm tags with PHPStan-coincident semantics are synonyms**,
  fully honored: `@psalm-param`, `@psalm-return`, `@psalm-var`,
  `@psalm-property(-read/-write)`, `@psalm-method`, `@psalm-extends`,
  `@psalm-implements`, `@psalm-use`, `@psalm-template`,
  `@psalm-assert`, `@psalm-assert-if-true`, `@psalm-assert-if-false`.
- **The variance markers** (`@template-covariant`,
  `@template-contravariant`) are honored as templates: the template
  itself is read, the variance marker recognized and dropped.
- **The ignored-divergent bucket**, parsed and ignored without error:
  purity tags (`@psalm-pure`, `@psalm-mutation-free`,
  `@psalm-immutable`, `@psalm-external-mutation-free`), taint
  annotations (`@psalm-taint-source`, `-sink`, `-escape`,
  `-unescape`, `-specialize`, `@psalm-flow`), and the Psalm-specific
  `this` refinements (`@psalm-if-this-is`, `@psalm-this-out`,
  `@psalm-self-out`).

## The conflict table

The dialects coexist on one docblock in real code (`@param` plus
`@psalm-param` plus `@phpstan-param` on one method). For one slot,
the tiers resolve as:

| slot | wins | over | over |
|---|---|---|---|
| return | `@phpstan-return` | `@psalm-return` | `@return` |
| param (per name) | `@phpstan-param` | `@psalm-param` | `@param` |
| var | `@phpstan-var` | `@psalm-var` | `@var` |
| property / method | `@phpstan-` form | `@psalm-` form | bare form |
| ancestor (per written head name) | `@phpstan-` form | `@psalm-` form | bare form |

Within one tier the first *parseable* tag wins; an unparseable tag
never consumes a slot. `@throws` accumulates across tiers instead of
resolving.

An annotation refines the native declaration only when the refinement
provably holds; when it provably fails, the native declaration wins;
a template-typed annotation that cannot be decided refines through
its bound. When a member declares no annotation of its own, the
nearest ancestor's annotation applies, checked against the inheriting
member's native declaration.

## Suppressions

Honored from v0.0.3 on, across **all** diagnostic families, with the
posture "over-suppression, never under-suppression":

| Written form | Comment kind | Directive |
|---|---|---|
| `@phpstan-ignore-line` | any | suppress, current line |
| `@phpstan-ignore-next-line` | any | suppress, next line |
| `@phpstan-ignore <identifiers>` | any | suppress, current and next line |
| `@psalm-suppress <identifiers>` | docblock | suppress, annotated declaration |
| `@psalm-suppress <identifiers>` | line, block | suppress, current and next line |

Written identifiers now filter through a correspondence table: a
directive whose identifiers *all* map narrows to the union of their
mapped Celerrate codes, so suppressing `class.notFound` no longer
extinguishes an unrelated `function.notFound` on the same line. Any
identifier the table does not map (including a foreign tool's newer
identifiers) keeps the pre-correspondence scope-wide fallback, so a
suppression the table cannot yet resolve still honors the user's
intent. `@psalm-suppress all` is explicitly scope-wide, not merely
unmapped. Lookup is exact-case per dialect.

For `@phpstan-ignore` and `@psalm-suppress`, the identifier list must
sit on the same physical line as the tag: parsing reads up to the
end of the tag's own line, so wrapping the list onto a continuation
line of a block comment or docblock silently drops the identifiers
left on that continuation line, still parsing the directive but
honoring fewer identifiers than written. Keep the list on one line,
or repeat the tag on its own line.

## The lowering table

Every parsed construct maps to a lattice value or a documented sound
widening. Transcribed from the rustdoc of
`crates/celerrate_phpdoc_bridge/src/lowering.rs`:

| construct | lowering |
|---|---|
| names: native keywords | the shared keyword table (`AnnotationSite::keyword_type`) |
| `list`, `non-empty-list`, `non-empty-array`, `associative-array` | their builders over `mixed` |
| `non-empty-string`, `numeric-string`, `literal-string` | their builders |
| `class-string[<T>]` | `class_string` (the template argument is never severed) |
| `interface-string[<T>]`, `enum-string[<T>]`, `trait-string[<T>]` | `class_string` (kind refinement: recorded debt) |
| `callable-string` | `non-empty-string` (widening) |
| `lowercase-string`, `uppercase-string` | `string` (widening) |
| `non-falsy-string`, `truthy-string` | `non-empty-string` (widening) |
| `literal-int` | `int` (no literal-int marker: widening) |
| `positive-int`, `negative-int`, `non-negative-int`, `non-positive-int` | `int_range` |
| `int<a, b>` (`min`/`max` open ends) | `int_range`; a non-literal bound widens to `int` |
| `int-mask<...>`, `int-mask-of<...>` | `int` (widening) |
| `array-key` | `int\|string` |
| `scalar` | `bool\|int\|float\|string`; `numeric` | `int\|float\|numeric-string` |
| `double`/`integer`/`boolean` | the PHP aliases |
| `noreturn`, `no-return`, `never-return`, `never-returns` | `never` |
| `non-empty-mixed` | `mixed`; `open-resource`, `closed-resource` | `resource` |
| `pure-callable` | `mixed` (the bare-callable widening); `pure-Closure` | `Closure` |
| `callable-object` | `object` (widening) |
| literals | `int_literal`/`float_literal`/`string_literal` (an unparseable float text widens to `float`) |
| `array<K, V>`, `list<V>`, `iterable<K, V>` and the non-empty forms | their builders; wrong arity widens the slots to their defaults |
| `key-of<T>`, `value-of<T>` | their builders |
| a bare `*` generic argument (the bivariant wildcard) | already rewritten to `Name("mixed")` at the parser (`parser::parse_generic_arguments`); it lowers through the "names: native keywords" row above, never through a dedicated construct here |
| sealed shapes | `shape` (keyless tuple fields number sequentially; identifier keys are string keys) |
| unsealed shapes | the general array (`non_empty_array` when a field is required): key `int\|string`, value = the field-and-tail union (`mixed` for a bare `...`) — widening |
| `object{...}` | `object` (widening) |
| callables (`callable`, `Closure`, purity prefixes) | `callable` (purity and Closure classness drop: widening); callable-scoped template names lower to `mixed` (decision 12) |
| `Foo::BAR`, `Foo::*` | `mixed` (constant and enum-case facts arrive with plans 6-7: recorded debt) |
| `$this` | `static` (design section 3) |
| offset access `T[K]` | `mixed` (widening) |
| conditionals | `conditional` for an in-scope template subject (Task 9); otherwise the undecided branch union (design section 3) |
| a keyword or dialect atom with a spurious `<...>` list | the atom, arguments dropped |
| any other name | a class type, qualified at the declaring site |
