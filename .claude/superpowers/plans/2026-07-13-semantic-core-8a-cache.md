# Semantic Core Part 8a: The Persistent Artifact Cache — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A content-addressed derived-artifact cache under `.celerrate/cache/` that re-seeds a fresh database at startup, proven byte-for-byte identical to a from-scratch analysis by a cross-process extension of the incremental harness.

**Architecture:** The cache sits above salsa, never inside it. Two packs ship in this part: `item_trees.bin` (keyed by file content hash, consulted inside the `item_tree` query through a dependency-inverted extension point registered as a salsa singleton input at the composition root) and `diagnostics.bin` (per-file composed diagnostics with per-name revalidation records, consulted in the CLI's `analyze_one` before any query runs). The symbol-index pack named by the spec is deliberately deferred: its inputs (the item trees) are already cached, so its economics are measured in part 8b's benchmark before any code is written, per the spec's drop-a-losing-class rule. Serialization goes through CLI-owned mirror types (`Stored*`), because `DiagnosticId` wraps a `&'static str` that must be re-interned through the registry and every `FileId` must be remapped to the current process's numbering.

**Tech Stack:** Rust, salsa 0.27 (singleton inputs), blake3 (content addressing and checksums), serde + postcard (pack payloads), tempfile (atomic writes).

**Branch:** `semantic-core-8a-cache`, from `main`. Spec: `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md` (section 2).

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result`/`Option`; test modules may locally `#[allow]`.
- Strict layering, DAG with no upward edges: the `ArtifactCache` trait is owned by `celerrate_semantics` (the consuming layer); the implementation and all disk I/O live in `celerrate_cli` (the composition root). No crate below the CLI may read or write `.celerrate/`.
- Determinism: a cache lookup is a pure function of tracked inputs. A hit must return byte-for-byte what the computation would produce. Cache failures of any kind (missing, truncated, corrupt, mismatched) are answered by recomputation, never by an error the user sees.
- TDD: every task starts from a failing test. Frequent commits, gitmoji + Conventional Commits, repository-configured identity, no AI attribution.
- Everything in English, full words, no abbreviated names.
- Every task ends with: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check` all green.
- New dependencies (all compatible with the deny.toml allowlist, which already admits CC0-1.0): `blake3 = "1"` (CC0-1.0 OR Apache-2.0), `serde = { version = "1", features = ["derive"] }` (MIT OR Apache-2.0), `postcard = { version = "1", features = ["use-std"] }` (MIT OR Apache-2.0).

## File Structure

```
Cargo.toml                                     modify: workspace dependencies
crates/celerrate_db/Cargo.toml                 modify: blake3
crates/celerrate_db/src/queries.rs             modify: ContentHash, content_hash
crates/celerrate_db/src/lib.rs                 modify: exports
crates/celerrate_diagnostics/src/registry.rs   modify: find_identifier
crates/celerrate_diagnostics/src/lib.rs        modify: exports
crates/celerrate_semantics/src/cache.rs        create: ArtifactCache, CacheHandle, ArtifactCacheInput
crates/celerrate_semantics/src/revalidation.rs create: ResolutionAnswer, ResolutionRecord, resolution_records
crates/celerrate_semantics/src/queries.rs      modify: item_tree consults the cache
crates/celerrate_semantics/src/lib.rs          modify: modules and exports
crates/celerrate_cli/Cargo.toml                modify: blake3, serde, postcard, tempfile
crates/celerrate_cli/src/cache/mod.rs          create: module root, persist entry point
crates/celerrate_cli/src/cache/pack.rs         create: magic, checksum, header, encode/decode, atomic write
crates/celerrate_cli/src/cache/stored.rs       create: Stored* mirror types and conversions
crates/celerrate_cli/src/cache/snapshot.rs     create: CacheSnapshot, SnapshotCache, pack loading
crates/celerrate_cli/src/cache/verdict.rs      create: validated_verdict (revalidation)
crates/celerrate_cli/src/analysis.rs           modify: AnalysisInputs.cache, analyze_one consults
crates/celerrate_cli/src/session.rs            modify: snapshot load, singleton registration, fields
crates/celerrate_cli/src/lib.rs                modify: cache module, persist in run()
crates/celerrate_cli/src/watch.rs              modify: persist after each rendered cycle
crates/celerrate_cli/tests/cache_seeding.rs    create: seeding and revalidation, session level
crates/celerrate_cli/tests/cache_consistency.rs create: the cross-process harness
```

---

### Task 1: The content-address query

**Files:**
- Modify: `Cargo.toml` (workspace dependencies)
- Modify: `crates/celerrate_db/Cargo.toml`
- Modify: `crates/celerrate_db/src/queries.rs`
- Modify: `crates/celerrate_db/src/lib.rs`

**Interfaces:**
- Produces: `celerrate_db::ContentHash` (= `[u8; 32]`) and `celerrate_db::content_hash(db: &dyn salsa::Database, file: SourceFile) -> ContentHash`, a tracked query so one revision hashes a file at most once, wherever the address is needed (the `item_tree` hit check, the verdict hit check, and the pack writer all share the memo).

- [ ] **Step 1: Add the dependencies**

In the workspace `Cargo.toml`, `[workspace.dependencies]` section, add (keeping the list alphabetical):

```toml
blake3 = "1"
postcard = { version = "1", features = ["use-std"] }
serde = { version = "1", features = ["derive"] }
```

In `crates/celerrate_db/Cargo.toml`, `[dependencies]`, add:

```toml
blake3 = { workspace = true }
```

- [ ] **Step 2: Write the failing test**

Append to the `tests` module of `crates/celerrate_db/src/queries.rs`:

```rust
use crate::content_hash;

#[test]
fn the_content_hash_is_a_function_of_the_bytes_alone() {
    let db = TestDatabase::default();
    let first = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
    let second = SourceFile::new(&db, FileId::new(9), b"<?php echo 1;".to_vec());
    let different = SourceFile::new(&db, FileId::new(2), b"<?php echo 2;".to_vec());
    assert_eq!(content_hash(&db, first), content_hash(&db, second));
    assert_ne!(content_hash(&db, first), content_hash(&db, different));
}

#[test]
fn editing_bytes_changes_the_hash() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
    let before = content_hash(&db, file);
    file.set_bytes(&mut db).to(b"<?php echo 2;".to_vec());
    assert_ne!(before, content_hash(&db, file));
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test --package celerrate_db`
Expected: FAIL to compile with "cannot find function `content_hash`".

- [ ] **Step 4: Implement the query**

In `crates/celerrate_db/src/queries.rs`, below `line_index`:

```rust
/// The content address of one file: the blake3 hash of its raw bytes.
/// Every persistent-cache entry is keyed by it. A tracked query so one
/// revision hashes a file at most once, wherever the address is needed.
pub type ContentHash = [u8; 32];

/// Hashes a file's raw bytes into its content address.
#[salsa::tracked]
pub fn content_hash(db: &dyn salsa::Database, file: SourceFile) -> ContentHash {
    *blake3::hash(file.bytes(db)).as_bytes()
}
```

In `crates/celerrate_db/src/lib.rs`, extend the `queries` re-export:

```rust
pub use queries::{
    ALLOCATED_IDENTIFIERS, ContentHash, SOURCE_TOO_LARGE, content_hash, file_diagnostics,
    line_index, parse, source_text,
};
```

- [ ] **Step 5: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_db`
Expected: PASS.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS (deny validates the new dependency licenses).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/celerrate_db
git commit -m "✨ feat(db): hash a file's content once per revision"
```

---

### Task 2: The artifact-cache extension point

**Files:**
- Create: `crates/celerrate_semantics/src/cache.rs`
- Modify: `crates/celerrate_semantics/src/queries.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `celerrate_db::{ContentHash, content_hash}` (Task 1).
- Produces: `celerrate_semantics::ArtifactCache` (trait, `Send + Sync`, method `fn item_tree(&self, file: FileId, content: ContentHash) -> Option<ItemTree>`), `celerrate_semantics::CacheHandle` (newtype `pub struct CacheHandle(pub Arc<dyn ArtifactCache>)`), and `celerrate_semantics::ArtifactCacheInput` (a `#[salsa::input(singleton)]` with one field `cache: CacheHandle`, `#[returns(ref)]`). The `item_tree` query consults a registered cache before lowering; with no singleton registered, behavior is unchanged.

This is the dependency-inverted extension point the umbrella design prescribes: the consuming layer owns the trait, the implementation is registered as a salsa input at the composition root. The singleton is created at most once per database (salsa enforces it); `ArtifactCacheInput::try_get(db)` returns `None` in every database that never registered one, which is every test database today. If the generated `builder` is unavailable on a singleton input in salsa 0.27, fall back to `ArtifactCacheInput::new(&db, handle)`: the durability of a never-mutated input only affects revalidation cost, not correctness.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/cache.rs` with only the test module for now (the types come in Step 3):

```rust
//! The artifact-cache extension point: the dependency-inverted trait
//! this layer consults, the handle the composition root registers as a
//! salsa singleton input, and nothing about disks. The persistent cache
//! is a CLI concern; this layer only asks "is the boundary artifact of
//! these bytes already known?".

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::sync::Arc;

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{ContentHash, SourceFile};
    use celerrate_source::FileId;

    use crate::cache::{ArtifactCache, ArtifactCacheInput, CacheHandle};
    use crate::items::ItemTree;
    use crate::queries::item_tree;

    /// A probe cache: returns a fixed tree for every lookup. This
    /// deliberately violates the trait's exactness contract, which is
    /// the point: the only way to observe that the query consulted the
    /// cache is to hand it a value the computation would never produce.
    struct Probe(ItemTree);

    impl ArtifactCache for Probe {
        fn item_tree(&self, _file: FileId, _content: ContentHash) -> Option<ItemTree> {
            Some(self.0.clone())
        }
    }

    /// A cache that never has anything.
    struct Empty;

    impl ArtifactCache for Empty {
        fn item_tree(&self, _file: FileId, _content: ContentHash) -> Option<ItemTree> {
            None
        }
    }

    #[test]
    fn a_registered_cache_is_consulted_before_lowering() {
        let db = TestDatabase::default();
        ArtifactCacheInput::builder(CacheHandle(Arc::new(Probe(ItemTree::default()))))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        let _ = db.take_executed();
        let tree = item_tree(&db, file);
        assert!(
            tree.declarations.is_empty(),
            "the probe's empty tree is served, not the lowered one",
        );
        let executed = db.take_executed();
        assert!(
            executed.iter().all(|query| !query.contains("parse(")),
            "a hit never parses: {executed:?}",
        );
    }

    #[test]
    fn a_cache_miss_computes_normally() {
        let db = TestDatabase::default();
        ArtifactCacheInput::builder(CacheHandle(Arc::new(Empty)))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 1);
    }

    #[test]
    fn no_registered_cache_computes_normally() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        assert_eq!(item_tree(&db, file).declarations.len(), 1);
    }
}
```

Declare the module in `crates/celerrate_semantics/src/lib.rs` (alphabetical order in the module list):

```rust
mod cache;
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_semantics cache`
Expected: FAIL to compile with "cannot find `ArtifactCache`" (and friends).

- [ ] **Step 3: Implement the extension point**

Prepend to `crates/celerrate_semantics/src/cache.rs`, above the test module:

```rust
use std::fmt;
use std::sync::Arc;

