# Type Engine (Design)

Date: 2026-07-14 (amended 2026-07-14)
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md`
(sections 2, 4, 5, 9, 11)
Predecessor: the Semantic Core sub-project, closed by
`.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md`
and the debt settlement in
`.claude/superpowers/specs/2026-07-14-cache-audit-debt-design.md`

Amendment history:

- 2026-07-14 — amended after a four-lens review (incremental engine,
  type system and PHP semantics, extensibility, product and cross-spec
  consistency): the narrowing floor extended to the forms the
  nullability family is unshippable without (`?->` chain semantics,
  property narrowing, `??`/`isset()`/negation, iteration typing); the
  typed-verdict persistent-cache design pulled forward from closure
  into this document (two artifact classes, recursive revalidation);
  fixpoint discipline made explicit (monotone ascent, an iteration
  budget below salsa's panic cap, deterministic widening to `mixed`);
  template variables promoted to lattice citizens with a three-valued
  trust judgment; a fifth extension point for comment directives
  (suppressions); the multi-implementation dispatch model fixed; the
  PHPStan dialect coverage redefined against a pinned reference; the
  argument-types family given a per-file coercion posture; a minimum
  shippable set for the anti-false-positive guillotine; the plan
  sequence pre-split into fourteen plans; and the dropped
  `source_symbol_table` debt re-homed.

## 1. Goal and scope

Turn the incremental engine into a type-aware engine: member semantics
(inheritance, receiver resolution), the type lattice, declared types
(native declarations, stub signatures, docblock annotations through the
bridge), interprocedural inference, the native plugin API, and the
three diagnostic families of the parent spec's v0.1 criterion. The
sub-project closes with a **public `v0.0.x` preview**: unknown members,
nullability, and argument types rendered by the existing text renderer,
the Symfony corpus with no visible false positive, and the incremental
benchmark re-run and re-published.

**Relationship to v0.1, stated exactly.** The parent criterion names
"unknown symbols, nullability, argument types". Top-level unknown
symbols shipped with the semantic core's preview; the unknown-members
family here completes that criterion family; nullability and argument
types are the other two. After this preview, sub-project 5 owes the
v0.1 criterion **no new diagnostics** — only the product surface
(configuration, baseline, output formats, migration) and the
matched-scope PHPStan comparison. This preview is a second public
milestone the parent's sequencing did not name; the parent spec
receives an amendment entry recording it.

**The minimum shippable set.** Each family passes the
anti-false-positive guillotine independently (section 8). The preview
ships as soon as the unknown-members family passes; a guillotined
family becomes a **named v0.1 blocker explicitly inherited by
sub-project 5**, never an orphaned debt. Two families cut still ship
one; the release notes say honestly what shipped and what did not.

This is the umbrella design for sub-project 3, the riskiest of the
project, which is exactly why the semantic core shipped a preview
before it began. It fixes the cross-cutting decisions the parent spec
delegated here (the body lowering question, the inference model, the
bridge's dialect scope, the check families' conservative stances, the
typed-artifact cache design, the part sequencing); each part then gets
its own TDD implementation plan.

New crates, both planned by the parent spec's layout:
`celerrate_types` (lattice and inference) and `celerrate_plugin` (the
API facade). One new first-party plugin crate: `celerrate_phpdoc_bridge`
(section 5). Extended crates: `celerrate_semantics` (the member
boundary), `celerrate_stubs` (the reserved signature payload),
`celerrate_cli` (composition, plugin registration).

Ordering principle (the approach decision of this design): **declared
types before inference**. The member boundary and the lattice come
first, then the three declared-type sources (native declarations, stub
signature deltas, the bridge), and only then inference — so the riskiest
component is built against ground truth: every inferred type can be
confronted with the corpus's existing annotations, which is the
validation harness inference needs. This is also the order in which the
plugin API meets its first real consumer early, validating the
dependency inversion before inference leans on it. The ordering is
load-bearing for performance too: declared returns cut the
interprocedural invalidation edge on an annotated corpus, which is the
single biggest reason the warm one-edit criterion (section 9) is
plausible. That assumption is measured early, not discovered at
closure: the residual — how many verdicts depend on *inferred* returns
— is instrumented as soon as inference exists.

Out of scope (deliberately):

- Generic mismatch diagnostics and full variance. Generics are
  **inference-only** in this sub-project (section 6): resolved and
  propagated, never reported.
- The divergent Psalm semantics (assertion tags with Psalm-specific
  meaning, purity tags, taint annotations): parsed and ignored without
  error, traced as debt toward a later complement (section 5).
- The rule framework and rich rendering (annotated spans, notes,
  suggestions, `celerrate explain`): sub-project 4.
- `celerrate.toml`, baseline, output formats,
  `migrate --from-phpstan`: sub-project 5. Consequence, stated as an
  accepted preview risk: there is **no user-side off-switch** for the
  new families and no demotion tier yet — a confirmed field false
  positive is answerable only by inline suppression or a patch
  release. The guillotine before release is the compensating control.
- Framework providers (Eloquent, facades, the Symfony container):
  sub-project 6. Laravel stays out of the measured corpus.
- The WASM plugin host and the declarative plugin tier. The WASM-level
  interface **sketch** is in scope as an acceptance artifact
  (section 4); the host is not.
- Publication or freeze of the Celerrate norm. The norm is an internal
  draft (section 7), per the parent spec's timing decision.
- Call-site-sensitive parameter inference: unannotated parameters are
  monovariant `mixed`, the parent spec's decision, reconducted.
- Verifying that a body honors its declared return type. Declared
  types are trusted (section 6); the "invalid return" family is future
  work, not one of the three shipped families.

## 2. The member boundary (`celerrate_semantics`)

The invalidation boundary extends to members, keeping the semantic
core's principle: stable identity, `Eq`-comparable projection, early
cutoff.

**The member `ItemTree`.** The shared traversal now descends into
member lists: methods, properties, class constants, and enum cases,
each carrying kind, name, `AstId`, flags (static, visibility, abstract,
final, readonly), its **signature as unresolved names** (parameter
types, return type, property type), and its **docblock text** — the
bridge reads docblocks from this projection, never from syntax trees,
or every keystroke would invalidate every signature. The cost is
accepted and stated: an edit inside a member's docblock defeats the
comment-only cutoff for that member, by design; a second-stage cutoff
at the parsed-annotation level (section 5) catches prose-only docblock
edits. Default values are reduced to their comparable form, defined as
**exactly the projection typed judgments read** (implicit nullability
from `= null`, const-expression structure feeding literal types) — an
invalidation-scope test pins that editing a default value invalidates
that signature's dependents. Editing a method body produces an
identical member `ItemTree` and salsa backdates it: no dependent of the
signature re-runs. Editing one signature invalidates that signature's
dependents, not the other members'.

Anonymous classes receive a synthetic identity anchored on their
`AstId` (closing the semantic core's narrowing). The `AstId` numbering
counts declaration nodes only, so statement edits still renumber
nothing; adding or removing an anonymous class inside a body does
renumber later declarations in the file — a rare edit class, accepted
and recorded. The names inside trait adaptation bodies (`insteadof`,
`as`) are now collected and resolved, closing the recorded
false-negative gap.

**Bodies lower to a body IR.** The parent spec left the desugaring
question open; this design fixes it: every function body is lowered by
a dedicated per-body query into a compact arena representation
(expressions and patterns densely numbered, syntactic sugar reduced),
owned by `celerrate_semantics`. The IR is **range-free**: no text
offsets in the IR or in inference's type tables, or any edit above a
body would shift offsets, break `Eq`, and structurally kill the
advertised early cutoffs. A separate body source-map query (free to
change on every edit, reparsing LRU-evicted trees on demand)
reconciles arena indices to `TextRange`s at rendering time — the same
late-reconciliation split the `ItemTree`/`AstIdMap` pair already uses.
The IR's content is **code plus recognized annotation content**
(inline `@var`, assertion tags, suppression directives): the
"comment-only edit" class is redefined as *trivia no annotation reader
consumes*. Lowered sugar explicitly includes the null-safe operator
`?->` with its **whole-chain short-circuit structure** (PHP semantics:
one null receiver short-circuits the entire chain, not one link) and
first-class callable syntax `$obj->method(...)` (lowered to a callable
signature extracted from the resolved member). Three reasons for the
IR: inference never touches a syntax tree (LRU eviction safety is
preserved structurally, not by discipline), an ignorable-trivia edit
inside a body produces an identical IR (early cutoff at the body
level), and the arena provides dense indices for inference's type
tables. This is the rust-analyzer `hir-def` pattern, transposed.

**Inheritance linearization.** A per-class query resolves `extends`,
`implements`, and trait `use` (with `insteadof` and `as`) into a
linearized member table: own members over trait members over inherited
members, with PHP's visibility and case rules (method names
case-insensitive, property and constant names case-sensitive). The
mechanism honors the semantic core's cycle posture structurally: the
query is an **iterative walk with a visited set**, reading ancestors'
member `ItemTree`s directly — it never demands its own kind
recursively, so `class A extends B; class B extends A` is a detected,
deterministically broken condition with a diagnostic, not a salsa
cycle. Generic arguments fixed at inheritance
(`@extends ServiceEntityRepository<User>`, `@implements`, `@use`) are
**threaded through linearization** — the Doctrine-on-Symfony
repository pattern flows through this, and without it the generics of
section 6 never reach `$repository->find($id)`. Member lookups go
through per-(class, name, kind) queries, never through "the whole
table": adding a member invalidates only the files that looked it up.

**Magic and dynamic suppression, scoped per kind.** `__get`/`__set`
suppress unknown-property diagnostics, `__call` unknown-method,
`__callStatic` unknown-static-method — directly or by inheritance;
blanket suppression would discard real method diagnostics on every
class with `__get` for no reason. `stdClass` and classes carrying
`#[AllowDynamicProperties]` suppress unknown-property diagnostics the
same way (`json_decode` alone makes this mandatory on any real
corpus). Dynamic type providers progressively restore precision later.

