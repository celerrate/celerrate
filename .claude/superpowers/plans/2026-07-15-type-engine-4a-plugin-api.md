# Type Engine 4a — Plugin API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `celerrate_plugin` (the aggregation facade), the
extension-point registries with their fixed dispatch rules, the
`celerrate_phpdoc_bridge` crate with its docblock lexer and complete
standard-PHPDoc coverage (`@param`, `@return`, `@var`, `@throws`,
`@property`, `@method` — the last two as virtual members), fill the
annotation seams plan 3 left open, and add the CI dependency-shape
check.

**Architecture:** Extension points stay owned by their consuming
layers: `celerrate_types` owns the type-syntax and
dynamic-type-provider traits and their registry salsa inputs;
`celerrate_semantics` owns the virtual-symbol trait and its registry.
Registries follow the shipped `ArtifactCacheInput` pattern
(`#[salsa::input(singleton)]` holding `Arc<dyn Trait>` handles,
registered once at the composition root at HIGH durability, consulted
with `try_get` — an unset registry is the no-plugin path every test
database takes by default). `celerrate_plugin` is a re-export facade
above `celerrate_types`; `celerrate_phpdoc_bridge` depends on
`celerrate_plugin` and nothing else in the workspace, enforced by a new
`cargo xtask dependency-shape` check. `celerrate_cli` registers the
bridge at the composition root and gains its first dependency on
`celerrate_types` through the facade.

**Tech Stack:** Rust, salsa 0.27 (`#[salsa::input(singleton)]`,
`#[salsa::tracked]`, `#[salsa::interned]`), hand-written recursive
descent for the docblock grammar, cargo-fuzz (libfuzzer) for the lexer
fuzz target, `cargo metadata` + serde_json in xtask.

**Design source:**
`.claude/superpowers/specs/2026-07-14-type-engine-design.md` sections 4
(the plugin API — the core of this plan), 5 (the bridge: standard
PHPDoc, virtual members, malformed-annotation posture; the PHPStan
dialect and suppressions are plans 4b/4c), 10 (docblock lexer fuzzing),
11 (plan "4a — Plugin API"). Inherited seams: plan 3's
`member_annotations` default body
(`crates/celerrate_types/src/declared.rs:172-185`), plan 3's
"function annotations have no seam yet" debt, plan 1a's deferred
second-stage docblock cutoff.

## Global Constraints

- **Zero panic, mechanically enforced**: Clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide;
  `unsafe_code` is forbidden. Production code returns `Result` or
  `Option`; test modules open with `#![allow(clippy::unwrap_used)]`
  (add `indexing_slicing`/`panic` allows only where a test needs
  them). Docblock text is user input: the lexer and parsers degrade to
  empty/`None`, never panic (fuzzed).
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **Strict layering**: the DAG gains two crates.
  `celerrate_plugin` depends on `celerrate_types`,
  `celerrate_semantics`, `celerrate_source`, `celerrate_diagnostics`,
  `salsa` (re-export facade — no logic). `celerrate_phpdoc_bridge`
  depends on `celerrate_plugin` **only** (normal and build
  dependencies; dev-dependencies are test-only and exempt, recorded).
  `celerrate_semantics` still must not depend on `celerrate_types`.
  Registry inputs live in the owning crates, never in
  `celerrate_plugin`.
- **No docblock diagnostics**: a malformed annotation is silently
  ignored (per construct, never per docblock). No new `CEL####`
  identifiers are allocated by this plan.
- **Determinism**: registration order is declared at the composition
  root; type-syntax dispatch is registered order + can-parse first
  win; provider claims are validated for overlap at registration.
  No wall clock, no environment reads, no iteration-order-dependent
  results inside queries.
- Everything in English, full words, no abbreviated names (standard
  acronyms fine).
- Commits: gitmoji + Conventional Commits
  (`✨ feat(plugin): …`, `✨ feat(phpdoc-bridge): …`), authored with the
  repository-configured identity — never override git identity, no
  Claude attribution anywhere.
- Local commands: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`, `cargo deny check`.
- Salsa pattern: free `#[salsa::tracked]` functions over
  `db: &dyn salsa::Database` plus the three inputs (`AnalyzedFileSet`,
  `StubIndexInput`, `ProjectConfiguration`); singleton registry inputs
  mirror `ArtifactCacheInput`
  (`crates/celerrate_semantics/src/cache.rs:24-49`): `Arc<dyn Trait>`
  in a `Clone` handle with a manual `Debug`, `try_get` consultation,
  HIGH durability at registration, set once per process, never
  mutated.

## Design decisions fixed by this plan

1. **Registries are singleton inputs consulted with `try_get`.** The
   design's "registry salsa inputs live in the owning crates" is
   implemented as one `#[salsa::input(singleton)]` per extension
   point, exactly like `ArtifactCacheInput`. Reading a registration
   reads the whole input, so the implementation travels in the same
   input as its identity (name, version, configuration) and every
   consumer records the dependency the design requires. These inputs
   never backdate (trait objects have no `Eq`) — acceptable because
   they are set once per process, before any query runs.
2. **`PluginIdentity` lives in `celerrate_semantics`.** Both owning
   crates need it and `celerrate_semantics` is the lower of the two;
   `celerrate_types` reuses it, `celerrate_plugin` re-exports it. The
   API version constant lives in `celerrate_plugin` (it describes the
   facade's surface) and is checked only at the composition root, so
   the owning crates never see it and the DAG holds.
3. **Exclusion, never a crash.** An API-version mismatch or a
   dynamic-provider claim conflict excludes the plugin (or the
   later-registered claimant) for the whole run; the run is reported
   degraded with one warning line on stderr. For compiled-in
   first-party plugins the version check cannot fail — dormant
   scaffolding, per the design's own wording.
4. **Virtual members are a separate list, and real members shadow
   them.** `LinearizedClass` gains `virtual_members` next to
   `members`; `MemberResolution` gains a `Virtual` variant; lookup
   consults source members, then stub members, then virtual members.
   Recorded divergence: PHPStan lets `@method` override a real
   method's *signature*; here existence is the only consumer until
   plan 8, and the real member wins — revisited with plan 4b if the
   corpus demands it.
5. **The trait returns both `@return` and `@var`;**
   `celerrate_types` picks per member kind (`Method` → return,
   `Property`/`ClassConstant` → var, `EnumCase` → none, functions →
   return). The trait stays subject-agnostic and object-safe.
6. **The 4a expression grammar is the standard notation only**:
   names, `?` prefix, `|`, `&`, `()`, `Type[]` suffix. No generics,
   shapes, literals, or `class-string<T>` — that is plan 4b's PHPStan
   dialect, which extends the same parser behind the same trait.
   Because 4a expressions contain no spaces, tag contents split on
   whitespace; 4b replaces the splitter together with the grammar.
7. **One keyword table.** `AnnotationSite::keyword_type` delegates to
   the native lowering's keyword table
   (`crates/celerrate_types/src/written.rs`, `lower_keyword`), so the
   annotation path and the native path can never disagree on `int`,
   `self`, `static`, `iterable`, and friends.
8. **The bridge answers `can_parse` with `true`.** It owns the
   inherited PHPDoc notation and registers first; the can-parse
   protocol itself is exercised by unit tests with fake
   implementations, so the dispatch rule is pinned even though the
   bridge is the only real implementation this sub-project.
9. **`MemberAnnotations` gains `throws`.** Standard PHPDoc is
   "complete" per the design; `@throws` is parsed, resolved, stored,
   and inherited element-wise like the value annotation. No consumer
   until a later sub-project — recorded, not invented.
10. **Virtual members contribute no magic markers.** A
    `@method __call` declaration does not set suppression markers —
    recorded debt, plan 8 decides whether the corpus needs it.

## File structure

- Create: `crates/celerrate_semantics/src/plugin.rs` (PluginIdentity)
- Create: `crates/celerrate_semantics/src/virtual_symbols.rs` (trait,
  payload, registry)
- Modify: `crates/celerrate_semantics/src/lib.rs`,
  `crates/celerrate_semantics/src/linearize.rs`,
  `crates/celerrate_semantics/src/member_lookup.rs`
- Create: `crates/celerrate_types/src/type_syntax.rs` (trait,
  `ParsedAnnotations`, `AnnotationSite`, registry, dispatch)
- Create: `crates/celerrate_types/src/dynamic_type_provider.rs`
  (trait, claims, `Invocation`, registry, claim validation)
- Modify: `crates/celerrate_types/src/lib.rs`,
  `crates/celerrate_types/src/declared.rs`,
  `crates/celerrate_types/tests/invalidation_scope.rs`
- Create: `crates/celerrate_plugin/` (facade, one `src/lib.rs`)
- Create: `crates/celerrate_phpdoc_bridge/` (`src/lib.rs`,
  `src/lexer.rs`, `src/expression.rs`, `src/tags.rs`,
  `src/syntax.rs`, `src/virtual_members.rs`, `tests/end_to_end.rs`)
- Create: `crates/celerrate_cli/src/plugins.rs`; modify
  `crates/celerrate_cli/src/session.rs`, `src/lib.rs` (or
  `main.rs` module list), `Cargo.toml`
- Create: `xtask/src/dependency_shape.rs`; modify `xtask/src/lib.rs`,
  `xtask/src/main.rs`, `.github/workflows/ci.yml`
- Create: `fuzz/fuzz_targets/docblock.rs`,
  `fuzz/corpus/docblock/seed`; modify `fuzz/Cargo.toml`,
  `.github/workflows/fuzz.yml`

## Task sizing note

Tasks follow the required step pattern (failing test → verify fail →
implement → verify pass → commit). Where a task lists several tests,
write them all in step 1 and drive them green together — they pin one
coherent surface. After every task: `cargo test --workspace`, clippy,
fmt before the commit.

---

### Task 1: `PluginIdentity` and the virtual-symbol extension point

**Files:**
- Create: `crates/celerrate_semantics/src/plugin.rs`
- Create: `crates/celerrate_semantics/src/virtual_symbols.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (module list +
  re-exports)

**Interfaces:**
- Consumes: nothing new (`salsa`, `std::sync::Arc`).
- Produces: `PluginIdentity { name, version, configuration: String }`;
  `VirtualMemberKind { Method, Property }`;
  `VirtualParameter { name, type_text: Option<String>, optional,
  variadic: bool }`;
  `VirtualMember { kind, name, is_static, type_text: Option<String>,
  parameters: Vec<VirtualParameter> }`;
  `trait VirtualSymbolProvider: Send + Sync { fn virtual_members(&self,
  class_docblock: &str) -> Vec<VirtualMember>; }`;
  `VirtualSymbolRegistration { identity, provider: Arc<dyn
  VirtualSymbolProvider> }`;
  `VirtualSymbolRegistry` (singleton input, field
  `registrations: Vec<VirtualSymbolRegistration>`, `#[returns(ref)]`).
  Tasks 2, 12, 14, 16 consume all of these under exactly these names.

- [x] **Step 1: Write the failing tests**

In `crates/celerrate_semantics/src/virtual_symbols.rs` (tests module):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use celerrate_db::testing::TestDatabase;

    #[derive(Debug)]
    struct FakeProvider {
        members: Vec<VirtualMember>,
    }

    impl VirtualSymbolProvider for FakeProvider {
        fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
            if class_docblock.contains("@fake") {
                self.members.clone()
            } else {
                Vec::new()
            }
        }
    }

    fn identity(name: &str) -> PluginIdentity {
        PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    #[test]
    fn an_unset_registry_is_the_no_plugin_path() {
        let db = TestDatabase::default();
        assert!(VirtualSymbolRegistry::try_get(&db).is_none());
    }

    #[test]
    fn a_registered_provider_answers_through_the_registry() {
        let db = TestDatabase::default();
        let member = VirtualMember {
            kind: VirtualMemberKind::Property,
            name: "title".to_owned(),
            is_static: false,
            type_text: Some("string".to_owned()),
            parameters: Vec::new(),
        };
        VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider { members: vec![member.clone()] }),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&db);

        let registry = VirtualSymbolRegistry::try_get(&db).unwrap();
        let registrations = registry.registrations(&db);
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].identity.name, "fake");
        assert_eq!(
            registrations[0].provider.virtual_members("/** @fake */"),
            vec![member],
        );
        assert!(
            registrations[0]
                .provider
                .virtual_members("/** plain prose */")
                .is_empty(),
        );
    }
}
```

(The `registrations[0]` indexing needs the test module's
`#![allow(clippy::indexing_slicing)]` — add it next to the unwrap
allow.)

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics virtual_symbols 2>&1 | tail -5`
Expected: FAIL to compile (module does not exist).

- [x] **Step 3: Implement**

`crates/celerrate_semantics/src/plugin.rs`:

```rust
//! Plugin identity: the vocabulary shared by every extension-point
//! registry. Defined here because `celerrate_semantics` is the lowest
//! crate that owns a registry; `celerrate_types` reuses it and
//! `celerrate_plugin` re-exports it.

/// The identity of one registered plugin. It travels in the same
/// salsa input as the implementation it identifies, so every read of
/// the implementation records a dependency on the identity too: a
/// version bump or a reconfiguration invalidates exactly like an
/// implementation change would, and the persistent cache's plugin-set
/// key (plan 9a) reads the same fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginIdentity {
    /// The public plugin name (for the bridge: `phpdoc-bridge`).
    pub name: String,
    /// The plugin's own version, distinct from the API version the
    /// composition root checks at registration.
    pub version: String,
    /// The plugin's configuration, serialized deterministically by
    /// the composition root. Part of the identity: a reconfigured
    /// plugin is a different plugin as far as invalidation goes.
    pub configuration: String,
}
```

`crates/celerrate_semantics/src/virtual_symbols.rs`:

```rust
//! The virtual-symbol extension point: members declared by annotation
//! rather than written in code (`@property`, `@method`).
//!
//! Owned by this crate per the design: the registry input lives with
//! the consuming layer, implementations are registered at the
//! composition root, `celerrate_plugin` re-exports the vocabulary.
//! Type expressions travel as **unresolved text** — this layer sits
//! below `celerrate_types` and cannot name `TypeId`; the text
//! resolves downstream through the type-syntax extension point
//! exactly like a real member's signature text.

use std::sync::Arc;

use crate::plugin::PluginIdentity;

/// The member kinds an annotation can declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VirtualMemberKind {
    Method,
    Property,
}