use celerrate_db::ContentHash;
use celerrate_source::FileId;

use crate::items::ItemTree;

/// What a registered artifact cache can answer. The contract is exact:
/// a `Some` must be byte-for-byte what the computation would produce
/// for a file with identity `file` whose bytes hash to `content` —
/// `ItemTree::from_root(file, &parse(bytes).tree())` — or `None`.
/// The cross-process harness holds implementations to it.
pub trait ArtifactCache: Send + Sync {
    /// The cached item tree of the file whose content hashes to
    /// `content`, already remapped to `file`.
    fn item_tree(&self, file: FileId, content: ContentHash) -> Option<ItemTree>;
}

/// The registered cache, as the cloneable handle a salsa input field
/// requires.
#[derive(Clone)]
pub struct CacheHandle(pub Arc<dyn ArtifactCache>);

impl fmt::Debug for CacheHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("CacheHandle").finish()
    }
}

/// The singleton input the composition root registers once, before any
/// query runs, and never mutates: reading it therefore never
/// invalidates anything. Databases that register nothing (every test
/// database) take the compute path through `try_get`'s `None`.
#[salsa::input(singleton)]
pub struct ArtifactCacheInput {
    #[returns(ref)]
    pub cache: CacheHandle,
}
```

In `crates/celerrate_semantics/src/queries.rs`, replace the `item_tree` function body:

```rust
/// The item tree of one file: range-free, so a body edit produces an
/// equal value and salsa backdates it — the early-cutoff boundary
/// every cross-file consumer sits behind. A cache registered at the
/// composition root is consulted first, keyed by the file's content
/// address; the lookup is a pure function of tracked inputs, so the
/// query stays deterministic either way.
#[salsa::tracked(returns(ref))]
pub fn item_tree(db: &dyn salsa::Database, file: SourceFile) -> ItemTree {
    if let Some(input) = crate::cache::ArtifactCacheInput::try_get(db)
        && let Some(tree) = input
            .cache(db)
            .0
            .item_tree(file.file_id(db), celerrate_db::content_hash(db, file))
    {
        return tree;
    }
    ItemTree::from_root(file.file_id(db), &celerrate_db::parse(db, file).tree())
}
```

In `crates/celerrate_semantics/src/lib.rs`, add the exports:

```rust
pub use cache::{ArtifactCache, ArtifactCacheInput, CacheHandle};
```

- [ ] **Step 4: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_semantics`
Expected: PASS, including every pre-existing item-tree and invalidation-scope test (no singleton registered there, so nothing changed for them).
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): consult a registered artifact cache before lowering"
```

---

### Task 3: The revalidation records query

**Files:**
- Create: `crates/celerrate_semantics/src/revalidation.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `collect_references`, `resolve_name`, `SymbolSources`, `UseTables`, `SymbolResolution`, `item_tree` (all existing in this crate).
- Produces:
  - `celerrate_semantics::ResolutionAnswer` — `enum { Unknown, Source, Stub { availability: StubAvailability } }`, `Debug + Clone + Copy + PartialEq + Eq`.
  - `celerrate_semantics::ResolutionRecord` — `struct { written: String, space: SymbolSpace, namespace: String, answer: ResolutionAnswer }`, `Debug + Clone + PartialEq + Eq`.
  - `celerrate_semantics::answer_of(resolution: Option<SymbolResolution>) -> ResolutionAnswer`.
  - `celerrate_semantics::resolution_records(db, file: SourceFile, files: AnalyzedFileSet, stubs: StubIndexInput, configuration: ProjectConfiguration) -> &Vec<ResolutionRecord>` (tracked, `returns(ref)`).

Why this reduction is complete: the reference diagnostics of a file are a pure function of (the file's own content, the resolution answer of each reference, the PHP version range). The content is the cache entry's key, the range lives in the pack header, and the answers are these records. A `Source` answer produces no diagnostic regardless of its declaration kind, so the kind is deliberately dropped; a `Stub` answer's diagnostics depend on exactly its availability window, so the window is kept whole.

- [ ] **Step 1: Write the failing test**

Create `crates/celerrate_semantics/src/revalidation.rs`:

```rust
//! The revalidation records of one file: which names its reference
//! checks looked up, and what each lookup answered, reduced to exactly
//! what the diagnostics depend on. A persisted diagnostics entry is
//! accepted only when every recorded answer still holds; the records
//! are what makes "deserialize plus revalidate" a sound substitute for
//! recomputation.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use crate::revalidation::{ResolutionAnswer, resolution_records};
    use crate::symbols::SymbolSpace;

    fn configuration(db: &TestDatabase) -> ProjectConfiguration {
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db)
    }

    fn stub_index_with_strlen(db: &TestDatabase) -> StubIndexInput {
        let index = StubIndex::from_symbols(vec![StubSymbol {
            name: "strlen".to_owned(),
            kind: StubSymbolKind::Function,
            availability: StubAvailability::ALWAYS,
        }]);
        StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(db)
    }

    #[test]
    fn every_reference_is_recorded_with_its_answer() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php class Known {} $a = new Known(); $b = new Missing(); $c = strlen('x');"
                .to_vec(),
        );
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let stubs = stub_index_with_strlen(&db);
        let configuration = configuration(&db);

        let records = resolution_records(&db, file, files, stubs, configuration);
        let summary: Vec<(&str, SymbolSpace, ResolutionAnswer)> = records
            .iter()
            .map(|record| (record.written.as_str(), record.space, record.answer))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Known", SymbolSpace::ClassLike, ResolutionAnswer::Source),
                ("Missing", SymbolSpace::ClassLike, ResolutionAnswer::Unknown),
                (
                    "strlen",
                    SymbolSpace::Function,
                    ResolutionAnswer::Stub {
                        availability: StubAvailability::ALWAYS,
                    }
                ),
            ],
        );
    }

    #[test]
    fn an_answer_flips_when_a_defining_file_appears() {
        use salsa::Setter as _;

        let mut db = TestDatabase::default();
        let referencing = SourceFile::new(&db, FileId::new(0), b"<?php new Missing();".to_vec());
        let other = SourceFile::new(&db, FileId::new(1), b"<?php".to_vec());
        let files = AnalyzedFileSet::new(&db, vec![referencing, other]);
        let stubs = stub_index_with_strlen(&db);
        let configuration = configuration(&db);

        let before = resolution_records(&db, referencing, files, stubs, configuration);
        assert_eq!(before[0].answer, ResolutionAnswer::Unknown);

        other
            .set_bytes(&mut db)
            .to(b"<?php class Missing {}".to_vec());
        let after = resolution_records(&db, referencing, files, stubs, configuration);
        assert_eq!(after[0].answer, ResolutionAnswer::Source);
    }
}
```

Declare the module in `crates/celerrate_semantics/src/lib.rs`:

```rust
mod revalidation;
```

Note: if `StubIndex::from_symbols` does not exist under that name, use whatever constructor the existing `reference_checks` or `index` tests use to build a stub index with one symbol — copy their helper verbatim rather than inventing one.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_semantics revalidation`
Expected: FAIL to compile with "cannot find function `resolution_records`".

- [ ] **Step 3: Implement the query**

Prepend to `crates/celerrate_semantics/src/revalidation.rs`, above the test module:

```rust
use std::collections::HashMap;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::{StubAvailability, StubIndexInput};

use crate::lookup::SymbolResolution;
use crate::queries::item_tree;
use crate::references::collect_references;
use crate::resolve::{SymbolSources, UseTables, resolve_name};
use crate::symbols::SymbolSpace;

/// The answer a resolution reduces to: exactly what the reference
/// diagnostics depend on, and nothing more. A `Source` answer produces
/// no diagnostic whatever its declaration kind, so the kind is
/// dropped; a `Stub` answer's diagnostics are a function of its
/// availability window, so the window is kept whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionAnswer {
    Unknown,
    Source,
    Stub { availability: StubAvailability },
}

/// Reduces a resolution to its answer.
pub fn answer_of(resolution: Option<SymbolResolution>) -> ResolutionAnswer {
    match resolution {
        None => ResolutionAnswer::Unknown,
        Some(SymbolResolution::Source { .. }) => ResolutionAnswer::Source,
        Some(SymbolResolution::Stub { availability, .. }) => {
            ResolutionAnswer::Stub { availability }
        }
    }
}

/// One reference with its answer: what a persisted diagnostics entry
/// must re-check before it may speak for this file again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionRecord {
    pub written: String,
    pub space: SymbolSpace,
    pub namespace: String,
    pub answer: ResolutionAnswer,
}

/// Every statically named reference of the file with its current
/// answer, in tree order. The same traversal and resolution path as
/// `reference_diagnostics`, reduced to answers instead of findings.
#[salsa::tracked(returns(ref))]
pub fn resolution_records(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<ResolutionRecord> {
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let tree = item_tree(db, file);
    let root = celerrate_db::parse(db, file).tree();
    let mut tables_by_namespace: HashMap<String, UseTables> = HashMap::new();
    let mut records = Vec::new();
    for reference in collect_references(&root) {
        let tables = tables_by_namespace
            .entry(reference.namespace.clone())
            .or_insert_with(|| UseTables::for_namespace(tree, &reference.namespace));
        let answer = answer_of(resolve_name(
            db,
            sources,
            &reference.namespace,
            tables,
            &reference.written,
            reference.space,
        ));
        records.push(ResolutionRecord {
            written: reference.written,
            space: reference.space,
            namespace: reference.namespace,
            answer,
        });
    }
    records
}
```

Add the exports to `crates/celerrate_semantics/src/lib.rs`:

```rust
pub use revalidation::{ResolutionAnswer, ResolutionRecord, answer_of, resolution_records};
```

- [ ] **Step 4: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_semantics`
Expected: PASS.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): record the resolution answers a file's checks depend on"
```

---

### Task 4: Re-interning a stored identifier

**Files:**
- Modify: `crates/celerrate_diagnostics/src/registry.rs`
- Modify: `crates/celerrate_diagnostics/src/lib.rs`

**Interfaces:**
- Produces: `celerrate_diagnostics::find_identifier(text: &str) -> Option<DiagnosticId>`.

`DiagnosticId` wraps a `&'static str`, so a deserialized identifier cannot be constructed directly: it is re-interned through the registry, the one canonical list of every allocated identifier. An identifier the registry does not know marks a cache entry from another era, and the entry is discarded, never guessed at.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module of `crates/celerrate_diagnostics/src/registry.rs`:

```rust
use super::find_identifier;

#[test]
fn a_registered_identifier_is_found_and_an_unknown_one_is_not() {
    let found = find_identifier("CEL0018").unwrap();
    assert_eq!(found.as_str(), "CEL0018");
    assert!(find_identifier("CEL9999").is_none());
    assert!(find_identifier("").is_none());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_diagnostics`
Expected: FAIL to compile with "cannot find function `find_identifier`".

- [ ] **Step 3: Implement**

In `crates/celerrate_diagnostics/src/registry.rs`, below `REGISTRY`:

```rust
/// The registered identifier whose text is `text`, re-interned to its
/// `'static` form. `None` for anything the registry does not know: a
/// deserialized identifier that fails this lookup comes from another
/// binary's era and its carrier is discarded, never guessed at.
pub fn find_identifier(text: &str) -> Option<DiagnosticId> {
    REGISTRY
        .iter()
        .find(|entry| entry.id.as_str() == text)
        .map(|entry| entry.id)
}
```

In `crates/celerrate_diagnostics/src/lib.rs`, add `find_identifier` to the `registry` re-export (next to the existing `REGISTRY` export).

- [ ] **Step 4: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_diagnostics`
Expected: PASS.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_diagnostics
git commit -m "✨ feat(diagnostics): re-intern a stored identifier through the registry"
```

---

### Task 5: The pack format

**Files:**
- Modify: `crates/celerrate_cli/Cargo.toml`
- Create: `crates/celerrate_cli/src/cache/mod.rs`
- Create: `crates/celerrate_cli/src/cache/pack.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (declare the module)

**Interfaces:**
- Produces (module `celerrate_cli::cache::pack`):
  - `CACHE_MAGIC: [u8; 8]` (= `*b"CELCACHE"`), `CACHE_SCHEMA_VERSION: u32` (= 1).
  - `PackHeader { schema: u32, binary: String, stub_blob: [u8; 32], php_minimum: (u8, u8), php_maximum: (u8, u8) }` with `PackHeader::current(range: PhpVersionRange) -> PackHeader`.
  - `Pack<Entries> { header: PackHeader, entries: Entries }`.
  - `encode<Entries: Serialize>(pack: &Pack<Entries>) -> Option<Vec<u8>>`.
  - `decode<Entries: DeserializeOwned>(bytes: &[u8], expected: &PackHeader) -> Option<Pack<Entries>>`.
  - `write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()>`.

On-disk shape: `magic (8 bytes) ++ blake3 checksum of the payload (32 bytes) ++ payload (postcard)`. Every rejection — short file, wrong magic, checksum mismatch, undecodable payload, header mismatch — answers `None`, and the caller regenerates. The header pins the stub *content* (a hash of `EMBEDDED_STUB_BLOB`), not just its format version: a new stub snapshot changes availability answers, so it must discard the packs.

The fuzz decision the spec delegates to this plan, decided here: the pack format does **not** join the fuzz targets. Its attack surface is a file inside the project's own `.celerrate/` directory (whoever writes there already controls the project source, which the parser fuzzers cover), and its decode path is checksum-gated postcard, structurally panic-free and exercised by the corruption tests below and by Task 10's on-disk corruption harness.

- [ ] **Step 1: Add the dependencies and the module skeleton**

In `crates/celerrate_cli/Cargo.toml`: add to `[dependencies]` (alphabetical):

```toml
blake3 = { workspace = true }
postcard = { workspace = true }
serde = { workspace = true }
tempfile = { workspace = true }
```

and remove `tempfile` from `[dev-dependencies]` (it moves to a real dependency for the atomic write; dev usage keeps working).

Create `crates/celerrate_cli/src/cache/mod.rs`:

```rust
//! The persistent artifact cache: a content-addressed derived-artifact
//! cache above salsa, persisted to `.celerrate/cache/` and used to
//! re-seed a fresh database at startup. Nothing here is ever fatal:
//! every failure mode of a cache file answers by recomputation.

pub mod pack;
```

In `crates/celerrate_cli/src/lib.rs`, add `pub mod cache;` to the module list.

- [ ] **Step 2: Write the failing tests**

Append to `crates/celerrate_cli/src/cache/pack.rs` (create the file with the test module first):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_project::{PhpVersion, PhpVersionRange};

    use super::{CACHE_MAGIC, Pack, PackHeader, decode, encode, write_atomically};

    fn header() -> PackHeader {
        PackHeader::current(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
    }

    fn sample() -> Pack<Vec<(u32, String)>> {
        Pack {
            header: header(),
            entries: vec![(1, "one".to_owned()), (2, "two".to_owned())],
        }
    }

    #[test]
    fn a_pack_round_trips() {
        let bytes = encode(&sample()).unwrap();
        assert_eq!(&bytes[..8], &CACHE_MAGIC);
        let decoded: Pack<Vec<(u32, String)>> = decode(&bytes, &header()).unwrap();
        assert_eq!(decoded, sample());
    }

    #[test]
    fn every_corruption_mode_answers_none() {
        let bytes = encode(&sample()).unwrap();

        // Truncated: shorter than the magic, shorter than the
        // checksum, and mid-payload.
        for length in [0, 4, 20, bytes.len() - 3] {
            let truncated = &bytes[..length];
            assert!(
                decode::<Vec<(u32, String)>>(truncated, &header()).is_none(),
                "a pack truncated to {length} bytes must be rejected",
            );
        }

        // Wrong magic.
        let mut wrong_magic = bytes.clone();
        wrong_magic[0] = b'X';
        assert!(decode::<Vec<(u32, String)>>(&wrong_magic, &header()).is_none());

        // A flipped payload byte fails the checksum.
        let mut flipped = bytes.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0xFF;
        assert!(decode::<Vec<(u32, String)>>(&flipped, &header()).is_none());

        // A flipped checksum byte fails the checksum.
        let mut bad_checksum = bytes.clone();
        bad_checksum[10] ^= 0xFF;
        assert!(decode::<Vec<(u32, String)>>(&bad_checksum, &header()).is_none());

        // Garbage of plausible length.
        let garbage = vec![0xAAu8; bytes.len()];
        assert!(decode::<Vec<(u32, String)>>(&garbage, &header()).is_none());
    }

    #[test]
    fn a_header_mismatch_discards_the_whole_pack() {
        let bytes = encode(&sample()).unwrap();
        let other_range = PackHeader::current(PhpVersionRange::new(
            PhpVersion::new(8, 2),
            PhpVersion::new(8, 5),
        ));
        assert!(decode::<Vec<(u32, String)>>(&bytes, &other_range).is_none());

        let mut other_schema = header();
        other_schema.schema += 1;
        assert!(decode::<Vec<(u32, String)>>(&bytes, &other_schema).is_none());

        let mut other_binary = header();
        other_binary.binary = "0.0.0-other".to_owned();
        assert!(decode::<Vec<(u32, String)>>(&bytes, &other_binary).is_none());
    }

    #[test]
    fn the_atomic_write_replaces_the_file_whole() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pack.bin");
        write_atomically(&path, b"first").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        write_atomically(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
    }
}
```

`celerrate_project` must be reachable from the CLI: it already is (a dependency).

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test --package celerrate_cli cache::pack`
Expected: FAIL to compile with "cannot find" for `PackHeader`, `encode`, and friends.

- [ ] **Step 4: Implement the format**

Prepend to `crates/celerrate_cli/src/cache/pack.rs`:

```rust
//! The pack file: `magic ++ blake3 checksum of the payload ++ payload`,
//! the payload being one postcard-encoded `Pack`. Every rejection —
//! short file, wrong magic, checksum mismatch, undecodable payload,
//! header mismatch — answers `None`, and the caller regenerates:
//! corruption is detected, never fatal, never visible.

use std::path::Path;

use celerrate_project::PhpVersionRange;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The first eight bytes of every pack file.
pub const CACHE_MAGIC: [u8; 8] = *b"CELCACHE";

/// Bumped whenever any stored shape changes. The header also carries
/// the binary version, so releases invalidate packs on their own; this
/// constant is what protects development builds within one version.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// What must match for a pack to be readable at all: the schema, the
/// binary, the stub content, and the PHP version range. Any mismatch
/// discards the whole pack, so entry keys only need to encode what
/// varies within one configuration — file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PackHeader {
    pub schema: u32,
    pub binary: String,
    /// The blake3 hash of the embedded stub blob: pins the stub
    /// *content*, not just its format — a new snapshot changes
    /// availability answers.
    pub stub_blob: [u8; 32],
    pub php_minimum: (u8, u8),
    pub php_maximum: (u8, u8),
}

impl PackHeader {
    /// The header of this binary analyzing under `range`.
    pub fn current(range: PhpVersionRange) -> Self {
        Self {
            schema: CACHE_SCHEMA_VERSION,
            binary: env!("CARGO_PKG_VERSION").to_owned(),
            stub_blob: *blake3::hash(celerrate_stubs::EMBEDDED_STUB_BLOB).as_bytes(),
            php_minimum: (range.minimum.major, range.minimum.minor),
            php_maximum: (range.maximum.major, range.maximum.minor),
        }
    }
}

/// One pack: its header and its entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Pack<Entries> {
    pub header: PackHeader,
    pub entries: Entries,
}

/// Encodes a pack into its on-disk bytes. `None` only if postcard
/// cannot serialize the value, which no stored shape can trigger; the
/// caller skips the write rather than failing the run.
pub fn encode<Entries: Serialize>(pack: &Pack<Entries>) -> Option<Vec<u8>> {
    let payload = postcard::to_stdvec(pack).ok()?;
    let mut bytes = Vec::with_capacity(CACHE_MAGIC.len() + 32 + payload.len());
    bytes.extend_from_slice(&CACHE_MAGIC);
    bytes.extend_from_slice(blake3::hash(&payload).as_bytes());
    bytes.extend_from_slice(&payload);
    Some(bytes)
}

/// Decodes and validates a pack, or answers `None` for anything less
/// than a whole, current, matching file.
pub fn decode<Entries: DeserializeOwned>(
    bytes: &[u8],
    expected: &PackHeader,
) -> Option<Pack<Entries>> {
    let magic = bytes.get(..CACHE_MAGIC.len())?;
    if magic != CACHE_MAGIC {
        return None;
    }
    let checksum = bytes.get(CACHE_MAGIC.len()..CACHE_MAGIC.len() + 32)?;
    let payload = bytes.get(CACHE_MAGIC.len() + 32..)?;
    if blake3::hash(payload).as_bytes() != checksum {
        return None;
    }
    let pack: Pack<Entries> = postcard::from_bytes(payload).ok()?;
    (pack.header == *expected).then_some(pack)
}

/// Writes bytes to `path` through a temporary file in the same
/// directory plus a rename, so a reader never sees a torn file and a
/// concurrent writer's last rename wins whole.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the pack path has no parent directory"))?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}
```

