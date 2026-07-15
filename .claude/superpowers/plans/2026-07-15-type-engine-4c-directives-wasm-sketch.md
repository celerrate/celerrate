# Type Engine 4c — Directives and the WASM Sketch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The fifth extension point — comment directives — owned by
`celerrate_semantics`, the inline suppressions (`@phpstan-ignore-line`,
`@phpstan-ignore-next-line`, `@phpstan-ignore`, `@psalm-suppress`)
honored from the preview on through the bridge, the directive filter
applied at the single composition point in `celerrate_cli`, and the
WASM-level interface sketch written against its four-case acceptance
checklist. Design source:
`.claude/superpowers/specs/2026-07-14-type-engine-design.md`, sections 4
(the fifth extension point, the sketch), 5 (inline suppressions, the two
over-suppression rules), 10 (docblock-lexer-grade fuzzing), and 11 item
7 (this plan).

**Architecture:** The comment-directive extension point mirrors the
three registries plan 4a built: a trait owned by its consuming layer, a
`#[salsa::input(singleton)]` registry in the same crate at HIGH
durability, implementations registered once at the composition root,
`try_get` unset as the no-plugin path. A per-file tracked query on the
syntax-gating precedent (an own-tree read producing strictly-local
output) collects comment trivia, consults the registered providers, and
resolves their symbolic scopes into sorted, deduplicated `TextRange`s —
`Eq`-comparable, so a prose-comment edit that leaves the directive set
unchanged backdates. The filter runs inside `composed_diagnostics`, the
one composition point the cache, the equivalence harness, and the pass
all share (audit finding I2's lesson): the exit-code count, the printed
report, and the persisted verdict are the same post-filter set by
construction, and a warm verdict hit stays parse-free.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), salsa 0.27
(singleton registry inputs, tracked per-file queries), rowan 0.16
(token-level trivia walk), postcard + blake3 packs (one schema bump),
cargo-fuzz (nightly, the existing `docblock` target).

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
  No indexing: use `.get()`, `.strip_prefix()`, `.split_once()`,
  iterators. `TextRange::new` panics on `start > end`: every
  construction site must make the ordering structurally impossible.
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **The one-dependency rule**: `celerrate_phpdoc_bridge` depends on
  `celerrate_plugin` and nothing else in the workspace
  (dev-dependencies exempt; the `fuzz/` nested workspace is not a
  member and is exempt). Enforced by `cargo xtask dependency-shape` —
  it must stay green after every task.
- **An extension point that proves insufficient is extended, never
  bypassed** (design section 4). This plan exists because the four
  inherited families offer no channel for "extinguish diagnostics
  here": the bridge implements suppressions through the new trait, not
  through a side channel.
- **Over-suppression, never under-suppression** (design section 5): a
  suppression that fails to suppress is a false positive by the
  parent's own rationale. Suppression extinguishes **all diagnostic
  families** on the target scope — decode, syntax, gating, and
  semantic alike.
- **No docblock diagnostics, and no directive diagnostics**: malformed
  directive content yields fewer identifiers or no directive, never an
  error. Plan 4c allocates no new `CEL####` identifier.
- **Determinism**: no wall clock, no randomness, no environment reads
  inside queries. Providers are pure functions of their arguments;
  contributions concatenate in registered order.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.

## Fixed decisions (the header the tasks implement)

1. **The fourth registry mirrors the first three.** The
   `CommentDirectiveProvider` trait, its vocabulary, its
   `CommentDirectiveRegistration { identity, provider }`, and its
   `#[salsa::input(singleton)] CommentDirectiveRegistry` all live in
   `celerrate_semantics` (the consuming layer — the design's
   load-bearing arrangement: a registry in `celerrate_plugin` would
   break the DAG upward). Set once per process at the composition root
   with `salsa::Durability::HIGH`; `try_get(db).is_none()` is the
   no-plugin path every plain test database takes. The implementation
   travels in the same struct as its `PluginIdentity`
   (`crates/celerrate_semantics/src/plugin.rs:12-23`), so reading it
   records a dependency an upgrade invalidates. Dispatch is the
   virtual-symbol model, not first-win: **contributions concatenate in
   registered order** — suppression is a union, order-independent by
   nature, and the union is the over-suppression-safe combinator.

2. **The directive vocabulary belongs to the trait; the tag table to
   the bridge** (design section 4, verbatim split). The vocabulary is
   `CommentKind { Line, Block, Docblock }`, `DirectiveScope
   { CurrentLine, NextLine, CurrentAndNextLine, AnnotatedDeclaration }`,
   and `CommentDirective::Suppress { scope, identifiers }`. The
   PHPStan/Psalm-to-Celerrate mapping table is
   `celerrate_phpdoc_bridge::directives`' rustdoc, exactly like the tag
   precedence table is `dialect`'s.

3. **Scopes are symbolic; resolution to offsets happens in the owning
   query.** A provider is a pure function of `(kind, text)` — it cannot
   see positions, so it answers in lines-relative-to-the-comment terms
   and `suppressed_ranges` resolves them against
   `celerrate_db::line_index`. `CurrentAndNextLine` is the fixed
   resolution of PHPStan 1.11's placement-dependent bare
   `@phpstan-ignore` (own line when the comment trails code, next line
   when it stands alone): covering both lines is the superset that
   under-suppresses neither placement.

4. **`suppressed_ranges` is the syntax-gating precedent transposed**
   (`crates/celerrate_semantics/src/syntax_gating.rs:26-51`): a
   `#[salsa::tracked(returns(ref))]` per-file query reading
   `celerrate_db::parse(db, file).tree()` — the design's deliberate
   boundary exception, an output strictly local to the file — walking
   `descendants_with_tokens()` for the three comment kinds, producing a
   sorted, deduplicated `Vec<TextRange>`. `Eq`-comparable: a
   prose-comment edit that leaves the set unchanged backdates, and
   dependents never re-run (pinned by a probe test).

