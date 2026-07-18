# Plugin Boundary Sealing — Design (issue #61)

Date: 2026-07-19
Status: Approved (brainstorming output)
Parent: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (section 5),
`.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`
Issue: #61 — Extension traits leak `&dyn salsa::Database`, breaking the
WASM-projectable-from-day-1 commitment

## 1. Problem

Two of the four extension points hand plugins a live salsa database handle:

- `DynamicTypeProvider::return_type` and `by_reference_types` take
  `db: &'db dyn salsa::Database` directly
  (`crates/celerrate_types/src/dynamic_type_provider.rs`).
- `AnnotationSite::database()` returns the handle to every `TypeSyntax`
  implementation (`crates/celerrate_types/src/type_syntax.rs`).

The facade makes the leak official: `celerrate_plugin` re-exports `salsa`
itself plus the whole `celerrate_diagnostics` and `celerrate_source` crates.
The effective plugin API is therefore the entire `salsa::Database` trait plus
the full `TypeId` method surface (~63 public methods, each taking
`&dyn salsa::Database`), pinned to a salsa minor version. A
`&dyn salsa::Database` cannot cross a WASM boundary, so the design-spec
commitment "the native API is WASM-projectable from day 1" is false in shape,
and the WASM sketch's acceptance claim ("every native trait projects onto the
sketch without reshaping") does not hold: the sketch's guest export is
`return_type(invocation) -> type?`, the native trait is
`return_type(db, invocation)`.

The other two extension points (`VirtualSymbolProvider`,
`CommentDirectiveProvider`) already cross the boundary with plain data and
need no signature change. `TypeData` is private and builder/query-based; the
lattice-encapsulation commitment is honored — the leak is specifically the
database handle and the wholesale re-exports.

## 2. Scope

All four remediation tracks from the issue, as one coherent boundary rework:

1. Call-scoped context objects owning the database internally
   (`TypeContext`, `InvocationSite`; `AnnotationSite` loses `database()`).
2. Purge the wholesale re-exports from `celerrate_plugin`.
3. Revalidate the WASM interface sketch against the reshaped native traits.
4. `#[non_exhaustive]` on the boundary structs to back the additive-extension
   promise.

Plus the first-party plugin migration (`celerrate_phpdoc_bridge`,
`celerrate_stdlib_provider`) and an extension of the `xtask dependency_shape`
check. `PLUGIN_API_VERSION` stays `0`: this rework is exactly what the
pre-v1 versioning exists to absorb.

## 3. Design

### 3.1 The sealed facade: `TypeContext<'db>`

A new type in `celerrate_types`:

```rust
pub struct TypeContext<'db> {
    db: &'db dyn salsa::Database, // private
}
```

- **`pub(crate)` constructor.** Only the engine's dispatch and consumption
  points can create one. No accessor returns the database.
- **Surface = the four host-interface families of the WASM sketch,
  enumerated as methods**: type construction (`mixed()`, `int()`,
  `string_literal(...)`, unions, shapes built field by field, callable
  signatures, template references), type interrogation (kind probes,
  nullability, literal values, constituents), argument value access (covered
  by interrogation of literal `TypeId`s), and symbol lookup (class and
  member existence, claim-key normalization). Each method is a one-line
  delegation to the corresponding `TypeId` (or semantics) function.