/// One parameter of a virtual method (`@method`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VirtualParameter {
    pub name: String,
    /// The written type expression, unresolved.
    pub type_text: Option<String>,
    /// True when the annotation gives a default value.
    pub optional: bool,
    pub variadic: bool,
}

/// One member declared by a class-like's docblock.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VirtualMember {
    pub kind: VirtualMemberKind,
    /// Original spelling (property names without the `$`).
    pub name: String,
    pub is_static: bool,
    /// The value or return type expression, unresolved.
    pub type_text: Option<String>,
    /// Parameters, for virtual methods; empty for properties.
    pub parameters: Vec<VirtualParameter>,
}

/// A provider contributes the members a class-like docblock declares.
/// Implementations must be deterministic pure functions of the
/// docblock text: no interior state, no environment reads (the
/// byte-identical harness is the mechanical detector).
pub trait VirtualSymbolProvider: Send + Sync {
    fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember>;
}

/// One registration: the implementation travels with its identity.
#[derive(Clone)]
pub struct VirtualSymbolRegistration {
    pub identity: PluginIdentity,
    pub provider: Arc<dyn VirtualSymbolProvider>,
}

impl std::fmt::Debug for VirtualSymbolRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VirtualSymbolRegistration")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// The registry: set once per process at the composition root, in the
/// high-durability tier, and never mutated — reading it therefore
/// never invalidates. Databases that register nothing (every test
/// database by default) take the no-plugin path. Providers are
/// consulted in registered order; contributions concatenate in that
/// order, so the result is independent of thread timing.
#[salsa::input(singleton)]
pub struct VirtualSymbolRegistry {
    #[returns(ref)]
    pub registrations: Vec<VirtualSymbolRegistration>,
}
```

In `crates/celerrate_semantics/src/lib.rs`: add `pub mod plugin;` and
`pub mod virtual_symbols;` to the module list and re-export
`plugin::PluginIdentity` and `virtual_symbols::{VirtualMember,
VirtualMemberKind, VirtualParameter, VirtualSymbolProvider,
VirtualSymbolRegistration, VirtualSymbolRegistry}` next to the
existing re-exports.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics virtual_symbols`
Expected: PASS. Then `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): plugin identity and the virtual-symbol extension point"
```

---
### Task 2: Virtual members through linearization

**Files:**
- Modify: `crates/celerrate_semantics/src/linearize.rs`

**Interfaces:**
- Consumes: Task 1's `VirtualMember`, `VirtualMemberKind`,
  `VirtualSymbolRegistry`; the existing `linearized_class` query, the
  BFS over `ClassMembers` (linearize.rs:180-265), `folded_member_key`
  (linearize.rs:159), `MemberKind`.
- Produces: `LinearizedVirtualMember { key: String, member:
  VirtualMember, owner: String }` and the new field
  `LinearizedClass::virtual_members: Vec<LinearizedVirtualMember>`
  (sorted by `(kind, key)`, stable — first entry per `(kind, key)` is
  the nearest declaration, exactly the real members' convention).
  Task 3 and plan 8 consume the field.

- [x] **Step 1: Write the failing tests**

In `linearize.rs`'s tests module (reuse the existing `Fixture`,
`fixture`, and `linearize` helpers at linearize.rs:780-907; add the
`FakeProvider`/`identity` pair from Task 1's test module — duplicate
them locally, the crate has no shared test-support module yet, which
is already recorded debt):

```rust
fn register_fake_provider(fixture: &Fixture, members: Vec<VirtualMember>) {
    VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
        identity: identity("fake"),
        provider: std::sync::Arc::new(FakeProvider { members }),
    }])
    .durability(salsa::Durability::HIGH)
    .new(&fixture.db);
}

fn virtual_property(name: &str) -> VirtualMember {
    VirtualMember {
        kind: VirtualMemberKind::Property,
        name: name.to_owned(),
        is_static: false,
        type_text: Some("string".to_owned()),
        parameters: Vec::new(),
    }
}

#[test]
fn a_class_docblock_contributes_virtual_members() {
    let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
    register_fake_provider(&fixture, vec![virtual_property("title")]);
    let linearized = linearize(&fixture, "Post").unwrap();
    assert_eq!(linearized.virtual_members.len(), 1);
    assert_eq!(linearized.virtual_members[0].member.name, "title");
    assert_eq!(linearized.virtual_members[0].owner, "post");
}

#[test]
fn virtual_members_inherit_and_the_nearest_declaration_wins() {
    let fixture = fixture(&[
        "<?php /** @fake */ class Base {}",
        "<?php /** @fake */ class Child extends Base {}",
    ]);
    register_fake_provider(&fixture, vec![virtual_property("title")]);
    let linearized = linearize(&fixture, "Child").unwrap();
    // Both declarations arrive; the walk order puts the child first.
    assert_eq!(linearized.virtual_members.len(), 2);
    assert_eq!(linearized.virtual_members[0].owner, "child");
    assert_eq!(linearized.virtual_members[1].owner, "base");
}

#[test]
fn a_class_without_docblock_contributes_nothing_and_no_registry_means_no_virtual_members() {
    let fixture = fixture(&["<?php class Plain {}"]);
    // No registry set at all: the no-plugin path.
    let linearized = linearize(&fixture, "Plain").unwrap();
    assert!(linearized.virtual_members.is_empty());
}

#[test]
fn providers_are_consulted_in_registered_order() {
    let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
    VirtualSymbolRegistry::builder(vec![
        VirtualSymbolRegistration {
            identity: identity("first"),
            provider: std::sync::Arc::new(FakeProvider {
                members: vec![virtual_property("alpha")],
            }),
        },
        VirtualSymbolRegistration {
            identity: identity("second"),
            provider: std::sync::Arc::new(FakeProvider {
                members: vec![virtual_property("alpha"), virtual_property("beta")],
            }),
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(&fixture.db);

    let linearized = linearize(&fixture, "Post").unwrap();
    let keys: Vec<(&str, &str)> = linearized
        .virtual_members
        .iter()
        .map(|entry| (entry.key.as_str(), entry.owner.as_str()))
        .collect();
    // Stable sort by (kind, key): both `alpha` entries stay in
    // registered order (first provider's first), `beta` follows.
    assert_eq!(keys, vec![("alpha", "post"), ("alpha", "post"), ("beta", "post")]);
}
```

Also update every existing constructor/literal of `LinearizedClass`
in this file's implementation and tests to carry
`virtual_members: Vec::new()` where a struct literal is built (the
compiler lists the sites).

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics linearize 2>&1 | tail -5`
Expected: FAIL to compile (`virtual_members` field missing).

- [x] **Step 3: Implement**

In `linearize.rs`:

```rust
/// One annotation-declared member in linearized position. Sorted with
/// the real members' convention: stable by (kind, key), nearest
/// declaration first — the first entry per (kind, key) wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearizedVirtualMember {
    /// Folded member key (method names lowercased, property names
    /// verbatim), from `folded_member_key`.
    pub key: String,
    pub member: VirtualMember,
    /// Folded key of the declaring class-like.
    pub owner: String,
}
```

Add `pub virtual_members: Vec<LinearizedVirtualMember>` to
`LinearizedClass` (linearize.rs:135-153).

Inside the `linearized_class` BFS (linearize.rs:180-265), where each
dequeued **source** class's `ClassMembers` group is consumed (the loop
at linearize.rs:219-220), collect virtual contributions in walk order:

```rust
// Virtual members: annotation-declared, contributed by the
// registered providers over the class-like's own docblock. The
// registry is a singleton input set once at the composition root;
// an unset registry (every plain test database) is the no-plugin
// path. Walk order here is the ancestry order, so after the stable
// sort the first entry per (kind, key) is the nearest declaration.
if let Some(registry) = VirtualSymbolRegistry::try_get(db) {
    if let Some(docblock) = &found.group.docblock {
        for registration in registry.registrations(db) {
            for member in registration.provider.virtual_members(docblock) {
                let kind = match member.kind {
                    VirtualMemberKind::Method => MemberKind::Method,
                    VirtualMemberKind::Property => MemberKind::Property,
                };
                virtual_entries.push(LinearizedVirtualMember {
                    key: folded_member_key(kind, &member.name),
                    member,
                    owner: current_class_key.clone(),
                });
            }
        }
    }
}
```

(`found.group` / `current_class_key` name the BFS locals as they exist
at linearize.rs:200-265 — bind to the actual local names there;
`virtual_entries` is a new `Vec` declared next to the real-member
accumulator.) After the walk, next to the real members' stable sort
(linearize.rs:273-275):

```rust
virtual_entries.sort_by(|left, right| {
    let rank = |member: &VirtualMember| match member.kind {
        VirtualMemberKind::Method => 0u8,
        VirtualMemberKind::Property => 1u8,
    };
    (rank(&left.member), &left.key).cmp(&(rank(&right.member), &right.key))
});
```

and store the result in the returned `LinearizedClass`.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics` — PASS; clippy; fmt.
The invalidation-scope suite
(`crates/celerrate_semantics/tests/invalidation_scope.rs`) must stay
green: with no registry set, behavior is byte-identical to before.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): virtual members flow through class linearization"
```

---

### Task 3: Virtual members in member lookup

**Files:**
- Modify: `crates/celerrate_semantics/src/member_lookup.rs`

**Interfaces:**
- Consumes: Task 2's `LinearizedClass::virtual_members`;
  the existing `lookup_member` (member_lookup.rs:55-95),
  `MemberResolution`, `MemberQuery`, `MemberKind`.
- Produces: the new variant
  `MemberResolution::Virtual { member: VirtualMember, owner: String }`.
  Precedence, fixed by this task: **source member, then stub member,
  then virtual member**. Tasks 8 and plan 8 consume the variant.

- [x] **Step 1: Write the failing tests**

In `member_lookup.rs`'s tests module (reuse the existing `fixture`
helpers at member_lookup.rs:165-258; add the fake-provider trio as in
Task 2):

```rust
#[test]
fn a_virtual_member_resolves_when_no_real_member_exists() {
    let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
    register_fake_provider(&fixture, vec![virtual_property("title")]);
    let query = MemberQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::ClassLike, "Post"),
        MemberKind::Property,
        folded_member_key(MemberKind::Property, "title"),
    );
    let resolution = lookup_member(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    match resolution {
        Some(MemberResolution::Virtual { member, owner }) => {
            assert_eq!(member.name, "title");
            assert_eq!(owner, "post");
        }
        other => panic!("expected a virtual resolution, got {other:?}"),
    }
}

#[test]
fn a_real_member_shadows_a_virtual_member_of_the_same_name() {
    let fixture = fixture(&[
        "<?php /** @fake */ class Post { public string $title; }",
    ]);
    register_fake_provider(&fixture, vec![virtual_property("title")]);
    let query = MemberQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::ClassLike, "Post"),
        MemberKind::Property,
        folded_member_key(MemberKind::Property, "title"),
    );
    let resolution = lookup_member(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    assert!(matches!(resolution, Some(MemberResolution::Source { .. })));
}

#[test]
fn virtual_members_answer_only_method_and_property_queries() {
    let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
    register_fake_provider(&fixture, vec![virtual_property("TITLE")]);
    let query = MemberQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::ClassLike, "Post"),
        MemberKind::ClassConstant,
        folded_member_key(MemberKind::ClassConstant, "TITLE"),
    );
    assert!(lookup_member(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .is_none());
}
```

(The `panic!` in the match arm needs the test module's existing panic
allow; add `#![allow(clippy::panic)]` if absent.)

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics member_lookup 2>&1 | tail -5`
Expected: FAIL to compile (`MemberResolution::Virtual` missing).

- [x] **Step 3: Implement**

Add the variant to `MemberResolution` (member_lookup.rs:35-46):

```rust
/// An annotation-declared member (`@property`, `@method`). Real
/// members — source and stub alike — shadow virtual members; the
/// type expressions inside `member` are unresolved text that
/// `celerrate_types` resolves through the type-syntax registry.
Virtual { member: VirtualMember, owner: String },
```

In `lookup_member` (member_lookup.rs:55-95), after the source-member
scan and the stub scans have both missed, and before the final `None`:

```rust
// Virtual members answer last: a real member of the same key,
// source or stub, always wins (decision 4 of the plan header).
let virtual_kind = match kind {
    MemberKind::Method => Some(VirtualMemberKind::Method),
    MemberKind::Property => Some(VirtualMemberKind::Property),
    MemberKind::ClassConstant | MemberKind::EnumCase => None,
};
if let (Some(virtual_kind), Some(linearized)) = (virtual_kind, linearized) {
    if let Some(entry) = linearized
        .virtual_members
        .iter()
        .find(|entry| entry.member.kind == virtual_kind && entry.key == *key)
    {
        return Some(MemberResolution::Virtual {
            member: entry.member.clone(),
            owner: entry.owner.clone(),
        });
    }
}
None
```

(`linearized` is the `Option<&LinearizedClass>` the function already
holds for the source scan — keep it alive until this point.)

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics` — PASS; clippy; fmt.
Note: `MemberResolution` gained a variant — the compiler lists every
match that must add an arm (`crates/celerrate_types/src/declared.rs`
matches it: give the new arm the same answer as `Stub`/absent for now;
Task 8 replaces it with real typing).

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_semantics crates/celerrate_types
git commit -m "✨ feat(semantics): virtual members resolve through member lookup behind real members"
```

---
### Task 4: The type-syntax extension point

**Files:**
- Create: `crates/celerrate_types/src/type_syntax.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (module + re-exports)
- Modify: `crates/celerrate_types/src/written.rs` (widen
  `lower_keyword`'s visibility if needed — it stays `pub(crate)`)

**Interfaces:**
- Consumes: `PluginIdentity` (Task 1), `TypeId` and its builders,
  `NameSite` and `qualified_class_name`
  (`crates/celerrate_types/src/declared.rs:25-33, 107-117`),
  `lower_keyword` (`crates/celerrate_types/src/written.rs:74-103`).
- Produces (Tasks 6, 8, 9, 13 and plan 4b consume these names):

```rust
pub struct AnnotationSite<'db, 'site> { /* private: db, site */ }
impl<'db> AnnotationSite<'db, '_> {
    pub fn database(&self) -> &'db dyn salsa::Database;
    pub fn keyword_type(&self, name: &str) -> Option<TypeId<'db>>;
    pub fn qualify_class_name(&self, written: &str) -> String;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedAnnotations<'db> {
    pub return_type: Option<TypeId<'db>>,
    pub value_type: Option<TypeId<'db>>,
    pub parameters: Vec<(String, TypeId<'db>)>,
    pub throws: Vec<TypeId<'db>>,
}

pub trait TypeSyntax: Send + Sync {
    fn can_parse(&self, docblock: &str) -> bool;
    fn parse_docblock<'db>(
        &self, site: &AnnotationSite<'db, '_>, docblock: &str,
    ) -> ParsedAnnotations<'db>;
    fn parse_type_expression<'db>(
        &self, site: &AnnotationSite<'db, '_>, expression: &str,
    ) -> Option<TypeId<'db>>;
}

pub struct TypeSyntaxRegistration {
    pub identity: PluginIdentity,
    pub implementation: std::sync::Arc<dyn TypeSyntax>,
}
#[salsa::input(singleton)]
pub struct TypeSyntaxRegistry {
    #[returns(ref)]
    pub registrations: Vec<TypeSyntaxRegistration>,
}

// Crate-internal dispatch (Tasks 6/8/9 call these):
pub(crate) fn annotations_for_docblock<'db>(
    db: &'db dyn salsa::Database, site: &NameSite<'_>, docblock: &str,
) -> ParsedAnnotations<'db>;
pub(crate) fn type_of_expression<'db>(
    db: &'db dyn salsa::Database, site: &NameSite<'_>, expression: &str,
) -> Option<TypeId<'db>>;
```

- [x] **Step 1: Write the failing tests**

In `type_syntax.rs`'s tests module (reuse the `fixture` helper shape
from `declared.rs:1019-1026`):