5. **Matching is by the diagnostic's start offset**, end-exclusive with
   one exception: an offset equal to a range's end matches when that
   end is the end of the text. The reported location
   (`path:line:column`) is the range start — that is the line the user
   targets — and the exception keeps an unexpected-end-of-file parse
   error (anchored exactly at the text's end) suppressible from the
   last line, or the suppression would under-suppress.

6. **The filter runs inside `composed_diagnostics`**
   (`crates/celerrate_cli/src/analysis.rs:140-155`), the single
   composition point `analyze_one`, `persist`, and the equivalence
   harness all share. Three consequences, each load-bearing: the exit
   code (`outcome.diagnostics.len()`), the printed report, and the
   persisted verdict are the same post-filter set by construction (the
   vendor-filter rationale at `analysis.rs:38-48`, applied again); a
   warm verdict hit serves already-filtered diagnostics and stays
   parse-free (the warm one-edit criterion is untouched); and the
   verdict's content-hash key covers every directive edit, because
   directives are strictly file-local by construction — a suppression
   comment edit changes the bytes, misses the cache, and recomputes.

7. **`CACHE_SCHEMA_VERSION` bumps 3 → 4.** The stored shapes do not
   change; their meaning does (verdict diagnostics are post-filter). In
   practice the header's binary self-hash already discards old packs on
   any rebuild; the bump is the named, reviewable record of the
   deliberate break, which is exactly what the constant is for
   (`crates/celerrate_cli/src/cache/pack.rs:16-30`).

8. **Identifiers are carried, never matched.** The identifier-bearing
   forms (`@phpstan-ignore method.notFound`, `@psalm-suppress
   PossiblyNullReference`) parse their identifier lists into
   `Vec<String>` and suppress every family on the scope regardless —
   identifier-level correspondence stays deferred to the rule framework
   (design section 5, verbatim).

9. **A docblock-attached `@psalm-suppress` maps to the annotated
   node's whole span** — its Psalm scope, never the docblock's own line
   where no diagnostic ever fires (design section 5). The annotated
   node is found by the exact inverse of `ast::docblock_token`
   (`crates/celerrate_syntax/src/ast/extensions.rs:126-137`): the next
   sibling element past whitespace, when it is a node. An orphan
   docblock (end of file, a token neighbor) falls back to
   `CurrentAndNextLine` — over-suppressed, never silently dropped.

10. **Recognition is substring-anchored, longest tag first.**
    `@phpstan-ignore-next-line` is checked before `-line` before the
    bare form, and a tag must end at a word boundary (whitespace, end
    of comment, or `*/`), so `@phpstan-ignored` is prose. The
    `contains`-grade posture is already pinned by
    `is_recognized_annotation`
    (`crates/celerrate_semantics/src/body.rs:410-423`), which keeps
    body-IR invalidation and directive recognition aligned: any comment
    the recognizer can read, an edit to it already invalidates.

11. **The WASM sketch is a document, not code**:
    `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`,
    the Celerrate-norm-draft precedent for internal design artifacts.
    It fixes the four checklist cases (guest statelessness,
    cancellation, fuel across re-entrancy, handle lifetime), enumerates
    the v0 host families (type construction, type interrogation,
    argument value access, symbol lookup), and shows every native trait
    projecting without reshaping. The host ships with sub-project 6.

12. **No new crate, no dependency-shape change.** Directives live in
    the existing bridge crate; `PLUGIN_CRATES`
    (`xtask/src/dependency_shape.rs:6-9`) is unchanged. The fuzz
    workspace gains a `celerrate_plugin` path dependency (it is a
    separate nested workspace, outside the shape check's scope).

## File structure

```
crates/celerrate_semantics/src/comment_directives.rs   NEW: vocabulary, trait, registry, suppressed_ranges, is_suppressed
crates/celerrate_semantics/src/lib.rs                  MODIFY: module + re-exports
crates/celerrate_plugin/src/lib.rs                     MODIFY: facade re-exports
crates/celerrate_phpdoc_bridge/src/directives.rs       NEW: the mapping table, comment_directives, adversarial pins
crates/celerrate_phpdoc_bridge/src/lib.rs              MODIFY: module + re-export
crates/celerrate_cli/src/plugins.rs                    MODIFY: the fourth registration
crates/celerrate_cli/src/analysis.rs                   MODIFY: the filter in composed_diagnostics
crates/celerrate_cli/src/cache/pack.rs                 MODIFY: CACHE_SCHEMA_VERSION 3 → 4
crates/celerrate_cli/tests/suppressions.rs             NEW: the four forms, end to end
crates/celerrate_cli/tests/cache_suppression.rs        NEW: warm-run semantics under suppression
fuzz/Cargo.toml                                        MODIFY: + celerrate_plugin
fuzz/fuzz_targets/docblock.rs                          MODIFY: drive the recognizer
fuzz/corpus/docblock/seed_directives                   NEW: directive seeds
.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md   NEW: the acceptance artifact
```

---

### Task 1: The comment-directive extension point

The vocabulary, the trait, the registration struct, the registry
singleton, and the facade re-export — the fifth extension point, shaped
exactly like the virtual-symbol point one file over
(`crates/celerrate_semantics/src/virtual_symbols.rs`). No query yet:
that is Task 2, against this surface.

**Files:**
- Create: `crates/celerrate_semantics/src/comment_directives.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`
- Modify: `crates/celerrate_plugin/src/lib.rs`

**Interfaces:**
- Consumes: `crate::plugin::PluginIdentity` (existing,
  `crates/celerrate_semantics/src/plugin.rs`).
- Produces: `CommentKind`, `DirectiveScope`,
  `CommentDirective::Suppress { scope: DirectiveScope, identifiers:
  Vec<String> }`, `trait CommentDirectiveProvider { fn directives(&self,
  kind: CommentKind, text: &str) -> Vec<CommentDirective> }`,
  `CommentDirectiveRegistration { identity: PluginIdentity, provider:
  Arc<dyn CommentDirectiveProvider> }`, `CommentDirectiveRegistry`
  (salsa singleton input with `registrations(db) ->
  &Vec<CommentDirectiveRegistration>`). Task 2 consumes the registry;
  Task 3 implements the trait through `celerrate_plugin`; Task 4
  registers at the composition root.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/comment_directives.rs` with the
test module first (the module body comes in Step 3):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use celerrate_db::testing::TestDatabase;

    #[derive(Debug)]
    struct FakeProvider;

    impl CommentDirectiveProvider for FakeProvider {
        fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
            if text.contains("@fake") && kind == CommentKind::Line {
                vec![CommentDirective::Suppress {
                    scope: DirectiveScope::CurrentLine,
                    identifiers: vec!["fake.identifier".to_owned()],
                }]
            } else {
                Vec::new()
            }
        }
    }

    fn identity(name: &str) -> crate::PluginIdentity {
        crate::PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    #[test]
    fn an_unset_registry_is_the_no_plugin_path() {
        let db = TestDatabase::default();
        assert!(CommentDirectiveRegistry::try_get(&db).is_none());
    }

    #[test]
    fn a_registered_provider_answers_through_the_registry() {
        let db = TestDatabase::default();
        let _ = CommentDirectiveRegistry::builder(vec![CommentDirectiveRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&db);

        let registry = CommentDirectiveRegistry::try_get(&db).unwrap();
        let registrations = registry.registrations(&db);
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].identity.name, "fake");
        assert_eq!(
            registrations[0]
                .provider
                .directives(CommentKind::Line, "// @fake"),
            vec![CommentDirective::Suppress {
                scope: DirectiveScope::CurrentLine,
                identifiers: vec!["fake.identifier".to_owned()],
            }],
        );
        assert!(
            registrations[0]
                .provider
                .directives(CommentKind::Docblock, "/** @fake */")
                .is_empty(),
            "the fake only answers line comments: the kind travels",
        );
        assert!(
            registrations[0]
                .provider
                .directives(CommentKind::Line, "// plain prose")
                .is_empty(),
        );
    }
}
```

Register the module in `crates/celerrate_semantics/src/lib.rs`: add
`mod comment_directives;` to the module list (alphabetical, between
`mod cache;` and `mod index;`) and the re-export (alphabetical among
the `pub use` items):

```rust
pub use comment_directives::{
    CommentDirective, CommentDirectiveProvider, CommentDirectiveRegistration,
    CommentDirectiveRegistry, CommentKind, DirectiveScope,
};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics comment_directives 2>&1 | tail -5`
Expected: FAIL to compile — `CommentDirective`, `CommentKind`,
`DirectiveScope`, `CommentDirectiveProvider`,
`CommentDirectiveRegistration`, `CommentDirectiveRegistry` are not
defined.

- [ ] **Step 3: Implement the extension point**

Fill the module body above the test module:

```rust
//! The comment-directive extension point: structured directives read
//! from comment trivia — today, suppressions ("extinguish every
//! diagnostic family on this scope").
//!
//! Owned by this crate per the design: the registry input lives with
//! the consuming layer, implementations are registered at the
//! composition root, `celerrate_plugin` re-exports the vocabulary.
//! The vocabulary (what a directive *is*) belongs to this trait; the
//! written tag table (what `@phpstan-ignore-line` *means*) is
//! bridge-internal, like the tag precedence table (design section 4).
//! Scopes are symbolic — a provider is a pure function of the comment
//! and cannot see positions; `suppressed_ranges` resolves them.

use std::sync::Arc;

use crate::plugin::PluginIdentity;

/// The comment shapes a provider may be handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentKind {
    /// `//` and `#` comments.
    Line,
    /// `/* ... */` comments.
    Block,
    /// `/** ... */` docblocks.
    Docblock,
}

/// Where a directive applies, relative to the comment that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectiveScope {
    /// The whole line(s) the comment covers — a trailing comment
    /// covers the code before it on the same line.
    CurrentLine,
    /// The whole line after the comment's last line.
    NextLine,
    /// Both of the above: the fixed over-suppression resolution of a
    /// placement-dependent directive (PHPStan 1.11's bare
    /// `@phpstan-ignore`).
    CurrentAndNextLine,
    /// The whole span of the node the comment annotates (a docblock's
    /// Psalm scope). Falls back to [`Self::CurrentAndNextLine`] when
    /// no annotated node exists: over-suppressed, never dropped.
    AnnotatedDeclaration,
}

/// One structured directive a comment carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommentDirective {
    /// Extinguish every diagnostic family on the scope. The
    /// identifiers are the foreign diagnostic names the written form
    /// carried (`@phpstan-ignore method.notFound`), carried for the
    /// rule framework's identifier-level correspondence, never matched
    /// here (design section 5).
    Suppress {
        scope: DirectiveScope,
        identifiers: Vec<String>,
    },
}

