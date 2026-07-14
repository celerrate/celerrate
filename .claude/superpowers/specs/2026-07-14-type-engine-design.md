# Type Engine (Design)

Date: 2026-07-14
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md`
(sections 2, 4, 5, 9, 11)
Predecessor: the Semantic Core sub-project, closed by
`.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md`
and the debt settlement in
`.claude/superpowers/specs/2026-07-14-cache-audit-debt-design.md`

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

This is the umbrella design for sub-project 3, the riskiest of the
project, which is exactly why the semantic core shipped a preview
before it began. It fixes the cross-cutting decisions the parent spec
delegated here (the body lowering question, the inference model, the
bridge's dialect scope, the check families' conservative stances, the
part sequencing); each part then gets its own TDD implementation plan.

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
dependency inversion before inference leans on it.

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
  `migrate --from-phpstan`: sub-project 5.
- Framework providers (Eloquent, facades, the Symfony container):
  sub-project 6. Laravel stays out of the measured corpus.
- The WASM plugin host and the declarative plugin tier. The WASM-level
  interface **sketch** is in scope as an acceptance artifact
  (section 5); the host is not.
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
final, readonly), and its **signature as unresolved names** (parameter
types, return type, property type, default values reduced to their
comparable form). Editing a method body produces an identical member
`ItemTree` and salsa backdates it: no dependent of the signature
re-runs. Editing one signature invalidates that signature's dependents,
not the other members'. Anonymous classes receive a synthetic identity
anchored on their `AstId` (closing the semantic core's narrowing), and
the names inside trait adaptation bodies (`insteadof`, `as`) are now
collected and resolved, closing the recorded false-negative gap.

**Bodies lower to a body IR.** The parent spec left the desugaring
question open; this design fixes it: every function body is lowered by
a dedicated per-body query into a compact arena representation
(expressions and patterns densely numbered, syntactic sugar reduced),
owned by `celerrate_semantics`. Three reasons: inference never touches
a syntax tree (LRU eviction safety is preserved structurally, not by
discipline), a comment-only edit inside a body produces an identical IR
(early cutoff at the body level), and the arena provides dense indices
for inference's type tables. This is the rust-analyzer `hir-def`
pattern, transposed.

**Inheritance linearization.** A per-class query resolves `extends`,
`implements`, and trait `use` (with `insteadof` and `as`) into a
linearized member table: own members over trait members over inherited
members, with PHP's visibility and case rules (method names
case-insensitive, property and constant names case-sensitive).
Inheritance cycles are detected and broken deterministically with a
diagnostic — no salsa cycle here, per the posture the semantic core
fixed. Member lookups go through per-(class, name, kind) queries, never
through "the whole table": adding a member invalidates only the files
that looked it up.

**Magic-method suppression.** On a class carrying `__get`, `__set`,
`__call`, or `__callStatic` (directly or by inheritance),
unknown-member diagnostics are conservatively suppressed: documented
engine semantics from the parent spec, applied here. Dynamic type
providers progressively restore precision later.

## 3. The lattice and declared types (`celerrate_types`, `celerrate_stubs`)

**The representation.** `celerrate_types` owns the lattice: scalars and
their literals (`'active'`, `42`), unions, intersections,
`array<K, V>`, lists and array shapes (`array{id: int}`), enums and
their cases, callable signatures, class types carrying generic
arguments, the late-static-binding placeholders (`static`, `self`,
`parent`), and the extremes (`mixed`, `never`, `void`, `null`). Types
are **interned in canonical form** (union members deterministically
ordered, opaque `TypeId`): cheap `Eq`/`Hash` for early cutoff, and the
representation is never exposed as a matchable enum — plugins construct
through builders and interrogate through query methods, the parent
spec's commitment.

**Widening is defined here, not in inference.** Literal-to-general
widening, union arity caps, array-shape depth caps: deterministic
lattice operations, because fixpoint termination (section 6) depends on
them and they must be identical regardless of a cycle's entry point.
Judgments (assignability, nullability, subtyping) are salsa queries.

**Native declared types.** The member `ItemTree` signatures (unresolved
names) resolve into lattice types through per-member queries: the
shortest path from the member boundary to a useful judgment.

**The stub signature payload.** The stub compiler extends into the
reserved extension point: parameters, returns, and property types as
**per-version deltas** (signature changed in 8.3, parameter added in
8.2, and so on). At consultation the parent spec's range rule applies:
arguments are checked against the **intersection** of the signature
across `[min, max]`, the call's return type is the **union**. The blob
takes a schema version bump (the format is version-stamped precisely
for this); the header field pinned by the cache-audit tests evolves
with it.

**Source precedence.** For one signature element: docblock annotation
(through the bridge) over native declaration over nothing (`mixed`). An
annotation **refines** the native declaration: it must be a subtype;
otherwise the annotation is ignored and the native declaration wins —
never a crash, never a silent widening.

## 4. The plugin API (`celerrate_plugin`)

**Ownership.** Extension points stay owned by their consuming layers,
as the parent spec requires: `celerrate_types` owns the type-syntax
trait (understand an annotation notation) and the dynamic-type-provider
trait (compute a return type), `celerrate_semantics` owns the
virtual-symbol and stub-provider traits. `celerrate_plugin` is the
aggregation facade re-exporting the stable surface; implementations
register as salsa inputs at the composition root (`celerrate_cli`), and
a plugin's identity (name, version, configuration) is itself a salsa
input — upgrading a plugin invalidates its contributions.

**WASM-projectable from day 1.** Opaque handles (`TypeId`, `SymbolId`),
construction through builders, interrogation through a narrow host
interface, no borrowed internals, no retained database references, no
closures. The **WASM-level interface sketch is an acceptance artifact**
of this sub-project even though the host ships later: dynamic type
providers are bidirectional (they query types while producing types),
which is the hard case across a WASM boundary, and it is sized now. The
API carries an explicit version checked at registration, and is not
called v1: its second dissimilar consumer (a framework dynamic type
provider) is sub-project 6.

**The mechanical constraint.** The bridge and the stdlib type provider
depend **only** on `celerrate_plugin`, enforced in CI like the
zero-panic policy. An extension point that proves insufficient is
extended, never bypassed.

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

Coverage, decided at scoping:

- **Standard PHPDoc**, complete: `@param`, `@return`, `@var`,
  `@throws`, `@property`, `@method`. The last two declare **virtual
  members**: through them the bridge also implements the
  virtual-symbol extension point owned by `celerrate_semantics`, and
  a member declared by `@property` or `@method` counts as existing for
  the unknown-members family (section 8) — one plugin, several
  extension points, which is what the facade is for.
- **The PHPStan dialect**, complete: type expressions (templates,
  conditional returns, array shapes, integer ranges, `value-of` and
  friends), `@phpstan-*` prefixed tags, `@phpstan-assert` and its
  variants.
- **Psalm synonym tags** (`@psalm-param`, `@psalm-return`,
  `@psalm-var`, ...): accepted with the shared grammar.
- **Divergent Psalm semantics** (Psalm-specific assertion behavior,
  purity tags, taint annotations): parsed and ignored without error,
  recorded as debt toward a later complement. Refusing even the trivial
  synonyms would make real code lose types for no technical reason;
  implementing the divergent semantics now would be effort the Symfony
  corpus cannot validate.

Per-tag precedence: the tool-prefixed tag wins over the bare tag
(`@phpstan-return` over `@return`), with a documented conflict table.
A malformed annotation is silently ignored — real-world code is full
of broken docblocks, and a docblock syntax diagnostic in this
sub-project would be a new kind of false positive. No docblock
diagnostics ship here.

**Inline suppressions** are honored from this preview on:
`@phpstan-ignore-line`, `@phpstan-ignore-next-line`, and
`@psalm-suppress` suppress the typed families' diagnostics on the
target line. The mapping is conservative: suppression extinguishes the
typed families on that line without attempting identifier-level
correspondence (that refinement arrives with the rule framework). A
codebase's existing suppression decisions are respected by default,
because re-reporting them reads as false positives.

## 6. Inference

**The model.** One demand-driven inference query per body: it consumes
the body IR and produces the type table (each arena expression maps to
a `TypeId`). Nothing above ever re-reads a syntax tree. The query key
applies the parent spec's decision: **(callee, resolved receiver
class)** where PHP semantics demand it — `static`/`$this` returns under
late static binding, trait methods analyzed per using class. "Paid
once" means once per key.

**Propagation and narrowing.** Locals by assignment propagation, with
a control-flow narrowing set fixed for this sub-project: `instanceof`,
`null` comparisons (`===`/`!==`), the `is_*` family, truthiness,
`match` arms, early returns, `assert()`, and the `@phpstan-assert`
family carried by the bridge. The set is a floor, not a ceiling — but
every added form must be covered by the harness before it influences a
published diagnostic.

**Interprocedural.** A call's type comes from the callee's **declared
return if present** (annotations are trusted; verifying that a body
honors its declaration is a future diagnostic, not one of the three
families), **inferred from the body otherwise**. Mutual recursion makes
these queries cyclic: they resolve through **salsa cycle recovery with
fixpoint iteration**, termination guaranteed by the lattice's widening
operations (section 3), the result deterministic regardless of which
participant is queried first. This is the infrastructure the semantic
core required of any potentially cyclic query before it lands: it is
born here, with its first real client.

**Generics, inference-only.** Template variables resolve at the call
site from argument types (`@template` through the bridge), class-level
generic arguments propagate (`new Collection<User>` by annotation or by
constructor inference) — but **no generic mismatch diagnostic is
emitted**. Resolution exists so that `$collection->first()` yields
`User|null` instead of `mixed`: it serves the precision of the three
shipped families, not a family of its own. Without it, Doctrine and
Symfony collections would degrade the whole corpus to `mixed` and empty
the nullability family of substance.

**Unannotated parameters: `mixed`, monovariant.** No
call-site-sensitive parameter inference in this sub-project.

**Engine invariants continue to apply**: cancellation in flight (watch
mode already runs), fan-out through snapshots only, no wall clock and
no environment reads inside queries. Peak memory on the corpus — the
inherited debt — is measured once inference exists, because inference
is what will move it.

## 7. Providers, stub curation, and the Celerrate norm draft

**The stdlib type provider.** A first-party plugin in code, compiled
into the binary, consuming the dynamic-type-provider extension point
(the API's second consumer after the bridge, putting it under cross
tension early). It covers the computation-dependent stdlib signatures
no declarative stub can express: `array_map` from its callable,
`json_decode` from its flags, `preg_match` `$matches` shapes,
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
that is doubtful at closure does not ship, and the others ship without
it. New `CEL####` identifiers are allocated through the
composition-root registry (the semantic core's uniqueness test covers
them by construction), permanent from the preview's publication.