```rust
#[derive(Debug)]
struct FakeSyntax {
    accepts: &'static str,
    answer_int_return: bool,
}

impl TypeSyntax for FakeSyntax {
    fn can_parse(&self, docblock: &str) -> bool {
        docblock.contains(self.accepts)
    }
    fn parse_docblock<'db>(
        &self, site: &AnnotationSite<'db, '_>, _docblock: &str,
    ) -> ParsedAnnotations<'db> {
        let db = site.database();
        ParsedAnnotations {
            return_type: self.answer_int_return.then(|| TypeId::int(db)),
            ..ParsedAnnotations::default()
        }
    }
    fn parse_type_expression<'db>(
        &self, site: &AnnotationSite<'db, '_>, expression: &str,
    ) -> Option<TypeId<'db>> {
        (expression == "int").then(|| TypeId::int(site.database()))
    }
}

#[test]
fn dispatch_is_registered_order_with_can_parse_first_win() {
    let fixture = fixture(&["<?php class C {}"]);
    let db = &fixture.db;
    TypeSyntaxRegistry::builder(vec![
        TypeSyntaxRegistration {
            identity: identity("first"),
            implementation: std::sync::Arc::new(FakeSyntax {
                accepts: "@return", answer_int_return: true,
            }),
        },
        TypeSyntaxRegistration {
            identity: identity("second"),
            implementation: std::sync::Arc::new(FakeSyntax {
                accepts: "@", answer_int_return: false,
            }),
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(db);

    // Both can parse this: the first registered wins.
    let parsed = annotations_for_docblock(db, &NameSite::Global, "/** @return int */");
    assert_eq!(parsed.return_type, Some(TypeId::int(db)));
    // Only the second can parse this: first win falls through.
    let parsed = annotations_for_docblock(db, &NameSite::Global, "/** @var string */");
    assert_eq!(parsed, ParsedAnnotations::default());
    // No implementation can parse: the default.
    let parsed = annotations_for_docblock(db, &NameSite::Global, "/** prose */");
    assert_eq!(parsed, ParsedAnnotations::default());
}

#[test]
fn an_unset_registry_answers_the_default() {
    let fixture = fixture(&["<?php class C {}"]);
    let parsed = annotations_for_docblock(&fixture.db, &NameSite::Global, "/** @return int */");
    assert_eq!(parsed, ParsedAnnotations::default());
    assert_eq!(
        type_of_expression(&fixture.db, &NameSite::Global, "int"),
        None,
    );
}

#[test]
fn expression_dispatch_takes_the_first_some() {
    let fixture = fixture(&["<?php class C {}"]);
    let db = &fixture.db;
    TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
        identity: identity("fake"),
        implementation: std::sync::Arc::new(FakeSyntax {
            accepts: "@", answer_int_return: false,
        }),
    }])
    .durability(salsa::Durability::HIGH)
    .new(db);
    assert_eq!(type_of_expression(db, &NameSite::Global, "int"), Some(TypeId::int(db)));
    assert_eq!(type_of_expression(db, &NameSite::Global, "garbage!!"), None);
}

#[test]
fn the_annotation_site_shares_the_native_keyword_table_and_the_site_qualifier() {
    let fixture = fixture(&["<?php class C {}"]);
    let db = &fixture.db;
    let site = AnnotationSite::new(db, &NameSite::Global);
    assert_eq!(site.keyword_type("int"), Some(TypeId::int(db)));
    assert_eq!(site.keyword_type("static"), Some(TypeId::static_placeholder(db)));
    assert_eq!(site.keyword_type("NotAKeyword"), None);
    assert_eq!(site.qualify_class_name("\\App\\User"), "App\\User");
}
```

(`identity` helper as in Task 1's tests. `AnnotationSite::new` is
`pub(crate)` — tests live in the crate, fine.)

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types type_syntax 2>&1 | tail -5`
Expected: FAIL to compile (module does not exist).

- [x] **Step 3: Implement**

`crates/celerrate_types/src/type_syntax.rs`:

```rust
//! The type-syntax extension point: understand an annotation
//! notation. Owned by this crate per the design; the registry input
//! lives here too, or the DAG would break upward. Dispatch rule,
//! fixed now: implementations are consulted in registered order with
//! a can-parse protocol, first win — registration order is declared
//! at the composition root and therefore deterministic.

use std::sync::Arc;

use celerrate_semantics::PluginIdentity;

use crate::declared::NameSite;
use crate::representation::TypeId;

/// A name-resolution and construction context for one annotation
/// parse, scoped to the declaring site. Handles are call-scoped:
/// implementations never retain the site, the database, or any
/// `TypeId` beyond the call (the WASM projection will enforce this
/// structurally; the native tier enforces it by review).
pub struct AnnotationSite<'db, 'site> {
    db: &'db dyn salsa::Database,
    site: &'site NameSite<'site>,
}

impl<'db, 'site> AnnotationSite<'db, 'site> {
    pub(crate) fn new(db: &'db dyn salsa::Database, site: &'site NameSite<'site>) -> Self {
        Self { db, site }
    }

    /// The database, for `TypeId` builders. Never retain it.
    pub fn database(&self) -> &'db dyn salsa::Database {
        self.db
    }

    /// The native keyword table (`int`, `string`, `self`, `static`,
    /// `iterable`, ...), shared with native signature lowering so the
    /// two paths can never disagree. `None` means ordinary class name.
    pub fn keyword_type(&self, name: &str) -> Option<TypeId<'db>> {
        crate::written::lower_keyword(self.db, name)
    }

    /// Qualifies a written class name at the declaring site
    /// (namespace and `use` imports), returning the fully qualified
    /// name — feed it to `TypeId::class`.
    pub fn qualify_class_name(&self, written: &str) -> String {
        crate::declared::qualified_class_name(self.site, written)
    }
}

/// One docblock, parsed: the annotation layer a member or function
/// declares. `return_type` and `value_type` are both carried; the
/// consumer picks by subject kind (decision 5 of the plan header).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedAnnotations<'db> {
    /// `@return`.
    pub return_type: Option<TypeId<'db>>,
    /// `@var`.
    pub value_type: Option<TypeId<'db>>,
    /// `@param`, by parameter name (without the `$`).
    pub parameters: Vec<(String, TypeId<'db>)>,
    /// `@throws`, accumulated across tags.
    pub throws: Vec<TypeId<'db>>,
}

/// An implementation understands one annotation notation. Must be a
/// deterministic pure function of its arguments; contributions are
/// consumed through deterministic dispatch. Object-safe by design
/// (lifetime-generic methods only), per the design's WASM projection
/// constraint.
pub trait TypeSyntax: Send + Sync {
    /// The can-parse protocol: consulted in registered order, the
    /// first implementation answering `true` wins the docblock.
    fn can_parse(&self, docblock: &str) -> bool;
    /// Parse one docblock into the annotation layer. A construct the
    /// notation cannot express degrades that element to absent, never
    /// the whole docblock (loss is per construct).
    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db>;
    /// Parse one bare type expression (virtual-member payloads).
    /// Dispatch: registered order, first `Some` wins.
    fn parse_type_expression<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<TypeId<'db>>;
}

/// One registration: the implementation travels with its identity,
/// so reading it records the dependency an upgrade invalidates.
#[derive(Clone)]
pub struct TypeSyntaxRegistration {
    pub identity: PluginIdentity,
    pub implementation: Arc<dyn TypeSyntax>,
}

impl std::fmt::Debug for TypeSyntaxRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TypeSyntaxRegistration")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// Set once per process at the composition root, HIGH durability,
/// never mutated. Unset (every plain test database): the no-plugin
/// path — annotations answer the default.
#[salsa::input(singleton)]
pub struct TypeSyntaxRegistry {
    #[returns(ref)]
    pub registrations: Vec<TypeSyntaxRegistration>,
}

/// Registered order, can-parse first win.
pub(crate) fn annotations_for_docblock<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    docblock: &str,
) -> ParsedAnnotations<'db> {
    let Some(registry) = TypeSyntaxRegistry::try_get(db) else {
        return ParsedAnnotations::default();
    };
    let annotation_site = AnnotationSite::new(db, site);
    for registration in registry.registrations(db) {
        if registration.implementation.can_parse(docblock) {
            return registration.implementation.parse_docblock(&annotation_site, docblock);
        }
    }
    ParsedAnnotations::default()
}

/// Registered order, first `Some` wins.
pub(crate) fn type_of_expression<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    expression: &str,
) -> Option<TypeId<'db>> {
    let registry = TypeSyntaxRegistry::try_get(db)?;
    let annotation_site = AnnotationSite::new(db, site);
    for registration in registry.registrations(db) {
        if let Some(answer) = registration
            .implementation
            .parse_type_expression(&annotation_site, expression)
        {
            return Some(answer);
        }
    }
    None
}
```

In `lib.rs`: `mod type_syntax;` plus
re-export `type_syntax::{AnnotationSite, ParsedAnnotations,
TypeSyntax, TypeSyntaxRegistration, TypeSyntaxRegistry}`. `NameSite`
and `qualified_class_name` stay `pub(crate)`; if `NameSite` lives in
`declared.rs` as `pub(crate)`, no visibility change is needed for the
dispatch functions' signatures.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types` — PASS; clippy; fmt.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): the type-syntax extension point with can-parse first-win dispatch"
```

---

### Task 5: The dynamic-type-provider extension point

**Files:**
- Create: `crates/celerrate_types/src/dynamic_type_provider.rs`
- Modify: `crates/celerrate_types/src/lib.rs`

**Interfaces:**
- Consumes: `PluginIdentity`, `TypeId`.
- Produces (plan 5/6 call `return_type`; plan 7 implements the trait;
  Task 16 validates claims at the composition root):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SymbolClaim {
    Function { key: String },                       // folded key
    Method { class_key: String, method_key: String }, // folded keys
}

pub struct Invocation<'db> {
    pub claim: SymbolClaim,
    pub receiver_type: Option<TypeId<'db>>,
    pub argument_types: Vec<TypeId<'db>>,
}

pub trait DynamicTypeProvider: Send + Sync {
    fn claims(&self) -> Vec<SymbolClaim>;
    fn return_type<'db>(
        &self, db: &'db dyn salsa::Database, invocation: &Invocation<'db>,
    ) -> Option<TypeId<'db>>;
}

pub struct DynamicTypeProviderRegistration {
    pub identity: PluginIdentity,
    pub provider: std::sync::Arc<dyn DynamicTypeProvider>,
}
#[salsa::input(singleton)]
pub struct DynamicTypeProviderRegistry {
    #[returns(ref)]
    pub registrations: Vec<DynamicTypeProviderRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimConflict {
    pub claim: SymbolClaim,
    pub first: String,   // plugin name holding the claim
    pub second: String,  // plugin name colliding with it
}

pub fn validate_claims(
    registrations: &[DynamicTypeProviderRegistration],
) -> Result<(), ClaimConflict>;
```

- [x] **Step 1: Write the failing tests**

```rust
#[derive(Debug)]
struct FakeProvider {
    claimed: Vec<SymbolClaim>,
}

impl DynamicTypeProvider for FakeProvider {
    fn claims(&self) -> Vec<SymbolClaim> {
        self.claimed.clone()
    }
    fn return_type<'db>(
        &self, db: &'db dyn salsa::Database, _invocation: &Invocation<'db>,
    ) -> Option<TypeId<'db>> {
        Some(TypeId::int(db))
    }
}

fn registration(name: &str, claimed: Vec<SymbolClaim>) -> DynamicTypeProviderRegistration {
    DynamicTypeProviderRegistration {
        identity: identity(name),
        provider: std::sync::Arc::new(FakeProvider { claimed }),
    }
}

#[test]
fn disjoint_claims_validate() {
    let registrations = vec![
        registration("first", vec![SymbolClaim::Function { key: "array_map".to_owned() }]),
        registration("second", vec![SymbolClaim::Function { key: "explode".to_owned() }]),
    ];
    assert_eq!(validate_claims(&registrations), Ok(()));
}

#[test]
fn overlapping_claims_are_a_registration_time_error_naming_both_plugins() {
    let claim = SymbolClaim::Method {
        class_key: "collection".to_owned(),
        method_key: "map".to_owned(),
    };
    let registrations = vec![
        registration("first", vec![claim.clone()]),
        registration("second", vec![claim.clone()]),
    ];
    assert_eq!(
        validate_claims(&registrations),
        Err(ClaimConflict {
            claim,
            first: "first".to_owned(),
            second: "second".to_owned(),
        }),
    );
}

#[test]
fn a_provider_overlapping_itself_is_also_refused() {
    let claim = SymbolClaim::Function { key: "current".to_owned() };
    let registrations = vec![registration("solo", vec![claim.clone(), claim.clone()])];
    assert!(validate_claims(&registrations).is_err());
}
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types dynamic_type_provider 2>&1 | tail -5`
Expected: FAIL to compile.