/// A provider translates one comment into the directives it carries.
/// Implementations must be deterministic pure functions of their
/// arguments: no interior state, no environment reads (the
/// byte-identical harness is the mechanical detector).
pub trait CommentDirectiveProvider: Send + Sync {
    fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective>;
}

/// One registration: the implementation travels with its identity.
#[derive(Clone)]
pub struct CommentDirectiveRegistration {
    pub identity: PluginIdentity,
    pub provider: Arc<dyn CommentDirectiveProvider>,
}

impl std::fmt::Debug for CommentDirectiveRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommentDirectiveRegistration")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// The registry: set once per process at the composition root, in the
/// high-durability tier, and never mutated — reading it therefore
/// never invalidates. Databases that register nothing (every test
/// database by default) take the no-plugin path. Providers are
/// consulted in registered order; contributions concatenate in that
/// order — suppression is a union, so the result is independent of
/// thread timing by construction.
#[salsa::input(singleton)]
pub struct CommentDirectiveRegistry {
    #[returns(ref)]
    pub registrations: Vec<CommentDirectiveRegistration>,
}
```

Extend the facade, `crates/celerrate_plugin/src/lib.rs`: the
`celerrate_semantics` re-export list gains the four vocabulary names
(keep every name already present):

```rust
pub use celerrate_semantics::{
    CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveScope, PluginIdentity,
    VirtualMember, VirtualMemberKind, VirtualParameter, VirtualSymbolProvider,
};
```

The registration and registry types are deliberately not re-exported:
the composition root reaches `celerrate_semantics` directly, exactly as
it does for `VirtualSymbolRegistration` today
(`crates/celerrate_cli/src/plugins.rs`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics comment_directives` — PASS (2 tests).
Then: `cargo test --workspace 2>&1 | tail -3` — PASS;
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all`.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_semantics/src/comment_directives.rs crates/celerrate_semantics/src/lib.rs crates/celerrate_plugin/src/lib.rs
git commit -m "✨ feat(semantics,plugin): the comment-directive extension point joins the registry family"
```

---
### Task 2: The per-file suppression query

`suppressed_ranges`: the own-tree read on the syntax-gating precedent.
It walks the file's comment tokens, hands each to the registered
providers, resolves the symbolic scopes against the line index, and
answers a sorted, deduplicated `Vec<TextRange>` — plus `is_suppressed`,
the one matching rule every consumer shares (decision 5). The tests use
a fake provider, not the bridge: the layer is proven against the trait,
which is what dependency inversion means here.