## 3. The lattice and declared types (`celerrate_types`, `celerrate_stubs`)

**The representation.** `celerrate_types` owns the lattice: scalars and
their literals (`'active'`, `42`), unions, intersections,
`array<K, V>`, lists and array shapes (`array{id: int}`), the
non-empty variants, enums and their cases, callable signatures, class
types carrying generic arguments, **type variables with their bounds**
(template types are lattice citizens: a signature mentioning `T of
FormTypeInterface` exists as a lattice value before any call-site
substitution), `class-string` and `class-string<T>` (the primary
template binder — lowering it to `string` would sever template
solving), the string subtypes the PHPStan dialect carries
(`non-empty-string`, `numeric-string`, `literal-string`), integer
ranges (`int<1, max>`, `positive-int`), `key-of`/`value-of`,
conditional return types (evaluated at the call site, falling back to
the branch union when the condition is undecided), `iterable<K, V>`
(desugared to `array<K, V>|Traversable<K, V>`), the
late-static-binding placeholders (`static`, `self`, `parent`; `@return
$this` collapses into `static` — sound for the three families, stated
so the bridge is deterministic), and the extremes (`mixed`, `never`,
`void`, `null`).

Types are **interned in canonical form** with an opaque `TypeId`:
cheap `Eq`/`Hash` for early cutoff, and the representation is never
exposed as a matchable enum — plugins construct through builders and
interrogate through query methods, the parent spec's commitment. Two
determinism invariants are fixed here because the byte-for-byte
harness depends on them: **canonical ordering is structural** (by
name and shape), never by interner handle — interning order is
timing-dependent under parallel fan-out — and `TypeId` values never
escape the process (persistence uses a structural serialization,
section 9).