- [ ] **Step 5: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_cli cache::pack`
Expected: PASS.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/celerrate_cli
git commit -m "✨ feat(cli): read and write versioned, checksummed cache packs"
```

---

### Task 6: The stored mirror types

**Files:**
- Create: `crates/celerrate_cli/src/cache/stored.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (declare `pub mod stored;`)

**Interfaces:**
- Consumes: `ItemTree`, `Declaration`, `DeclarationKind`, `UseImport`, `ImportKind`, `AstId` (semantics); `Diagnostic`, `Severity`, `find_identifier` (diagnostics); `ResolutionRecord`, `ResolutionAnswer` (Task 3); `StubAvailability`, `StubDeprecation`, `PhpVersion` (stubs, project).
- Produces (all `Debug + Clone + PartialEq + Eq + Serialize + Deserialize`):
  - `StoredItemTree` with `StoredItemTree::of(tree: &ItemTree) -> StoredItemTree` and `to_item_tree(&self, file: FileId) -> ItemTree`.
  - `StoredDiagnostic` with `StoredDiagnostic::of(diagnostic: &Diagnostic) -> StoredDiagnostic` and `to_diagnostic(&self, file: FileId) -> Option<Diagnostic>` (`None` when the stored identifier is unknown to the registry).
  - `StoredRecord` with `StoredRecord::of(record: &ResolutionRecord) -> StoredRecord`, `space(&self) -> SymbolSpace`, `matches(&self, answer: ResolutionAnswer) -> bool`.
  - `StoredVerdict { diagnostics: Vec<StoredDiagnostic>, records: Vec<StoredRecord> }`.

The mirrors are the schema. `FileId` is process-local (assigned by the virtual file system in walk order), so nothing stored carries one: an item tree stores each declaration's `AstId` *index* and the loader stamps the current `FileId` back in; a diagnostic stores its range as two offsets and the loader stamps the file. `DiagnosticId` is re-interned through Task 4's `find_identifier`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_cli/src/cache/stored.rs` with the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
    use celerrate_semantics::{ItemTree, ResolutionAnswer};
    use celerrate_source::{FileId, TextRange, TextSize};
    use celerrate_stubs::{StubAvailability, StubDeprecation};

    use super::{StoredAnswer, StoredDiagnostic, StoredItemTree, StoredRecord};

    fn parsed_tree(file: u32, source: &str) -> ItemTree {
        let parse = celerrate_syntax::parse(source);
        ItemTree::from_root(FileId::new(file), &parse.tree())
    }

    #[test]
    fn an_item_tree_round_trips_onto_another_file_identity() {
        let source = "<?php namespace App; use Lib\\Helper as H; \
                      class Service extends Base implements Contract {} \
                      function run() {} const LIMIT = 3;";
        let original = parsed_tree(3, source);
        let stored = StoredItemTree::of(&original);
        let remapped = stored.to_item_tree(FileId::new(9));
        assert_eq!(remapped, parsed_tree(9, source));
    }

    #[test]
    fn a_diagnostic_round_trips_and_an_unknown_identifier_is_rejected() {
        let original = Diagnostic {
            id: DiagnosticId::new("CEL0018"),
            severity: Severity::Error,
            file: FileId::new(3),
            range: TextRange::new(TextSize::from(5), TextSize::from(12)),
            message: "unknown class Missing".to_owned(),
        };
        let stored = StoredDiagnostic::of(&original);
        let remapped = stored.to_diagnostic(FileId::new(9)).unwrap();
        assert_eq!(remapped.id, original.id);
        assert_eq!(remapped.severity, original.severity);
        assert_eq!(remapped.file, FileId::new(9));
        assert_eq!(remapped.range, original.range);
        assert_eq!(remapped.message, original.message);

        let unknown = StoredDiagnostic {
            id: "CEL9999".to_owned(),
            ..stored
        };
        assert!(unknown.to_diagnostic(FileId::new(9)).is_none());
    }

    #[test]
    fn every_answer_shape_round_trips_through_matches() {
        let answers = [
            ResolutionAnswer::Unknown,
            ResolutionAnswer::Source,
            ResolutionAnswer::Stub {
                availability: StubAvailability::ALWAYS,
            },
            ResolutionAnswer::Stub {
                availability: StubAvailability {
                    introduced: Some(celerrate_project::PhpVersion::new(8, 2)),
                    removed: Some(celerrate_project::PhpVersion::new(8, 4)),
                    deprecated: Some(StubDeprecation {
                        since: Some(celerrate_project::PhpVersion::new(8, 3)),
                    }),
                },
            },
            ResolutionAnswer::Stub {
                availability: StubAvailability {
                    introduced: None,
                    removed: None,
                    deprecated: Some(StubDeprecation { since: None }),
                },
            },
        ];
        for answer in answers {
            let record = StoredRecord {
                written: "Name".to_owned(),
                space: super::StoredSpace::ClassLike,
                namespace: String::new(),
                answer: StoredAnswer::of(answer),
            };
            assert!(record.matches(answer), "{answer:?} must match itself");
            for other in answers {
                if other != answer {
                    assert!(!record.matches(other), "{answer:?} must not match {other:?}");
                }
            }
        }
    }
}
```

Declare `pub mod stored;` in `crates/celerrate_cli/src/cache/mod.rs`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_cli cache::stored`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the mirrors**

Prepend to `crates/celerrate_cli/src/cache/stored.rs`:

```rust
//! The serialized forms of the cached artifacts. Mirror types rather
//! than derives on the domain types, because the conversion is the
//! schema: a `FileId` is process-local and must be stamped back in at
//! load, and a `DiagnosticId` wraps a `'static` string that must be
//! re-interned through the registry. Every `to_*` conversion is total
//! except identifier re-interning, whose failure discards the entry.