**Files:**
- Modify: `crates/celerrate_semantics/src/comment_directives.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `CommentDirectiveRegistry` (Task 1),
  `celerrate_db::parse(db, file) -> &Parse` (`.tree() -> SyntaxNode`),
  `celerrate_db::line_index(db, file) -> &LineIndex`,
  `celerrate_source::{LineColumn, LineIndex, TextRange, TextSize}`,
  `celerrate_syntax::{SyntaxKind, SyntaxToken}`.
- Produces: `#[salsa::tracked(returns(ref))] pub fn suppressed_ranges(
  db: &dyn salsa::Database, file: SourceFile) -> Vec<TextRange>` and
  `pub fn is_suppressed(suppressed: &[TextRange], offset: TextSize,
  text_end: TextSize) -> bool`. Task 4's filter consumes both.

- [ ] **Step 1: Write the failing tests**

Extend the test module in
`crates/celerrate_semantics/src/comment_directives.rs`. The fake
provider grows the four scopes; the fixtures register it and build one
in-memory file. Add these items inside `mod tests` (the Task 1
`FakeProvider` is replaced by this richer one — update the two Task 1
tests' expectations accordingly: the registry test's `"// @fake"` case
now goes through the same `@line`-style markers, so change its comment
texts to use `@line` and expect `CurrentLine` with no identifiers, or
keep `@fake` as an additional marker below):

```rust
    use celerrate_source::{TextSize, FileId};
    use salsa::Setter as _;

    #[derive(Debug)]
    struct FakeProvider;

    impl CommentDirectiveProvider for FakeProvider {
        fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
            let mut directives = Vec::new();
            if text.contains("@fake") && kind == CommentKind::Line {
                directives.push(CommentDirective::Suppress {
                    scope: DirectiveScope::CurrentLine,
                    identifiers: vec!["fake.identifier".to_owned()],
                });
            }
            for (marker, scope) in [
                ("@line", DirectiveScope::CurrentLine),
                ("@next", DirectiveScope::NextLine),
                ("@both", DirectiveScope::CurrentAndNextLine),
                ("@declaration", DirectiveScope::AnnotatedDeclaration),
            ] {
                if text.contains(marker) {
                    directives.push(CommentDirective::Suppress {
                        scope,
                        identifiers: Vec::new(),
                    });
                }
            }
            directives
        }
    }

    fn fixture(source: &str) -> (TestDatabase, celerrate_db::SourceFile) {
        let db = TestDatabase::default();
        let _ = CommentDirectiveRegistry::builder(vec![CommentDirectiveRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let file = celerrate_db::SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        (db, file)
    }

    fn offset_of(source: &str, needle: &str) -> TextSize {
        TextSize::from(u32::try_from(source.find(needle).unwrap()).unwrap())
    }

    fn suppressed_at(db: &TestDatabase, file: celerrate_db::SourceFile, source: &str, needle: &str) -> bool {
        is_suppressed(
            suppressed_ranges(db, file),
            offset_of(source, needle),
            TextSize::of(source),
        )
    }

    #[test]
    fn without_a_registry_nothing_is_suppressed() {
        let db = TestDatabase::default();
        let source = "<?php\n$x = 1; // @line\n";
        let file = celerrate_db::SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        assert!(suppressed_ranges(&db, file).is_empty());
    }

    #[test]
    fn a_current_line_directive_covers_its_whole_line_and_only_it() {
        let source = "<?php\n$x = 1; // @line\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
        assert!(!suppressed_at(&db, file, source, "<?php"));
    }

    #[test]
    fn a_next_line_directive_covers_the_line_below_and_only_it() {
        let source = "<?php\n// @next\n$x = 1;\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(!suppressed_at(&db, file, source, "// @next"));
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
    }

    #[test]
    fn a_both_lines_directive_covers_its_line_and_the_next() {
        let source = "<?php\n$x = 1; // @both\n$y = 2;\n$z = 3;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(suppressed_at(&db, file, source, "$y"));
        assert!(!suppressed_at(&db, file, source, "$z"));
    }

    #[test]
    fn a_docblock_directive_covers_the_annotated_declaration_whole() {
        let source = "<?php\n/** @declaration */\nclass Service {\n    public function boot() { $inside = 1; }\n}\n$outside = 1;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$inside"));
        assert!(!suppressed_at(&db, file, source, "$outside"));
    }

    #[test]
    fn an_orphan_docblock_falls_back_to_its_own_and_the_next_line() {
        let source = "<?php\n$x = 1;\n/** @declaration */";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "@declaration"));
        assert!(!suppressed_at(&db, file, source, "$x"));
    }

    #[test]
    fn a_next_line_directive_on_the_last_line_suppresses_nothing() {
        let source = "<?php\n$x = 1; // @next";
        let (db, file) = fixture(source);
        assert!(suppressed_ranges(&db, file).is_empty());
    }

    #[test]
    fn an_end_of_file_anchor_is_suppressible_from_the_last_line() {
        // Decision 5's exception: a diagnostic anchored exactly at the
        // text's end (an unexpected-end-of-file parse error) belongs
        // to the last line.
        let source = "<?php\n$x = 1; // @line";
        let (db, file) = fixture(source);
        assert!(is_suppressed(
            suppressed_ranges(&db, file),
            TextSize::of(source),
            TextSize::of(source),
        ));
    }

    #[test]
    fn identical_resolved_ranges_deduplicate() {
        let source = "<?php\n$x = 1; // @line @fake\n";
        let (db, file) = fixture(source);
        assert_eq!(suppressed_ranges(&db, file).len(), 1);
    }

    #[salsa::tracked]
    fn suppression_count(db: &dyn salsa::Database, file: celerrate_db::SourceFile) -> usize {
        suppressed_ranges(db, file).len()
    }

    #[test]
    fn a_prose_comment_edit_backdates_the_suppression_set() {
        let source = "<?php\n$x = 1; // @line\n// prose\n";
        let (mut db, file) = fixture(source);
        assert_eq!(suppression_count(&db, file), 1);
        db.take_executed();

        file.set_bytes(&mut db)
            .to(b"<?php\n$x = 1; // @line\n// edited prose\n".to_vec());
        assert_eq!(suppression_count(&db, file), 1);

        let executed = db.take_executed();
        assert!(
            executed.iter().any(|query| query.contains("suppressed_ranges")),
            "the own-tree read re-runs on any edit: {executed:?}",
        );
        assert!(
            !executed.iter().any(|query| query.contains("suppression_count")),
            "an identical set backdates: the consumer never re-ran: {executed:?}",
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics comment_directives 2>&1 | tail -5`
Expected: FAIL to compile — `suppressed_ranges` and `is_suppressed` are
not defined.

- [ ] **Step 3: Implement the query**

Add to `crates/celerrate_semantics/src/comment_directives.rs`, below
the registry:

```rust
use celerrate_db::SourceFile;
use celerrate_source::{LineColumn, LineIndex, TextRange, TextSize};
use celerrate_syntax::{SyntaxKind, SyntaxToken};

/// The file's suppressed ranges: every comment handed to every
/// registered provider, the symbolic scopes resolved against the line
/// index, sorted and deduplicated. An own-tree read for strictly-local
/// output — the syntax-gating precedent. `Eq`-comparable: a comment
/// edit that leaves the directive set unchanged backdates, and
/// dependents never re-run.
#[salsa::tracked(returns(ref))]
pub fn suppressed_ranges(db: &dyn salsa::Database, file: SourceFile) -> Vec<TextRange> {
    let Some(registry) = CommentDirectiveRegistry::try_get(db) else {
        return Vec::new();
    };
    let registrations = registry.registrations(db);
    if registrations.is_empty() {
        return Vec::new();
    }
    let root = celerrate_db::parse(db, file).tree();
    let index = celerrate_db::line_index(db, file);
    let text_end = root.text_range().end();
    let mut ranges = Vec::new();
    for element in root.descendants_with_tokens() {
        let Some(token) = element.as_token() else {
            continue;
        };
        let Some(kind) = comment_kind(token.kind()) else {
            continue;
        };
        for registration in registrations {
            for directive in registration.provider.directives(kind, token.text()) {
                match directive {
                    CommentDirective::Suppress { scope, .. } => {
                        if let Some(range) = resolve_scope(scope, token, index, text_end) {
                            ranges.push(range);
                        }
                    }
                }
            }
        }
    }
    ranges.sort_by_key(|range| (range.start(), range.end()));
    ranges.dedup();
    ranges
}

/// Whether a diagnostic anchored at `offset` falls in a suppressed
/// range. Matching is by the diagnostic's start — the location the
/// report names — end-exclusive, except at the very end of the file:
/// a diagnostic anchored exactly at the text's end (an
/// unexpected-end-of-file parse error) belongs to the last line and
/// must be suppressible from it, or the suppression under-suppresses
/// (design section 5's rule, in the one place every consumer shares).
pub fn is_suppressed(suppressed: &[TextRange], offset: TextSize, text_end: TextSize) -> bool {
    suppressed.iter().any(|range| {
        offset >= range.start()
            && (offset < range.end() || (offset == range.end() && range.end() == text_end))
    })
}

/// The trivia kinds a provider may read.
fn comment_kind(kind: SyntaxKind) -> Option<CommentKind> {
    match kind {
        SyntaxKind::LineComment => Some(CommentKind::Line),
        SyntaxKind::BlockComment => Some(CommentKind::Block),
        SyntaxKind::DocComment => Some(CommentKind::Docblock),
        _ => None,
    }
}

/// Resolves a symbolic scope to concrete offsets. `None` means the
/// scope names nothing that exists (a next-line directive on the last
/// line): nothing to suppress, nothing to under-suppress.
fn resolve_scope(
    scope: DirectiveScope,
    token: &SyntaxToken,
    index: &LineIndex,
    text_end: TextSize,
) -> Option<TextRange> {
    let comment = token.text_range();
    let first_line = index.line_column(comment.start()).line;
    let last_line = index.line_column(comment.end()).line;
    match scope {
        DirectiveScope::CurrentLine => whole_lines(index, text_end, first_line, last_line),
        DirectiveScope::NextLine => {
            let next = last_line.checked_add(1)?;
            whole_lines(index, text_end, next, next)
        }
        DirectiveScope::CurrentAndNextLine => {
            whole_lines(index, text_end, first_line, last_line.saturating_add(1))
        }
        DirectiveScope::AnnotatedDeclaration => annotated_node_range(token).or_else(|| {
            whole_lines(index, text_end, first_line, last_line.saturating_add(1))
        }),
    }
}

/// The covering range of the whole lines `first..=last`, newline
/// included. `None` when `first` does not exist; a `last` past the
/// file's end clamps to the end of text.
fn whole_lines(index: &LineIndex, text_end: TextSize, first: u32, last: u32) -> Option<TextRange> {
    let start = index.offset(LineColumn {
        line: first,
        column: 0,
    })?;
    let end = last
        .checked_add(1)
        .and_then(|below| {
            index.offset(LineColumn {
                line: below,
                column: 0,
            })
        })
        .unwrap_or(text_end);
    Some(TextRange::new(start, end.max(start)))
}

/// The node a docblock annotates: the exact inverse of
/// `celerrate_syntax::ast::docblock_token` — the next sibling element
/// past whitespace, when it is a node. Anything else (an orphan
/// docblock at the end of the file, a token neighbor) answers `None`
/// and the caller falls back to the line-based scope.
fn annotated_node_range(token: &SyntaxToken) -> Option<TextRange> {
    let mut current = token.next_sibling_or_token();
    while let Some(element) = current {
        if let Some(node) = element.as_node() {
            return Some(node.text_range());
        }
        let next = element.as_token()?;
        if next.kind() != SyntaxKind::Whitespace {
            return None;
        }
        current = element.next_sibling_or_token();
    }
    None
}
```

Extend the re-export in `crates/celerrate_semantics/src/lib.rs`:

```rust
pub use comment_directives::{
    CommentDirective, CommentDirectiveProvider, CommentDirectiveRegistration,
    CommentDirectiveRegistry, CommentKind, DirectiveScope, is_suppressed, suppressed_ranges,
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics comment_directives` — PASS (12 tests).
Then: `cargo test --workspace 2>&1 | tail -3` — PASS;
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all`.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_semantics/src/comment_directives.rs crates/celerrate_semantics/src/lib.rs
git commit -m "✨ feat(semantics): suppressed ranges resolve comment directives over the file's own tree"
```

---

### Task 3: The bridge's directive recognition

The written forms translate into the vocabulary: the bridge implements
the fifth extension point exactly as it implements the other two — one
struct, one more `impl`, depending on `celerrate_plugin` and nothing
else. The mapping table is this module's rustdoc, the bridge-internal
half of decision 2.

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/directives.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs`

**Interfaces:**
- Consumes: `celerrate_plugin::{CommentDirective,
  CommentDirectiveProvider, CommentKind, DirectiveScope}` (Task 1's
  facade re-export), `crate::syntax::PhpdocBridge` (existing).
- Produces: `impl CommentDirectiveProvider for PhpdocBridge` and
  `pub fn comment_directives(kind: CommentKind, text: &str) ->
  Vec<CommentDirective>` (re-exported from `lib.rs`; the fuzz target
  drives it in Task 6). Task 4 registers the impl.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_phpdoc_bridge/src/directives.rs` with the test
module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn suppress(scope: DirectiveScope, identifiers: &[&str]) -> CommentDirective {
        CommentDirective::Suppress {
            scope,
            identifiers: identifiers.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn ignore_line_suppresses_the_current_line_in_any_comment_kind() {
        for (kind, text) in [
            (CommentKind::Line, "// @phpstan-ignore-line"),
            (CommentKind::Line, "# @phpstan-ignore-line"),
            (CommentKind::Block, "/* @phpstan-ignore-line */"),
            (CommentKind::Docblock, "/** @phpstan-ignore-line */"),
        ] {
            assert_eq!(
                comment_directives(kind, text),
                vec![suppress(DirectiveScope::CurrentLine, &[])],
                "{text}",
            );
        }
    }

    #[test]
    fn ignore_next_line_suppresses_the_next_line_and_is_not_read_as_the_bare_form() {
        assert_eq!(
            comment_directives(CommentKind::Line, "// @phpstan-ignore-next-line"),
            vec![suppress(DirectiveScope::NextLine, &[])],
        );
    }

    #[test]
    fn the_bare_form_carries_identifiers_and_covers_both_placements() {
        assert_eq!(
            comment_directives(
                CommentKind::Line,
                "// @phpstan-ignore method.notFound, property.notFound (nullable receiver)",
            ),
            vec![suppress(
                DirectiveScope::CurrentAndNextLine,
                &["method.notFound", "property.notFound"],
            )],
        );
        assert_eq!(
            comment_directives(CommentKind::Line, "// @phpstan-ignore"),
            vec![suppress(DirectiveScope::CurrentAndNextLine, &[])],
        );
    }

    #[test]
    fn psalm_suppress_in_a_docblock_targets_the_annotated_declaration() {
        assert_eq!(
            comment_directives(
                CommentKind::Docblock,
                "/**\n * @psalm-suppress PossiblyNullReference, InvalidArgument\n */",
            ),
            vec![suppress(
                DirectiveScope::AnnotatedDeclaration,
                &["PossiblyNullReference", "InvalidArgument"],
            )],
        );
    }

    #[test]
    fn psalm_suppress_in_an_ordinary_comment_covers_both_lines() {
        assert_eq!(
            comment_directives(CommentKind::Block, "/* @psalm-suppress InvalidArgument */"),
            vec![suppress(
                DirectiveScope::CurrentAndNextLine,
                &["InvalidArgument"],
            )],
        );
    }

    #[test]
    fn a_docblock_may_carry_several_directives() {
        assert_eq!(
            comment_directives(
                CommentKind::Docblock,
                "/**\n * @psalm-suppress UndefinedClass\n * @phpstan-ignore-next-line\n */",
            ),
            vec![
                suppress(DirectiveScope::AnnotatedDeclaration, &["UndefinedClass"]),
                suppress(DirectiveScope::NextLine, &[]),
            ],
        );
    }

    #[test]
    fn a_tag_must_end_at_a_word_boundary() {
        // Prose that merely embeds the letters is not a directive.
        assert!(comment_directives(CommentKind::Line, "// @phpstan-ignored").is_empty());
        assert!(comment_directives(CommentKind::Line, "// @phpstan-ignore-linear").is_empty());
        assert!(comment_directives(CommentKind::Line, "// @psalm-suppressive").is_empty());
    }

    #[test]
    fn plain_prose_carries_nothing() {
        assert!(comment_directives(CommentKind::Line, "// a plain remark").is_empty());
        assert!(comment_directives(CommentKind::Docblock, "/** @param int $x */").is_empty());
    }

    #[test]
    fn adversarial_inputs_never_panic() {
        let inputs = [
            "@phpstan-ignore",
            "@phpstan-ignore-",
            "@phpstan-ignore-next-line@phpstan-ignore-line",
            "@@@@phpstan-ignore@psalm-suppress",
            "// @phpstan-ignore ((((((",
            "// @phpstan-ignore ,,,,,",
            "/* @psalm-suppress */ trailing",
            "@psalm-suppress \u{0} \u{7f} é漢字",
            "@",
            "",
            "*/ @phpstan-ignore-line /*",
        ];
        for input in inputs {
            for kind in [CommentKind::Line, CommentKind::Block, CommentKind::Docblock] {
                let _ = comment_directives(kind, input);
            }
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge directives 2>&1 | tail -5`
Expected: FAIL to compile — `comment_directives` is not defined.

- [ ] **Step 3: Implement the recognizer**

Fill the module body above the test module:

```rust
//! The suppression-directive recognizer: the bridge's implementation
//! of the comment-directive extension point.
//!
//! # The mapping table (bridge-internal, design section 5)
//!
//! | Written form                    | Comment kind | Directive                        |
//! |---------------------------------|--------------|----------------------------------|
//! | `@phpstan-ignore-line`          | any          | suppress, current line           |
//! | `@phpstan-ignore-next-line`     | any          | suppress, next line              |
//! | `@phpstan-ignore <identifiers>` | any          | suppress, current and next line  |
//! | `@psalm-suppress <identifiers>` | docblock     | suppress, annotated declaration  |
//! | `@psalm-suppress <identifiers>` | line, block  | suppress, current and next line  |
//!
//! PHPStan 1.11's bare `@phpstan-ignore` targets its own line when the
//! comment trails code and the next line otherwise; covering both
//! lines is the superset that under-suppresses neither placement
//! (design section 5: over-suppression, never under-suppression). A
//! docblock-attached `@psalm-suppress` maps to the annotated
//! declaration's whole span — its Psalm scope, not the docblock's own
//! line where no diagnostic ever fires. Identifiers are carried, never
//! matched: identifier-level correspondence is the rule framework's.
//! Malformed content yields fewer identifiers or no directive, never
//! an error — no docblock diagnostics.

use celerrate_plugin::{CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveScope};

use crate::syntax::PhpdocBridge;

const PHPSTAN_IGNORE: &str = "@phpstan-ignore";
const PSALM_SUPPRESS: &str = "@psalm-suppress";

impl CommentDirectiveProvider for PhpdocBridge {
    fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
        comment_directives(kind, text)
    }
}

/// Every directive one comment carries, in written order. Total over
/// arbitrary input.
pub fn comment_directives(kind: CommentKind, text: &str) -> Vec<CommentDirective> {
    let mut directives = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find('@') {
        let Some(tail) = rest.get(position..) else {
            break;
        };
        if let Some(after) = tail.strip_prefix(PHPSTAN_IGNORE) {
            directives.extend(phpstan_directive(after));
        } else if let Some(after) = tail.strip_prefix(PSALM_SUPPRESS) {
            directives.extend(psalm_directive(kind, after));
        }
        // `@` is ASCII: one past it is always a character boundary.
        rest = rest.get(position + 1..).unwrap_or("");
    }
    directives
}

/// Classifies what follows `@phpstan-ignore`, longest suffix first —
/// `-next-line` before `-line` before the bare identifier-bearing form.
fn phpstan_directive(after_tag: &str) -> Option<CommentDirective> {
    if let Some(rest) = after_tag.strip_prefix("-next-line") {
        ends_word(rest).then(|| suppress(DirectiveScope::NextLine, Vec::new()))
    } else if let Some(rest) = after_tag.strip_prefix("-line") {
        ends_word(rest).then(|| suppress(DirectiveScope::CurrentLine, Vec::new()))
    } else if ends_word(after_tag) {
        Some(suppress(
            DirectiveScope::CurrentAndNextLine,
            identifiers_of(after_tag),
        ))
    } else {
        None
    }
}

fn psalm_directive(kind: CommentKind, after_tag: &str) -> Option<CommentDirective> {
    if !ends_word(after_tag) {
        return None;
    }
    let scope = match kind {
        CommentKind::Docblock => DirectiveScope::AnnotatedDeclaration,
        CommentKind::Line | CommentKind::Block => DirectiveScope::CurrentAndNextLine,
    };
    Some(suppress(scope, identifiers_of(after_tag)))
}

fn suppress(scope: DirectiveScope, identifiers: Vec<String>) -> CommentDirective {
    CommentDirective::Suppress { scope, identifiers }
}

/// A tag ends at a word boundary: the end of the comment, whitespace,
/// or a closing `*/`. `@phpstan-ignored` is prose, not a directive.
fn ends_word(after: &str) -> bool {
    after.is_empty()
        || after.starts_with(|character: char| character.is_whitespace())
        || after.starts_with("*/")
}

/// The identifier list after a bare tag: the rest of that line, a
/// parenthesized trailer dropped (`@phpstan-ignore method.notFound
/// (nullable receiver)`), the closing `*/` dropped, comma-separated,
/// trimmed of whitespace and docblock decoration. Malformed content
/// yields fewer identifiers, never a lost directive.
fn identifiers_of(after_tag: &str) -> Vec<String> {
    let mut line = after_tag.lines().next().unwrap_or("");
    if let Some((before, _)) = line.split_once("*/") {
        line = before;
    }
    if let Some((before, _)) = line.split_once('(') {
        line = before;
    }
    line.split(',')
        .map(|identifier| identifier.trim().trim_matches('*').trim())
        .filter(|identifier| !identifier.is_empty())
        .map(str::to_owned)
        .collect()
}
```

Register the module in `crates/celerrate_phpdoc_bridge/src/lib.rs`: add
`mod directives;` to the module list (alphabetical, before `mod
expression;`) and the re-export:

```rust
pub use directives::comment_directives;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_phpdoc_bridge directives` — PASS (9 tests).
Then: `cargo xtask dependency-shape` — PASS (the bridge gained no
workspace dependency: the trait arrived through the facade).
Then: `cargo test --workspace 2>&1 | tail -3` — PASS;
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all`.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_phpdoc_bridge/src/directives.rs crates/celerrate_phpdoc_bridge/src/lib.rs
git commit -m "✨ feat(phpdoc-bridge): the suppression tags translate through the comment-directive point"
```

---
### Task 4: The composition root registers, the composition point filters

The bridge joins the fourth registry, and `composed_diagnostics` — the
one point `analyze_one`, `persist`, and the equivalence harness share —
drops every diagnostic whose anchor falls in a suppressed range
(decision 6). The schema constant records the break (decision 7). The
tests drive the whole product through `run`, the four written forms end
to end.

**Files:**
- Modify: `crates/celerrate_cli/src/plugins.rs`
- Modify: `crates/celerrate_cli/src/analysis.rs:140-155`
- Modify: `crates/celerrate_cli/src/cache/pack.rs:16-30`
- Test: `crates/celerrate_cli/tests/suppressions.rs`

**Interfaces:**
- Consumes: `celerrate_semantics::{CommentDirectiveRegistration,
  CommentDirectiveRegistry, is_suppressed, suppressed_ranges}` (Tasks
  1-2), `impl CommentDirectiveProvider for PhpdocBridge` (Task 3),
  `celerrate_db::source_text`.
- Produces: the user-visible behavior every later test relies on:
  `celerrate check` never reports a diagnostic whose anchor a directive
  covers, and the exit code counts the post-filter set.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_cli/tests/suppressions.rs`:

```rust
//! Inline suppressions, end to end: the four written forms extinguish
//! every diagnostic family on their scope, the report and the exit
//! code count the same post-filter set, and nothing leaks across
//! files (design sections 4 and 5).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{Outcome, run};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn a_trailing_ignore_line_extinguishes_the_finding() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
    assert!(!text.contains("CEL0018"), "{text}");
}

#[test]
fn a_hash_comment_carries_the_directive_too() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); # @phpstan-ignore-line\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn ignore_next_line_targets_the_line_below_and_only_it() {
    let root = project(&[(
        "a.php",
        "<?php\n// @phpstan-ignore-next-line\nnew MissingOne();\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn the_bare_identifier_form_covers_both_of_its_placements() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore class.notFound\n// @phpstan-ignore class.notFound\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn psalm_suppress_on_a_declaration_docblock_covers_its_whole_span() {
    let root = project(&[(
        "a.php",
        "<?php\n/** @psalm-suppress UndefinedClass */\nclass Service\n{\n    public function boot(): void\n    {\n        new MissingOne();\n    }\n}\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn suppression_extinguishes_the_syntax_family_too() {
    // Design section 5: suppression is family-agnostic — exempting the
    // existing families would re-report exactly what it forbids.
    let root = project(&[("a.php", "<?php\n$x = ; // @phpstan-ignore-line\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn a_directive_never_leaks_into_another_file() {
    let root = project(&[
        ("a.php", "<?php\n// @phpstan-ignore-next-line\n"),
        ("b.php", "<?php\nnew MissingOne();\n"),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("MissingOne"), "{text}");
}

#[test]
fn an_unrelated_line_still_reports_beside_a_suppressed_one() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --test suppressions 2>&1 | tail -12`
Expected: FAIL — every suppression test asserts `Outcome::Clean` or an
absent marker and gets `DiagnosticsReported` with the finding printed:
nothing filters yet.

- [ ] **Step 3: Implement — registration, filter, schema record**

**`crates/celerrate_cli/src/plugins.rs`** — the registration function
gains the fourth registry. The complete new `register_plugins` (the
`ExcludedPlugin`/`RegisteredPlugins`/`admission` items above it are
untouched):

```rust
pub fn register_plugins(database: &AnalysisDatabase) -> RegisteredPlugins {
    let mut excluded = Vec::new();
    let mut type_syntax = Vec::new();
    let mut virtual_symbols = Vec::new();
    let mut comment_directives = Vec::new();
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
            comment_directives.push(celerrate_semantics::CommentDirectiveRegistration {
                identity: descriptor.identity.clone(),
                provider: bridge.clone(),
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
```

The claim-validation block in the middle is untouched. The registry
writes at the bottom gain one:

```rust
    let _ = celerrate_types::TypeSyntaxRegistry::builder(type_syntax)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_semantics::VirtualSymbolRegistry::builder(virtual_symbols)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_semantics::CommentDirectiveRegistry::builder(comment_directives)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_types::DynamicTypeProviderRegistry::builder(dynamic_providers)
        .durability(salsa::Durability::HIGH)
        .new(database);

    RegisteredPlugins { excluded }
}
```

**`crates/celerrate_cli/src/analysis.rs`** — the import at line 13
becomes `use celerrate_source::{FileId, TextSize};`, and
`composed_diagnostics` (lines 140-155) becomes:

```rust
/// One file's diagnostics, computed: decode and syntax, then references
/// and gating, then the directive filter. The single composition
/// point — `analyze_one` serves it on a cache miss, `persist`
/// re-composes through it, and the equivalence harness recomputes
/// through it — so the composers cannot drift (audit finding I2's
/// first hand-maintained mirror). Filtering here, below the verdict,
/// is sound because directives are strictly file-local: the verdict's
/// content-hash key covers every directive edit, and it keeps the
/// exit-code count, the printed report, and the persisted verdict the
/// same post-filter set by construction (the vendor-filter rationale
/// above, applied again).
pub fn composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let mut diagnostics = celerrate_db::file_diagnostics(database, file).clone();
    diagnostics.extend(
        celerrate_semantics::semantic_diagnostics(
            database,
            file,
            inputs.files,
            inputs.stubs,
            inputs.configuration,
        )
        .iter()
        .cloned(),
    );
    let suppressed = celerrate_semantics::suppressed_ranges(database, file);
    if !suppressed.is_empty() {
        let text_end = celerrate_db::source_text(database, file)
            .as_ref()
            .map(|text| TextSize::of(text.text()))
            .unwrap_or_default();
        diagnostics.retain(|diagnostic| {
            !celerrate_semantics::is_suppressed(suppressed, diagnostic.range.start(), text_end)
        });
    }
    diagnostics
}
```

**`crates/celerrate_cli/src/cache/pack.rs`** — the schema constant
(line 30) becomes 4, and its doc-comment history gains the entry:

```rust
/// 4: verdict diagnostics are stored post-suppression (plan 4c). The
/// shapes are unchanged; their meaning is not — a pack written before
/// the directive filter existed carries findings the filter would have
/// extinguished, and must not speak. In practice the binary self-hash
/// already discards it; this is the named, reviewable record.
pub const CACHE_SCHEMA_VERSION: u32 = 4;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli --test suppressions` — PASS (8 tests).
Then: `cargo test --workspace 2>&1 | tail -3` — PASS (in particular
`cache_equivalence` and the check snapshots: no existing fixture
carries a directive, so nothing moves);
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all`.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/plugins.rs crates/celerrate_cli/src/analysis.rs crates/celerrate_cli/src/cache/pack.rs crates/celerrate_cli/tests/suppressions.rs
git commit -m "✨ feat(cli): the directive filter extinguishes suppressed findings at the composition point"
```

---

### Task 5: The cache under suppression

Pins what decision 6 promised: the pack stores the post-filter verdict
and serves it equal to recomputation (the equivalence net's property,
exercised over a suppressed fixture), a warm run stays suppressed, and
a directive edit is an ordinary content-hash miss — the finding comes
back without any special cache handling.

**Files:**
- Test: `crates/celerrate_cli/tests/cache_suppression.rs`

**Interfaces:**
- Consumes: `celerrate_cli::{run, Outcome}`,
  `celerrate_cli::session::Session`,
  `celerrate_cli::analysis::composed_diagnostics`,
  `celerrate_cli::cache::verdict::{VerdictLookup, lookup_verdict}` (all
  existing, the `cache_equivalence.rs` surface).
- Produces: nothing — a test-only task.

- [ ] **Step 1: Write the failing-or-passing tests**

This task is test-only: if Task 4's implementation is right, these pass
immediately; any failure is a Task 4 defect surfacing, to be fixed
there. Create `crates/celerrate_cli/tests/cache_suppression.rs`:

```rust
//! Suppression under the persistent cache: the pack stores the
//! post-filter verdict, a warm run serves it parse-free and equal to
//! recomputation, and a directive edit is a plain content-hash miss —
//! stale suppression is structurally impossible (decision 6 of plan
//! 4c: directives are strictly file-local).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::analysis::composed_diagnostics;
use celerrate_cli::cache::verdict::{VerdictLookup, lookup_verdict};
use celerrate_cli::session::Session;
use celerrate_cli::{Outcome, run};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
    );
    (outcome, String::from_utf8(output).unwrap())
}

const SUPPRESSED_AND_NOT: &str =
    "<?php\nnew MissingOne(); // @phpstan-ignore-line\nnew MissingTwo();\n";

#[test]
fn the_pack_stores_the_post_filter_verdict_and_serves_it_equal() {
    let root = project(&[("a.php", SUPPRESSED_AND_NOT)]);
    check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let database = &inputs.database;
    let &file = session.sources.values().next().unwrap();

    let VerdictLookup::Hit(stored) = lookup_verdict(&inputs, file) else {
        panic!("the persisted verdict must revalidate on an unchanged project");
    };
    assert_eq!(
        stored.diagnostics.len(),
        1,
        "the suppressed finding never entered the pack",
    );
    assert!(
        stored.diagnostics[0].message.contains("MissingTwo"),
        "{}",
        stored.diagnostics[0].message,
    );

    let file_id = file.file_id(database);
    let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
    let served: Vec<_> = stored
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length).unwrap())
        .collect();
    assert_eq!(
        served,
        composed_diagnostics(&inputs, file),
        "a served verdict must equal recomputation through the shared point",
    );
}

#[test]
fn removing_the_directive_restores_the_finding_on_a_warm_run() {
    let root = project(&[("a.php", SUPPRESSED_AND_NOT)]);
    let (first, _) = check(root.path());
    assert_eq!(first, Outcome::DiagnosticsReported);

    std::fs::write(
        root.path().join("a.php"),
        "<?php\nnew MissingOne();\nnew MissingTwo();\n",
    )
    .unwrap();
    let (second, text) = check(root.path());
    assert_eq!(second, Outcome::DiagnosticsReported);
    assert!(text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn a_warm_run_over_an_unchanged_project_stays_suppressed() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\n",
    )]);
    let (cold, _) = check(root.path());
    assert_eq!(cold, Outcome::Clean);
    let (warm, text) = check(root.path());
    assert_eq!(warm, Outcome::Clean, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --package celerrate_cli --test cache_suppression`
Expected: PASS (3 tests). A failure here is a Task 4 defect: fix it in
the Task 4 files, never by weakening these assertions.

- [ ] **Step 3: Full gate and commit**

Run: `cargo test --workspace 2>&1 | tail -3` — PASS;
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all`.

```bash
git add crates/celerrate_cli/tests/cache_suppression.rs
git commit -m "✅ test(cli): suppression verdicts replay equal and directive edits miss the cache"
```

---

### Task 6: Directive fuzzing

The recognizer joins the docblock fuzz target — the same contract as
the docblock lexer (design section 10: arbitrary input, never a panic)
— with a directive seed so the corpus reaches the new code fast. The
in-source adversarial pins landed with Task 3; this is the libFuzzer
side.

**Files:**
- Modify: `fuzz/Cargo.toml`
- Modify: `fuzz/fuzz_targets/docblock.rs`
- Create: `fuzz/corpus/docblock/seed_directives`

**Interfaces:**
- Consumes: `celerrate_phpdoc_bridge::comment_directives` (Task 3),
  `celerrate_plugin::CommentKind` (Task 1).
- Produces: nothing — a test-only task.

- [ ] **Step 1: Extend the target and the corpus**

`fuzz/Cargo.toml` `[dependencies]` gains (the fuzz crate is a separate
nested workspace: the one-dependency rule does not apply to it):

```toml
celerrate_plugin = { path = "../crates/celerrate_plugin" }
```

`fuzz/fuzz_targets/docblock.rs` becomes:

```rust
#![no_main]

use celerrate_plugin::CommentKind;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let tags = celerrate_phpdoc_bridge::lex_docblock(text);
        let _ = celerrate_phpdoc_bridge::extract_member_docblock(&tags);
        let _ = celerrate_phpdoc_bridge::extract_virtual_members(&tags);
        let _ = celerrate_phpdoc_bridge::parse_type_expression_text(text);
        for kind in [CommentKind::Line, CommentKind::Block, CommentKind::Docblock] {
            let _ = celerrate_phpdoc_bridge::comment_directives(kind, text);
        }
    }
});
```

Create `fuzz/corpus/docblock/seed_directives`:

```
/** @psalm-suppress PossiblyNullReference, InvalidArgument */
// @phpstan-ignore-line
// @phpstan-ignore-next-line
// @phpstan-ignore method.notFound, property.notFound (nullable receiver)
# @phpstan-ignore-line
/* @psalm-suppress all */
/** @phpstan-ignore identifier */
```

- [ ] **Step 2: Run the fuzzer**

Run: `cargo +nightly fuzz run docblock -- -max_total_time=60`
Expected: no crash, no panic, exit 0 after the time budget. If the
nightly toolchain is unavailable on this machine, run
`cd fuzz && cargo check` to prove the target compiles, and record the
skipped run in the closure ledger (Task 8) rather than skipping
silently.

- [ ] **Step 3: Full gate and commit**

Run: `cargo test --workspace 2>&1 | tail -3` — PASS;
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all`.