- **YAGNI inclusion criterion.** The v0 surface is what the bridge and the
  stdlib provider actually consume today, reconciled with the families the
  sketch names. No speculative builders. A new need extends the facade —
  never bypasses it (the design spec's rule).
- **Retention is a compile error, not a review item.** `TypeContext` is
  `Copy` and `'db`-bound, but plugin implementations are `Arc<dyn Trait>`
  and therefore `'static`: storing a `TypeContext` or a `TypeId<'db>` in a
  field of `self` does not compile.
- **No internal ripple.** The ~63 `TypeId::xxx(db)` methods keep their
  signatures; engine code keeps calling them directly. They become
  structurally unreachable from plugin crates, which can no longer name or
  obtain a `&dyn salsa::Database`.

### 3.2 Sites and trait signatures

**`AnnotationSite`** (existing, `type_syntax.rs`):

- `database()` is removed — it was the leak.
- Gains `types(&self) -> TypeContext<'db>`.
- Everything else is unchanged: `keyword_type()`, `qualify_class_name()`,
  `declaring_scope()`, `enclosing_class_scope()`,
  `enclosing_class_docblock()`.

**`InvocationSite<'db, 'call>`** (new, `dynamic_type_provider.rs`):

```rust
pub struct InvocationSite<'db, 'call> {
    db: &'db dyn salsa::Database,       // private
    invocation: &'call Invocation<'db>, // private
}
```

Accessors: `claim()`, `receiver_type()`, `argument_types()` (slice), and
`types() -> TypeContext<'db>`. Constructed only by the engine's consumption
point (`pub(crate)`).

**Trait signatures become:**

```rust
fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>>;
fn by_reference_types<'db>(&self, site: &InvocationSite<'db, '_>) -> Vec<(usize, TypeId<'db>)>;
```

— exactly the shape of the sketch's guest exports.

**`Invocation`** stays as the engine-internal data struct (the engine
constructs it; tests construct it through a new `Invocation::new(...)`
constructor, required once it is `#[non_exhaustive]`). It leaves the public
facade: `celerrate_plugin` re-exports `InvocationSite`, not `Invocation`.

**Documentary contracts migrate unchanged**: the persisted-cache purity
obligation (plan 9a, decision 5), monotonicity with respect to the fixpoint,
and the `None` fallback are re-documented on `InvocationSite` and the trait
with the same semantics.

The two `celerrate_semantics` extension points are already clean (plain data
in, plain data out) and do not change.

### 3.3 Facade purge, `#[non_exhaustive]`, mechanical enforcement

**`celerrate_plugin` purge:**

- Removed: `pub use salsa;`, `pub use celerrate_diagnostics as diagnostics;`,
  `pub use celerrate_source as source;`.
- Replacement rule: **nominal, type-by-type re-export of boundary vocabulary
  only**. If the directive/span vocabulary needs a specific type (for
  example a span type inside `CommentDirective`), that precise type is
  re-exported by name — never a whole crate.
- Added to the facade: `InvocationSite`, `TypeContext`. Removed from it:
  `Invocation`, `salsa`. `AnnotationSite` stays (boundary type).
- `PLUGIN_API_VERSION` stays `0`.

**`#[non_exhaustive]`:**

- The four the issue names: `Invocation`, `ParsedAnnotations`, `SymbolClaim`
  (the enum: variants stay constructible; exhaustive matching outside the
  crate now requires a wildcard arm), `PluginDescriptor`.
- Sister vocabulary structs, for coherence: `ParsedAssertion`,
  `ParsedTemplate`, `ParsedAncestor`, `VirtualMember`, `VirtualParameter`.
- Accepted consequence: the bridge can no longer build
  `ParsedAnnotations { .. }` as a cross-crate literal; it goes through
  `ParsedAnnotations::default()` plus field mutation (fields stay `pub`).
  Engine-constructed structs gain constructors where tests need them.

**Mechanical enforcement extension.** The current
`xtask dependency_shape` rule reads "plugin crates depend only on
`celerrate_plugin` **in the workspace**" — `salsa` is external, so a plugin
crate could add it as a direct dependency and recover the handle. The check
is extended: `salsa` (and workspace crates other than the facade) is
forbidden in the `[dependencies]` of plugin crates. `[dev-dependencies]`
stay exempt (the end-to-end tests need the whole seam, as documented in the
plugin crates' manifests).

### 3.4 First-party migration, sketch amendment, tests, documentation

**Migration of the two consumers** (mechanical, compiler-driven):

- `celerrate_phpdoc_bridge`: the ~10 `site.database()` call sites disappear;
  `lowering.rs` and `syntax.rs` go through `site.types()`. Internal helpers
  taking `db: &dyn salsa::Database` take `TypeContext` instead.
- `celerrate_stdlib_provider`: `return_type(db, invocation)` becomes
  `return_type(site)`; `array_functions`, `json_functions`,
  `pattern_functions`, `string_functions` receive `TypeContext` instead of
  `db`.
- This is the living acceptance test: if the facade cannot express the two
  real plugins, the facade is extended — never bypassed.

**WASM sketch amendment**
(`.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`):

- The projection table (section 7) is updated with the site-based
  signatures; native trait and guest export now genuinely coincide.
- The rustdoc note "the native tier enforces it by review" is replaced: the
  enforcement is structural (the database is owned by the sites, `salsa` is
  unnameable from a plugin crate, retention is prevented by the `'static`
  implementation bound).
- The acceptance claim "every native trait projects onto the sketch without
  reshaping" becomes true again.

**Tests (TDD, in order):**

1. `TypeContext`: sealed construction (enforced by visibility), delegations
   spot-checked per family (a sample per family, not all ~60 one-liners).
2. `InvocationSite`: accessors; the existing by-reference-channel test
   migrated.
3. The existing bridge and stdlib-provider suites migrated — **zero
   behavioral regression expected**: snapshots and end-to-end tests must
   pass unchanged, the proof that the rework is pure boundary.
4. `xtask dependency_shape`: a test case with `salsa` in a plugin crate's
   `[dependencies]` must fail the check.

**Documentation:** a `[Unreleased]` CHANGELOG entry (internal breaking
change of the v0 plugin API), the facade rustdoc rewritten with the
nominal-re-export rule, issue #61 closed referencing the commits.

## 4. Alternatives considered

- **Methods duplicated on each site** (no shared facade): ~60 methods
  duplicated on two types, surfaces drifting apart over time, the WASM
  projection table duplicated. Rejected: DRY violation with no benefit.
- **A `&dyn PluginHost<'db>` host trait** (maximal dependency inversion):
  there will only ever be one implementation (the engine); `dyn` plus `'db`
  lifetimes plus object-safety complicate everything the sealed struct gets
  for free. Rejected: YAGNI — visibility-based sealing is sufficient and is
  what the sketch's projection actually needs.