**Widening is defined here, not in inference.** Literal-to-general
widening, union arity caps, and a **general structural depth cap**
covering array shapes, generic-argument nesting, and callable-signature
nesting alike (an unbounded `Collection<Collection<...>>` chain must
hit a cap no matter which constructor grew it): deterministic lattice
operations, because fixpoint termination (section 6) depends on them
and they must be identical regardless of a cycle's entry point. An
arity cap **collapses to a deterministic join** (the common supertype,
`mixed` at worst); it never truncates a subset, which would make the
widened value depend on accumulation order. Judgments (assignability,
nullability, subtyping) are salsa queries.

**The subtype judgment is three-valued** — holds, fails, cannot-prove
— because template variables force it: `T of Foo <: Foo` holds
definitionally through the bound; a judgment involving an unbound
variable that cannot be decided answers cannot-prove, and every
consumer states its posture toward that verdict. No consumer treats
cannot-prove as silent discard.

**Native declared types.** The member `ItemTree` signatures (unresolved
names) resolve into lattice types through per-member queries: the
shortest path from the member boundary to a useful judgment.

**The stub signature payload.** The stub compiler extends into the
reserved extension point: parameters, returns, and property types as
**per-version deltas** (signature changed in 8.3, parameter added in
8.2, and so on). At consultation the parent spec's range rule applies:
arguments are checked against the **intersection** of the signature
across `[min, max]`, the call's return type is the **union**. The
degenerate case is guarded: a parameter whose types across the range
are disjoint has an empty intersection, and an empty intersection
**silences the check for that parameter** rather than weaponizing
`never` against every call. The blob takes a schema version bump (the
format is version-stamped precisely for this); the header field pinned
by the cache-audit tests evolves with it.

**Source precedence.** For one signature element: docblock annotation
(through the bridge) over native declaration over nothing (`mixed`). An
annotation **refines** the native declaration under the three-valued
judgment: *holds* refines, *fails* is ignored and the native
declaration wins, *cannot-prove* (template types, principally) refines
through the bound and is traced — never a crash, never a silent
widening, and never a silently dropped template annotation, because the
ground-truth harness (section 10) cannot see annotations discarded
before it runs.

**Declared types inherit.** Symfony puts its types on interfaces;
implementations rarely repeat them. When a member declares no
annotation of its own, the nearest ancestor's annotation along the
section 2 linearization applies, checked by the trust rule against the
*inheriting* member's native declaration. Without this, every concrete
normalizer in the corpus sees native-only types.

## 4. The plugin API (`celerrate_plugin`)

**Ownership, and where the registries live.** Extension points stay
owned by their consuming layers, as the parent spec requires:
`celerrate_types` owns the type-syntax trait (understand an annotation
notation) and the dynamic-type-provider trait (compute a return type),
`celerrate_semantics` owns the virtual-symbol trait and the
comment-directive trait (below). The **registry salsa inputs holding
the implementations live in the owning crates too** — this is the
load-bearing arrangement: if the registries lived in
`celerrate_plugin`, the owning crates would read a `celerrate_plugin`
type and the DAG would break upward. `celerrate_plugin` is the
aggregation facade re-exporting the stable surface (including the span
and diagnostic vocabulary plugins need — `celerrate_source` and
`celerrate_diagnostics` types are re-exported, so plugin crates still
declare exactly one dependency); implementations are constructed and
set at the composition root (`celerrate_cli`). One clarification the
parent's wording needs: the parent assigns the *stub-provider* trait
to `celerrate_semantics`, while the designed overlay-merge point sits
in `celerrate_stubs`, below it — the resolution is that
plugin-contributed stubs merge **at the symbol-index level in
`celerrate_semantics`**, not inside `celerrate_stubs`; the blob's
merge point serves the build-time refinements overlay only.

Four salsa facts are pinned so the inversion is implementable as
specified: a plugin's implementation travels **in the same input
struct as its identity** (name, version, configuration), so reading
the implementation records a dependency that both the persistent
cache's plugin-set key and an upgrade invalidate; trait objects have
no `Eq`, so these inputs never backdate — acceptable because they are
set once per process; the extension traits carry the supertraits salsa
input fields require and stay **object-safe** (no generic methods —
builders and query methods are designed dyn-compatible from the
start); and the plugin registry sits in the **high-durability tier**
next to stubs and configuration, or every user-code edit would walk
it.