```bash
git add fuzz/Cargo.toml fuzz/fuzz_targets/docblock.rs fuzz/corpus/docblock/seed_directives
git commit -m "✅ test(phpdoc-bridge): the directive recognizer joins the docblock fuzz target"
```

---
### Task 7: The WASM-level interface sketch

A document, not code (decision 11): the acceptance artifact design
section 4 demands, written against its four-case checklist so the
native trait signatures this sub-project froze provably project onto a
sandboxed guest without reshaping. The host itself is sub-project 6.

**Files:**
- Create: `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`

**Interfaces:**
- Consumes: the four native traits as shipped (`TypeSyntax`,
  `DynamicTypeProvider`, `VirtualSymbolProvider`,
  `CommentDirectiveProvider`).
- Produces: the sketch sub-project 6 extends rather than reshapes.

- [ ] **Step 1: Write the document**

Create `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`
with exactly this content:

```markdown
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
| `DynamicTypeProvider` (`celerrate_types`) | `claims() -> list`, `return_type(invocation) -> type?` | all four |
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
```

- [ ] **Step 2: Verify the document against its checklist**

Read the written file once, checking mechanically: each of the four
cases has its own section with normative rules (sections 2-5); the
four v0 families are enumerated (section 6); all four native traits —
including this plan's `CommentDirectiveProvider` — appear in the
projection table (section 7); every checklist line (section 8) points
at a section that exists. Fix any gap before committing.