- [x] **Step 3: Implement**

Write the module with the exact interface block above, plus:

```rust
/// The design's dispatch rule, fixed now: providers claim symbols;
/// overlapping claims are a registration-time error unless resolved
/// by documented precedence at the composition root (none is
/// documented yet — the composition root excludes the later
/// registrant and reports the run degraded). Deterministic: claims
/// are gathered in registered order.
pub fn validate_claims(
    registrations: &[DynamicTypeProviderRegistration],
) -> Result<(), ClaimConflict> {
    let mut holders: std::collections::BTreeMap<SymbolClaim, String> =
        std::collections::BTreeMap::new();
    for registration in registrations {
        for claim in registration.provider.claims() {
            if let Some(first) = holders.get(&claim) {
                return Err(ClaimConflict {
                    claim,
                    first: first.clone(),
                    second: registration.identity.name.clone(),
                });
            }
            holders.insert(claim, registration.identity.name.clone());
        }
    }
    Ok(())
}
```

Document on the trait: contributions are widened at the consumption
boundary inside `celerrate_types` (plan 5's fixpoint) — a provider
never controls termination; implementations must be deterministic and
monotone; `None` falls back to the declared or inferred type.
`Invocation` is an owned struct so plan 7 extends it additively
(argument *values* already travel as literal types interrogable on
`TypeId`). Manual `Debug` for the registration struct, registry as a
singleton input — same shapes as Task 4. Re-export everything from
`lib.rs`.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types` — PASS; clippy; fmt.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): the dynamic-type-provider extension point with claim validation"
```

---
### Task 6: Fill the member annotation seam

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`

**Interfaces:**
- Consumes: `annotations_for_docblock` (Task 4), `lookup_member` /
  `MemberResolution` (Task 3), the existing `declaring_site`
  (declared.rs:353-376) and `inherited_annotations` (declared.rs:232).
- Produces: `member_annotations` now answers parsed annotations;
  `MemberAnnotations` gains `pub throws: Vec<TypeId<'db>>` (Default
  still derives; the inheritance merge covers it element-wise). The
  query signature does not change — plan 3 built the seam for exactly
  this one-body swap.

- [x] **Step 1: Write the failing tests**

Replace the plan-3 pinned test
`the_annotation_seam_answers_the_default_until_the_bridge_lands`
(declared.rs:1254-1270) with:

```rust
#[test]
fn the_annotation_seam_answers_the_default_with_no_registered_syntax() {
    // Unchanged body of the old pinned test: no registry, no
    // annotations — the no-plugin path every test database takes.
    let fixture = fixture(&["<?php class C { /** @return int */ public function f() {} }"]);
    let query = member_query(&fixture, "C", MemberKind::Method, "f");
    let annotations = super::member_annotations(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    assert_eq!(annotations, super::MemberAnnotations::default());
}

#[test]
fn the_seam_parses_the_own_docblock_through_the_registry() {
    let fixture = fixture(&[
        "<?php class C { /** @return int */ public function f(): string {} }",
    ]);
    register_fake_syntax(&fixture); // Task 4's FakeSyntax: "@return int" -> int
    let query = member_query(&fixture, "C", MemberKind::Method, "f");
    let annotations = super::member_annotations(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    assert_eq!(annotations.value, Some(TypeId::int(&fixture.db)));
}

#[test]
fn the_value_annotation_is_picked_by_member_kind() {
    // A fake syntax answering BOTH return_type=int and value_type=string
    // proves the kind-based pick: methods read @return, properties @var.
    let fixture = fixture(&[
        "<?php class C { /** @tags */ public $p; /** @tags */ public function f() {} }",
    ]);
    register_fake_syntax_both(&fixture);
    let property = member_query(&fixture, "C", MemberKind::Property, "p");
    let method = member_query(&fixture, "C", MemberKind::Method, "f");
    let db = &fixture.db;
    assert_eq!(
        super::member_annotations(db, fixture.files, fixture.stubs, fixture.configuration, property).value,
        Some(TypeId::string(db)),
    );
    assert_eq!(
        super::member_annotations(db, fixture.files, fixture.stubs, fixture.configuration, method).value,
        Some(TypeId::int(db)),
    );
}

#[test]
fn stub_and_missing_members_answer_the_default() {
    let fixture = fixture(&["<?php class C {}"]);
    register_fake_syntax(&fixture);
    let query = member_query(&fixture, "C", MemberKind::Method, "ghost");
    assert_eq!(
        super::member_annotations(
            &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
        ),
        super::MemberAnnotations::default(),
    );
}
```

Test helpers to add once in the tests module: `member_query(fixture,
class, kind, name)` interning a `MemberQuery` with the folded keys
(the shape at declared.rs:1255-1263 already does this inline — factor
it), `register_fake_syntax` registering Task 4's `FakeSyntax` at HIGH
durability, and `register_fake_syntax_both` with a fake whose
`parse_docblock` fills `return_type: Some(int)` and `value_type:
Some(string)` when the docblock contains `@tags`.

Existing tests to keep green untouched: `the_trust_rule_is_three_valued`,
the inheritance tests (they inject readers directly and bypass the
seam).

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared 2>&1 | tail -5`
Expected: the two new registry tests FAIL (seam still answers the
default); the compile fails first if `throws` is referenced before it
exists — add the field in the same step.

- [x] **Step 3: Implement**

Add `pub throws: Vec<TypeId<'db>>` to `MemberAnnotations`
(declared.rs:164-170) and merge it in `inherited_annotations`
(declared.rs:232-272) with the same element-wise rule as `value`: if
own `throws` is empty, the nearest declaring ancestor's fills it.

Swap the seam body (declared.rs:172-185):

```rust
#[salsa::tracked]
pub fn member_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> MemberAnnotations<'db> {
    // Stub members carry no docblocks (their types come from the
    // signature payload), virtual members have no docblock of their
    // own, and unresolved members have nothing to parse.
    let Some(MemberResolution::Source { member, owner, .. }) =
        lookup_member(db, files, stubs, configuration, query)
    else {
        return MemberAnnotations::default();
    };
    let Some(docblock) = member.docblock.clone() else {
        return MemberAnnotations::default();
    };
    // The declaring site: the owner class-like's namespace and use
    // tables, exactly as native signature resolution derives them —
    // reuse `declaring_site` (this module, declared.rs:353) and build
    // the same `NameSite` `resolve_member_signature` uses.
    let parsed = with_declaring_site(db, files, &owner, |site| {
        crate::type_syntax::annotations_for_docblock(db, site, &docblock)
    });
    MemberAnnotations {
        value: match member.kind {
            MemberKind::Method => parsed.return_type,
            MemberKind::Property | MemberKind::ClassConstant => parsed.value_type,
            MemberKind::EnumCase => None,
        },
        parameters: parsed.parameters,
        throws: parsed.throws,
    }
}
```

`with_declaring_site` is a small private helper to write in this
task: it wraps the existing site-derivation code (`declaring_site`,
declared.rs:353-376) so the borrowed `NameSite` (namespace string +
`UseTables`) lives across the closure call, and falls back to
`NameSite::Global` when the owner is not a source class. Factor it
out of the current native-path call sites if that is cleaner — both
paths must derive the site identically (that is the point of the
helper).

Note for the implementer: `member_annotations(query for class C)`
where C inherits the member without redeclaring it resolves to the
ancestor's member and parses the ancestor's docblock directly — the
same answer the inheritance walk would produce, one step earlier.
Consistent by construction; do not "fix" it.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types` — PASS; clippy; fmt.
`cargo test --workspace` — the seam changes nothing for databases
without a registry, so everything else stays green.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): the member annotation seam parses docblocks through the type-syntax registry"
```

---

### Task 7: The second-stage docblock cutoff

Plan 1a's deferred test: "the second-stage cutoff (the
parsed-annotation query) is plan 4's". A prose-only docblock edit
re-runs the annotation parse but nothing above it.

**Files:**
- Modify: `crates/celerrate_types/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: `member_annotations`, `declared_member_signature`, the
  `TestDatabase::take_executed()` instrumentation and the
  `executions_of` pattern (see
  `crates/celerrate_semantics/tests/invalidation_scope.rs` and the
  existing file for the established shape), a fake `TypeSyntax`
  registered in the fixture.
- Produces: the pinned two-stage cutoff.

- [x] **Step 1: Write the failing test**

```rust
#[test]
fn a_prose_only_docblock_edit_backdates_at_the_parsed_annotation_stage() {
    // Stage one (member tree) must re-run: the raw docblock is a
    // member-tree field, the spec's accepted cost. Stage two (the
    // parsed annotations) re-runs and produces an equal value, so
    // everything above — the declared signature — backdates.
    let mut fixture = fixture_with_fake_syntax(&[
        "<?php class C { /** @return int */ public function f() {} }",
    ]);
    let query = member_query(&fixture, "C", MemberKind::Method, "f");
    let _ = declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    fixture.db.take_executed();

    set_source(
        &mut fixture, 0,
        "<?php class C { /** @return int (documented better) */ public function f() {} }",
    );
    let _ = declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );

    let log = fixture.db.take_executed();
    assert_eq!(executions_of(&log, "member_annotations"), 1, "{log:?}");
    assert_eq!(executions_of(&log, "declared_member_signature"), 0, "{log:?}");
}

#[test]
fn an_annotation_edit_reaches_the_declared_signature() {
    let mut fixture = fixture_with_fake_syntax(&[
        "<?php class C { /** @return int */ public function f() {} }",
    ]);
    let query = member_query(&fixture, "C", MemberKind::Method, "f");
    let _ = declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    fixture.db.take_executed();

    set_source(
        &mut fixture, 0,
        "<?php class C { /** @return string */ public function f() {} }",
    );
    let _ = declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );

    let log = fixture.db.take_executed();
    assert_eq!(executions_of(&log, "member_annotations"), 1, "{log:?}");
    assert_eq!(executions_of(&log, "declared_member_signature"), 1, "{log:?}");
}
```

The fake syntax here must ignore prose while reading tags — give it
the shape "extract `@return <one word>`, map `int`/`string` through
`site.keyword_type`, ignore everything else", so the prose edit
produces an equal `MemberAnnotations` and the tag edit a different
one. `fixture_with_fake_syntax`, `set_source` (edit one file's bytes
via `set_bytes`), `member_query`, and `executions_of` follow the
existing shapes in this test file and in
`crates/celerrate_semantics/tests/invalidation_scope.rs:285-315`.

- [x] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_types --test invalidation_scope 2>&1 | tail -5`
Expected: FAIL to compile (helpers missing) then, once compiling,
both must pass — if the backdate assertion fails, the seam is reading
something range-carrying; fix the seam, not the test.

- [x] **Step 3: Implement the helpers, run to green**

Run: `cargo test --package celerrate_types --test invalidation_scope`
Expected: PASS; clippy; fmt.

- [x] **Step 4: Commit**

```bash
git add crates/celerrate_types
git commit -m "✅ test(types): the two-stage docblock cutoff is pinned at the parsed-annotation level"
```

---

### Task 8: Virtual member typing

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`

**Interfaces:**
- Consumes: `MemberResolution::Virtual` (Task 3),
  `type_of_expression` (Task 4), `refine` (declared.rs:192-208),
  `with_declaring_site` (Task 6).
- Produces: `declared_member_signature` answers a full
  `DeclaredSignature` for virtual members. Rules: the native side of
  a virtual member is `mixed` (no native declaration exists), so a
  parsed annotation refines with `Trust::Refined`; an absent or
  unparseable expression stays `(mixed, NativeOnly)`; parameters
  carry `optional`/`variadic` from the annotation, `by_reference:
  false`.

- [x] **Step 1: Write the failing tests**

```rust
#[test]
fn a_virtual_property_types_through_the_type_syntax_registry() {
    let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
    register_fake_virtual_provider(&fixture, vec![virtual_property_with_text("title", "int")]);
    register_fake_syntax(&fixture); // parse_type_expression: "int" -> int
    let query = member_query(&fixture, "Post", MemberKind::Property, "title");
    let signature = declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.value_type, TypeId::int(&fixture.db));
    assert_eq!(signature.value_trust, Trust::Refined);
}

#[test]
fn a_virtual_method_carries_its_annotated_parameters() {
    let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
    register_fake_virtual_provider(&fixture, vec![VirtualMember {
        kind: VirtualMemberKind::Method,
        name: "find".to_owned(),
        is_static: true,
        type_text: Some("int".to_owned()),
        parameters: vec![VirtualParameter {
            name: "id".to_owned(),
            type_text: Some("int".to_owned()),
            optional: false,
            variadic: false,
        }],
    }]);
    register_fake_syntax(&fixture);
    let query = member_query(&fixture, "Post", MemberKind::Method, "find");
    let signature = declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.value_type, TypeId::int(&fixture.db));
    assert_eq!(signature.parameters.len(), 1);
    assert_eq!(signature.parameters[0].name, "id");
    assert_eq!(signature.parameters[0].parameter_type, Some(TypeId::int(&fixture.db)));
}

#[test]
fn an_unparseable_virtual_type_degrades_to_mixed_native_only() {
    let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
    register_fake_virtual_provider(&fixture, vec![virtual_property_with_text("title", "no<such>notation")]);
    register_fake_syntax(&fixture);
    let query = member_query(&fixture, "Post", MemberKind::Property, "title");
    let signature = declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.value_type, TypeId::mixed(&fixture.db));
    assert_eq!(signature.value_trust, Trust::NativeOnly);
}
```

(`register_fake_virtual_provider` mirrors Task 2's helper;
`celerrate_types` already dev-consumes `celerrate_semantics` types
directly, so the fake provider compiles here.)

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared 2>&1 | tail -5`
Expected: FAIL — the Virtual arm (Task 3 gave it the absent answer)
returns `None`.

- [x] **Step 3: Implement**

In `declared_member_signature`'s resolution match:

```rust
MemberResolution::Virtual { member, owner } => {
    // A virtual member's whole type comes from its annotation text,
    // resolved through the type-syntax registry at the owner's site.
    // There is no native declaration: refinement runs against
    // `mixed`, so any parsed annotation holds (Trust::Refined) and
    // an absent or unparseable one stays (mixed, NativeOnly).
    let mixed = TypeId::mixed(db);
    with_declaring_site(db, files, &owner, |site| {
        let annotation = member
            .type_text
            .as_deref()
            .and_then(|text| crate::type_syntax::type_of_expression(db, site, text));
        let (value_type, value_trust) =
            refine(db, files, stubs, configuration, mixed, annotation);
        let parameters = member
            .parameters
            .iter()
            .map(|parameter| {
                let annotation = parameter
                    .type_text
                    .as_deref()
                    .and_then(|text| crate::type_syntax::type_of_expression(db, site, text));
                let (parameter_type, trust) =
                    refine(db, files, stubs, configuration, mixed, annotation);
                DeclaredParameter {
                    name: parameter.name.clone(),
                    parameter_type: Some(parameter_type),
                    trust,
                    optional: parameter.optional,
                    variadic: parameter.variadic,
                    by_reference: false,
                }
            })
            .collect();
        Some(DeclaredSignature { parameters, value_type, value_trust, by_reference: false })
    })
}
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types` — PASS; clippy; fmt.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): virtual members resolve their annotated types through the registry"
```

---

### Task 9: The function annotation seam

Plan 3's recorded debt: "function annotations have no seam yet".

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (re-export
  `function_annotations`)

**Interfaces:**
- Consumes: `FunctionQuery` (declared.rs:797-802), the source
  free-function path inside `declared_function_signature`
  (declared.rs:804-862), `annotations_for_docblock`, `refine`.
- Produces:

```rust
#[salsa::tracked]
pub fn function_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> MemberAnnotations<'db>
```

  (reuses `MemberAnnotations`; `value` carries `@return`). Functions
  do not inherit — no walk. `declared_function_signature` now refines
  its source-path value and parameters through the trust rule instead
  of hard-coding `None`/`Trust::NativeOnly`; the stub path is
  untouched.

- [x] **Step 1: Write the failing tests**

```rust
#[test]
fn a_function_docblock_parses_through_the_registry() {
    let fixture = fixture(&["<?php /** @return int */ function f(): string {}"]);
    register_fake_syntax(&fixture);
    let query = FunctionQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::Function, "f"),
    );
    let annotations = super::function_annotations(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    assert_eq!(annotations.value, Some(TypeId::int(&fixture.db)));
}

#[test]
fn the_function_signature_refines_under_the_trust_rule() {
    // int <: string fails: the annotation is rejected, native wins.
    let fixture = fixture(&["<?php /** @return int */ function f(): string {}"]);
    register_fake_syntax(&fixture);
    let query = FunctionQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::Function, "f"),
    );
    let signature = declared_function_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.value_type, TypeId::string(&fixture.db));
    assert_eq!(signature.value_trust, Trust::RejectedAnnotation);
}

#[test]
fn an_unannotated_function_stays_native_only() {
    let fixture = fixture(&["<?php function f(): string {}"]);
    register_fake_syntax(&fixture);
    let query = FunctionQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::Function, "f"),
    );
    let signature = declared_function_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.value_trust, Trust::NativeOnly);
}
```

(A refining case — `@return Dog` against native `Animal` — mirrors
the member tests; add it with a two-class fixture and a fake syntax
that resolves class names through `site.qualify_class_name`.)

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared 2>&1 | tail -5`
Expected: FAIL to compile (`function_annotations` missing).

- [x] **Step 3: Implement**

`function_annotations`: locate the source `FreeFunction` exactly as
`declared_function_signature`'s source path does (same lookup, same
folded key), take its `docblock` (the field shipped with plan 1a),
build `NameSite::Source` from `function.namespace` and its file's use
tables (the existing site derivation for functions), call
`annotations_for_docblock`, map `value: parsed.return_type`,
`parameters: parsed.parameters`, `throws: parsed.throws`. Absent
function or docblock, or a stub-only function: the default.

In `declared_function_signature`'s source path: call
`function_annotations`, then per parameter look the annotation up by
name and route through `declared_parameter(..., annotation)` (replace
the hard-coded `None` at declared.rs:855), and for the value replace
the direct lowering + `Trust::NativeOnly` (declared.rs:858-859) with
`refine(db, files, stubs, configuration, native_value, annotations.value)`.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types` — PASS; clippy; fmt.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): the function annotation seam closes plan 3's recorded debt"
```

---
### Task 10: The `celerrate_plugin` facade

**Files:**
- Create: `crates/celerrate_plugin/Cargo.toml`
- Create: `crates/celerrate_plugin/src/lib.rs`

**Interfaces:**
- Consumes: everything Tasks 1, 4, 5 produced.
- Produces: the single dependency plugin crates declare. Re-exported
  surface (Tasks 11-14 use only these paths):
  `PLUGIN_API_VERSION: u32`, `PluginDescriptor { identity,
  api_version }`, `PluginIdentity`, the three trait families with
  their payload types, `TypeId` + `CallableParameter` + `ShapeField` +
  `ShapeKey`, `Trust`, and the module re-exports `source`,
  `diagnostics`, `salsa`. Registries are **not** re-exported — the
  composition root reaches them through the owning crates.

- [x] **Step 1: Write the failing test**

`crates/celerrate_plugin/src/lib.rs` will carry one test pinning the
surface compiles and the version is what the composition root checks:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn the_api_version_starts_at_zero() {
        assert_eq!(super::PLUGIN_API_VERSION, 0);
    }
}
```

(The real test of the facade is Task 11: the bridge crate compiles
against it alone; the dependency-shape check of Task 17 makes that
mechanical.)

- [x] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_plugin 2>&1 | tail -5`
Expected: FAIL — the crate does not exist.

- [x] **Step 3: Implement**

`crates/celerrate_plugin/Cargo.toml`:

```toml
[package]
name = "celerrate_plugin"
description = "The plugin API facade: the one dependency a Celerrate plugin declares"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_diagnostics = { path = "../celerrate_diagnostics" }
celerrate_semantics = { path = "../celerrate_semantics" }
celerrate_source = { path = "../celerrate_source" }
celerrate_types = { path = "../celerrate_types" }
salsa = { workspace = true }

[lints]
workspace = true
```

`crates/celerrate_plugin/src/lib.rs`:

```rust
//! The plugin API facade. Extension points are owned by their
//! consuming layers (`celerrate_types`, `celerrate_semantics`); this
//! crate aggregates and re-exports the stable surface so a plugin
//! crate declares exactly one dependency. Implementations are
//! constructed and registered at the composition root
//! (`celerrate_cli`). An extension point that proves insufficient is
//! extended, never bypassed.
//!
//! The API is deliberately not called v1: its second *dissimilar*
//! consumer (a framework dynamic type provider) is sub-project 6.

/// The API version the composition root checks at registration. A
/// mismatch excludes the plugin for the whole run and the run is
/// reported degraded. For compiled-in first-party plugins the check
/// cannot fail — dormant scaffolding whose first real exercise is
/// the WASM host. Distinct from the plugin version inside
/// `PluginIdentity`; only the latter keys the cache.
pub const PLUGIN_API_VERSION: u32 = 0;

/// What a plugin exposes for registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub identity: PluginIdentity,
    pub api_version: u32,
}

// Identity and the virtual-symbol extension point.
pub use celerrate_semantics::{
    PluginIdentity, VirtualMember, VirtualMemberKind, VirtualParameter, VirtualSymbolProvider,
};

// The type-syntax and dynamic-type-provider extension points, and the
// type vocabulary plugins construct and interrogate through.
pub use celerrate_types::{
    AnnotationSite, CallableParameter, DynamicTypeProvider, Invocation, ParsedAnnotations,
    ShapeField, ShapeKey, SymbolClaim, Trust, TypeId, TypeSyntax,
};

// The span and diagnostic vocabulary, and salsa for trait signatures.
pub use celerrate_diagnostics as diagnostics;
pub use celerrate_source as source;
pub use salsa;
```

(If a name in the `pub use` lists is not re-exported at its owner's
root yet, add the root re-export in the owning crate rather than
deep-pathing here.)

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_plugin` — PASS. Then the full
workspace gate (the `crates/*` glob picks the crate up
automatically): `cargo test --workspace`, clippy, fmt, `cargo deny check`.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_plugin Cargo.lock
git commit -m "✨ feat(plugin): the aggregation facade with the dormant API version check"
```

---

### Task 11: The bridge crate and the docblock lexer

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/Cargo.toml`
- Create: `crates/celerrate_phpdoc_bridge/src/lib.rs`
- Create: `crates/celerrate_phpdoc_bridge/src/lexer.rs`

**Interfaces:**
- Consumes: `celerrate_plugin` only (normal deps).
- Produces: `Tag { name: String, content: String }` and
  `lex_docblock(text: &str) -> Vec<Tag>` — total, never panics,
  fuzzed by Task 15. Tasks 12-14 consume the tag stream; plan 4b's
  dialect modules consume the same stream (one plugin, one docblock
  lexer, dialect modules behind it).

- [x] **Step 1: Write the failing tests**

In `lexer.rs`'s tests module:

```rust
#[test]
fn tags_split_on_lines_with_decoration_stripped() {
    let tags = lex_docblock(
        "/**\n * Summary prose, ignored.\n *\n * @param int $id the identifier\n * @return string\n */",
    );
    assert_eq!(
        tags,
        vec![
            Tag { name: "param".to_owned(), content: "int $id the identifier".to_owned() },
            Tag { name: "return".to_owned(), content: "string".to_owned() },
        ],
    );
}

#[test]
fn a_single_line_docblock_lexes() {
    assert_eq!(
        lex_docblock("/** @return int */"),
        vec![Tag { name: "return".to_owned(), content: "int".to_owned() }],
    );
}

#[test]
fn continuation_lines_fold_into_the_open_tag() {
    let tags = lex_docblock("/**\n * @param int $id\n *   spanning two lines\n */");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].content, "int $id spanning two lines");
}

#[test]
fn hyphenated_tag_names_lex_whole() {
    let tags = lex_docblock("/** @property-read string $title */");
    assert_eq!(tags[0].name, "property-read");
    assert_eq!(tags[0].content, "string $title");
}

#[test]
fn adversarial_inputs_never_panic() {
    for input in [
        "", "/**", "*/", "/**/", "@", "/** @ */", "/** @@ */",
        "/** *** */", "no docblock at all", "/** @return */",
        "\u{0}\u{0}@\u{0}", "/**\r\n * @var\r\n */",
    ] {
        let _ = lex_docblock(input);
    }
}
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge 2>&1 | tail -5`
Expected: FAIL — the crate does not exist.

- [x] **Step 3: Implement**

`Cargo.toml` (dev-dependencies arrive in Task 13; start without):

```toml
[package]
name = "celerrate_phpdoc_bridge"
description = "First-party plugin translating the inherited PHPDoc convention family"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_plugin = { path = "../celerrate_plugin" }

[lints]
workspace = true
```

`src/lib.rs`:

```rust
//! The `phpdoc-bridge` plugin: translates the inherited PHPDoc
//! convention family (standard PHPDoc in this plan; the PHPStan
//! dialect and Psalm synonyms arrive with plan 4b as internal
//! modules over the same lexer). Depends on `celerrate_plugin` and
//! nothing else in the workspace — enforced by
//! `cargo xtask dependency-shape`. No docblock diagnostics: malformed
//! annotations are silently ignored, per construct.

mod lexer;

pub use lexer::{Tag, lex_docblock};
```

`src/lexer.rs`:

```rust
//! The docblock lexer: one per plugin, shared by every dialect
//! module. Total over arbitrary input (fuzzed): any string yields a
//! tag list, never a panic.

/// One `@tag` occurrence: the name without `@`, and the raw content
/// up to the next tag or the end of the docblock, decoration
/// stripped, continuation lines folded with single spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub content: String,
}

/// Splits a docblock into its tags. Summary prose before the first
/// tag is ignored; inline `{@...}` forms are not interpreted.
pub fn lex_docblock(text: &str) -> Vec<Tag> {
    let body = text.strip_prefix("/**").unwrap_or(text);
    let body = body.strip_suffix("*/").unwrap_or(body);
    let mut tags: Vec<Tag> = Vec::new();
    for line in body.lines() {
        let line = line
            .trim_start()
            .trim_start_matches('*')
            .trim();
        if let Some(rest) = line.strip_prefix('@') {
            let boundary = rest
                .find(|character: char| {
                    !(character.is_ascii_alphanumeric() || character == '-')
                })
                .unwrap_or(rest.len());
            let name = rest.get(..boundary).unwrap_or_default();
            let content = rest.get(boundary..).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            tags.push(Tag {
                name: name.to_owned(),
                content: content.trim().to_owned(),
            });
        } else if !line.is_empty() {
            if let Some(open) = tags.last_mut() {
                if !open.content.is_empty() {
                    open.content.push(' ');
                }
                open.content.push_str(line);
            }
        }
    }
    tags
}
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_phpdoc_bridge` — PASS; clippy;
fmt; `cargo deny check` (new crate).

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_phpdoc_bridge Cargo.lock
git commit -m "✨ feat(phpdoc-bridge): the bridge crate and its total docblock lexer"
```

---

### Task 12: Standard tag extraction and the expression grammar

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/expression.rs`
- Create: `crates/celerrate_phpdoc_bridge/src/tags.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs`

**Interfaces:**
- Consumes: `Tag`/`lex_docblock` (Task 11);
  `celerrate_plugin::{VirtualMember, VirtualMemberKind,
  VirtualParameter}`.
- Produces (Tasks 13-15 consume):

```rust
// expression.rs — the 4a standard notation (decision 6):
//   union        := intersection ('|' intersection)*
//   intersection := suffixed ('&' suffixed)*
//   suffixed     := atom ('[' ']')*
//   atom         := '?' suffixed | '(' union ')' | name
// Anything else (generics, shapes, literals, class-string<T> — the
// PHPStan dialect, plan 4b) answers None: per-construct loss.
pub enum TypeExpression {
    Name(String),
    Nullable(Box<TypeExpression>),
    Union(Vec<TypeExpression>),
    Intersection(Vec<TypeExpression>),
    ArrayOf(Box<TypeExpression>),
}
pub fn parse_type_expression_text(text: &str) -> Option<TypeExpression>;