1. **Unknown members** — method, property, class constant, enum case
   on the receiver's resolved type. Conservative stances: a `mixed` or
   dynamic receiver is silent; magic-method suppression (section 2)
   applies; on a union receiver the family reports only if the member
   is missing on **all** non-null constituents (missing on only some is
   a future "possibly undefined" diagnostic; the null constituent
   belongs to the nullability family).
2. **Nullability** — dereference of a possibly-null value: method
   call or property access on an un-narrowed `User|null`. Entirely
   dependent on narrowing (section 6): this family is what puts
   narrowing to the test.
3. **Argument types** — assignability of each argument against its
   parameter (the intersection across the range for stubs), plus arity
   (missing and excess arguments, named arguments included). `mixed`
   passes everywhere — the PHPStan default posture, and the only one
   compatible with monovariant unannotated parameters.

## 9. The preview product and closure

**The product.** `celerrate check` renders the three new families
through the existing text renderer (still documented as temporary),
`--watch` re-analyzes them incrementally, and the persistent cache
extends to the new artifact classes (member `ItemTree`s, per-body
inference results, the enriched diagnostics) under the same economic
criterion: an artifact class that does not beat recomputation is
dropped. The pack and blob schemas take version bumps; the cache-audit
adversarial tests extend to the new artifacts.