- [ ] **Step 3: Commit**

```bash
git add .claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md
git commit -m "📝 docs(plugin): the WASM-level interface sketch against its acceptance checklist"
```

---

### Task 8: Closure — documentation, ledger, the full gate

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (module doc)
- Modify: `.claude/superpowers/plans/2026-07-15-type-engine-4c-directives-wasm-sketch.md` (this file: the ledger)

- [ ] **Step 1: Bring the bridge's crate doc up to date**

The module doc of `crates/celerrate_phpdoc_bridge/src/lib.rs` names
where each table lives; it gains the directives line. After "the total
lowering table is `lowering`'s rustdoc;" insert:

```
//! the suppression mapping table (the written tags to the directive
//! vocabulary) is `directives`' rustdoc;
```

- [ ] **Step 2: Verify against the design, then write the ledger**

Walk the "Verification against the design" section at the end of this
plan; every clause must point at landed code or a recorded deviation.
Then append a `## Accepted debt at closure` section to this plan file
recording at least (reworded as executed reality, plus anything
discovered during execution):

- Identifier-level correspondence is deferred to the rule framework:
  an identifier-bearing suppression extinguishes every family on its
  scope regardless of the identifier (design section 5, verbatim).
- PHPStan 1.11's placement-dependent bare `@phpstan-ignore` resolves
  to both candidate lines — the over-suppression superset — rather
  than inspecting whether the comment trails code.