use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
use celerrate_project::PhpVersion;
use celerrate_semantics::{
    AstId, Declaration, DeclarationKind, ImportKind, ItemTree, ResolutionAnswer, ResolutionRecord,
    SymbolSpace, UseImport,
};
use celerrate_source::{FileId, TextRange, TextSize};
use celerrate_stubs::{StubAvailability, StubDeprecation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredDeclarationKind {
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Constant,
}

impl StoredDeclarationKind {
    fn of(kind: DeclarationKind) -> Self {
        match kind {
            DeclarationKind::Class => Self::Class,
            DeclarationKind::Interface => Self::Interface,
            DeclarationKind::Trait => Self::Trait,
            DeclarationKind::Enum => Self::Enum,
            DeclarationKind::Function => Self::Function,
            DeclarationKind::Constant => Self::Constant,
        }
    }

    fn to_kind(self) -> DeclarationKind {
        match self {
            Self::Class => DeclarationKind::Class,
            Self::Interface => DeclarationKind::Interface,
            Self::Trait => DeclarationKind::Trait,
            Self::Enum => DeclarationKind::Enum,
            Self::Function => DeclarationKind::Function,
            Self::Constant => DeclarationKind::Constant,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDeclaration {
    kind: StoredDeclarationKind,
    name: String,
    namespace: String,
    ast_index: u32,
    extends: Vec<String>,
    implements: Vec<String>,
    trait_uses: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredImportKind {
    Class,
    Function,
    Constant,
}

impl StoredImportKind {
    fn of(kind: ImportKind) -> Self {
        match kind {
            ImportKind::Class => Self::Class,
            ImportKind::Function => Self::Function,
            ImportKind::Constant => Self::Constant,
        }
    }

    fn to_kind(self) -> ImportKind {
        match self {
            Self::Class => ImportKind::Class,
            Self::Function => ImportKind::Function,
            Self::Constant => ImportKind::Constant,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredUseImport {
    kind: StoredImportKind,
    target: String,
    alias: String,
    namespace: String,
    ast_index: u32,
}

/// One file's item tree with its process-local file identity removed:
/// only the declaration indexes survive, and `to_item_tree` stamps the
/// current identity back in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredItemTree {
    declarations: Vec<StoredDeclaration>,
    imports: Vec<StoredUseImport>,
}

impl StoredItemTree {
    pub fn of(tree: &ItemTree) -> Self {
        Self {
            declarations: tree
                .declarations
                .iter()
                .map(|declaration| StoredDeclaration {
                    kind: StoredDeclarationKind::of(declaration.kind),
                    name: declaration.name.clone(),
                    namespace: declaration.namespace.clone(),
                    ast_index: declaration.ast_id.index,
                    extends: declaration.extends.clone(),
                    implements: declaration.implements.clone(),
                    trait_uses: declaration.trait_uses.clone(),
                })
                .collect(),
            imports: tree
                .imports
                .iter()
                .map(|import| StoredUseImport {
                    kind: StoredImportKind::of(import.kind),
                    target: import.target.clone(),
                    alias: import.alias.clone(),
                    namespace: import.namespace.clone(),
                    ast_index: import.ast_id.index,
                })
                .collect(),
        }
    }

    pub fn to_item_tree(&self, file: FileId) -> ItemTree {
        ItemTree {
            declarations: self
                .declarations
                .iter()
                .map(|declaration| Declaration {
                    kind: declaration.kind.to_kind(),
                    name: declaration.name.clone(),
                    namespace: declaration.namespace.clone(),
                    ast_id: AstId {
                        file,
                        index: declaration.ast_index,
                    },
                    extends: declaration.extends.clone(),
                    implements: declaration.implements.clone(),
                    trait_uses: declaration.trait_uses.clone(),
                })
                .collect(),
            imports: self
                .imports
                .iter()
                .map(|import| UseImport {
                    kind: import.kind.to_kind(),
                    target: import.target.clone(),
                    alias: import.alias.clone(),
                    namespace: import.namespace.clone(),
                    ast_id: AstId {
                        file,
                        index: import.ast_index,
                    },
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredSeverity {
    Warning,
    Error,
}

/// One diagnostic with its process-local file identity removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDiagnostic {
    pub id: String,
    pub severity: StoredSeverity,
    pub start: u32,
    pub end: u32,
    pub message: String,
}

impl StoredDiagnostic {
    pub fn of(diagnostic: &Diagnostic) -> Self {
        Self {
            id: diagnostic.id.as_str().to_owned(),
            severity: match diagnostic.severity {
                Severity::Warning => StoredSeverity::Warning,
                Severity::Error => StoredSeverity::Error,
            },
            start: diagnostic.range.start().into(),
            end: diagnostic.range.end().into(),
            message: diagnostic.message.clone(),
        }
    }

    /// `None` when the stored identifier is unknown to the registry:
    /// the entry comes from another era and is discarded.
    pub fn to_diagnostic(&self, file: FileId) -> Option<Diagnostic> {
        Some(Diagnostic {
            id: find_identifier(&self.id)?,
            severity: match self.severity {
                StoredSeverity::Warning => Severity::Warning,
                StoredSeverity::Error => Severity::Error,
            },
            file,
            range: TextRange::new(TextSize::from(self.start), TextSize::from(self.end)),
            message: self.message.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredSpace {
    ClassLike,
    Function,
    Constant,
}

impl StoredSpace {
    fn of(space: SymbolSpace) -> Self {
        match space {
            SymbolSpace::ClassLike => Self::ClassLike,
            SymbolSpace::Function => Self::Function,
            SymbolSpace::Constant => Self::Constant,
        }
    }

    pub fn to_space(self) -> SymbolSpace {
        match self {
            Self::ClassLike => SymbolSpace::ClassLike,
            Self::Function => SymbolSpace::Function,
            Self::Constant => SymbolSpace::Constant,
        }
    }
}

fn stored_version(version: Option<PhpVersion>) -> Option<(u8, u8)> {
    version.map(|version| (version.major, version.minor))
}

/// A resolution answer in stored form. The deprecation nests two
/// options deliberately: the outer one is "is the symbol deprecated at
/// all", the inner one is "does the deprecation name a version".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredAnswer {
    Unknown,
    Source,
    Stub {
        introduced: Option<(u8, u8)>,
        removed: Option<(u8, u8)>,
        deprecated: Option<Option<(u8, u8)>>,
    },
}

impl StoredAnswer {
    pub fn of(answer: ResolutionAnswer) -> Self {
        match answer {
            ResolutionAnswer::Unknown => Self::Unknown,
            ResolutionAnswer::Source => Self::Source,
            ResolutionAnswer::Stub { availability } => Self::Stub {
                introduced: stored_version(availability.introduced),
                removed: stored_version(availability.removed),
                deprecated: availability
                    .deprecated
                    .map(|deprecation| stored_version(deprecation.since)),
            },
        }
    }
}

/// One revalidation record in stored form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRecord {
    pub written: String,
    pub space: StoredSpace,
    pub namespace: String,
    pub answer: StoredAnswer,
}

impl StoredRecord {
    pub fn of(record: &ResolutionRecord) -> Self {
        Self {
            written: record.written.clone(),
            space: StoredSpace::of(record.space),
            namespace: record.namespace.clone(),
            answer: StoredAnswer::of(record.answer),
        }
    }

    /// Whether the recorded answer still holds.
    pub fn matches(&self, answer: ResolutionAnswer) -> bool {
        self.answer == StoredAnswer::of(answer)
    }
}

/// One reported file's persisted verdict: its composed diagnostics and
/// the records that must revalidate before they may speak again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVerdict {
    pub diagnostics: Vec<StoredDiagnostic>,
    pub records: Vec<StoredRecord>,
}
```

If `TextSize::from(self.start)` needs `u32: Into<TextSize>` differently, `TextSize::new` is the fallback; `.into()` on `TextSize -> u32` mirrors what `text-size` provides. `StubAvailability` and `StubDeprecation` are consumed via `ResolutionAnswer` only; the unused-import lint will say if the direct imports are unnecessary — trim to what compiles.

- [ ] **Step 4: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_cli cache::stored`
Expected: PASS.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): mirror the cached artifacts into serializable stored forms"
```

---

### Task 7: The snapshot, loaded at startup and registered as the singleton

**Files:**
- Create: `crates/celerrate_cli/src/cache/snapshot.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (declare `pub mod snapshot;`)
- Modify: `crates/celerrate_cli/src/session.rs`
- Create: `crates/celerrate_cli/tests/cache_seeding.rs`

**Interfaces:**
- Consumes: Tasks 1-6.
- Produces:
  - `cache::snapshot::{ITEM_TREES_PACK, DIAGNOSTICS_PACK}` (= `"item_trees.bin"`, `"diagnostics.bin"`).
  - `cache::snapshot::CacheSnapshot { item_trees: HashMap<ContentHash, StoredItemTree>, verdicts: HashMap<ContentHash, StoredVerdict> }`, `Default`, with `CacheSnapshot::load(cache_directory: &Path, expected: &PackHeader) -> CacheSnapshot`.
  - `cache::snapshot::SnapshotCache(pub Arc<CacheSnapshot>)` implementing `celerrate_semantics::ArtifactCache`.
  - `Session` gains three fields: `pub cache: Arc<CacheSnapshot>`, `pub cache_directory: PathBuf` (= `<root>/.celerrate/cache`), `pub cache_loaded_range: PhpVersionRange` (the range the snapshot was validated against). `Session::start` loads the snapshot and registers the singleton.

One subtlety is spelled out here because two later tasks depend on it: item-tree entries are range-independent (a pure parse projection), but the header check is per-pack wholesale, per the spec — a range change between runs discards them too, a deliberate simplicity-over-economics trade the spec makes. Verdict entries *are* range-dependent, and under `--watch` the range can change at runtime (a `composer.json` edit): `cache_loaded_range` is what Task 8 compares against before attaching verdicts to a pass.

- [ ] **Step 1: Write the failing integration test**

Create `crates/celerrate_cli/tests/cache_seeding.rs`:

```rust
//! The cache snapshot seeds a fresh session. These tests hand-write
//! pack files and observe the session serving from them; the probe
//! entries deliberately violate the exactness contract, because a
//! correct entry is indistinguishable from a recomputation.

#![allow(clippy::unwrap_used)]

use std::path::Path;

use celerrate_cli::cache::pack::{Pack, PackHeader, encode, write_atomically};
use celerrate_cli::cache::snapshot::ITEM_TREES_PACK;
use celerrate_cli::cache::stored::StoredItemTree;
use celerrate_cli::session::Session;
use celerrate_project::{PhpVersion, PhpVersionRange};
use celerrate_semantics::{ItemTree, item_tree};

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

fn write_item_trees_pack(root: &Path, header: &PackHeader, entries: Vec<([u8; 32], StoredItemTree)>) {
    let directory = root.join(".celerrate/cache");
    std::fs::create_dir_all(&directory).unwrap();
    let bytes = encode(&Pack {
        header: header.clone(),
        entries,
    })
    .unwrap();
    write_atomically(&directory.join(ITEM_TREES_PACK), &bytes).unwrap();
}

/// The probe: an empty stored tree for a file that declares one class.
/// A session that serves it consulted the pack; a session that lowers
/// the file would see the declaration.
#[test]
fn a_matching_pack_seeds_the_item_tree_query() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);

    // The content hash must be computed exactly as the session will:
    // over the file's raw bytes.
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let probe = StoredItemTree::of(&ItemTree::default());
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_item_trees_pack(root.path(), &header, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    let tree = item_tree(&session.database, file);
    assert!(
        tree.declarations.is_empty(),
        "the probe tree is served from the pack, not lowered from source",
    );
}

/// A pack written under another PHP version range is ignored whole.
#[test]
fn a_range_mismatch_ignores_the_pack() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let probe = StoredItemTree::of(&ItemTree::default());
    let other_header = PackHeader::current(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 2),
    ));
    write_item_trees_pack(root.path(), &other_header, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    assert_eq!(
        item_tree(&session.database, file).declarations.len(),
        1,
        "the mismatched pack is ignored and the file is lowered",
    );
}

/// A corrupt pack is silently absent.
#[test]
fn a_corrupt_pack_is_silently_absent() {
    let root = project(&[("a.php", "<?php class Marker {}")]);
    let directory = root.path().join(".celerrate/cache");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(ITEM_TREES_PACK), b"garbage").unwrap();

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    assert_eq!(item_tree(&session.database, file).declarations.len(), 1);
    assert!(
        session.internal_errors.is_empty(),
        "corruption is never an error the user sees",
    );
}
```

Notes for the implementer: the zero-configuration fallback range is `PhpVersionRange::point(PhpVersion::new(8, 5))` (the latest stable — see `celerrate_project::version::LATEST_STABLE_VERSION`); the first test's project has no `composer.json`, so `PackHeader::current` must be built over exactly that fallback for the header to match. `blake3` must be added to `[dev-dependencies]` of `celerrate_cli` for the test's hashing (it is already a real dependency; re-listing it under dev-dependencies is unnecessary — the test can use it through the crate only if re-exported, so just add `blake3 = { workspace = true }` to dev-dependencies for directness). The `session` and `cache` modules must be `pub` for the integration test; `session` already is.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_cli --test cache_seeding`
Expected: FAIL to compile with "cannot find `snapshot`" (and the missing `Session` fields).

- [ ] **Step 3: Implement the snapshot**

Create `crates/celerrate_cli/src/cache/snapshot.rs`:

```rust
//! The loaded cache: an immutable snapshot, fixed for the lifetime of
//! the process, of whatever validated on disk. Anything that fails any
//! check is silently absent; the run recomputes and the next persist
//! rewrites it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use celerrate_db::ContentHash;
use celerrate_semantics::{ArtifactCache, ItemTree};
use celerrate_source::FileId;
use serde::de::DeserializeOwned;

use super::pack::{PackHeader, decode};
use super::stored::{StoredItemTree, StoredVerdict};

pub const ITEM_TREES_PACK: &str = "item_trees.bin";
pub const DIAGNOSTICS_PACK: &str = "diagnostics.bin";

/// Whatever the packs validated to. Both maps may be empty; nothing
/// downstream distinguishes "no cache" from "no valid cache".
#[derive(Debug, Default)]
pub struct CacheSnapshot {
    pub item_trees: HashMap<ContentHash, StoredItemTree>,
    pub verdicts: HashMap<ContentHash, StoredVerdict>,
}

impl CacheSnapshot {
    pub fn load(cache_directory: &Path, expected: &PackHeader) -> Self {
        Self {
            item_trees: load_pack(&cache_directory.join(ITEM_TREES_PACK), expected),
            verdicts: load_pack(&cache_directory.join(DIAGNOSTICS_PACK), expected),
        }
    }
}

fn load_pack<Entry: DeserializeOwned>(
    path: &Path,
    expected: &PackHeader,
) -> HashMap<ContentHash, Entry> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashMap::new();
    };
    match decode::<Vec<(ContentHash, Entry)>>(&bytes, expected) {
        Some(pack) => pack.entries.into_iter().collect(),
        None => HashMap::new(),
    }
}

/// The snapshot as the artifact cache the semantics layer consults:
/// a lookup by content address, with the current file identity stamped
/// back in.
pub struct SnapshotCache(pub Arc<CacheSnapshot>);

impl ArtifactCache for SnapshotCache {
    fn item_tree(&self, file: FileId, content: ContentHash) -> Option<ItemTree> {
        self.0
            .item_trees
            .get(&content)
            .map(|stored| stored.to_item_tree(file))
    }
}
```

Declare `pub mod snapshot;` in `crates/celerrate_cli/src/cache/mod.rs`.

In `crates/celerrate_cli/src/session.rs`:

1. Add the imports:

```rust
use crate::cache::pack::PackHeader;
use crate::cache::snapshot::{CacheSnapshot, SnapshotCache};
use celerrate_project::PhpVersionRange;
use celerrate_semantics::{ArtifactCacheInput, CacheHandle};
```

2. Add the fields to `Session`:

```rust
    /// The cache snapshot this session was seeded from: consulted for
    /// verdicts on the first pass, compared against on persist.
    pub cache: Arc<CacheSnapshot>,
    /// Where this project's packs live: `<root>/.celerrate/cache`.
    pub cache_directory: PathBuf,
    /// The PHP version range the snapshot validated against. Under
    /// `--watch` a manifest edit can move the range at runtime, and
    /// range-dependent verdicts must not survive that: passes compare
    /// this against the current range before attaching them.
    pub cache_loaded_range: PhpVersionRange,
```

3. In `Session::start`, after the `configuration` input is created and before the `Self { ... }` construction, load and register:

```rust
        let cache_directory = root.join(".celerrate").join("cache");
        let cache_loaded_range = discovery.php_version_range;
        let cache = Arc::new(CacheSnapshot::load(
            &cache_directory,
            &PackHeader::current(cache_loaded_range),
        ));
        ArtifactCacheInput::builder(CacheHandle(Arc::new(SnapshotCache(cache.clone()))))
            .durability(salsa::Durability::HIGH)
            .new(&database);
```

and carry the three fields into the `Session` literal.

- [ ] **Step 4: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_cli --test cache_seeding`
Expected: PASS.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS (every existing session and watch test still passes: no cache directory means an empty snapshot and unchanged behavior).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): load the cache snapshot and register it at startup"
```

---

### Task 8: The validated verdict, served instead of re-analysis

**Files:**
- Create: `crates/celerrate_cli/src/cache/verdict.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (declare `pub mod verdict;`)
- Modify: `crates/celerrate_cli/src/analysis.rs`
- Modify: `crates/celerrate_cli/src/session.rs` (`inputs()` attaches the snapshot, range-gated)
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs` (new tests)

**Interfaces:**
- Consumes: Tasks 3, 6, 7.
- Produces:
  - `AnalysisInputs` gains `pub cache: Arc<CacheSnapshot>`.
  - `cache::verdict::validated_verdict<'inputs>(inputs: &'inputs AnalysisInputs, file: SourceFile) -> Option<&'inputs StoredVerdict>` — the entry under the file's content hash, accepted only after every record's answer re-resolves identically against the current database. Task 9's writer reuses it.
  - `analyze_one` consults it first and maps the stored diagnostics through `to_diagnostic`; any unknown identifier rejects the whole entry (recompute).

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_cli/tests/cache_seeding.rs`:

```rust
use celerrate_cli::analysis::analyze;
use celerrate_cli::cache::snapshot::DIAGNOSTICS_PACK;
use celerrate_cli::cache::stored::{
    StoredAnswer, StoredDiagnostic, StoredRecord, StoredSeverity, StoredSpace, StoredVerdict,
};

fn write_diagnostics_pack(
    root: &Path,
    header: &PackHeader,
    entries: Vec<([u8; 32], StoredVerdict)>,
) {
    let directory = root.join(".celerrate/cache");
    std::fs::create_dir_all(&directory).unwrap();
    let bytes = encode(&Pack {
        header: header.clone(),
        entries,
    })
    .unwrap();
    write_atomically(&directory.join(DIAGNOSTICS_PACK), &bytes).unwrap();
}

fn probe_verdict() -> StoredVerdict {
    StoredVerdict {
        diagnostics: vec![StoredDiagnostic {
            id: "CEL0018".to_owned(),
            severity: StoredSeverity::Error,
            start: 10,
            end: 17,
            message: "planted by the cache probe".to_owned(),
        }],
        records: vec![StoredRecord {
            written: "Missing".to_owned(),
            space: StoredSpace::ClassLike,
            namespace: String::new(),
            answer: StoredAnswer::Unknown,
        }],
    }
}

/// The source references `Missing`, which resolves to nothing: the
/// recorded `Unknown` answer still holds, so the planted verdict is
/// served instead of a recomputation.
#[test]
fn a_verdict_whose_records_still_hold_is_served() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_diagnostics_pack(root.path(), &header, vec![(hash, probe_verdict())]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    let messages: Vec<&str> = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec!["planted by the cache probe"],
        "the stored verdict speaks, not a recomputation",
    );
}

/// A defining file appeared since the verdict was recorded: `Missing`
/// now resolves to a source declaration, the recorded `Unknown` answer
/// no longer holds, and the entry is discarded — the planted probe
/// must not survive.
#[test]
fn a_verdict_whose_answer_flipped_is_discarded() {
    let source = "<?php new Missing();";
    let root = project(&[
        ("a.php", source),
        ("b.php", "<?php class Missing {}"),
    ]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_diagnostics_pack(root.path(), &header, vec![(hash, probe_verdict())]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message != "planted by the cache probe"),
        "a stale verdict must be recomputed: {:?}",
        outcome.diagnostics,
    );
    assert!(
        outcome.diagnostics.is_empty(),
        "Missing now resolves, so the honest answer is no diagnostics",
    );
}

/// An entry carrying an identifier this binary does not know is from
/// another era: the whole entry is discarded and the file recomputed.
#[test]
fn a_verdict_with_an_unknown_identifier_is_discarded() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.diagnostics[0].id = "CEL9999".to_owned();
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1, "recomputed honestly");
    assert!(outcome.diagnostics[0].message.contains("Missing"));
}
```

Add `#![allow(clippy::indexing_slicing)]` to the test file's inner attributes if the `verdict.diagnostics[0]` access trips the lint in integration tests (integration tests are still linted; keep the allow at file level next to `unwrap_used`).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_cli --test cache_seeding`
Expected: the three new tests FAIL (the probe verdict is never served, or compilation fails on the missing exports).

- [ ] **Step 3: Implement the consult**

Create `crates/celerrate_cli/src/cache/verdict.rs`:

```rust
//! Revalidation: a persisted verdict may speak for a file again only
//! when the file's bytes hash to its key (checked by the map lookup)
//! and every recorded resolution answer still holds against the
//! current database. The re-resolution goes through the same memoized
//! per-name lookups the checks use, so a pass that validates many
//! entries pays each name once.

use std::collections::HashMap;

use celerrate_db::SourceFile;
use celerrate_semantics::{SymbolSources, UseTables, answer_of, item_tree, resolve_name};

use crate::analysis::AnalysisInputs;

use super::stored::StoredVerdict;

/// The stored verdict for `file`, if one exists under its content
/// address and every record revalidates. `None` means recompute.
pub fn validated_verdict<'inputs>(
    inputs: &'inputs AnalysisInputs,
    file: SourceFile,
) -> Option<&'inputs StoredVerdict> {
    let database = &inputs.database;
    let stored = inputs
        .cache
        .verdicts
        .get(&celerrate_db::content_hash(database, file))?;
    let sources = SymbolSources {
        files: inputs.files,
        stubs: inputs.stubs,
        configuration: inputs.configuration,
    };
    let tree = item_tree(database, file);
    let mut tables_by_namespace: HashMap<&str, UseTables> = HashMap::new();
    for record in &stored.records {
        let tables = tables_by_namespace
            .entry(record.namespace.as_str())
            .or_insert_with(|| UseTables::for_namespace(tree, &record.namespace));
        let answer = answer_of(resolve_name(
            database,
            sources,
            &record.namespace,
            tables,
            &record.written,
            record.space.to_space(),
        ));
        if !record.matches(answer) {
            return None;
        }
    }
    Some(stored)
}
```

Declare `pub mod verdict;` in `crates/celerrate_cli/src/cache/mod.rs`.

In `crates/celerrate_cli/src/analysis.rs`:

1. Add to the imports: `use std::sync::Arc;` is present; add `use crate::cache::snapshot::CacheSnapshot;`.
2. Add the field to `AnalysisInputs`:

```rust
    /// The cache snapshot the pass may serve verdicts from. Attached
    /// range-gated by `Session::inputs`; an empty default when the
    /// range moved since the snapshot was loaded.
    pub cache: Arc<CacheSnapshot>,
```

3. In `analyze_one`, serve the verdict first:

```rust
fn analyze_one(inputs: &AnalysisInputs, file: SourceFile) -> Result<Vec<Diagnostic>, FileId> {
    let database = &inputs.database;
    let file_id = file.file_id(database);
    guarded(file_id, || {
        if let Some(stored) = crate::cache::verdict::validated_verdict(inputs, file)
            && let Some(diagnostics) = stored
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.to_diagnostic(file_id))
                .collect::<Option<Vec<_>>>()
        {
            return diagnostics;
        }
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
        diagnostics
    })
}
```

In `crates/celerrate_cli/src/session.rs`, `Session::inputs`, attach the snapshot with the range gate:

```rust
    pub fn inputs(&self) -> AnalysisInputs {
        let current_range = self.configuration.php_version_range(&self.database);
        AnalysisInputs {
            database: self.database.clone(),
            files: self.files,
            stubs: self.stubs,
            configuration: self.configuration,
            reported: self.reported_files(),
            cache: if current_range == self.cache_loaded_range {
                self.cache.clone()
            } else {
                Arc::new(CacheSnapshot::default())
            },
        }
    }
```

Any other construction site of `AnalysisInputs` (the analysis tests build none by hand today; if one exists, give it `cache: Arc::new(CacheSnapshot::default())`).

- [ ] **Step 4: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_cli --test cache_seeding`
Expected: PASS.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): serve a validated cached verdict instead of re-analyzing"
```

---

### Task 9: Persisting the packs after every completed pass

**Files:**
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (the `persist` entry point)
- Modify: `crates/celerrate_cli/src/lib.rs` (`run` persists after rendering)
- Modify: `crates/celerrate_cli/src/watch.rs` (each cycle persists after rendering)
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs` (new tests via `run`)

**Interfaces:**
- Consumes: everything above.
- Produces: `cache::persist(session: &mut Session, outcome: &AnalysisOutcome)`. Item-tree entries cover the whole analyzed set (project and vendor); verdict entries cover reported, non-panicked files only. Entries are sorted by key and deduplicated; a pack is rewritten only when its entries differ from the session's current snapshot; after writing, the session's snapshot is replaced by what was written, so consecutive watch cycles compare against the last persisted state, not the startup state. `persist` is best-effort throughout: an I/O failure skips the write and nothing else.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_cli/tests/cache_seeding.rs`:

```rust
use celerrate_cli::run;

fn run_check(root: &Path) -> (celerrate_cli::Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn a_completed_run_writes_both_packs_and_the_gitignore() {
    let root = project(&[("a.php", "<?php class A {} new Missing();")]);
    let (_, _) = run_check(root.path());

    let cache = root.path().join(".celerrate/cache");
    assert!(cache.join(ITEM_TREES_PACK).is_file());
    assert!(cache.join(DIAGNOSTICS_PACK).is_file());
    assert_eq!(
        std::fs::read_to_string(root.path().join(".celerrate/.gitignore")).unwrap(),
        "*\n",
        "the cache directory ignores itself, like Cargo's target directory",
    );
}

#[test]
fn the_written_packs_validate_and_carry_the_analyzed_files() {
    let source_a = "<?php class A {}";
    let source_b = "<?php new Missing();";
    let root = project(&[("a.php", source_a), ("b.php", source_b)]);
    let (_, _) = run_check(root.path());

    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    let bytes = std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap();
    let pack: Pack<Vec<([u8; 32], StoredItemTree)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let keys: Vec<[u8; 32]> = pack.entries.iter().map(|(key, _)| *key).collect();
    assert!(keys.contains(&*blake3::hash(source_a.as_bytes()).as_bytes()));
    assert!(keys.contains(&*blake3::hash(source_b.as_bytes()).as_bytes()));
    assert!(keys.is_sorted(), "entries are written in key order");

    let bytes =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();
    let pack: Pack<Vec<([u8; 32], StoredVerdict)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    assert_eq!(pack.entries.len(), 2, "one verdict per reported file");
}

/// The second run over an unchanged project serves and rewrites
/// nothing: its packs must decode to exactly the first run's.
#[test]
fn a_second_run_leaves_equivalent_packs_behind() {
    let root = project(&[("a.php", "<?php new Missing();")]);
    let (_, first_output) = run_check(root.path());
    let first_trees =
        std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap();
    let first_verdicts =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();

    let (_, second_output) = run_check(root.path());
    assert_eq!(first_output, second_output, "byte-identical rendering");
    assert_eq!(
        first_trees,
        std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap(),
    );
    assert_eq!(
        first_verdicts,
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap(),
    );
}
```

And a unit test for the panicked exclusion, in `crates/celerrate_cli/src/cache/mod.rs`'s test module (create it):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crate::analysis::AnalysisOutcome;
    use crate::session::Session;

    /// A file the pass reported as panicked yields no verdict entry:
    /// nothing a panic touched enters the persistent cache.
    #[test]
    fn a_panicked_file_is_never_persisted() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        std::fs::write(root.path().join("b.php"), "<?php class B {}").unwrap();
        let mut session = Session::start(root.path());
        let panicked = *session.sources.keys().next().unwrap();

        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: vec![panicked],
        };
        super::persist(&mut session, &outcome);

        assert_eq!(
            session.cache.verdicts.len(),
            1,
            "the healthy file has a verdict, the panicked one does not",
        );
        assert_eq!(
            session.cache.item_trees.len(),
            2,
            "item trees are content projections and stay cacheable",
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_cli`
Expected: FAIL (no `persist`, packs never written).

- [ ] **Step 3: Implement persist**

Extend `crates/celerrate_cli/src/cache/mod.rs`:

```rust
pub mod pack;
pub mod snapshot;
pub mod stored;
pub mod verdict;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use celerrate_db::ContentHash;
use celerrate_source::FileId;
use serde::Serialize;

use crate::analysis::{AnalysisInputs, AnalysisOutcome};
use crate::session::Session;

use pack::{Pack, PackHeader};
use snapshot::{CacheSnapshot, DIAGNOSTICS_PACK, ITEM_TREES_PACK};
use stored::{StoredDiagnostic, StoredItemTree, StoredRecord, StoredVerdict};

/// Persists the packs after one completed pass, best-effort: an I/O
/// failure skips the write and nothing else. The session's snapshot is
/// replaced by what this pass concluded, so the next cycle's equality
/// check compares against the last persisted state.
pub fn persist(session: &mut Session, outcome: &AnalysisOutcome) {
    let inputs = session.inputs();
    let database = &inputs.database;
    let header = PackHeader::current(session.configuration.php_version_range(database));

    let mut trees: Vec<(ContentHash, StoredItemTree)> = session
        .sources
        .values()
        .map(|&file| {
            (
                celerrate_db::content_hash(database, file),
                StoredItemTree::of(celerrate_semantics::item_tree(database, file)),
            )
        })
        .collect();
    sort_entries(&mut trees);

    let panicked: BTreeSet<FileId> = outcome.panicked.iter().copied().collect();
    let mut verdicts: Vec<(ContentHash, StoredVerdict)> = Vec::new();
    for &file in inputs.reported.iter() {
        if panicked.contains(&file.file_id(database)) {
            continue;
        }
        let stored = match verdict::validated_verdict(&inputs, file) {
            Some(stored) => stored.clone(),
            None => composed_verdict(&inputs, file),
        };
        verdicts.push((celerrate_db::content_hash(database, file), stored));
    }
    sort_entries(&mut verdicts);

    if prepare_directory(&session.cache_directory).is_err() {
        return;
    }
    write_when_changed(
        &session.cache_directory.join(ITEM_TREES_PACK),
        &header,
        &trees,
        &session.cache.item_trees,
    );
    write_when_changed(
        &session.cache_directory.join(DIAGNOSTICS_PACK),
        &header,
        &verdicts,
        &session.cache.verdicts,
    );
    session.cache = Arc::new(CacheSnapshot {
        item_trees: trees.into_iter().collect(),
        verdicts: verdicts.into_iter().collect(),
    });
    session.cache_loaded_range = session.configuration.php_version_range(database);
}

/// One reported file's verdict, composed exactly as `analyze_one`
/// composes its diagnostics, with the records the entry must
/// revalidate against. Every query here is memoized from the pass.
fn composed_verdict(inputs: &AnalysisInputs, file: celerrate_db::SourceFile) -> StoredVerdict {
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
    let records = celerrate_semantics::resolution_records(
        database,
        file,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
    );
    StoredVerdict {
        diagnostics: diagnostics.iter().map(StoredDiagnostic::of).collect(),
        records: records.iter().map(StoredRecord::of).collect(),
    }
}

/// Deterministic pack order: by key, one entry per key.
fn sort_entries<Entry>(entries: &mut Vec<(ContentHash, Entry)>) {
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.dedup_by(|left, right| left.0 == right.0);
}

/// Creates the cache directory and its self-ignoring `.gitignore`.
fn prepare_directory(cache_directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_directory)?;
    if let Some(dot_celerrate) = cache_directory.parent() {
        let gitignore = dot_celerrate.join(".gitignore");
        if !gitignore.exists() {
            std::fs::write(gitignore, "*\n")?;
        }
    }
    Ok(())
}

/// Rewrites a pack only when its entries differ from the loaded state.
fn write_when_changed<Entry: Serialize + PartialEq>(
    path: &Path,
    header: &PackHeader,
    entries: &[(ContentHash, Entry)],
    loaded: &HashMap<ContentHash, Entry>,
) {
    let unchanged = entries.len() == loaded.len()
        && entries
            .iter()
            .all(|(key, value)| loaded.get(key) == Some(value));
    if unchanged && path.is_file() {
        return;
    }
    let Some(bytes) = pack::encode(&Pack {
        header: header.clone(),
        entries: entries.to_vec(),
    }) else {
        return;
    };
    let _ = pack::write_atomically(path, &bytes);
}
```

`entries.to_vec()` needs `Entry: Clone`; add the bound (`Entry: Serialize + PartialEq + Clone`). Both stored types derive `Clone`.

In `crates/celerrate_cli/src/lib.rs`, in `run`'s `Command::Check` arm, after the successful `render_check` and before `Outcome::of(...)`:

```rust
            cache::persist(&mut session, &outcome);
```

(the `let mut session` binding already exists; the render's error return stays above the persist, so a dead output stream skips the write too, harmlessly).

In `crates/celerrate_cli/src/watch.rs`, in the `watch` loop, immediately after the `render_cycle` early return:

```rust
        if render::render_cycle(output, session, &outcome, reanalyzed, started.elapsed()).is_err() {
            return Outcome::InternalError;
        }
        crate::cache::persist(session, &outcome);
```

- [ ] **Step 4: Run to verify it passes, then the workspace gates**

Run: `cargo test --package celerrate_cli`
Expected: PASS, including every pre-existing `check`, `registry`, and `watcher` test.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): persist the artifact packs after every completed pass"
```

---

### Task 10: The cross-process incremental harness

**Files:**
- Create: `crates/celerrate_cli/tests/cache_consistency.rs`

**Interfaces:**
- Consumes: `celerrate_cli::run` (the whole product as a function), Tasks 7-9.

This is the part's most critical test, and it is an extension of the existing harness discipline, not a new idea: after any edit, a run seeded from the persisted cache must render byte-for-byte what a from-scratch run renders. "Cross-process" is simulated by fresh `Session`s over fresh databases (each `run` call builds one), which exercises exactly the persistence boundary: nothing survives between runs except the packs on disk.

- [ ] **Step 1: Write the harness (it should pass immediately if Tasks 7-9 are correct; any failure is a real defect)**

Create `crates/celerrate_cli/tests/cache_consistency.rs`:

```rust
//! The cross-process extension of the incremental correctness harness:
//! edit sequences replayed over a project on disk, with every
//! cache-seeded run asserted byte-for-byte identical to a from-scratch
//! run over the same state. Nothing survives between runs except
//! `.celerrate/cache/`, which is exactly the boundary under test.

#![allow(clippy::unwrap_used)]

use std::path::Path;

use celerrate_cli::run;

fn run_check(root: &Path) -> String {
    let mut output = Vec::new();
    let _ = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
    );
    String::from_utf8(output).unwrap()
}

/// The rendering is root-relative, but notices and internal errors may
/// name absolute paths: normalize both roots to one marker before
/// comparing.
fn normalized(output: &str, root: &Path) -> String {
    output.replace(&root.display().to_string(), "<root>")
}

/// Copies the project, excluding the cache: the from-scratch twin.
fn copy_without_cache(source: &Path, destination: &Path) {
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == ".celerrate" {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_without_cache(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// One step of an edit sequence.
enum Step {
    Write(&'static str, &'static str),
    Delete(&'static str),
}

/// Replays the steps over one cached project directory; after the
/// initial state and after every step, the cached run must render what
/// a from-scratch run over a cache-free copy renders.
fn assert_cached_matches_fresh(initial: &[(&str, &str)], steps: &[Step]) {
    let cached = tempfile::tempdir().unwrap();
    for (path, contents) in initial {
        let path = cached.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    let mut assert_state_matches = |label: &str| {
        let cached_output = run_check(cached.path());
        let fresh = tempfile::tempdir().unwrap();
        copy_without_cache(cached.path(), fresh.path());
        let fresh_output = run_check(fresh.path());
        assert_eq!(
            normalized(&cached_output, cached.path()),
            normalized(&fresh_output, fresh.path()),
            "cached and from-scratch renderings diverged {label}",
        );
    };

    // The first run both checks the cold state and writes the cache;
    // the second checks the warm no-change state.
    assert_state_matches("on the cold state");
    assert_state_matches("on the warm unchanged state");

    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::Write(path, contents) => {
                let path = cached.path().join(path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, contents).unwrap();
            }
            Step::Delete(path) => {
                std::fs::remove_file(cached.path().join(path)).unwrap();
            }
        }
        assert_state_matches(&format!("after step {index}"));
    }
}

#[test]
fn body_and_comment_edits_replay_consistently() {
    assert_cached_matches_fresh(
        &[
            ("src/Service.php", "<?php class Service { public function run() { return 1; } }"),
            ("src/User.php", "<?php class User {}"),
        ],
        &[
            Step::Write(
                "src/Service.php",
                "<?php class Service { public function run() { return 2; } }",
            ),
            Step::Write(
                "src/Service.php",
                "<?php /* documented */ class Service { public function run() { return 2; } }",
            ),
        ],
    );
}

/// The stale-verdict trap in both directions: a cached unknown-symbol
/// diagnostic must die when a defining file appears, and come back
/// when it goes.
#[test]
fn a_definition_appearing_and_vanishing_replays_consistently() {
    assert_cached_matches_fresh(
        &[("src/Consumer.php", "<?php new Missing();")],
        &[
            Step::Write("src/Definer.php", "<?php class Missing {}"),
            Step::Delete("src/Definer.php"),
        ],
    );
}

/// A signature-level edit in one file must be seen by the cached
/// verdicts of another: renaming the declared class flips its
/// consumers' resolution.
#[test]
fn a_rename_in_another_file_replays_consistently() {
    assert_cached_matches_fresh(
        &[
            ("src/Consumer.php", "<?php new Widget();"),
            ("src/Widget.php", "<?php class Widget {}"),
        ],
        &[
            Step::Write("src/Widget.php", "<?php class Renamed {}"),
            Step::Write("src/Widget.php", "<?php class Widget {}"),
        ],
    );
}

/// Composer projects: a vendor file's symbols resolve from the cache
/// like from source, and vendor diagnostics stay unreported.
#[test]
fn a_composer_project_replays_consistently() {
    assert_cached_matches_fresh(
        &[
            (
                "composer.json",
                r#"{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            (
                "vendor/lib/src/Helper.php",
                "<?php namespace Lib; class Helper { public function broken( }",
            ),
            (
                "vendor/composer/installed.json",
                r#"{"packages": []}"#,
            ),
            (
                "src/App.php",
                "<?php namespace App; use Lib\\Helper; new Helper();",
            ),
        ],
        &[Step::Write(
            "src/App.php",
            "<?php namespace App; use Lib\\Helper; new Helper(); new Gone();",
        )],
    );
}

/// Every corruption mode of a pack on disk regenerates silently: the
/// run's rendering never changes.
#[test]
fn corrupted_packs_never_change_the_rendering() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.php"), "<?php new Missing();").unwrap();
    let baseline = normalized(&run_check(root.path()), root.path());

    let cache = root.path().join(".celerrate/cache");
    for pack in ["item_trees.bin", "diagnostics.bin"] {
        let path = cache.join(pack);
        let original = std::fs::read(&path).unwrap();

        // Truncated.
        std::fs::write(&path, &original[..original.len() / 2]).unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

        // Garbage.
        std::fs::write(&path, b"not a pack at all").unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

        // A flipped byte deep in the payload.
        let mut flipped = std::fs::read(&path).unwrap();
        if let Some(last) = flipped.last_mut() {
            *last ^= 0xFF;
        }
        std::fs::write(&path, &flipped).unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
    }

    // After all that abuse the packs are healthy again: one more
    // clean pair of runs.
    assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
}
```

Add `#![allow(clippy::indexing_slicing)]` at file level if the slice in the truncation step trips the lint.

The vendor fixture: if `ProjectDiscovery` requires more of `vendor/composer/installed.json` than an empty package list for the vendor tree to be walked, mirror whatever shape `crates/celerrate_project` tests use for an installed dependency; the intent of the fixture is a vendor file that both defines a resolvable symbol and contains a syntax error that must stay unreported.

- [ ] **Step 2: Run the harness**

Run: `cargo test --package celerrate_cli --test cache_consistency`
Expected: PASS. Any failure here is a correctness defect in Tasks 7-9 — a divergence between the cached and honest runs — and must be fixed before this task is complete, not accommodated in the test.

- [ ] **Step 3: Run the workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_cli/tests/cache_consistency.rs
git commit -m "✅ test(cli): extend the incremental harness across processes"
```

---

### Task 11: The syntax-tree retention measurement

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md` (amendment recording the numbers and the decision)
- Possibly modify: `crates/celerrate_db/src/queries.rs` (only if the decision is to evict)

**Interfaces:**
- Consumes: the finished cache (Tasks 1-10), a locally cloned symfony/demo.

This closes the debt part 4 recorded: LRU eviction of syntax trees was deferred to part 8, where the memory economics are measured alongside the persistent cache. The outcome is a measured decision recorded in the spec, either way.

- [ ] **Step 1: Prepare the corpus (local, ad hoc — pinning is part 8b's job)**

```bash
git clone --depth 1 https://github.com/symfony/demo /tmp/claude/symfony-demo
cd /tmp/claude/symfony-demo && composer install --no-scripts --ignore-platform-reqs
```

If `composer` is unavailable locally, say so and ask how to proceed rather than substituting a synthetic corpus: the spec names the corpus deliberately.

- [ ] **Step 2: Measure the retained build**

```bash
cd <repository root>
cargo build --release
/usr/bin/time -v ./target/release/celerrate check /tmp/claude/symfony-demo 2>&1 | grep -E "Maximum resident|Elapsed"
# Run twice more; keep the median. Then the warm run (cache present):
/usr/bin/time -v ./target/release/celerrate check /tmp/claude/symfony-demo 2>&1 | grep -E "Maximum resident|Elapsed"
```

Record: cold peak RSS, warm peak RSS, cold and warm wall time, corpus size (`find /tmp/claude/symfony-demo -name '*.php' | wc -l`).

- [ ] **Step 3: Measure the evicting build**

Apply this local, uncommitted patch to `crates/celerrate_db/src/queries.rs` — `lru` caps the memoized parses; the attribute cannot keep `returns(ref)`, so the two call sites in this crate change from `parse(db, file).tree()` to owned access, and `crates/celerrate_semantics` has three call sites (`queries.rs`, `reference_checks.rs`, `revalidation.rs`) plus `syntax_gating.rs` if it parses — the compiler lists them:

```rust
#[salsa::tracked(lru = 64)]
pub fn parse(db: &dyn salsa::Database, file: SourceFile) -> Parse {
    ...unchanged body...
}
```

Rebuild and repeat Step 2's measurements on the same corpus.

- [ ] **Step 4: Decide by the rule, then restore or keep**

The decision rule, fixed here so the measurement cannot be argued with after the fact:

- Retention stands if the retained build's cold peak RSS on symfony/demo is at most 1.5 GiB **and** at most 2x the evicting build's.
- Otherwise eviction wins: keep the patch (tuning the capacity among 16/64/256 for the smallest wall-time regression, which must stay under 10% cold), make the workspace green, and re-run the whole cache-consistency harness.

If retention stands, revert the Step 3 patch (`git checkout -- crates`).

- [ ] **Step 5: Record the amendment**

Append to the amendment history header of `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md` (create the `Amendment history:` block under the `Status:` line, following the parent spec's convention):

```markdown
Amendment history:

- YYYY-MM-DD — syntax-tree retention measured on symfony/demo
  (N PHP files): retained build peaked at X MiB cold / Y MiB warm,
  evicting build (lru = 64 on parse) at Z MiB cold / W MiB warm, wall
  time A s versus B s. Decision: [retention stands, no mechanism |
  parse is capped at lru = N], by the rule the part 8a plan fixed.
```

with the real numbers and today's date.

- [ ] **Step 6: Verify and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: PASS.

```bash
git add .claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md
git commit -m "📝 docs(specs): record the syntax-tree retention measurement"
# Only if eviction won:
# git add crates
# git commit -m "⚡️ perf(db): cap memoized parses under an LRU"
```

Note: the spec file sits under `.claude/`, which the user's global gitignore covers; `git add -f` it, as every spec commit in this repository does.

---

## Done means

- A second `celerrate check` over an unchanged project renders byte-for-byte what the first did, serving item trees and verdicts from `.celerrate/cache/` without re-lowering or re-checking unchanged files.
- The cross-process harness passes: every edit scenario (body, comment, rename-across-files, definition appearing and vanishing, Composer project) renders identically from cache and from scratch.
- Every corruption mode of a pack regenerates silently; no cache condition is ever visible in the output or the exit code.
- A panicked file's verdict is never persisted; installed dependencies are indexed from cache but never reported.
- The syntax-tree retention decision is measured on symfony/demo and recorded in the spec.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, and `cargo deny check` all pass.