// tags.rs:
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberDocblock {
    pub return_type: Option<TypeExpression>,      // @return
    pub value_type: Option<TypeExpression>,       // @var
    pub parameters: Vec<(String, TypeExpression)>, // @param
    pub throws: Vec<TypeExpression>,              // @throws
}
pub fn extract_member_docblock(tags: &[Tag]) -> MemberDocblock;
pub fn extract_virtual_members(tags: &[Tag]) -> Vec<VirtualMember>;
```

- [x] **Step 1: Write the failing tests**

`expression.rs` tests:

```rust
#[test]
fn the_standard_grammar_parses() {
    use TypeExpression::*;
    assert_eq!(parse_type_expression_text("int"), Some(Name("int".to_owned())));
    assert_eq!(
        parse_type_expression_text("?string"),
        Some(Nullable(Box::new(Name("string".to_owned())))),
    );
    assert_eq!(
        parse_type_expression_text("int|null"),
        Some(Union(vec![Name("int".to_owned()), Name("null".to_owned())])),
    );
    assert_eq!(
        parse_type_expression_text("Countable&Traversable"),
        Some(Intersection(vec![
            Name("Countable".to_owned()),
            Name("Traversable".to_owned()),
        ])),
    );
    assert_eq!(
        parse_type_expression_text("User[]"),
        Some(ArrayOf(Box::new(Name("User".to_owned())))),
    );
    assert_eq!(
        parse_type_expression_text("(int|string)[]"),
        Some(ArrayOf(Box::new(Union(vec![
            Name("int".to_owned()),
            Name("string".to_owned()),
        ])))),
    );
    assert_eq!(
        parse_type_expression_text("\\App\\User"),
        Some(Name("\\App\\User".to_owned())),
    );
}

#[test]
fn dialect_constructs_and_garbage_answer_none() {
    for text in [
        "array<int, string>", "array{id: int}", "class-string<T>",
        "'literal'", "int<1, max>", "", "|", "?", "int|", "((int)",
        "int string",
    ] {
        assert_eq!(parse_type_expression_text(text), None, "{text}");
    }
}

#[test]
fn adversarial_expressions_never_panic() {
    for text in ["????", "(((((", "]][[", "\u{0}|\u{0}", "&&&", "a".repeat(10_000).as_str()] {
        let _ = parse_type_expression_text(text);
    }
}
```

`tags.rs` tests:

```rust
#[test]
fn a_member_docblock_extracts_all_standard_tags() {
    let tags = lex_docblock(
        "/**\n * @param int $id\n * @param ?string $name optional prose\n * @return bool\n * @throws \\RuntimeException\n */",
    );
    let extracted = extract_member_docblock(&tags);
    assert_eq!(extracted.parameters.len(), 2);
    assert_eq!(extracted.parameters[0].0, "id");
    assert_eq!(extracted.parameters[1].0, "name");
    assert!(extracted.return_type.is_some());
    assert_eq!(extracted.throws.len(), 1);
    assert_eq!(extracted.value_type, None);
}

#[test]
fn var_reads_the_value_type_and_first_tag_wins_on_duplicates() {
    let tags = lex_docblock("/** @var int */");
    assert!(extract_member_docblock(&tags).value_type.is_some());
    let tags = lex_docblock("/**\n * @return int\n * @return string\n */");
    assert_eq!(
        extract_member_docblock(&tags).return_type,
        Some(TypeExpression::Name("int".to_owned())),
    );
}

#[test]
fn malformed_tags_are_ignored_per_construct() {
    // The unparseable @param drops; the good one survives; the
    // by-reference and variadic sigils are tolerated.
    let tags = lex_docblock(
        "/**\n * @param array<int> $broken\n * @param int $good\n * @param string &$reference\n * @param int ...$rest\n * @param $untyped\n */",
    );
    let extracted = extract_member_docblock(&tags);
    let names: Vec<&str> = extracted.parameters.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, vec!["good", "reference", "rest"]);
}

#[test]
fn property_tags_declare_virtual_properties() {
    let tags = lex_docblock(
        "/**\n * @property string $title\n * @property-read int $id\n * @property-write ?string $slug\n * @property $untyped\n */",
    );
    let members = extract_virtual_members(&tags);
    assert_eq!(members.len(), 4);
    assert!(members.iter().all(|member| member.kind == VirtualMemberKind::Property));
    assert_eq!(members[0].name, "title");
    assert_eq!(members[0].type_text.as_deref(), Some("string"));
    assert_eq!(members[3].name, "untyped");
    assert_eq!(members[3].type_text, None);
}

#[test]
fn method_tags_declare_virtual_methods() {
    let tags = lex_docblock(
        "/**\n * @method static User find(int $id, ?string $name = null)\n * @method void clear()\n * @method broken(\n */",
    );
    let members = extract_virtual_members(&tags);
    assert_eq!(members.len(), 2);
    let find = &members[0];
    assert_eq!(find.name, "find");
    assert!(find.is_static);
    assert_eq!(find.type_text.as_deref(), Some("User"));
    assert_eq!(find.parameters.len(), 2);
    assert_eq!(find.parameters[0].name, "id");
    assert!(!find.parameters[0].optional);
    assert_eq!(find.parameters[1].name, "name");
    assert!(find.parameters[1].optional);
    let clear = &members[1];
    assert_eq!(clear.name, "clear");
    assert!(!clear.is_static);
    assert_eq!(clear.type_text.as_deref(), Some("void"));
    assert!(clear.parameters.is_empty());
}
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge 2>&1 | tail -5`
Expected: FAIL to compile.

- [x] **Step 3: Implement**

`expression.rs`: a recursive-descent parser over a peekable char
cursor with a depth guard (a `depth: u32` parameter refusing past 64 —
adversarial nesting must not overflow the stack). Name characters:
`is_alphanumeric()`, `_`, `\`, and any character `>= '\u{80}'`.
The parse must consume the entire input (trailing whitespace allowed)
or answer `None`. No `unwrap`, no indexing — iterate with
`chars().peekable()` and build owned strings.

`tags.rs`: over the `Tag` stream, dispatch by `tag.name`:

- `param`: `content.split_whitespace()`; if the first token starts
  with `$`, `&$`, or `...$` there is no type — skip (nothing to
  contribute). Otherwise token 1 parses as the type (unparseable →
  skip this tag), token 2 must be the variable: strip leading `&`,
  then `...`, then `$` — the remainder is the name (empty → skip).
  Trailing prose ignored. First tag per parameter name wins.
- `return` / `var`: first whitespace token parses as the type;
  first tag wins.
- `throws`: first token parses; every parseable tag accumulates.
- `property`, `property-read`, `property-write`: tokens are
  `[type] $name`; a single `$name` token means untyped. The
  read/write distinction is not modeled (existence only — recorded
  debt). `type_text` stores the raw type token verbatim (unresolved
  text is the virtual-symbol contract).
- `method`: find `(`; the segment before it splits on whitespace —
  the last token is the method name (must be a valid identifier,
  else skip), preceding tokens may be `static` and/or one return
  type token. The segment inside the parentheses (no nested
  parentheses in 4a — a nested `(` skips the tag) splits on `,`;
  each parameter splits on whitespace into `[type] $name [= default]`
  — a `=` anywhere after the name marks `optional: true`, a `...$`
  prefix marks `variadic: true`. Missing `)` skips the tag.

Wire `mod expression; mod tags;` plus `pub use` of the produced names
in `lib.rs`.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_phpdoc_bridge` — PASS; clippy; fmt.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_phpdoc_bridge
git commit -m "✨ feat(phpdoc-bridge): standard tag extraction over the 4a expression grammar"
```

---
### Task 13: The bridge as type syntax

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/syntax.rs`
- Create: `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs`,
  `crates/celerrate_phpdoc_bridge/Cargo.toml` (dev-dependencies)

**Interfaces:**
- Consumes: Tasks 11-12; `celerrate_plugin::{AnnotationSite,
  ParsedAnnotations, TypeSyntax, TypeId}`.
- Produces: `PhpdocBridge` (unit struct, `new()`, `Default`,
  `Clone`), implementing `TypeSyntax`; `descriptor() ->
  PluginDescriptor` with identity
  `{ name: "phpdoc-bridge", version: env!("CARGO_PKG_VERSION"),
  configuration: "" }` and `api_version: PLUGIN_API_VERSION`.
  Task 16 registers exactly this pair.

- [x] **Step 1: Write the failing tests**

Unit tests in `syntax.rs` need an `AnnotationSite`, which only
`celerrate_types` constructs — so the lowering is tested end-to-end.
Add dev-dependencies to the bridge's `Cargo.toml` (test-only, exempt
from the dependency-shape rule, recorded):

```toml
[dev-dependencies]
celerrate_db = { path = "../celerrate_db" }
celerrate_project = { path = "../celerrate_project" }
celerrate_semantics = { path = "../celerrate_semantics" }
celerrate_source = { path = "../celerrate_source" }
celerrate_stubs = { path = "../celerrate_stubs" }
celerrate_types = { path = "../celerrate_types" }
```

`tests/end_to_end.rs` — build the standard fixture (the quartet
recipe from `crates/celerrate_semantics/src/linearize.rs:834-863`:
`TestDatabase`, per-source `SourceFile`, `AnalyzedFileSet`,
`StubIndexInput` at HIGH, `ProjectConfiguration` at MEDIUM), then
register the bridge:

```rust
fn register_bridge(db: &celerrate_db::testing::TestDatabase) {
    let bridge = std::sync::Arc::new(celerrate_phpdoc_bridge::PhpdocBridge::new());
    let identity = celerrate_phpdoc_bridge::descriptor().identity;
    celerrate_types::TypeSyntaxRegistry::builder(vec![
        celerrate_types::TypeSyntaxRegistration {
            identity: identity.clone(),
            implementation: bridge,
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(db);
}

#[test]
fn a_return_annotation_refines_the_declared_member_signature() {
    let fixture = fixture(&[
        "<?php class Animal {} class Dog extends Animal {} class Kennel { /** @return Dog */ public function adopt(): Animal {} }",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "Kennel", MemberKind::Method, "adopt");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    let dog = celerrate_types::TypeId::class(&fixture.db, "Dog", Vec::new());
    assert_eq!(signature.value_type, dog);
    assert_eq!(signature.value_trust, celerrate_types::Trust::Refined);
}

#[test]
fn class_names_qualify_at_the_declaring_site() {
    let fixture = fixture(&[
        "<?php namespace App\\Model; class User {}",
        "<?php namespace App;\nuse App\\Model\\User;\nclass Repository { /** @return User|null */ public function find() {} }",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "App\\Repository", MemberKind::Method, "find");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    let db = &fixture.db;
    let expected = celerrate_types::TypeId::union(
        db,
        [
            celerrate_types::TypeId::class(db, "App\\Model\\User", Vec::new()),
            celerrate_types::TypeId::null(db),
        ],
    );
    assert_eq!(signature.value_type, expected);
}

#[test]
fn param_var_and_throws_annotations_land() {
    let fixture = fixture(&[
        "<?php class C { /** @var int[] */ public $numbers; /** @param ?string $name @return bool */ public function greet($name) {} }",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let numbers = member_query(&fixture, "C", MemberKind::Property, "numbers");
    let numbers_signature = celerrate_types::declared_member_signature(
        db, fixture.files, fixture.stubs, fixture.configuration, numbers,
    )
    .unwrap();
    let key = celerrate_types::TypeId::union(db, [
        celerrate_types::TypeId::int(db),
        celerrate_types::TypeId::string(db),
    ]);
    assert_eq!(
        numbers_signature.value_type,
        celerrate_types::TypeId::array(db, key, celerrate_types::TypeId::int(db)),
    );
    let greet = member_query(&fixture, "C", MemberKind::Method, "greet");
    let greet_signature = celerrate_types::declared_member_signature(
        db, fixture.files, fixture.stubs, fixture.configuration, greet,
    )
    .unwrap();
    assert_eq!(
        greet_signature.parameters[0].parameter_type,
        Some(celerrate_types::TypeId::union(db, [
            celerrate_types::TypeId::string(db),
            celerrate_types::TypeId::null(db),
        ])),
    );
}

#[test]
fn a_function_docblock_flows_through_the_function_seam() {
    let fixture = fixture(&["<?php /** @return int */ function answer() {}"]);
    register_bridge(&fixture.db);
    let query = celerrate_types::FunctionQuery::new(
        &fixture.db,
        celerrate_semantics::folded_symbol_key(celerrate_semantics::SymbolSpace::Function, "answer"),
    );
    let signature = celerrate_types::declared_function_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.value_type, celerrate_types::TypeId::int(&fixture.db));
    assert_eq!(signature.value_trust, celerrate_types::Trust::Refined);
}
```

(One caveat baked into the third test: the docblock
`@param ... @return ...` on one line lexes as one `param` tag whose
content contains `@return` — if the lexer folds it, split the fixture
docblock across lines instead; the assertion shape stays. Multi-tag
single-line docblocks are not part of the contract.) Add a
`fixture`/`member_query` support block at the top of the file
following `linearize.rs:834-863` and `declared.rs`'s test helpers.

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge 2>&1 | tail -5`
Expected: FAIL to compile (`PhpdocBridge`, `descriptor` missing).

- [x] **Step 3: Implement**

`src/syntax.rs`:

```rust
//! The bridge as a type-syntax implementation: standard PHPDoc over
//! the 4a expression grammar, lowered through the facade's builders.

use celerrate_plugin::{AnnotationSite, ParsedAnnotations, TypeId, TypeSyntax};

use crate::expression::TypeExpression;
use crate::lexer::lex_docblock;
use crate::tags::extract_member_docblock;

/// The `phpdoc-bridge` plugin. Stateless by design: guest
/// statelessness is the WASM sketch's first acceptance case, and the
/// native tier honors it by construction.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhpdocBridge;

impl PhpdocBridge {
    pub fn new() -> Self {
        Self
    }
}

impl TypeSyntax for PhpdocBridge {
    fn can_parse(&self, _docblock: &str) -> bool {
        // The bridge owns the inherited notation and registers first
        // (decision 8): it claims every docblock it is offered.
        true
    }

    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db> {
        let tags = lex_docblock(docblock);
        let extracted = extract_member_docblock(&tags);
        ParsedAnnotations {
            return_type: extracted.return_type.as_ref().map(|expression| lower(site, expression)),
            value_type: extracted.value_type.as_ref().map(|expression| lower(site, expression)),
            parameters: extracted
                .parameters
                .iter()
                .map(|(name, expression)| (name.clone(), lower(site, expression)))
                .collect(),
            throws: extracted.throws.iter().map(|expression| lower(site, expression)).collect(),
        }
    }

    fn parse_type_expression<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<TypeId<'db>> {
        crate::expression::parse_type_expression_text(expression)
            .map(|parsed| lower(site, &parsed))
    }
}