- Recognition is word-boundary substring matching: prose that embeds a
  literal tag at a word boundary over-suppresses. The posture is
  deliberately aligned with `is_recognized_annotation`
  (`crates/celerrate_semantics/src/body.rs`), so any comment the
  recognizer reads already invalidates the body IR on edit.
- Line-based scopes from a multi-line comment resolve against the
  whole comment token's lines, not the tag's own line inside it.
- A docblock directive before a *statement* suppresses that
  statement's node span (its Psalm scope) — broader than one line when
  the statement spans several.
- No unused-suppression diagnostic: a directive that suppresses
  nothing is silent (a reporting feature for the rule framework era).
- The sketch is an internal draft; its first mechanical exercise is
  sub-project 6's host. The dormant API-version gate remains dormant.
- If Task 6's libFuzzer run was skipped for a missing nightly
  toolchain, say so here.

- [ ] **Step 3: The full gate**

Run, each in turn, all green:

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo xtask dependency-shape
cargo xtask phpdoc-cases --check
```

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_phpdoc_bridge/src/lib.rs .claude/superpowers/plans/2026-07-15-type-engine-4c-directives-wasm-sketch.md
git commit -m "📝 docs(phpdoc-bridge): the directive table's home and the 4c closure ledger"
```

---

## Verification against the design (for the final reviewer)