**Inherited debts close here.** The second revalidation mirror
(`resolution_records`/`reference_diagnostics`, reshaped by this
sub-project) disappears into the same single composition point as the
first; peak memory is measured on the corpus with inference active.

**The benchmark.** The protocol re-runs and re-publishes: warm one-edit
**stays sub-second** on the Symfony corpus with inference active — an
acceptance criterion of the closure, not a hope. Published numbers say
honestly what is enabled, as always.

## 10. Testing

TDD throughout, the five tiers of the parent spec, plus three harnesses
specific to this sub-project:

1. **Annotation ground truth**: on the corpus, inferred body types are
   confronted with the types the annotations declare. A divergence is
   an inference bug, a bridge bug, or a wrong corpus annotation — in
   all three cases a signal, before any diagnostic exists.
2. **Invalidation scope over the new edit classes**: editing a body
   does not re-run callers' inference while the inferred return is
   identical (early cutoff on inference results); editing a docblock
   invalidates only that signature's dependents; editing one signature
   does not invalidate other members' bodies.
3. **Docblock lexer fuzzing**: the same contract as the PHP parser —
   arbitrary input, never a panic.

The incremental correctness harness (byte-for-byte identity with a
from-scratch analysis, varying thread counts) extends over the typed
families; the corpus anti-false-positive runs gate each family's
release; zero-panic lints apply to every new crate.

## 11. Implementation plans

One design (this document), nine implementation plans, each its own TDD
cycle, in order:

1. **Members**: the member `ItemTree`, the body IR, inheritance
   linearization.
2. **Lattice**: `celerrate_types` — representation, interning,
   widening, judgments. The Celerrate norm draft is written here.
3. **Declared**: native signature resolution, the stub signature
   deltas, the compiler extension.
4. **Plugin API and bridge**: `celerrate_plugin`, the docblock lexer,
   PHPDoc plus PHPStan plus Psalm synonyms, suppressions, the WASM
   sketch as acceptance artifact.
5. **Inference core**: locals, narrowing, returns, the fixpoint and
   widening infrastructure.
6. **Interprocedural**: receiver-keyed queries, late static binding,
   traits, generics inference-only.
7. **Providers**: the stdlib type provider, the refinements overlay,
   curation (the norm finds its first consumer).
8. **Checks**: the three families and their identifiers.
9. **Closure**: the cache extension, the inherited debts, the corpus,
   the benchmark, the `v0.0.x` release.