**Dispatch is fixed now, not when sub-project 6 forces it.** Two rules,
cheap to specify today and breaking changes tomorrow: **dynamic type
providers claim symbols** — a provider declares the (fully qualified)
callees it answers for; overlapping claims are a registration-time
error unless resolved by documented precedence at the composition
root — and **type-syntax implementations are consulted in registered
order with a can-parse protocol, first win**; registration order is
declared at the composition root and therefore deterministic. Both
rules keep contribution order independent of thread timing.

**The fifth extension point: comment directives.** The four inherited
families (type syntax, dynamic type providers, virtual symbols,
discovery) offer no channel for "extinguish diagnostics here", so the
bridge could not implement suppressions without bypassing the API —
violating this section's own non-negotiable. `celerrate_semantics`
owns a comment-directive trait: a per-file query collects comment
trivia (an own-tree read for strictly-local output, the precedent
syntax gating set), hands each comment to the registered providers,
and providers return **structured directives** (suppress: span scope,
optional foreign identifier). The composition root applies the
directive filter to rendered diagnostics. The
PHPStan/Psalm-to-Celerrate mapping table is bridge-internal, like the
tag precedence table; the directive vocabulary belongs to the trait.

**WASM-projectable from day 1.** Opaque handles (`TypeId`, `SymbolId`),
construction through builders, interrogation through a narrow host
interface, no borrowed internals, no retained database references, no
closures. The **WASM-level interface sketch is an acceptance artifact**
of this sub-project even though the host ships later, and it has an
acceptance checklist, because these four cases are what break naive
designs and they shape the *native* trait signatures:

1. **Guest statelessness.** Cross-call guest state makes contributions
   order-dependent under parallel fan-out, poisoning the cache and the
   byte-identical harness: the sketch mandates instance-per-call (or
   fresh-from-snapshot) semantics, and guest-side memoization is
   forbidden — a guest cache skips host callbacks, so salsa records no
   dependency and invalidation silently breaks.
2. **Cancellation.** `salsa::Cancelled` cannot unwind through a guest
   frame: a cancelled host callback converts to a trap, and the host
   re-raises `Cancelled` after the guest frame collapses.
3. **Fuel across re-entrancy.** Host→guest→host→guest nesting needs a
   fixed accounting rule (fuel is per outermost call; host-callback
   time burns no guest fuel) or "budget exceeded is a pure function of
   the input" stops being true.
4. **Handle lifetime.** Handles are **call-scoped** (the host-side
   handle table is invalidated per call); a guest caching a `TypeId`
   across calls holds nothing.

The host interface's v0 families are enumerated so sub-project 6
extends rather than reshapes: type construction, type interrogation,
**argument value access** (the stdlib provider already needs literal
flags and regex strings, not just `TypeId`s), and symbol lookup.