- Design §4 "`celerrate_semantics` owns a comment-directive trait" →
  Task 1 (trait, vocabulary, registration, registry all in
  `crates/celerrate_semantics/src/comment_directives.rs`; the facade
  re-exports vocabulary only).
- Design §4 "a per-file query collects comment trivia (an own-tree
  read for strictly-local output, the precedent syntax gating set),
  hands each comment to the registered providers" → Task 2
  (`suppressed_ranges`, the `syntax_version_diagnostics` pattern).
- Design §4 "providers return structured directives (suppress: span
  scope, optional foreign identifier)" → Task 1
  (`CommentDirective::Suppress { scope, identifiers }`).
- Design §4 "the composition root applies the directive filter to
  rendered diagnostics" → Task 4 (`composed_diagnostics` in
  `celerrate_cli`, the single composition point; count, print, and
  persist see one post-filter set).
- Design §4 "the PHPStan/Psalm-to-Celerrate mapping table is
  bridge-internal ...; the directive vocabulary belongs to the trait"
  → decision 2; Tasks 1 and 3.
- Design §4 registry facts (implementation travels with identity, no
  backdating expected of trait objects, object-safe trait, HIGH
  durability) → Task 1, mirroring `virtual_symbols.rs` field for
  field.
- Design §4 "registration order ... deterministic" → decision 1
  (concatenation in registered order; suppression is a union).
- Design §4 the WASM sketch as acceptance artifact, four cases, v0
  host families → Task 7.
- Design §5 the four written forms, "honored from this preview on" →
  Tasks 3 and 4, pinned end to end in
  `crates/celerrate_cli/tests/suppressions.rs`.
- Design §5 "suppression extinguishes all diagnostic families" →
  Task 4 (`suppression_extinguishes_the_syntax_family_too`).
- Design §5 "a docblock-attached `@psalm-suppress` maps to the
  annotated declaration's whole span" → decision 9, Task 2
  (`annotated_node_range`), Task 4
  (`psalm_suppress_on_a_declaration_docblock_covers_its_whole_span`).
- Design §5 "over-suppression, never under-suppression" → decisions 3,
  5, 9, 10; the EOF exception and the both-lines resolution exist for
  exactly this rule.
- Design §5 "identifier-level correspondence stays deferred" →
  decision 8; the ledger records it.
- Design §9 "the pack and blob schemas take version bumps" (the
  verdict's share of it) → Task 4, decision 7
  (`CACHE_SCHEMA_VERSION` 3 → 4); Task 5 pins the equivalence net over
  a suppressed fixture.
- Design §10 fuzzing contract ("arbitrary input, never a panic") →
  Task 3 (adversarial pins), Task 6 (the libFuzzer target and seeds).
- Design §10 invalidation over the new edit class → Task 2
  (`a_prose_comment_edit_backdates_the_suppression_set`), Task 5
  (directive edits are content-hash misses).
- The mechanical constraint (bridge depends only on
  `celerrate_plugin`) → unchanged and re-checked in Task 3 Step 4 and
  the closure gate; no new plugin crate, `PLUGIN_CRATES` untouched
  (decision 12).
- No new `CEL####` identifier is allocated; the registry's gapless
  test stays at its current count (Global Constraints).

## Accepted debt at closure

Every clause above was walked against landed code at closure; none
pointed at anything missing. The debt below is what the design itself
accepted, executed as reality, plus what execution surfaced.

- Identifier-level correspondence stayed deferred to the rule
  framework: an identifier-bearing suppression
  (`@phpstan-ignore method.notFound`, `@psalm-suppress
  PossiblyNullReference`) extinguishes every diagnostic family on its
  scope regardless of which identifier is named — design section 5's
  own deferral, executed exactly as decision 8 states.
- PHPStan 1.11's placement-dependent bare `@phpstan-ignore` resolves to
  both candidate lines — the current line and the next, the
  over-suppression superset — rather than inspecting whether the
  comment trails code on the same line.
- Recognition is word-boundary substring matching: prose that happens
  to embed a literal tag at a word boundary (`// see
  @phpstan-ignore-line above`) over-suppresses rather than
  under-suppresses. The posture is deliberately aligned with
  `is_recognized_annotation`
  (`crates/celerrate_semantics/src/body.rs:415-423`), so any comment
  the recognizer can read already invalidates the body IR on edit —
  recognition and invalidation cannot drift apart.
- Line-based scopes resolve against the whole comment token's lines,
  not the tag's own line inside it: a directive buried on an interior
  line of a multi-line block or docblock comment still covers the
  comment's first and last lines, never just the line the tag sits on.
- A docblock directive placed before a *statement* (not only before a
  declaration) suppresses that statement's whole node span — its Psalm
  scope — which is broader than one line whenever the statement itself
  spans several.
- No unused-suppression diagnostic exists: a directive that suppresses
  nothing is silent. Reporting that is left to the rule framework era,
  per the "no docblock diagnostics" global constraint.
- The WASM sketch
  (`.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`)
  stayed an internal draft, exactly as decision 11 fixed it: nothing in
  it is implemented in this sub-project, and its first mechanical
  exercise is sub-project 6's host. The dormant API-version gate
  (`celerrate_plugin::PLUGIN_API_VERSION`) remains dormant — no
  compiled-in plugin can fail it yet.
- Task 6's libFuzzer run was skipped: the `cargo-fuzz` subcommand is
  not installed on this machine, so `cargo +nightly fuzz run docblock
  -- -max_total_time=60` could not execute. `cd fuzz && cargo check`
  was run instead and proved the extended target (the
  `comment_directives` calls added to `fuzz_targets/docblock.rs`)
  compiles clean. No libFuzzer session against the new seed corpus
  (`fuzz/corpus/docblock/seed_directives`) has run on this branch; the
  in-source adversarial pins (Task 3's
  `adversarial_inputs_never_panic`) are the coverage that did execute.