/// Lowers a parsed expression through the facade's builders. Keywords
/// go through the shared native table; everything else qualifies at
/// the declaring site and becomes a class type.
fn lower<'db>(site: &AnnotationSite<'db, '_>, expression: &TypeExpression) -> TypeId<'db> {
    let db = site.database();
    match expression {
        TypeExpression::Name(name) => site.keyword_type(name).unwrap_or_else(|| {
            TypeId::class(db, &site.qualify_class_name(name), Vec::new())
        }),
        TypeExpression::Nullable(inner) => {
            TypeId::union(db, [lower(site, inner), TypeId::null(db)])
        }
        TypeExpression::Union(parts) => {
            TypeId::union(db, parts.iter().map(|part| lower(site, part)).collect::<Vec<_>>())
        }
        TypeExpression::Intersection(parts) => TypeId::intersection(
            db,
            parts.iter().map(|part| lower(site, part)).collect::<Vec<_>>(),
        ),
        TypeExpression::ArrayOf(element) => {
            let key = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
            TypeId::array(db, key, lower(site, element))
        }
    }
}
```

`src/lib.rs` additions:

```rust
mod syntax;

pub use syntax::PhpdocBridge;

/// What the composition root registers.
pub fn descriptor() -> celerrate_plugin::PluginDescriptor {
    celerrate_plugin::PluginDescriptor {
        identity: celerrate_plugin::PluginIdentity {
            name: "phpdoc-bridge".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            configuration: String::new(),
        },
        api_version: celerrate_plugin::PLUGIN_API_VERSION,
    }
}
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_phpdoc_bridge` — PASS; then the
workspace gate.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_phpdoc_bridge Cargo.lock
git commit -m "✨ feat(phpdoc-bridge): standard PHPDoc lowers through the type-syntax point end to end"
```

---

### Task 14: The bridge as virtual symbols

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/virtual_members.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs`,
  `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs`

**Interfaces:**
- Consumes: `extract_virtual_members` (Task 12),
  `celerrate_plugin::{VirtualMember, VirtualSymbolProvider}`.
- Produces: `impl VirtualSymbolProvider for PhpdocBridge`. Task 16
  registers the same `Arc<PhpdocBridge>` in both registries.

- [x] **Step 1: Write the failing tests**

In `virtual_members.rs` (pure, no database):

```rust
#[test]
fn the_bridge_contributes_property_and_method_members() {
    let bridge = PhpdocBridge::new();
    let members = bridge.virtual_members(
        "/**\n * @property string $title\n * @method static User find(int $id)\n */",
    );
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].name, "title");
    assert_eq!(members[1].name, "find");
}

#[test]
fn a_docblock_without_virtual_tags_contributes_nothing() {
    let bridge = PhpdocBridge::new();
    assert!(bridge.virtual_members("/** @return int */").is_empty());
}
```

In `tests/end_to_end.rs` (register the bridge in **both** registries;
extend `register_bridge` to also set `VirtualSymbolRegistry` with the
same `Arc`):

```rust
#[test]
fn a_property_annotation_declares_a_member_that_exists_and_types() {
    let fixture = fixture(&[
        "<?php /** @property string $title */ class Post {}",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "Post", MemberKind::Property, "title");
    let resolution = celerrate_semantics::lookup_member(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    );
    assert!(matches!(
        resolution,
        Some(celerrate_semantics::MemberResolution::Virtual { .. }),
    ));
    let signature = celerrate_types::declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.value_type, celerrate_types::TypeId::string(&fixture.db));
    assert_eq!(signature.value_trust, celerrate_types::Trust::Refined);
}

#[test]
fn a_method_annotation_declares_a_typed_virtual_method() {
    let fixture = fixture(&[
        "<?php class User {} /** @method static User find(int $id) */ class Repository {}",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "Repository", MemberKind::Method, "find");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::class(&fixture.db, "User", Vec::new()),
    );
    assert_eq!(signature.parameters.len(), 1);
    assert_eq!(
        signature.parameters[0].parameter_type,
        Some(celerrate_types::TypeId::int(&fixture.db)),
    );
}
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge 2>&1 | tail -5`
Expected: FAIL to compile (`VirtualSymbolProvider` not implemented).

- [x] **Step 3: Implement**

`src/virtual_members.rs`:

```rust
//! The bridge as a virtual-symbol provider: `@property` (and its
//! read/write variants) and `@method` declare members that exist for
//! the unknown-members family. Payload text stays unresolved — it
//! types downstream through the type-syntax point.

use celerrate_plugin::{VirtualMember, VirtualSymbolProvider};

use crate::lexer::lex_docblock;
use crate::syntax::PhpdocBridge;
use crate::tags::extract_virtual_members;

impl VirtualSymbolProvider for PhpdocBridge {
    fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
        extract_virtual_members(&lex_docblock(class_docblock))
    }
}
```

Add `mod virtual_members;` to `lib.rs`.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_phpdoc_bridge` — PASS; workspace
gate.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_phpdoc_bridge
git commit -m "✨ feat(phpdoc-bridge): property and method annotations declare virtual members"
```

---

### Task 15: Docblock lexer fuzzing

**Files:**
- Create: `fuzz/fuzz_targets/docblock.rs`
- Create: `fuzz/corpus/docblock/seed`
- Modify: `fuzz/Cargo.toml`, `.github/workflows/fuzz.yml`

**Interfaces:**
- Consumes: `lex_docblock`, `parse_type_expression_text`,
  `extract_member_docblock`, `extract_virtual_members` (all public).
- Produces: the third fuzz target, same contract as the PHP parser —
  arbitrary input, never a panic (design section 10, harness 4).

- [x] **Step 1: Add the target**

`fuzz/Cargo.toml`: add
`celerrate_phpdoc_bridge = { path = "../crates/celerrate_phpdoc_bridge" }`
to `[dependencies]` and a third `[[bin]]` block named `docblock`
(same `test = false, doc = false, bench = false` shape as `lex`).

`fuzz/fuzz_targets/docblock.rs`:

```rust
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let tags = celerrate_phpdoc_bridge::lex_docblock(text);
        let _ = celerrate_phpdoc_bridge::extract_member_docblock(&tags);
        let _ = celerrate_phpdoc_bridge::extract_virtual_members(&tags);
        let _ = celerrate_phpdoc_bridge::parse_type_expression_text(text);
    }
});
```

`fuzz/corpus/docblock/seed` (committed seed floor):

```
/**
 * Summary prose.
 *
 * @param int|string $id
 * @param ?App\Model\User $user
 * @return (int|string)[]
 * @throws \RuntimeException
 * @property-read string $title
 * @method static User find(int $id, ?string $name = null)
 * @var Countable&Traversable
 */
```

`.github/workflows/fuzz.yml`: add
`- run: cargo +nightly fuzz run docblock -- -max_total_time=${{ steps.duration.outputs.seconds }} -timeout=25 -rss_limit_mb=4096`
after the `parse` line; update the job comment to "Three targets at
30 minutes each" and `timeout-minutes` from 90 to 120.

- [x] **Step 2: Verify locally**

Run: `cargo check --manifest-path fuzz/Cargo.toml` (the target
compiles without nightly). If a nightly toolchain is available, a
short smoke run:
`cargo +nightly fuzz run docblock -- -max_total_time=30 -rss_limit_mb=4096`
Expected: no crash. If nightly is unavailable locally, the CI smoke
run (60 seconds on push) covers it — say so in the task report.

- [x] **Step 3: Commit**

```bash
git add fuzz .github/workflows/fuzz.yml
git commit -m "✅ test(phpdoc-bridge): the docblock lexer joins the fuzz harness"
```

---
### Task 16: Composition-root registration

**Files:**
- Create: `crates/celerrate_cli/src/plugins.rs`
- Modify: `crates/celerrate_cli/Cargo.toml` (add `celerrate_types`,
  `celerrate_plugin`, `celerrate_phpdoc_bridge`),
  `crates/celerrate_cli/src/session.rs`, the crate's module list, and
  the command path that reports to stderr (follow where `Session`
  results surface — `check.rs`/`main.rs`).

**Interfaces:**
- Consumes: the three registries, `validate_claims`,
  `celerrate_phpdoc_bridge::{PhpdocBridge, descriptor}`,
  `celerrate_plugin::PLUGIN_API_VERSION`.
- Produces:

```rust
pub struct ExcludedPlugin { pub name: String, pub reason: String }
pub struct RegisteredPlugins { pub excluded: Vec<ExcludedPlugin> }
pub fn register_plugins(database: &AnalysisDatabase) -> RegisteredPlugins
```

  Registration order is THE deterministic contribution order and is
  declared here, once: `phpdoc-bridge` first (currently alone).
  `Session` stores the result; a degraded run prints one warning line
  per excluded plugin on stderr before diagnostics render.

- [x] **Step 1: Write the failing tests**

In `plugins.rs`'s tests module:

```rust
#[test]
fn the_composition_root_registers_the_bridge_in_every_registry_it_serves() {
    let database = AnalysisDatabase::default();
    let plugins = register_plugins(&database);
    assert!(plugins.excluded.is_empty());
    let syntax = celerrate_types::TypeSyntaxRegistry::try_get(&database).unwrap();
    let registrations = syntax.registrations(&database);
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].identity.name, "phpdoc-bridge");
    assert_eq!(
        registrations[0].identity.version,
        env!("CARGO_PKG_VERSION"),
    );
    let virtual_symbols =
        celerrate_semantics::VirtualSymbolRegistry::try_get(&database).unwrap();
    assert_eq!(virtual_symbols.registrations(&database).len(), 1);
    let providers =
        celerrate_types::DynamicTypeProviderRegistry::try_get(&database).unwrap();
    assert!(providers.registrations(&database).is_empty());
}

#[test]
fn an_api_version_mismatch_excludes_and_reports() {
    // Exercise the dormant check through the internal helper that
    // takes the descriptor as data (the public path cannot mismatch
    // for compiled-in plugins — the design says so; this pins the
    // scaffolding anyway).
    let mismatched = celerrate_plugin::PluginDescriptor {
        identity: celerrate_phpdoc_bridge::descriptor().identity,
        api_version: celerrate_plugin::PLUGIN_API_VERSION + 1,
    };
    let verdict = admission(&mismatched);
    assert!(matches!(verdict, Err(reason) if reason.contains("API version")));
}
```

(The bridge crate version equals the workspace version — every crate
uses `version.workspace = true` — so the `env!` assertion holds in
the CLI too.)

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli plugins 2>&1 | tail -5`
Expected: FAIL to compile.

- [x] **Step 3: Implement**

`crates/celerrate_cli/src/plugins.rs`:

```rust
//! The composition root's plugin registration: the one place
//! implementations are constructed and set into the owning crates'
//! registries. Order here IS the deterministic dispatch order. The
//! registries sit in the high-durability tier next to stubs and
//! configuration, set once per process, never mutated.

use std::sync::Arc;

use celerrate_plugin::{PLUGIN_API_VERSION, PluginDescriptor};

use crate::database::AnalysisDatabase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedPlugin {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisteredPlugins {
    pub excluded: Vec<ExcludedPlugin>,
}

/// The dormant API-version gate (the parent's crash semantics:
/// exclude, degrade, never crash).
fn admission(descriptor: &PluginDescriptor) -> Result<(), String> {
    if descriptor.api_version == PLUGIN_API_VERSION {
        Ok(())
    } else {
        Err(format!(
            "plugin API version {} does not match the binary's {}",
            descriptor.api_version, PLUGIN_API_VERSION,
        ))
    }
}

pub fn register_plugins(database: &AnalysisDatabase) -> RegisteredPlugins {
    let mut excluded = Vec::new();
    let mut type_syntax = Vec::new();
    let mut virtual_symbols = Vec::new();
    let dynamic_providers = Vec::new();

    // Registration order, declared once: phpdoc-bridge first.
    let descriptor = celerrate_phpdoc_bridge::descriptor();
    match admission(&descriptor) {
        Ok(()) => {
            let bridge = Arc::new(celerrate_phpdoc_bridge::PhpdocBridge::new());
            type_syntax.push(celerrate_types::TypeSyntaxRegistration {
                identity: descriptor.identity.clone(),
                implementation: bridge.clone(),
            });
            virtual_symbols.push(celerrate_semantics::VirtualSymbolRegistration {
                identity: descriptor.identity,
                provider: bridge,
            });
        }
        Err(reason) => excluded.push(ExcludedPlugin {
            name: descriptor.identity.name,
            reason,
        }),
    }

    // Overlapping dynamic-provider claims exclude the later
    // registrant (no documented precedence exists yet) — dormant
    // until plan 7 registers the stdlib provider.
    if let Err(conflict) = celerrate_types::validate_claims(&dynamic_providers) {
        excluded.push(ExcludedPlugin {
            name: conflict.second.clone(),
            reason: format!("claim conflict with {} on {:?}", conflict.first, conflict.claim),
        });
        // With more than one provider, rebuild the vector without the
        // excluded registrant and validate again; with none, nothing
        // to do.
    }

    celerrate_types::TypeSyntaxRegistry::builder(type_syntax)
        .durability(salsa::Durability::HIGH)
        .new(database);
    celerrate_semantics::VirtualSymbolRegistry::builder(virtual_symbols)
        .durability(salsa::Durability::HIGH)
        .new(database);
    celerrate_types::DynamicTypeProviderRegistry::builder(dynamic_providers)
        .durability(salsa::Durability::HIGH)
        .new(database);

    RegisteredPlugins { excluded }
}
```

Wiring: in `Session::start`
(`crates/celerrate_cli/src/session.rs:100-151`), immediately after
the four existing inputs are created, call
`let plugins = crate::plugins::register_plugins(&database);` and
store it on the `Session` struct (`pub plugins: RegisteredPlugins`).
Where the check/watch commands own a `Session`, print for each
exclusion, before diagnostics render:
`eprintln!("warning: plugin {name} excluded: {reason}; the run is degraded");`
Add the module to the crate's module list and the three new
dependencies to `Cargo.toml`.