**Determinism and versioning, honestly.** Native trait signatures
cannot express purity: the native tier's determinism is a review
guarantee (first-party only in this sub-project), the WASM tier's will
be sandbox-enforced, and the byte-identical harness at varying thread
counts is the only mechanical detector for either. The API carries a
single explicit version checked by the composition-root registry at
registration; a mismatch excludes the plugin for the whole run with
the run reported degraded (the parent's crash semantics). For
compiled-in first-party plugins the check cannot fail — it is dormant
scaffolding whose first real exercise is the WASM host, and the spec
says so rather than claiming it verifies anything today. The API
version is distinct from the plugin version inside the identity input;
only the latter keys the cache. The API is not called v1: its second
*dissimilar* consumer (a framework dynamic type provider) is
sub-project 6 — the stdlib provider (section 7) is a second consumer
but a same-shaped one, and does not satisfy the parent's v1 gate.

**The mechanical constraint.** The bridge and the stdlib type provider
depend **only** on `celerrate_plugin`, enforced in CI by an xtask over
`cargo metadata` (the workspace's existing xtask pattern; nothing
currently checks dependency shape). An extension point that proves
insufficient is extended, never bypassed.

## 5. The bridge (`celerrate_phpdoc_bridge`)

One first-party plugin, enabled by default, public name
`phpdoc-bridge`: it translates the inherited PHPDoc convention family
into internal types. **One plugin, one docblock lexer, two explicit
semantic dialects as internal modules** (`dialect/phpstan`,
`dialect/psalm`), per the parent spec: the dialects coexist on the same
docblock in real code (`@param` plus `@psalm-param` plus
`@phpstan-param` on one method), so the inter-dialect precedence table
is inherently coupled and belongs inside one owner — splitting it into
two plugins would move the hard part into an unowned seam. The name
says what it reads: the legacy PHPDoc family, as opposed to the
Celerrate norm, which will also live in docblocks.

Coverage:

- **Standard PHPDoc**, complete: `@param`, `@return`, `@var`,
  `@throws`, `@property`, `@method`. The last two declare **virtual
  members**: through them the bridge also implements the
  virtual-symbol extension point owned by `celerrate_semantics`, and
  a member declared by `@property` or `@method` counts as existing for
  the unknown-members family (section 8). The virtual-member payload
  carries its type expressions **as unresolved text** — the
  virtual-symbol trait lives below `celerrate_types` and cannot name
  `TypeId`; existence answers the unknown-members family at the
  semantics layer, and the expressions resolve downstream through the
  type-syntax point exactly like real members' signatures. Dialect
  precedence (which tag wins) is decided at extraction time; grammar
  is resolution-time. One plugin, several extension points — which is
  what the facade is for.
- **The PHPStan dialect**, with coverage defined measurably rather
  than asserted: the ambition is completeness, and the yardstick is
  the **test corpus of `phpstan/phpdoc-parser` at a pinned version** —
  the exit criterion of the dialect plan is a published coverage
  statement against that reference, with everything beyond it traced
  as debt. Every parsed construct maps into the lattice through a
  **total lowering table**: a lattice value, or a documented sound
  widening (`non-empty-string` to `string` is sound for the three
  families; `class-string<T>` to `string` is not, and is not taken).
  Loss is **per construct, never per annotation**: one unsupported
  construct inside a docblock never discards the docblock.
- **Psalm tags with PHPStan-coincident semantics are synonyms** and
  fully honored — this includes `@psalm-param`, `@psalm-return`,
  `@psalm-var`, and crucially **`@psalm-assert` in its non-divergent
  forms**: `webmozart/assert`, a transitive dependency of practically
  everything Symfony, annotates with `@psalm-assert`, and classifying
  it as divergent would blind narrowing after every assertion. Only
  the genuinely divergent behaviors fall in the ignored bucket, and
  they are enumerated so "ignored without error" is testable:
  `@template-covariant`/`@template-contravariant` variance markers,
  the `=`-prefixed assertion forms, `@psalm-if-true`/`@psalm-if-false`
  divergences, purity tags (`@psalm-pure`, `@psalm-mutation-free`),
  and taint annotations. Parsed, ignored, traced as debt toward a
  later complement.

Per-tag precedence: the tool-prefixed tag wins over the bare tag
(`@phpstan-return` over `@return`), with a documented conflict table.
A malformed annotation is silently ignored — real-world code is full
of broken docblocks, and a docblock syntax diagnostic in this
sub-project would be a new kind of false positive. No docblock
diagnostics ship here. The bridge's annotation parse is a separate
per-member query over the docblock text the member `ItemTree` carries
(section 2), so a prose-only docblock edit re-runs the parse and
backdates at the parsed-annotation level: the two-stage cutoff the
testing section's docblock claim depends on.

**Inline suppressions**, implemented through the comment-directive
extension point (section 4), honored from this preview on:
`@phpstan-ignore-line`, `@phpstan-ignore-next-line`, `@phpstan-ignore`
(the identifier-bearing form, the recommended one since PHPStan 1.11),
and `@psalm-suppress`. Two rules make "conservative" mean
**over-suppression, never under-suppression** — a suppression that
fails to suppress is a false positive by the parent's own rationale:
suppression extinguishes **all diagnostic families** on the target
scope (the parent's rationale is family-agnostic; exempting the
existing families would re-report exactly what it forbids), and a
docblock-attached `@psalm-suppress` maps to **the annotated
declaration's whole span** (its Psalm scope), not to the docblock's
own line where no diagnostic ever fires. Identifier-level
correspondence stays deferred to the rule framework.

## 6. Inference

**The model.** One demand-driven inference query per body: it consumes
the body IR and produces the type table (each arena expression maps to
a `TypeId`). Nothing above ever re-reads a syntax tree. The query key
is pinned precisely, because its naive reading multiplies memo tables
by the class hierarchy: the default key is the **defining class** —
one inference per body — with `static` and `$this` carried as symbolic
late-static-binding placeholders in the result and **substituted at
the call site**. Per-receiver keys exist only where post-hoc
substitution is impossible: trait methods, analyzed per using class
(PHPStan's model). The receiver key is a class *definition* identity,
never a type carrying generic arguments (template substitution stays
outside the memo, or the key space is infinite). Late static binding
**forwards** through `parent::`/`self::`/`static::` calls and rebinds
on explicit class names (`Foo::create()`): the placeholder carries the
forwarded receiver, not the call site's lexical class.

**Propagation and narrowing.** Locals by assignment propagation. The
narrowing **subjects** are locals and property fetches on a stable base
(`$this->prop`, `self::$prop`), with a conservative invalidation rule:
any intervening method call, closure creation, or by-reference use
kills property narrowings — lazily initialized services are half of
Symfony, and a locals-only rule would false-positive on every one of
them. The narrowing **forms** fixed for this sub-project: `instanceof`
(and its negation), `null` comparisons (`===`/`!==`), comparisons to
literal `true`/`false` (the `strpos` returns `int|false` idiom),
`isset()` and `empty()`, the `is_*` family, truthiness, **negation and
boolean composition** (`!`, `&&`, `||` distribution — early returns
are vacuous without them), the null-coalescing operators `??` and
`??=` (which drop null from their left operand), `match` arms
including the `match(true)` idiom, `switch` with strict cases, early
returns, `assert()`, and the assertion tags carried by the bridge
(`@phpstan-assert` and the non-divergent `@psalm-assert`, section 5).
The **null-safe operator `?->`** is part of narrowing's contract, with
its chain rule stated: inside the short-circuited suffix the receiver
is the non-null type; only the final chain result re-acquires `|null`.
A naive per-link reading reports correct-by-construction Symfony code,
so this form is in the floor, not in the extensions. **Iteration
typing** is a named inference component, because it is the delivery
mechanism for the generics precision this section promises: `foreach`
resolves element and key types through the protocol chain
(`array<K, V>`, `iterable<K, V>`, `Traversable<K, V>`,
`IteratorAggregate::getIterator()` unwrapping, `Generator`), with
template substitution through each step. The set is a floor, not a
ceiling — but every added form must be covered by the harness before
it influences a published diagnostic.

**By-reference parameters**, three rules: an argument bound to a
by-reference parameter is exempt from the pre-call assignability check
(`preg_match($re, $s, $matches)` with an undefined `$matches` is the
most common stdlib idiom in existence); after the call the local's
type becomes the parameter's declared type (the general write-back
rule — the stdlib provider refines `$matches` further); and passing a
variable by reference invalidates its current narrowing.

**Interprocedural.** A call's type comes from the callee's **declared
return if present** (annotations are trusted; verifying that a body
honors its declaration is a future diagnostic, not one of the three
families), **inferred from the body otherwise**. Mutual recursion makes
these queries cyclic: they resolve through salsa cycle recovery with
fixpoint iteration, and the discipline is spelled out because widening
alone buys termination, not entry-point independence, and salsa's own
machinery panics past a fixed iteration cap
(`salsa-0.27.2/src/cycle.rs`, `MAX_ITERATIONS = 200`) — a reachable
zero-panic breach if unmanaged:

- **Monotone ascent is forced structurally**: each cycle iterate is
  joined with the previous approximation. Inference with narrowing is
  not naturally monotone; without the join, iterates can oscillate
  between two values forever and different entry points can converge
  to different fixpoints — the intermittent kind of bug the
  byte-identical harness catches only by luck.
- **An explicit iteration budget below salsa's cap**: exhaustion
  widens the result deterministically to `mixed`, never reaches the
  panic, never surfaces an error.
- **The termination argument covers call-graph growth inside the
  cycle**: receiver refinement across iterations changes which
  callees participate; the participant set is bounded by the finite
  class set, and resolution is monotone over the join discipline.
- **Provider contributions are widened at the consumption boundary**
  inside `celerrate_types`: a plugin never controls termination. The
  monotonicity expectation is documented on the trait; a
  non-convergent contribution hits the budget and widens to `mixed` —
  the deterministic bailout, not a detection panic.

**Generics, inference-only.** Template variables resolve at the call
site from argument types (`@template` through the bridge, bound
through `class-string<T>` parameters), class-level generic arguments
propagate through `new Collection<User>` annotations, constructor
inference, and the inheritance-position arguments threaded by
linearization (section 2) — but **no generic mismatch diagnostic is
emitted**. Solver failure semantics are fixed: multiple constraints on
one variable take the least upper bound; conflicting or failed
constraints fall back to the template bound, then `mixed` — **never to
the first-seen constituent**, which would leak wrong member sets into
the unknown-members family. Resolution exists so that
`$collection->first()` yields `User|null` instead of `mixed`: it
serves the precision of the three shipped families, not a family of
its own. Without it, Doctrine and Symfony collections would degrade
the whole corpus to `mixed` and empty the nullability family of
substance.

**Unannotated parameters: `mixed`, monovariant.** No
call-site-sensitive parameter inference in this sub-project.

**Engine invariants continue to apply**: cancellation in flight (watch
mode already runs; a fixture pins clean unwind when an edit lands
mid-fixpoint, with no provisional value served or persisted), fan-out
through snapshots only, no wall clock and no environment reads inside
queries. **Memory has named levers, not just a measurement**: body IR
arenas and full expression type tables are the LRU candidates (salsa
0.27 supports `lru = N` on tracked functions — the eviction the
semantic core deferred now has its first real subjects), while
inferred returns stay resident (small, hot, and the fixpoint's
currency). Peak memory on the corpus — the inherited debt — is
measured with inference active against a budget, an acceptance number
rather than a data point (section 9).

## 7. Providers, stub curation, and the Celerrate norm draft

**The stdlib type provider.** A first-party plugin in code, compiled
into the binary, consuming the dynamic-type-provider extension point
(the API's second consumer after the bridge, putting it under cross
tension early — though not the *dissimilar* consumer the v1 gate
requires, section 4). It covers the computation-dependent stdlib
signatures no declarative stub can express: `array_map` from its
callable, `json_decode` from its flags, `preg_match` `$matches` shapes,
`array_filter`, `explode`, `current`, and so on. The exact list is
driven by the corpus, not by completeness.

**Stub curation** — the named workstream the parent spec insists on.
The "Celerrate refinements" overlay (the functionMap equivalent)
activates: enriched signatures written **in the internal draft of the
Celerrate norm**, which finds its first real consumer there — exactly
what keeps a draft honest. Exit criterion: "good enough for the
corpus", measured by the residual `mixed` rate on the corpus's stdlib
calls, never by completeness. This is where PHPStan spent years; it is
a workstream with a measured exit, not a bullet point.

**The Celerrate norm draft** is written during the lattice part
(designing the lattice against a target syntax keeps both honest,
per the parent spec) and revised by curation. It is internal: no public
documentation, no migration tooling, no stability promise. It freezes
in v1.x, informed by real-world feedback.

## 8. The check families

Three families, each under the anti-false-positive guillotine: a family
that is doubtful at closure does not ship, the others ship without it,
the minimum shippable set is the unknown-members family alone, and a
guillotined family becomes a named v0.1 blocker inherited by
sub-project 5 (section 1). New `CEL####` identifiers are allocated
through the composition-root registry (the semantic core's uniqueness
test covers them by construction), permanent from the preview's
publication.

1. **Unknown members** — method, property, class constant, enum case
   on the receiver's resolved type. Conservative stances: a `mixed` or
   dynamic receiver is silent; the per-kind magic and
   dynamic-property suppression (section 2) applies. On a **union**
   receiver the family reports only if the member is missing on all
   non-null constituents, where "missing on a constituent" already
   accounts for that constituent's suppressions, and a constituent
   that is an unresolvable class, `object`, or `mixed` counts as
   possibly-having the member (the unknown-symbol family already
   reported the unresolvable class; double-reporting it here would be
   a false positive). Missing on only some constituents is a future
   "possibly undefined" diagnostic; the null constituent belongs to
   the nullability family. On an **intersection** receiver the dual
   rule applies: the member exists if present on any intersectand,
   and suppression applies if any intersectand carries it — narrowing
   produces intersections (`Foo&Countable` after two instanceofs), so
   the dual is not optional. Nested forms compose from the two rules.
2. **Nullability** — dereference of a possibly-null value: method
   call or property access on an un-narrowed `User|null`. Entirely
   dependent on narrowing (section 6): this family is what puts
   narrowing to the test, and the `?->` chain rule and property
   narrowing are its load-bearing prerequisites, fixed in the floor.
3. **Argument types** — assignability of each argument against its
   parameter (the intersection across the range for stubs), plus arity
   (missing and excess arguments, named arguments included). `mixed`
   passes everywhere — the PHPStan default posture, and the only one
   compatible with monovariant unannotated parameters. **Coercion
   follows the calling file's declared mode**: under
   `declare(strict_types=1)` the check is strict; in a weak-mode file,
   coercions PHP performs at runtime are not reported (reporting
   working code is what the guillotine forbids, literally), and an
   object with `__toString` (`Stringable`) passes a `string` parameter
   there. The corpus is uniformly strict, so the family stays
   substantive where it is measured; the posture is deterministic
   because the mode is a fact of the file. Two recorded stances:
   argument unpacking of a non-shape value silences arity for that
   call (spread makes both missing and excess undecidable, and
   string-keyed spread acts as named arguments since 8.1), and named
   arguments are checked against the declared receiver type's
   parameter names — PHP permits overrides to rename parameters, and
   checking the declared type is PHPStan's stance too.

## 9. The preview product and closure

**The product.** `celerrate check` renders the three new families
through the existing text renderer (still documented as temporary),
`--watch` re-analyzes them incrementally, and the persistent cache
extends to the new artifacts. The pack and blob schemas take version
bumps; the cache-audit adversarial tests and the served-equals-
recomputed equivalence net extend to the new artifacts, or the net
silently stops guarding.

**The typed-artifact cache is designed here, not discovered at plan
9** — it is the hardest cache problem of the sub-project and the
flagship criterion rests on it. The shipped cache is cheap because a
verdict's cross-file dependencies reduce to name-resolution answers
that re-check in microseconds; typed verdicts additionally depend on
other files' member signatures (cheap: signature-hash records, the
same reduced-answer pattern) and on **inferred returns of unannotated
callees** (not cheap: revalidating one naively means re-running
inference transitively). Two artifact classes, decided now:

- **Per-body inferred signatures** (returns, principally): small,
  persisted in a **structural serialization** (`TypeId` is a
  process-local interner handle and never hits disk), keyed by body
  content plus their own recorded reduced answers, revalidated
  **recursively and memoized** — each body validates once per run,
  constructive-trace style. This extends the settlement's constructive
  derivation direction rather than adding a third revalidation mirror.
- **Full expression type tables**: large, presumed a net loss, not
  persisted — recomputed on demand from cached inferred signatures.

The load-bearing assumption is stated and measured, not hoped:
declared returns cut most interprocedural edges on an annotated corpus
(section 1), so the recursive-revalidation frontier stays shallow. The
residual is instrumented from plan 5 on (`CELERRATE_CACHE_STATS`
grows typed counters). **The fallback lever if the economics fail**:
typed verdicts drop from the cache and warm runs re-infer — the warm
number then converges toward cold-with-inference, the sub-second
criterion is missed, and the release decision escalates rather than
silently shipping a dishonest number. Watch mode gets its own
economics measurement: packs are rewritten per cycle, inference
artifacts fatten them, and persisting on debounce or exit is the
recorded alternative if per-cycle persist costs the editing loop.

**Inherited debts close here.** The second revalidation mirror
(`resolution_records`/`reference_diagnostics`, reshaped by this
sub-project) disappears into the same single composition point as the
first. Peak memory is measured on the corpus with inference active,
against a budget (section 6). The architecture audit's
`source_symbol_table` serial rebuild — the debt recorded for "the next
scale-up", which this sub-project is — is measured in plan 1a with
member-enlarged entries on the signature-edit class, and either fixed
there or explicitly re-deferred with the measurement as justification.

