# Plugin Boundary Sealing Implementation Plan (issue #61)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Seal the plugin API boundary so no extension trait hands plugins a `&dyn salsa::Database`, making the WASM-projectable-from-day-1 commitment structurally true.

**Architecture:** A sealed `TypeContext<'db>` facade in `celerrate_types` owns the database privately and exposes the type construction and interrogation families as one-line delegations to `TypeId`. `AnnotationSite` exposes it via `types()` (its `database()` accessor is demoted to `pub(crate)`); a new `InvocationSite` replaces the raw `db + Invocation` pair in `DynamicTypeProvider`. `celerrate_plugin` stops re-exporting `salsa` and whole crates; `xtask dependency_shape` additionally forbids `salsa` as a direct plugin dependency.

**Tech Stack:** Rust, salsa 0.27, cargo workspace, xtask.

**Spec:** `.claude/superpowers/specs/2026-07-19-plugin-boundary-sealing-design.md`

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Test modules may locally `#[allow]` these lints.
- TDD: failing test → minimal implementation → refactor. No production code without a test that demanded it.
- Strict layering: dependencies form a DAG with no upward edges.
- Everything in English, full words, no abbreviated names (standard acronyms fine).
- Commits: gitmoji + Conventional Commits (e.g. `♻️ refactor(types): ...`). Repository-configured identity; never any Claude attribution.
- Verification commands: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`.
- The workspace must compile and all tests must pass at every commit.
- `PLUGIN_API_VERSION` stays `0` throughout.

---

### Task 1: The sealed `TypeContext<'db>` facade

**Files:**
- Create: `crates/celerrate_types/src/type_context.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (add `mod type_context;` and `pub use type_context::TypeContext;`)

**Interfaces:**
- Consumes: `TypeId<'db>` construction and interrogation methods (`crates/celerrate_types/src/construction.rs`), `ShapeField<'db>`, `CallableParameter<'db>`.
- Produces: `TypeContext<'db>` — `Copy`, `pub(crate) fn new(db: &'db dyn salsa::Database) -> Self`, and the delegation methods listed in Step 3. Tasks 2–5 rely on `TypeContext::new` and these method names exactly.

- [ ] **Step 1: Write the failing tests**

In `crates/celerrate_types/src/type_context.rs` (create the file with only the test module for the red step; the struct comes in Step 3):

```rust
//! The sealed type facade: the one surface through which plugins
//! construct and interrogate types. Owns the database reference
//! privately — the native embodiment of the WASM host-interface
//! families (construction, interrogation), sketch sections 6 and 7.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::TypeContext;
    use crate::representation::TypeId;

    #[test]
    fn construction_delegates_to_the_type_id_builders() {
        let db = TestDatabase::default();
        let context = TypeContext::new(&db);
        // One spot check per construction shape: atom, literal,
        // parameterized, aggregate.
        assert_eq!(context.int(), TypeId::int(&db));
        assert_eq!(context.string_literal("active"), TypeId::string_literal(&db, "active"));
        assert_eq!(
            context.list(context.string()),
            TypeId::list(&db, TypeId::string(&db))
        );
        assert_eq!(
            context.union([context.int(), context.null()]),
            TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)])
        );
        assert_eq!(
            context.class("App\\User", Vec::new()),
            TypeId::class(&db, "App\\User", Vec::new())
        );
    }

    #[test]
    fn interrogation_delegates_to_the_type_id_queries() {
        let db = TestDatabase::default();
        let context = TypeContext::new(&db);
        let int_literal = context.int_literal(42);
        assert_eq!(context.int_literal_value(int_literal), Some(42));
        assert!(context.is_null(context.null()));
        assert_eq!(
            context.class_name(context.class("App\\User", Vec::new())),
            Some("App\\User".to_owned())
        );
        let union = context.union([context.int(), context.null()]);
        assert_eq!(context.constituents(union).len(), 2);
    }

    #[test]
    fn the_context_is_copy_so_helpers_can_pass_it_by_value() {
        let db = TestDatabase::default();
        let context = TypeContext::new(&db);
        let copy = context;
        assert_eq!(context.int(), copy.int());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types type_context 2>&1 | tail -20`
Expected: compile FAILURE — `TypeContext` not found (the module has no struct yet). Add `mod type_context;` to `crates/celerrate_types/src/lib.rs` next to the other private modules first if the module is not picked up.

- [ ] **Step 3: Implement `TypeContext`**

Prepend to `crates/celerrate_types/src/type_context.rs` (above the test module), after the module docblock:

```rust
use crate::declared::CallableParameter;
use crate::representation::{ShapeField, TypeId};
```