Expectation to verify while here: registering the bridge changes **no
existing diagnostic** — `cargo test --package celerrate_cli` (the
check snapshots, cache equivalence, and registry tests all stay
green; the new crates allocate no `CEL####` identifiers, so
`tests/registry.rs` is unaffected).

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli` — PASS; full workspace
gate.

- [x] **Step 5: Commit**

```bash
git add crates/celerrate_cli Cargo.lock
git commit -m "✨ feat(cli): the composition root registers the bridge into the extension registries"
```

---

### Task 17: The CI dependency-shape check

**Files:**
- Create: `xtask/src/dependency_shape.rs`
- Modify: `xtask/src/lib.rs`, `xtask/src/main.rs`,
  `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `cargo metadata --format-version 1 --no-deps`
  (serde_json; xtask deliberately depends on no `celerrate_*` crate),
  `crate::workspace_root()`, the `Result` alias (`xtask/src/lib.rs:22`).
- Produces: `cargo xtask dependency-shape` — fails when a plugin
  crate declares a workspace dependency other than
  `celerrate_plugin` (dev-dependencies exempt), or when a listed
  plugin crate is missing from the workspace.

- [x] **Step 1: Write the failing tests**

In `dependency_shape.rs`'s tests module (pure JSON fixtures, no
cargo invocation):

```rust
fn metadata(packages: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "packages": packages })
}

fn package(name: &str, dependencies: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "name": name, "dependencies": dependencies })
}

#[test]
fn a_clean_shape_passes() {
    let value = metadata(serde_json::json!([
        package("celerrate_phpdoc_bridge", serde_json::json!([
            { "name": "celerrate_plugin", "kind": null },
            { "name": "celerrate_types", "kind": "dev" },
        ])),
        package("celerrate_cli", serde_json::json!([
            { "name": "celerrate_types", "kind": null },
        ])),
    ]));
    assert!(check(&value).is_ok());
}

#[test]
fn a_workspace_dependency_beyond_the_facade_fails() {
    let value = metadata(serde_json::json!([
        package("celerrate_phpdoc_bridge", serde_json::json!([
            { "name": "celerrate_plugin", "kind": null },
            { "name": "celerrate_types", "kind": null },
        ])),
    ]));
    let error = check(&value).unwrap_err().to_string();
    assert!(error.contains("celerrate_phpdoc_bridge"));
    assert!(error.contains("celerrate_types"));
}

#[test]
fn a_missing_plugin_crate_fails() {
    let value = metadata(serde_json::json!([
        package("celerrate_cli", serde_json::json!([])),
    ]));
    assert!(check(&value).is_err());
}
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package xtask dependency_shape 2>&1 | tail -5`
Expected: FAIL to compile.

- [x] **Step 3: Implement**

```rust
//! The dependency-shape check: plugin crates depend on
//! `celerrate_plugin` and nothing else in the workspace. An extension
//! point that proves insufficient is extended, never bypassed — this
//! check is what makes "never bypassed" mechanical.

/// The plugin crates under the rule. Plan 7 adds the stdlib type
/// provider here.
const PLUGIN_CRATES: &[&str] = &["celerrate_phpdoc_bridge"];
const ALLOWED_DEPENDENCY: &str = "celerrate_plugin";

pub fn run() -> crate::Result<()> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(crate::workspace_root()?)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    check(&metadata)
}

pub(crate) fn check(metadata: &serde_json::Value) -> crate::Result<()> {
    let packages = metadata
        .get("packages")
        .and_then(|value| value.as_array())
        .ok_or("cargo metadata: no packages array")?;
    let mut seen = std::collections::BTreeSet::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        if !PLUGIN_CRATES.contains(&name) {
            continue;
        }
        seen.insert(name.to_owned());
        let Some(dependencies) = package.get("dependencies").and_then(|value| value.as_array())
        else {
            continue;
        };
        for dependency in dependencies {
            let Some(dependency_name) =
                dependency.get("name").and_then(|value| value.as_str())
            else {
                continue;
            };
            // Dev-dependencies are test-only and exempt (recorded in
            // the design decisions); normal and build kinds are not.
            if dependency.get("kind").and_then(|value| value.as_str()) == Some("dev") {
                continue;
            }
            if dependency_name.starts_with("celerrate_")
                && dependency_name != ALLOWED_DEPENDENCY
            {
                return Err(format!(
                    "dependency shape violated: {name} depends on {dependency_name}; \
                     plugin crates depend only on {ALLOWED_DEPENDENCY}",
                )
                .into());
            }
        }
    }
    for expected in PLUGIN_CRATES {
        if !seen.contains(*expected) {
            return Err(
                format!("dependency shape: plugin crate {expected} not found in the workspace")
                    .into(),
            );
        }
    }
    Ok(())
}
```

`xtask/src/lib.rs`: add `pub mod dependency_shape;`.
`xtask/src/main.rs`: add the arm
`(Some("dependency-shape"), None) => xtask::dependency_shape::run(),`
and extend the usage line. `.github/workflows/ci.yml`: in the `lint`
job, after the clippy step, add
`- run: cargo xtask dependency-shape`.

- [x] **Step 4: Run the tests and the check to verify they pass**

Run: `cargo test --package xtask && cargo xtask dependency-shape`
Expected: PASS and a clean exit (the bridge's manifest is already
shaped right). Clippy; fmt.

- [x] **Step 5: Commit**

```bash
git add xtask .github/workflows/ci.yml
git commit -m "✨ feat(xtask): the dependency-shape check makes the one-dependency rule mechanical"
```

---

### Task 18: Closure — verification, debt ledger, the full gate

**Files:**
- Modify: `.claude/superpowers/plans/2026-07-15-type-engine-4a-plugin-api.md`
  (this file: check every box, append the debt ledger)
- Modify: `crates/celerrate_plugin/src/lib.rs` /
  `crates/celerrate_phpdoc_bridge/src/lib.rs` (crate-doc polish only,
  if review finds gaps)

- [x] **Step 1: Verify against the design (the checklist below)**

Walk `.claude/superpowers/specs/2026-07-14-type-engine-design.md`
sections 4 and 5 (standard-PHPDoc scope) against the shipped code —
each line names its tasks:

- §4 registries in owning crates, implementations at the composition
  root, facade re-exports → Tasks 1, 4, 5, 10, 16.
- §4 identity travels with the implementation; never backdates; HIGH
  durability → Tasks 1, 4, 5 (input shapes), 16 (durability).
- §4 object-safe traits, builders and query methods, no matchable
  enum exposure → Tasks 4, 5 (trait shapes), 10 (facade surface).
- §4 dispatch fixed now: claims + registration-time conflict;
  can-parse first win, registered order → Tasks 5, 4, 16.
- §4 API version, dormant for compiled-in plugins, degraded runs →
  Tasks 10, 16.
- §4 mechanical constraint (xtask over cargo metadata) → Task 17.
- §5 standard PHPDoc complete; virtual members through the
  virtual-symbol point with unresolved-text payloads → Tasks 1-3, 8,
  12, 14.
- §5 malformed annotations silently ignored, loss per construct, no
  docblock diagnostics → Tasks 11, 12 (posture tests).
- §5 two-stage cutoff at the parsed-annotation level → Tasks 6, 7.
- §10 harness 4, docblock lexer fuzzing → Task 15.
- Plan 3 debts: the seam swap (Task 6), the function seam (Task 9).

Out of scope, confirmed still out: the PHPStan dialect and its
pinned-reference coverage, Psalm synonyms, tool-prefixed tag
precedence (4b); comment directives, suppressions, the WASM sketch
(4c); the template scope convention (arrives with `@template` in 4b);
the stdlib provider (plan 7).

- [x] **Step 2: Write the debt ledger**

Append to this plan file a section `## Accepted debt at closure`
listing at minimum (plus whatever execution surfaced):

- Virtual members contribute no magic markers (`@method __call` does
  not suppress) — plan 8 decides whether the corpus needs it.
- Real members shadow virtual members entirely; PHPStan lets
  `@method` override a real method's signature — revisit with 4b if
  the corpus demands it.
- `@property-read`/`@property-write` collapse to existence; the
  read/write distinction is unmodeled.
- Constructor-promoted properties do not read the constructor's
  `@param` docblock for their property type.
- The 4a expression grammar excludes generics, shapes, literals,
  `class-string<T>` — plan 4b's dialect replaces the
  whitespace-splitting tag grammar and the expression parser
  together.
- `can_parse` is trivially `true` for the bridge; the protocol is
  exercised only by unit fakes until a second implementation exists.
- The dynamic-type-provider trait has no caller until plan 5 and no
  implementation until plan 7; `Invocation` is deliberately minimal.
- `throws` annotations are parsed, resolved, and inherited with no
  consumer in this sub-project.
- Multi-tag single-line docblocks (`@param ... @return ...` on one
  line) are not lexed apart — each tag on its own line is the
  contract.
- `member_annotations` on an inheriting class parses the ancestor's
  docblock directly through the child's query (consistent with the
  walk by construction, one redundant path).

- [x] **Step 3: The full gate, one last time**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check && cargo xtask dependency-shape`
Expected: all green.

- [x] **Step 4: Commit**

```bash
git add .claude/superpowers/plans/2026-07-15-type-engine-4a-plugin-api.md crates
git commit -m "📝 docs(plugin): the plugin API surface, its dispatch rules, and the closure debt ledger"
```

---

## Verification against the design (for the final reviewer)

- Design §4 "registry salsa inputs … live in the owning crates" →
  Tasks 1, 4, 5; the facade holds no registry (Task 10).
- Design §4 "implementation travels in the same input struct as its
  identity … never backdate … high-durability tier" → input shapes in
  Tasks 1/4/5, HIGH durability pinned in Task 16's test.
- Design §4 "providers claim symbols … registration-time error" →
  Task 5 (`validate_claims`), Task 16 (exclusion path).
- Design §4 "consulted in registered order with a can-parse
  protocol, first win" → Task 4 (dispatch + tests).
- Design §4 "single explicit version checked … dormant scaffolding" →
  Tasks 10, 16.
- Design §4 "enforced in CI by an xtask over cargo metadata" →
  Task 17.
- Design §5 "Standard PHPDoc, complete … @property/@method declare
  virtual members … unresolved text … resolve downstream through the
  type-syntax point" → Tasks 1, 2, 3, 8, 12, 14.
- Design §5 "malformed annotation is silently ignored … no docblock
  diagnostics" → Tasks 11, 12; no `CEL####` allocated anywhere.
- Design §5 "separate per-member query … two-stage cutoff" → Tasks 6,
  7.
- Design §10 harness 4 "docblock lexer fuzzing … never a panic" →
  Task 15 (+ Task 11's adversarial unit tests).
- Plan 3 seam ("plan 4a swaps one query body") → Task 6. Plan 3 debt
  "function annotations have no seam yet" → Task 9. Plan 1a deferred
  "second-stage cutoff is plan 4's" → Task 7.

## Accepted debt at closure

- Virtual members contribute no magic markers (`@method __call` does
  not suppress) — plan 8 decides whether the corpus needs it.
- Real members shadow virtual members entirely; PHPStan lets
  `@method` override a real method's signature — revisit with 4b if
  the corpus demands it.
- `@property-read`/`@property-write` collapse to existence; the
  read/write distinction is unmodeled.
- Constructor-promoted properties do not read the constructor's
  `@param` docblock for their property type.
- The 4a expression grammar excludes generics, shapes, literals,
  `class-string<T>` — plan 4b's dialect replaces the
  whitespace-splitting tag grammar and the expression parser
  together.
- `can_parse` is trivially `true` for the bridge; the protocol is
  exercised only by unit fakes until a second implementation exists.
- The dynamic-type-provider trait has no caller until plan 5 and no
  implementation until plan 7; `Invocation` is deliberately minimal.
- `throws` annotations are parsed, resolved, and inherited with no
  consumer in this sub-project.
- Multi-tag single-line docblocks (`@param ... @return ...` on one
  line) are not lexed apart — each tag on its own line is the
  contract.
- `member_annotations` on an inheriting class parses the ancestor's
  docblock directly through the child's query (consistent with the
  walk by construction, one redundant path).
- **PLAN-TEXT DEVIATION (adjudicated at review, human can veto —
  validated by the human on 2026-07-15, deviation accepted):** the
  plan's Task 7 asserted `declared_member_signature` backdates to 0
  executions on a prose-only docblock edit. Unachievable by
  construction: `declared_member_signature` reads `lookup_member`,
  whose `Member` value carries the raw docblock (whole-struct `Eq`),
  the design's own accepted cost. The shipped pin asserts the honest
  mechanism instead: `member_annotations` and `declared_member_signature`
  both re-run (`executions_of(&log, ...) == 1`, with rationale
  comments), the declared value type is unchanged, and a
  member-dependent hierarchy probe (`subtype_of(value_type, Entity)`)
  is spared at 0 executions, with a tag-edit companion test proving
  the probe family discriminates (both in
  `crates/celerrate_types/tests/invalidation_scope.rs`,
  `a_prose_only_docblock_edit_backdates_at_the_parsed_annotation_stage`
  and `an_annotation_edit_reaches_the_declared_signature`).
- `@return`/`@var` slots take the first PARSEABLE tag (fix a717703,
  `crates/celerrate_phpdoc_bridge/src/tags.rs`) — an unparseable tag
  consumes nothing, aligned with the per-construct-loss posture.
- The fake-provider/fake-syntax test helpers are duplicated across
  the `virtual_symbols`, `linearize`, `member_lookup`, `declared`, and
  `type_syntax` test modules — the pre-existing
  no-shared-test-support-module debt, now larger.
- Coverage gaps carried: the throws-inheritance fill-from-ancestor
  branch (`declared.rs`'s `inherited_annotations`) is untested
  (mirror of the tested value merge); a virtual parameter with an
  absent (vs. unparseable) `type_text` is untested; no "build" kind
  fixture exists in the dependency-shape tests
  (`xtask/src/dependency_shape.rs` only exercises `null` and `dev`
  kinds); the docblock lexer
  (`crates/celerrate_phpdoc_bridge/src/lexer.rs`) drops a mid-docblock
  line starting with `@` followed by a non-name character instead of
  folding it into the open tag's content.
- `function_annotations` and `declared_function_signature`
  (`crates/celerrate_types/src/declared.rs`) duplicate the five-step
  source-function lookup (symbol query, declaration lookup, file
  index binary search, member-tree find, docblock/site derivation) —
  extraction deferred; drift risk on future edits.
- Degraded-run plugin warnings print through a raw `eprintln!` in
  `crates/celerrate_cli/src/lib.rs` rather than the injectable output
  stream `run` receives, so the exclusion path has no in-process test;
  route it through a testable surface when plan 7 makes exclusion
  reachable.