**The benchmark.** The protocol re-runs and re-publishes, with the
scenario set extended first: the current scripted edit appends a
comment line, which is precisely the edit class the body IR's early
cutoff neutralizes — as the flagship scenario it would be
near-tautological. The protocol gains a **warm body-edit** scenario
(changed inferred return, exercising inference invalidation) and a
**warm signature-edit** scenario; warm one-edit **stays sub-second**
on the Symfony corpus with inference active across the scenario set —
an acceptance criterion of the closure, not a hope, with the fallback
above if it fails. The new **cold-full number is recorded** with an
explicit position on the parent's ~20x-PHPStan v0.1 trajectory.
Published numbers say honestly what is enabled, as always.

**Substance is gated, not just precision.** Every existing gate is
precision-side, and a silent engine would pass them all. Two
recall-side gates join: a **seeded-defect suite** per family (a known
null dereference, a known wrong argument, a known unknown member —
each must be reported, per family, as a closure gate), and a published
corpus **substance number**: the residual `mixed` rate on expressions,
the same metric stub curation already uses.

**Product surface at closure**, named so plan 9c owns it: the README
rewrite (it describes two diagnostic families today), the CHANGELOG
entry and release version, user-facing documentation of the new
`CEL####` identifiers (no `explain` pages until sub-project 4; the
repository documentation is the interim home), and the publication
home of the bridge's conflict and precedence tables.