(Adjust the two import paths to wherever `ShapeField` and `CallableParameter` are defined — check `crates/celerrate_types/src/lib.rs`'s existing `pub use` lines for their true modules; do not guess.)

```rust
/// The sealed facade plugins construct and interrogate types through.
/// `Copy` and `'db`-bound: implementations are `'static`
/// (`Arc<dyn Trait>`), so retaining one in plugin state is a compile
/// error — "never retain" is structural, not reviewed. The surface is
/// exactly what the first-party plugins consume (the YAGNI criterion
/// of the design); a new need extends the facade, never bypasses it.
#[derive(Clone, Copy)]
pub struct TypeContext<'db> {
    db: &'db dyn salsa::Database,
}

impl<'db> TypeContext<'db> {
    /// Constructed only by the engine's dispatch and consumption
    /// points. No accessor returns the database.
    pub(crate) fn new(db: &'db dyn salsa::Database) -> Self {
        Self { db }
    }

    // --- Construction: atoms ---
    pub fn mixed(self) -> TypeId<'db> { TypeId::mixed(self.db) }
    pub fn never(self) -> TypeId<'db> { TypeId::never(self.db) }
    pub fn null(self) -> TypeId<'db> { TypeId::null(self.db) }
    pub fn object(self) -> TypeId<'db> { TypeId::object(self.db) }
    pub fn resource(self) -> TypeId<'db> { TypeId::resource(self.db) }
    pub fn bool(self) -> TypeId<'db> { TypeId::bool(self.db) }
    pub fn int(self) -> TypeId<'db> { TypeId::int(self.db) }
    pub fn float(self) -> TypeId<'db> { TypeId::float(self.db) }
    pub fn string(self) -> TypeId<'db> { TypeId::string(self.db) }
    pub fn non_empty_string(self) -> TypeId<'db> { TypeId::non_empty_string(self.db) }
    pub fn numeric_string(self) -> TypeId<'db> { TypeId::numeric_string(self.db) }
    pub fn literal_string_type(self) -> TypeId<'db> { TypeId::literal_string_type(self.db) }
    pub fn static_placeholder(self) -> TypeId<'db> { TypeId::static_placeholder(self.db) }

    // --- Construction: literals and ranges ---
    pub fn bool_literal(self, value: bool) -> TypeId<'db> { TypeId::bool_literal(self.db, value) }
    pub fn int_literal(self, value: i64) -> TypeId<'db> { TypeId::int_literal(self.db, value) }
    pub fn int_range(self, minimum: Option<i64>, maximum: Option<i64>) -> TypeId<'db> {
        TypeId::int_range(self.db, minimum, maximum)
    }
    pub fn float_literal(self, value: f64) -> TypeId<'db> { TypeId::float_literal(self.db, value) }
    pub fn string_literal(self, value: &str) -> TypeId<'db> { TypeId::string_literal(self.db, value) }

    // --- Construction: composites ---
    pub fn union(self, constituents: impl IntoIterator<Item = TypeId<'db>>) -> TypeId<'db> {
        TypeId::union(self.db, constituents)
    }
    pub fn intersection(self, intersectands: impl IntoIterator<Item = TypeId<'db>>) -> TypeId<'db> {
        TypeId::intersection(self.db, intersectands)
    }
    pub fn array(self, key: TypeId<'db>, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::array(self.db, key, value)
    }
    pub fn non_empty_array(self, key: TypeId<'db>, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::non_empty_array(self.db, key, value)
    }
    pub fn list(self, value: TypeId<'db>) -> TypeId<'db> { TypeId::list(self.db, value) }
    pub fn non_empty_list(self, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::non_empty_list(self.db, value)
    }
    pub fn shape(self, fields: Vec<ShapeField<'db>>) -> TypeId<'db> {
        TypeId::shape(self.db, fields)
    }
    pub fn iterable(self, key: TypeId<'db>, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::iterable(self.db, key, value)
    }
    pub fn callable(
        self,
        parameters: Vec<CallableParameter<'db>>,
        return_type: TypeId<'db>,
    ) -> TypeId<'db> {
        TypeId::callable(self.db, parameters, return_type)
    }

    // --- Construction: classes, templates, type operators ---
    pub fn class(self, name: &str, arguments: Vec<TypeId<'db>>) -> TypeId<'db> {
        TypeId::class(self.db, name, arguments)
    }
    pub fn class_string(self, argument: Option<TypeId<'db>>) -> TypeId<'db> {
        TypeId::class_string(self.db, argument)
    }
    pub fn template(self, scope: &str, name: &str, bound: TypeId<'db>) -> TypeId<'db> {
        TypeId::template(self.db, scope, name, bound)
    }
    pub fn key_of(self, subject: TypeId<'db>) -> TypeId<'db> { TypeId::key_of(self.db, subject) }
    pub fn value_of(self, subject: TypeId<'db>) -> TypeId<'db> { TypeId::value_of(self.db, subject) }
    pub fn conditional(
        self,
        subject: TypeId<'db>,
        matches: TypeId<'db>,
        then_branch: TypeId<'db>,
        otherwise_branch: TypeId<'db>,
        negated: bool,
    ) -> TypeId<'db> {
        TypeId::conditional(self.db, subject, matches, then_branch, otherwise_branch, negated)
    }

    // --- Interrogation ---
    pub fn is_null(self, subject: TypeId<'db>) -> bool { subject.is_null(self.db) }
    pub fn is_list(self, subject: TypeId<'db>) -> bool { subject.is_list(self.db) }
    pub fn bool_literal_value(self, subject: TypeId<'db>) -> Option<bool> {
        subject.bool_literal_value(self.db)
    }
    pub fn int_literal_value(self, subject: TypeId<'db>) -> Option<i64> {
        subject.int_literal_value(self.db)
    }
    pub fn float_literal_value(self, subject: TypeId<'db>) -> Option<f64> {
        subject.float_literal_value(self.db)
    }
    pub fn string_literal_value(self, subject: TypeId<'db>) -> Option<String> {
        subject.string_literal_value(self.db)
    }
    pub fn constituents(self, subject: TypeId<'db>) -> Vec<TypeId<'db>> {
        subject.constituents(self.db)
    }
    pub fn array_key(self, subject: TypeId<'db>) -> Option<TypeId<'db>> {
        subject.array_key(self.db)
    }
    pub fn array_value(self, subject: TypeId<'db>) -> Option<TypeId<'db>> {
        subject.array_value(self.db)
    }
    pub fn class_name(self, subject: TypeId<'db>) -> Option<String> {
        subject.class_name(self.db)
    }
    pub fn class_arguments(self, subject: TypeId<'db>) -> Vec<TypeId<'db>> {
        subject.class_arguments(self.db)
    }
    pub fn callable_return(self, subject: TypeId<'db>) -> Option<TypeId<'db>> {
        subject.callable_return(self.db)
    }
}
```

In `crates/celerrate_types/src/lib.rs`, add `mod type_context;` alongside the other module declarations and `pub use type_context::TypeContext;` alongside the other re-exports.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types type_context`
Expected: 3 tests PASS.

- [ ] **Step 5: Full-crate check and commit**

Run: `cargo clippy --package celerrate_types --all-targets -- -D warnings && cargo fmt --all`
Expected: clean.

```bash
git add crates/celerrate_types/src/type_context.rs crates/celerrate_types/src/lib.rs
git commit -m "✨ feat(types): the sealed TypeContext facade over the TypeId surface"
```

---

### Task 2: `AnnotationSite::types()`

**Files:**
- Modify: `crates/celerrate_types/src/type_syntax.rs` (the `impl AnnotationSite` block, around line 83)

**Interfaces:**
- Consumes: `TypeContext::new` (Task 1).
- Produces: `AnnotationSite::types(&self) -> TypeContext<'db>`. Tasks 4 and 5 rely on this exact name. `database()` is NOT removed here (Task 5 demotes it).

- [ ] **Step 1: Write the failing test**

Append to the `tests` module of `crates/celerrate_types/src/type_syntax.rs`:

```rust
#[test]
fn the_annotation_site_exposes_the_sealed_type_context() {
    let fixture = fixture(&["<?php class C {}"]);
    let db = &fixture.db;
    let site = AnnotationSite::new(db, &NameSite::Global, AnnotationContext::default());
    // The facade builds the same interned types as the raw builders:
    // a plugin needs nothing beyond the site.
    assert_eq!(site.types().int(), TypeId::int(db));
    assert_eq!(
        site.types().class("App\\User", Vec::new()),
        TypeId::class(db, "App\\User", Vec::new())
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_types the_annotation_site_exposes_the_sealed_type_context`
Expected: compile FAIL — no method `types` on `AnnotationSite`.

- [ ] **Step 3: Implement `types()`**

In the `impl<'db, 'site> AnnotationSite<'db, 'site>` block of `crates/celerrate_types/src/type_syntax.rs`, directly above `pub fn database(&self)`:

```rust
/// The sealed type facade: construction and interrogation without
/// the database. Call-scoped like the site itself.
pub fn types(&self) -> crate::type_context::TypeContext<'db> {
    crate::type_context::TypeContext::new(self.db)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types type_syntax`
Expected: all PASS, including the new test.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/type_syntax.rs
git commit -m "✨ feat(types): AnnotationSite::types exposes the sealed facade"
```

---

### Task 3: `InvocationSite` and the reshaped `DynamicTypeProvider` trait

The trait signature change is atomic: every implementor (the stdlib provider, the in-crate test fakes) and the consumption points in `flow.rs` move in this one task. The workspace compiles again at the end of the task, not between steps.

**Files:**
- Modify: `crates/celerrate_types/src/dynamic_type_provider.rs`
- Modify: `crates/celerrate_types/src/flow.rs:1198-1256` (the `provider_return` and `provider_by_reference` methods)
- Modify: `crates/celerrate_types/src/inference.rs` (test fake `SymbolicProvider`, around line 2227)
- Modify: `crates/celerrate_stdlib_provider/src/lib.rs`, `array_functions.rs`, `json_functions.rs`, `pattern_functions.rs`, `string_functions.rs`

**Interfaces:**
- Consumes: `TypeContext::new` (Task 1).
- Produces:
  - `InvocationSite<'db, 'call>` with `pub(crate) fn new(db: &'db dyn salsa::Database, invocation: &'call Invocation<'db>) -> Self`, and public accessors `claim(&self) -> &SymbolClaim`, `receiver_type(&self) -> Option<TypeId<'db>>`, `argument_types(&self) -> &[TypeId<'db>]`, `types(&self) -> TypeContext<'db>`.
  - Trait methods: `fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>>` and `fn by_reference_types<'db>(&self, site: &InvocationSite<'db, '_>) -> Vec<(usize, TypeId<'db>)>` (default body returns `Vec::new()`).
  - Task 5 re-exports `InvocationSite` from `celerrate_plugin` and drops `Invocation`.

- [ ] **Step 1: Write the failing test**

In the `tests` module of `crates/celerrate_types/src/dynamic_type_provider.rs`, add:

```rust
#[test]
fn the_invocation_site_exposes_the_invocation_and_the_sealed_facade() {
    let db = TestDatabase::default();
    let claim = SymbolClaim::Function { key: "array_map".to_owned() };
    let invocation = Invocation {
        claim: claim.clone(),
        receiver_type: None,
        argument_types: vec![TypeId::int(&db)],
    };
    let site = InvocationSite::new(&db, &invocation);
    assert_eq!(site.claim(), &claim);
    assert_eq!(site.receiver_type(), None);
    assert_eq!(site.argument_types(), &[TypeId::int(&db)]);
    assert_eq!(site.types().int(), TypeId::int(&db));
}
```

Add `InvocationSite` to the test module's `use super::{...}` list.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_types dynamic_type_provider`
Expected: compile FAIL — `InvocationSite` not found.

- [ ] **Step 3: Implement `InvocationSite` and reshape the trait**

In `crates/celerrate_types/src/dynamic_type_provider.rs`, after the `Invocation` struct:

```rust
/// The call-scoped context a dynamic-type provider answers from: the
/// invocation plus the sealed type facade. Owns the database
/// privately — a provider can neither name nor obtain
/// `salsa::Database` (the WASM-projectable shape, sketch section 7).
/// Constructed only by the engine's consumption points.
pub struct InvocationSite<'db, 'call> {
    db: &'db dyn salsa::Database,
    invocation: &'call Invocation<'db>,
}

impl<'db, 'call> InvocationSite<'db, 'call> {
    pub(crate) fn new(db: &'db dyn salsa::Database, invocation: &'call Invocation<'db>) -> Self {
        Self { db, invocation }
    }

    pub fn claim(&self) -> &'call SymbolClaim {
        &self.invocation.claim
    }

    pub fn receiver_type(&self) -> Option<TypeId<'db>> {
        self.invocation.receiver_type
    }

    pub fn argument_types(&self) -> &'call [TypeId<'db>] {
        &self.invocation.argument_types
    }

    /// The sealed type facade. Call-scoped like the site itself.
    pub fn types(&self) -> crate::type_context::TypeContext<'db> {
        crate::type_context::TypeContext::new(self.db)
    }
}
```

Reshape the trait methods (keep the whole existing rustdoc contract — purity, monotonicity, `None` fallback — moving the sentence about `Invocation` to mention `InvocationSite`):

```rust
pub trait DynamicTypeProvider: Send + Sync {
    fn claims(&self) -> Vec<SymbolClaim>;
    fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>>;
    fn by_reference_types<'db>(&self, site: &InvocationSite<'db, '_>) -> Vec<(usize, TypeId<'db>)> {
        let _ = site;
        Vec::new()
    }
}
```

- [ ] **Step 4: Migrate the consumption points in `flow.rs`**

In `provider_return` (`crates/celerrate_types/src/flow.rs`, around line 1215):

```rust
if let Some(answer) = registration.provider.return_type(
    &crate::dynamic_type_provider::InvocationSite::new(db, &invocation),
) {
```

In `provider_by_reference` (around line 1247):

```rust
let contributions = registration.provider.by_reference_types(
    &crate::dynamic_type_provider::InvocationSite::new(db, &invocation),
);
```

The `Invocation` construction just above each call stays as it is.

- [ ] **Step 5: Migrate the in-crate test fakes**

`FakeProvider` in `dynamic_type_provider.rs` tests:

```rust
impl DynamicTypeProvider for FakeProvider {
    fn claims(&self) -> Vec<SymbolClaim> {
        self.claimed.clone()
    }
    fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>> {
        Some(site.types().int())
    }
}
```

The existing `the_by_reference_channel_defaults_to_empty` test becomes:

```rust
assert!(
    provider
        .by_reference_types(&InvocationSite::new(&db, &invocation))
        .is_empty()
);
```

`SymbolicProvider` in `inference.rs` (around line 2227): apply the same signature change — replace `db: &'db dyn salsa::Database, invocation: &Invocation<'db>` with `site: &InvocationSite<'db, '_>`, and inside the body replace reads of `invocation.claim` / `invocation.receiver_type` / `invocation.argument_types` with `site.claim()` / `site.receiver_type()` / `site.argument_types()`, and every `TypeId::xxx(db, ...)` with `site.types().xxx(...)`. Import `InvocationSite` where `Invocation` is imported.

- [ ] **Step 6: Migrate `celerrate_stdlib_provider`**

`crates/celerrate_stdlib_provider/src/lib.rs`: change the import line 15 to

```rust
use celerrate_plugin::{DynamicTypeProvider, InvocationSite, SymbolClaim, TypeContext, TypeId};
```

(`TypeContext` and `InvocationSite` reach the facade in Task 5; until then, if the build objects, import them from `celerrate_types` temporarily is NOT allowed — instead add them to `celerrate_plugin`'s re-export list now, alongside the existing `Invocation` entry, and let Task 5 do the removals only.)

The trait implementation becomes:

```rust
impl DynamicTypeProvider for StdlibProvider {
    fn claims(&self) -> Vec<SymbolClaim> { /* unchanged */ }

    fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>> {
        let SymbolClaim::Function { key } = site.claim() else {
            return None;
        };
        function_return(site.types(), key, site.argument_types())
    }

    fn by_reference_types<'db>(&self, site: &InvocationSite<'db, '_>) -> Vec<(usize, TypeId<'db>)> {
        let SymbolClaim::Function { key } = site.claim() else {
            return Vec::new();
        };
        match key.as_str() {
            "preg_match" => pattern_functions::preg_match_matches(site.types(), site.argument_types()),
            _ => Vec::new(),
        }
    }
}
```

(Adapt the exact match arms to the current bodies at `lib.rs:60-100` — the transformation rule is uniform: `db: &'db dyn salsa::Database` parameters become `context: TypeContext<'db>`; `invocation.argument_types` becomes `site.argument_types()`.)

In `function_return` and in every helper of `array_functions.rs`, `json_functions.rs`, `pattern_functions.rs`, `string_functions.rs`:

- Parameter `db: &'db dyn salsa::Database` → `context: TypeContext<'db>`.
- Every `TypeId::xxx(db, ...)` → `context.xxx(...)`.
- Every interrogation `value.method(db)` → `context.method(value)` (the surface: `array_key`, `array_value`, `bool_literal_value`, `callable_return`, `constituents`, `float_literal_value`, `int_literal_value`, `is_list`, `is_null`, `string_literal_value`).
- Imports: `use celerrate_plugin::{TypeContext, TypeId};` replaces `use celerrate_plugin::{TypeId, salsa};`.
- Test modules (dev-scope, exempt from the dependency rule) keep constructing `Invocation` — they now import it from `celerrate_types` (already a dev-dependency) instead of the facade, and call the trait through `InvocationSite::new`... which is `pub(crate)` to `celerrate_types`. **They cannot.** The test helper `function_invocation` (`lib.rs:113`) therefore changes strategy: the tests exercise the public trait surface through the engine seam the end-to-end tests already use, OR the helpers under test (`function_return` and the module functions) are called directly with a `TypeContext`. `TypeContext::new` is also `pub(crate)`. Resolution, fixed here: `celerrate_types` exposes a **test-only constructor** behind a feature-gate-free, clearly named function in its `testing` support: add to `crates/celerrate_types/src/type_context.rs`:

```rust
/// Test-only construction seam for out-of-crate test suites (the
/// plugin crates' dev-dependencies). Not part of the plugin API:
/// plugin production code cannot reach it because plugin crates
/// cannot depend on `celerrate_types` (the dependency-shape check).
pub fn testing_type_context<'db>(db: &'db dyn salsa::Database) -> TypeContext<'db> {
    TypeContext::new(db)
}
```

and export it from `celerrate_types`'s lib (`pub use type_context::testing_type_context;`). The stdlib provider's unit tests then build contexts with `celerrate_types::testing_type_context(&db)` and call the module helpers directly; `function_invocation` and any test that needs an `InvocationSite` route through an equivalent `celerrate_types` testing seam: add next to it in `dynamic_type_provider.rs`:

```rust
/// Test-only construction seam, same contract as
/// `testing_type_context`.
pub fn testing_invocation_site<'db, 'call>(
    db: &'db dyn salsa::Database,
    invocation: &'call Invocation<'db>,
) -> InvocationSite<'db, 'call> {
    InvocationSite::new(db, invocation)
}
```

exported as `pub use dynamic_type_provider::testing_invocation_site;`.

- [ ] **Step 7: Run the full test suite**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: all PASS — the stdlib provider's behavioral tests (array, json, pattern, string) pass unchanged, proving the rework is pure boundary.

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_types crates/celerrate_stdlib_provider crates/celerrate_plugin
git commit -m "♻️ refactor(types): InvocationSite seals the dynamic-type-provider boundary"
```

---

### Task 4: Migrate the PHPDoc bridge off `database()`

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/lowering.rs`, `crates/celerrate_phpdoc_bridge/src/syntax.rs`
- Modify: `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs:123`
- Modify: `crates/celerrate_types/tests/invalidation_scope.rs:588,1432`

**Interfaces:**
- Consumes: `AnnotationSite::types()` (Task 2), the `TypeContext` methods (Task 1).
- Produces: a bridge with zero `site.database()` calls and zero `salsa` imports. Task 5's sealing compiles because of this task.

- [ ] **Step 1: Migrate `lowering.rs`**

The transformation rule, applied to every function in the file:

- The import line 41-42 becomes:

```rust
use celerrate_plugin::{
    AnnotationSite, CallableParameter, ParsedAncestor, ShapeField, ShapeKey, TypeContext, TypeId,
};
```

(`TypeContext` is on the facade since Task 3 Step 6.)

- `LoweringScope::declare_template(&mut self, db: &'db dyn salsa::Database, ...)` → `(&mut self, context: TypeContext<'db>, ...)`; its body's `TypeId::template(db, scope_key, &name, bound)` → `context.template(scope_key, &name, bound)`.
- Every `let db = site.database();` (lines 91, 162, 244, 327, 384, 411, 449) → `let context = site.types();`.
- Every free function taking `db: &'db dyn salsa::Database` (for example `array_key` at line 153, `lower_dialect_name` at line 186) → `context: TypeContext<'db>`.
- Every `TypeId::xxx(db, ...)` → `context.xxx(...)`; every `TypeId::xxx(db)` → `context.xxx()`.
- Every interrogation `value.class_name(db)` / `value.class_arguments(db)` → `context.class_name(value)` / `context.class_arguments(value)`.

The compiler drives completeness: after the import change, every leftover site fails to build. Fix them all; introduce **no** new `salsa` mention.

- [ ] **Step 2: Migrate `syntax.rs` and the test suites**

- `syntax.rs:151` `let db = site.database();` → `let context = site.types();`, then apply the same `TypeId::xxx(db, ...)` → `context.xxx(...)` rule in that function.
- `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs:123`: same substitution inside the fake (`let db = site.database();` → `let context = site.types();` plus builder rewrites).
- `crates/celerrate_types/tests/invalidation_scope.rs:588`: `TypeId::class(site.database(), &site.qualify_class_name(word), Vec::new())` → `site.types().class(&site.qualify_class_name(word), Vec::new())`; line 1432's `let db = site.database();` likewise becomes `let context = site.types();` with the follow-on rewrites.

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: all PASS — the bridge's snapshot and end-to-end suites pass unchanged (zero behavioral regression).

Run: `grep -rn "database()\|salsa" crates/celerrate_phpdoc_bridge/src/`
Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_phpdoc_bridge crates/celerrate_types/tests
git commit -m "♻️ refactor(bridge): lower through the sealed TypeContext facade"
```

---

### Task 5: Seal the boundary

**Files:**
- Modify: `crates/celerrate_types/src/type_syntax.rs` (demote `database()`)
- Modify: `crates/celerrate_plugin/src/lib.rs` (purge the re-exports)

**Interfaces:**
- Consumes: Tasks 1-4 (no remaining out-of-crate `database()` caller, no plugin `salsa` use).
- Produces: the final facade surface. `celerrate_plugin` re-exports exactly: the semantics vocabulary (unchanged line), and from `celerrate_types`: `AnnotationSite`, `AssertionPolarity`, `CallableParameter`, `DynamicTypeProvider`, `InvocationSite`, `ParsedAncestor`, `ParsedAnnotations`, `ParsedAssertion`, `ParsedTemplate`, `ShapeField`, `ShapeKey`, `SymbolClaim`, `Trust`, `TypeContext`, `TypeId`, `TypeSyntax`. Gone: `Invocation`, `salsa`, `diagnostics`, `source`.

- [ ] **Step 1: Demote `database()`**

In `crates/celerrate_types/src/type_syntax.rs`, change:

```rust
/// The database, for `TypeId` builders. Never retain it.
pub fn database(&self) -> &'db dyn salsa::Database {
```

to:

```rust
/// Engine-internal escape hatch for in-crate test fakes. Sealed:
/// plugins go through `types()` — they can neither name nor obtain
/// `salsa::Database` (the structural enforcement issue #61 demanded).
pub(crate) fn database(&self) -> &'db dyn salsa::Database {
```

- [ ] **Step 2: Purge the facade re-exports**

In `crates/celerrate_plugin/src/lib.rs`, replace lines 33-44 (the `celerrate_types` re-export and the wholesale block) with:

```rust
// The type-syntax and dynamic-type-provider extension points, and the
// type vocabulary plugins construct and interrogate through. Nominal
// re-exports only: never `salsa`, never a whole crate — the boundary
// surface is enumerable by reading this list.
pub use celerrate_types::{
    AnnotationSite, AssertionPolarity, CallableParameter, DynamicTypeProvider, InvocationSite,
    ParsedAncestor, ParsedAnnotations, ParsedAssertion, ParsedTemplate, ShapeField, ShapeKey,
    SymbolClaim, Trust, TypeContext, TypeId, TypeSyntax,
};
```

(The `celerrate_semantics` re-export block stays as it is. If Task 3 Step 6 already edited this list, reconcile to exactly the list above.) Delete the three wholesale lines:

```rust
pub use celerrate_diagnostics as diagnostics;
pub use celerrate_source as source;
pub use salsa;
```

Remove `celerrate_diagnostics` and `celerrate_source` from `crates/celerrate_plugin/Cargo.toml` `[dependencies]` if nothing else in the crate uses them (check `grep -n "diagnostics\|source" crates/celerrate_plugin/src/lib.rs` — expected: nothing left). Keep `salsa` as a dependency only if the crate itself still names it (expected: it does not; remove it).

- [ ] **Step 3: Verify the seal**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: all PASS.

Run: `grep -rn "salsa" crates/celerrate_phpdoc_bridge/src crates/celerrate_stdlib_provider/src crates/celerrate_plugin/src`
Expected: no matches — no plugin-reachable path names salsa.

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_types crates/celerrate_plugin
git commit -m "♻️ refactor(plugin): seal the facade to nominal boundary re-exports"
```

---

### Task 6: `#[non_exhaustive]` on the boundary vocabulary

**Files:**
- Modify: `crates/celerrate_types/src/dynamic_type_provider.rs` (`Invocation`, `SymbolClaim`)
- Modify: `crates/celerrate_types/src/type_syntax.rs` (`ParsedAnnotations`, `ParsedAssertion`, `ParsedTemplate`, `ParsedAncestor`)
- Modify: `crates/celerrate_plugin/src/lib.rs` (`PluginDescriptor`)
- Modify: `crates/celerrate_semantics/src/virtual_symbols.rs` (`VirtualMember`, `VirtualParameter`)
- Modify: `crates/celerrate_phpdoc_bridge/src/syntax.rs`, `tags.rs`, `virtual_members.rs` (literal constructions → `default()` + field mutation, or constructors)
- Modify: `crates/celerrate_stdlib_provider/src/lib.rs` tests (`Invocation` literals → `Invocation::new`)

**Interfaces:**
- Consumes: the sealed vocabulary of Task 5.
- Produces: `Invocation::new(claim: SymbolClaim, receiver_type: Option<TypeId<'db>>, argument_types: Vec<TypeId<'db>>) -> Invocation<'db>`; `VirtualParameter::new(name: String) -> VirtualParameter` and `VirtualMember::new(kind: VirtualMemberKind, name: String) -> VirtualMember` (remaining fields set by mutation) — only if the bridge constructs them as literals; check first, and skip any constructor nothing needs.

- [ ] **Step 1: Annotate and add constructors**

Add `#[non_exhaustive]` directly above the `pub struct`/`pub enum` line of: `Invocation`, `SymbolClaim` (the enum — variants stay constructible; out-of-crate exhaustive matches now need a `_` arm), `ParsedAnnotations`, `ParsedAssertion`, `ParsedTemplate`, `ParsedAncestor`, `PluginDescriptor`, `VirtualMember`, `VirtualParameter`.

Add to `impl<'db> Invocation<'db>` (create the impl block if missing):

```rust
/// Constructor for the engine and for test suites: cross-crate
/// literal construction is closed by `#[non_exhaustive]`.
pub fn new(
    claim: SymbolClaim,
    receiver_type: Option<TypeId<'db>>,
    argument_types: Vec<TypeId<'db>>,
) -> Self {
    Self { claim, receiver_type, argument_types }
}
```

- [ ] **Step 2: Let the compiler enumerate the breakage, fix each site**

Run: `cargo test --workspace 2>&1 | grep -E "^error" | head -30`

Fix every reported site by category:

- `flow.rs` (same crate as `Invocation`): literals still compile — leave them or switch to `Invocation::new`, whichever the compiler forces (same-crate literals stay legal; keep them).
- Bridge `ParsedAnnotations { ... }` literals (`syntax.rs`): rewrite as

```rust
let mut annotations = ParsedAnnotations::default();
annotations.return_type = ...;
// (one assignment per field the literal set; fields stay `pub`)
```

- Bridge `ParsedAssertion`/`ParsedTemplate`/`ParsedAncestor`/`VirtualMember`/`VirtualParameter` literals: same crate-boundary rule. Where a struct has no `Default` and the bridge builds it whole (for example `ParsedAssertion` with all four fields), prefer adding a plain `pub fn new(...)` taking every field, next to the struct, over derives — additive and explicit. Mirror the `Invocation::new` shape exactly.
- Stdlib tests' `Invocation { ... }` (`lib.rs:117`): → `Invocation::new(claim, receiver_type, argument_types)` with `Invocation` imported from `celerrate_types` (dev-dependency).
- Any out-of-crate exhaustive `match` on `SymbolClaim` now needs a `_` arm — expected in the stdlib provider (`return_type`'s `let SymbolClaim::Function { key } else` form already tolerates it; adjust only what the compiler names).

- [ ] **Step 3: Run the full suite**

Run: `cargo test --workspace 2>&1 | tail -10` then `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: all PASS, clippy clean.

- [ ] **Step 4: Commit**

```bash
git add crates/
git commit -m "♻️ refactor(plugin): non_exhaustive boundary vocabulary backs additive extension"
```

---

### Task 7: Extend the dependency-shape check

**Files:**
- Modify: `xtask/src/dependency_shape.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `check()` also rejects `salsa` in the non-dev dependencies of a plugin crate.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module of `xtask/src/dependency_shape.rs`:

```rust
#[test]
#[allow(clippy::unwrap_used)]
fn a_direct_salsa_dependency_fails_even_though_it_is_not_a_workspace_crate() {
    // The sealing is only mechanical if the check also closes the
    // external route: a plugin adding salsa directly would recover
    // the database handle the facade hides.
    let value = metadata(serde_json::json!([
        package(
            "celerrate_phpdoc_bridge",
            serde_json::json!([
                { "name": "celerrate_plugin", "kind": null },
                { "name": "salsa", "kind": null },
            ])
        ),
        package(
            "celerrate_stdlib_provider",
            serde_json::json!([{ "name": "celerrate_plugin", "kind": null }])
        ),
    ]));
    let error = check(&value).unwrap_err().to_string();
    assert!(error.contains("celerrate_phpdoc_bridge"));
    assert!(error.contains("salsa"));
}

#[test]
fn a_dev_scoped_salsa_dependency_stays_exempt() {
    let value = metadata(serde_json::json!([
        package(
            "celerrate_phpdoc_bridge",
            serde_json::json!([
                { "name": "celerrate_plugin", "kind": null },
                { "name": "salsa", "kind": "dev" },
            ])
        ),
        package(
            "celerrate_stdlib_provider",
            serde_json::json!([{ "name": "celerrate_plugin", "kind": null }])
        ),
    ]));
    assert!(check(&value).is_ok());
}
```

- [ ] **Step 2: Run the tests to verify the first fails**

Run: `cargo test --package xtask dependency_shape`
Expected: `a_direct_salsa_dependency_fails...` FAILS (check currently passes salsa through); the dev-exemption test passes.

- [ ] **Step 3: Implement**

In `xtask/src/dependency_shape.rs`, next to `ALLOWED_DEPENDENCY`:

```rust
/// External crates a plugin crate must not depend on directly: the
/// boundary sealing (issue #61) is only mechanical if the external
/// route to the database handle is closed too.
const FORBIDDEN_EXTERNAL_DEPENDENCIES: &[&str] = &["salsa"];
```

In the dependency loop, after the dev-kind exemption `continue`, before the `celerrate_` check:

```rust
if FORBIDDEN_EXTERNAL_DEPENDENCIES.contains(&dependency_name) {
    return Err(format!(
        "dependency shape violated: {name} depends on {dependency_name} directly; \
         plugin crates reach the engine only through {ALLOWED_DEPENDENCY}",
    )
    .into());
}
```

Update the module docblock's first sentence to mention the external rule.

- [ ] **Step 4: Run the tests and the real check**

Run: `cargo test --package xtask dependency_shape`
Expected: all PASS.

Run: `cargo run --package xtask -- dependency-shape` (use the exact subcommand name from `xtask/src/main.rs`)
Expected: exit 0 — the real workspace satisfies the extended rule (the plugin crates' `salsa.workspace = true` entries are dev-scoped).

- [ ] **Step 5: Commit**

```bash
git add xtask/src/dependency_shape.rs
git commit -m "✅ test(xtask): dependency shape also forbids a direct salsa dependency"
```

---

### Task 8: Sketch amendment, rustdoc, CHANGELOG

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md` (sections 5 and 7)
- Modify: `CHANGELOG.md` (`[Unreleased]`)

**Interfaces:**
- Consumes: the final shapes of Tasks 3 and 5.
- Produces: documentation matching the code; nothing downstream.

- [ ] **Step 1: Amend the WASM sketch**

In `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`:

- Section 5, last bullet: replace "`AnnotationSite` and `Invocation` are borrowed per call and never stored" with "`AnnotationSite` and `InvocationSite` are borrowed per call and never stored, and both own the database privately behind the sealed `TypeContext` facade".
- Section 7 table, `DynamicTypeProvider` row: guest exports become `claims() -> list`, `return_type(site) -> type?`, `by_reference_types(site) -> list<(index, handle)>`.
- Append to the end of the file:

```markdown
## 9. Amendment 2026-07-19 — the native tier now enforces the shape

Issue #61: the native traits leaked `&dyn salsa::Database`
(`DynamicTypeProvider` took it as a parameter; `AnnotationSite`
exposed it via `database()`), so "every native trait projects onto
the sketch without reshaping" was false — the guest export
`return_type(invocation)` had no database parameter to project.

Resolved by the boundary sealing
(`.claude/superpowers/specs/2026-07-19-plugin-boundary-sealing-design.md`):
the sealed `TypeContext` facade carries the construction and
interrogation families of section 6; `InvocationSite` replaces the
raw `db + Invocation` pair; the facade re-exports are nominal and
`salsa` is unnameable from a plugin crate (extended
`dependency_shape` check). What section 5 called "the native tier
enforces it by review" is now structural: retention is a compile
error (`'static` implementation bound), and the database handle is
unreachable. The acceptance property holds in shape, not just in
intent.
```

- [ ] **Step 2: CHANGELOG entry**

Under `## [Unreleased]` in `CHANGELOG.md`, add a `### Changed` section (above the existing `### Fixed`):

```markdown
### Changed

- The plugin API boundary is sealed (issue #61): `DynamicTypeProvider`
  receives a call-scoped `InvocationSite` instead of a raw salsa
  database handle, `AnnotationSite` no longer exposes `database()`,
  and both sites hand out a sealed `TypeContext` facade for type
  construction and interrogation. `celerrate_plugin` now re-exports
  boundary vocabulary nominally — no more `salsa` or whole-crate
  re-exports — and the boundary structs are `#[non_exhaustive]`.
  Breaking for the v0 plugin API; `PLUGIN_API_VERSION` stays 0.
```

- [ ] **Step 3: Final full verification**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: everything clean.

- [ ] **Step 4: Commit**

```bash
git add .claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md CHANGELOG.md
git commit -m "📝 docs(spec): record the plugin boundary sealing in the sketch and changelog"
```

---

## Self-Review Notes

- Spec coverage: 3.1 → Task 1; 3.2 → Tasks 2-4; 3.3 → Tasks 5-7; 3.4 → Tasks 3-4 (migration), 8 (sketch, CHANGELOG). The spec's "database() is removed" is implemented as a `pub(crate)` demotion (Task 5): sealing-equivalent for plugins, and it spares every in-crate test fake a rewrite — recorded here as the deliberate mechanism.
- The spec's symbol-lookup family is deliberately absent from `TypeContext` v0: no plugin consumes it today (the YAGNI criterion of spec section 3.1); the facade extends when the first consumer appears.
- Type consistency: `TypeContext::new` (Tasks 1-3), `types()` on both sites (Tasks 2-3), `InvocationSite::new` (Task 3), `Invocation::new` (Task 6), `testing_type_context`/`testing_invocation_site` (Task 3 Step 6) are the shared names; later tasks use them verbatim.