## 10. Testing

TDD throughout, the five tiers of the parent spec, plus four harnesses
specific to this sub-project:

1. **Annotation ground truth**: on the corpus, inferred body types are
   confronted with the types the annotations declare, under a defined
   relation — **inferred is a subtype of annotated** (compatibility,
   not equality: inference-only generics make precision asymmetric by
   design). Divergences land in a **committed baseline classified by
   divergence class** (wrong vendor annotation, known precision gap,
   suspected inference bug), the corpus-snapshot mechanism reused; the
   gate is on regressions against the baseline, not on zero — a
   drowning protocol is no protocol. The initial triage of that
   baseline is budgeted work in plan 6, not a surprise.
2. **Invalidation scope over the new edit classes**: editing a body
   does not re-run callers' inference while the inferred return is
   identical (early cutoff on inference results); a prose-only
   docblock edit re-runs the annotation parse but nothing above it
   (the two-stage cutoff of section 5); editing one signature does not
   invalidate other members' bodies; editing a default value does
   invalidate its signature's dependents.
3. **Fixpoint determinism fixtures**: the same mutual-recursion
   cluster queried from every entry point, across thread counts,
   asserting identical results; plus the cancellation-mid-fixpoint
   fixture (an edit lands during iteration; clean unwind, no
   provisional value served or persisted).
4. **Docblock lexer fuzzing**: the same contract as the PHP parser —
   arbitrary input, never a panic.

The incremental correctness harness (byte-for-byte identity with a
from-scratch analysis, varying thread counts) extends over the typed
families; the corpus anti-false-positive runs gate each family's
release; the seeded-defect recall suite gates each family's substance
(section 9); zero-panic lints apply to every new crate.

## 11. Implementation plans

One design (this document), fourteen implementation plans, each its
own TDD cycle, pre-split where the predecessor's history says one
number would hide two plans:

1. **1a — Members**: the member `ItemTree` (docblock field included),
   inheritance linearization with generic-argument threading, the
   `source_symbol_table` measurement.
2. **1b — Body IR**: the range-free arena, the source map, the
   lowering table (`?->` chains, first-class callables), the
   redefined comment-only edit class.
3. **2 — Lattice**: `celerrate_types` — representation (template
   variables included), interning and structural canonical order,
   widening with the structural depth cap, the three-valued judgments.
   The Celerrate norm draft is written here.
4. **3 — Declared**: native signature resolution, docblock
   inheritance, the stub signature deltas, the compiler extension.
5. **4a — Plugin API**: `celerrate_plugin`, the registries and
   dispatch rules, the docblock lexer, standard PHPDoc including
   virtual members, the CI dependency-shape check.
6. **4b — The PHPStan dialect**: the pinned-reference coverage, the
   total lowering table, the Psalm synonym and ignored-tag tables.
7. **4c — Directives and the WASM sketch**: the comment-directive
   extension point, suppressions, the sketch against its acceptance
   checklist.
8. **5 — Inference core**: locals, the narrowing floor, returns, the
   fixpoint discipline (join ascent, budget, bailout), typed cache
   counters.
9. **6 — Interprocedural**: defining-class keys and
   late-static-binding substitution, traits, generics inference-only,
   iteration typing, the ground-truth baseline and its triage.
10. **7 — Providers**: the stdlib type provider, the refinements
    overlay, curation (the norm finds its first consumer).
11. **8 — Checks**: the three families, their conservative stances,
    their identifiers, the seeded-defect suite.
12. **9a — Cache**: the typed-artifact classes, recursive
    revalidation, the equivalence-net and adversarial-test extension,
    the watch persist economics.
13. **9b — Corpus and benchmark**: the extended scenario set, the
    published numbers, peak memory against its budget.
14. **9c — Release**: the `v0.0.x` release, README, CHANGELOG,
    identifier documentation, the published conflict tables.
