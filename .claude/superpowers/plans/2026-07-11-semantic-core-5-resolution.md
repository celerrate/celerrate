# Semantic Core Part 5: Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The global symbol index (project and vendor item trees plus the
version-filtered stubs, under case-folded keys) and top-level PHP name
resolution (fully qualified, relative qualified, and unqualified names with
the real per-space fallback rules), with per-name lookup queries proving the
invalidation firewall: adding a symbol in one file never re-runs the
consumers of files that do not reference it.

**Architecture:** Per the spec
`.claude/superpowers/specs/2026-07-11-semantic-core-design.md` (section 6,
"The symbol index" and "Name resolution"). Decisions fixed here:

- **One logical index, two physical tables.** The source table
  (`source_symbol_table`, over the analyzed file set's item trees) and the
  stub table (`stub_symbol_table`, over the version-filtered
  `stubs_in_range` view) stay separate; the per-name lookup consults the
  source table first, then the stubs. A physical merge would re-copy the
  ~100k stub entries every time a project signature changes; the split gives
  the same observable index (project declarations shadow stubs) with the
  stub side recomputed only on configuration or stub changes.
- **The per-name firewall is a tracked lookup query keyed by an interned
  name.** `lookup_symbol(db, files, stubs, configuration, query)` takes an
  interned `SymbolQuery` (symbol space plus pre-folded key). When any file's
  signatures change, the table query re-runs, every cached `lookup_symbol`
  re-runs (a cheap binary search), and salsa backdates the lookups whose
  answer did not change: consumers behind them are spared. This is exactly
  the spec's "lookups go through per-name queries, not through a dependency
  on the whole index". Salsa 0.27 supports interned structs
  (`#[salsa::interned]`, `'db` lifetime) and tracked functions mixing input
  and interned arguments (verified against the vendored salsa 0.27.2 tests).
- **Case folding is ASCII, per space.** Class-likes and functions fold the
  whole fully qualified name with `to_ascii_lowercase` (PHP's engine folds
  ASCII only); constants fold their namespace segments and keep the terminal
  segment case-sensitive (namespaces are always case-insensitive in PHP,
  only the constant's own name is not). Entries retain the original spelling
  (the "did you mean" diagnostics of sub-project 4 will need it).
- **Three symbol spaces.** `SymbolSpace { ClassLike, Function, Constant }`:
  classes, interfaces, traits, and enums share one space (PHP resolves them
  through one table); functions and constants each have their own.
- **The analyzed file set is a base-db input.** `AnalyzedFileSet` (a
  `Vec<SourceFile>`) joins `celerrate_db`: the spec's section 2 lists it
  among the base-db inputs and its field type lives there. Membership
  changes invalidate whole-project queries; editing one member's bytes never
  touches the set itself. Project-versus-vendor durability is per
  `SourceFile`, set by the composition root (part 7): lookups do not care.
- **Resolution context is (namespace, use tables).** `UseTables` groups a
  file's imports by their `ItemTree` namespace field (the whole namespace
  block sees its imports, position within the block does not matter). Class
  and function aliases match case-insensitively (folded map keys); constant
  aliases match case-sensitively (verbatim keys). A duplicate alias keeps
  the last import (PHP fatals on it; tolerance picks a deterministic
  winner).
- **Candidate lists implement the real PHP rules.** A leading `\` is
  absolute. A `namespace\` first segment is relative to the current
  namespace. A qualified name resolves its first segment through the class
  use table (aliases name classes or namespaces), else prefixes the current
  namespace, and never falls back to the global namespace. An unqualified
  name resolves through its own space's use table (an import wins outright),
  else the current namespace; classes have no global fallback, functions and
  constants fall back to the global namespace.
- **Duplicates resolve to the deterministic first entry.** Tables are
  sorted by (space, key) with declaration identity breaking ties (file set
  order, then tree order); lookup answers the first entry.
  Duplicate-declaration diagnostics are later work.
- **The boundary holds.** Everything in this part reads item trees and the
  stub index, never another file's syntax tree. The section 6 syntax-gating
  exception is not exercised here (it belongs to part 6).

Out of scope (later parts): reference collection and the unknown-symbol /
version-gating diagnostics with their `CEL####` identifiers (part 6), the
CLI and parallel fan-out (part 7), the persistent cache and LRU eviction
(part 8). Inheritance name *resolution semantics* (linearization) stay
sub-project 3; this part only resolves names to declared symbols.

**Tech Stack:** Rust edition 2024, salsa 0.27 (existing workspace
dependency). Existing crates: `celerrate_source`, `celerrate_syntax`,
`celerrate_db` (instrumented `TestDatabase`, consistency harness),
`celerrate_project` (`ProjectConfiguration`, `PhpVersion`,
`PhpVersionRange`), `celerrate_stubs` (`StubIndexInput`, `StubIndex`,
`StubSymbol`, `StubSymbolKind`, `StubAvailability`, `stubs_in_range`),
`celerrate_semantics` (`ItemTree`, `Declaration`, `DeclarationKind`,
`UseImport`, `ImportKind`, `AstId`, `item_tree` query). No new external
dependencies anywhere.

## Global Constraints

- Zero panic, mechanically enforced: the workspace denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` is forbidden.
  Only test modules may `#[allow]` / `#![allow]` these lints. Production
  code uses `.get()`, `partition_point`, `unwrap_or_default`, never
  indexing.
- Strict layering, DAG with no upward edges: `celerrate_semantics` gains
  dependencies on `celerrate_project` and `celerrate_stubs` (both below it
  in the parent spec's layout); `celerrate_db` gains nothing and must never
  know about semantics.
- Error resilience: malformed input resolves to whatever the item tree
  carries; empty or wreckage names produce empty candidate lists, never
  failures.
- Determinism: all tables are sorted, all queries are pure functions of
  their inputs; no wall-clock time, no randomness, no environment reads
  inside queries.
- TDD throughout: every step of behavior starts from a failing test.
- Everything in English, full words, no abbreviated names (standard
  acronyms fine).
- Commits: gitmoji + Conventional Commits, repository-configured identity,
  no Claude attribution anywhere.
- Local commands that must stay green after every task:
  `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `cargo deny check`.

## File Structure

```
crates/celerrate_db/src/input.rs         AnalyzedFileSet input (task 1)
crates/celerrate_db/src/lib.rs           export AnalyzedFileSet (task 1)
crates/celerrate_db/src/testing.rs       state-level harness form (task 8)
crates/celerrate_semantics/Cargo.toml    + celerrate_project, celerrate_stubs (task 4)
crates/celerrate_semantics/src/lib.rs    module wiring and exports (tasks 2-7)
crates/celerrate_semantics/src/symbols.rs   SymbolSpace, folding, FQN join (tasks 2, 4)
crates/celerrate_semantics/src/index.rs     SymbolTable, StubSymbolTable, table queries (tasks 3, 4)
crates/celerrate_semantics/src/lookup.rs    SymbolQuery, SymbolResolution, lookup_symbol (task 5)
crates/celerrate_semantics/src/resolve.rs   UseTables, resolve_candidates, SymbolSources, resolve_name (tasks 6, 7)
crates/celerrate_semantics/tests/invalidation_scope.rs   the firewall matrix (task 7)
crates/celerrate_semantics/tests/incremental_consistency.rs  resolution replay (task 8)
.claude/superpowers/specs/2026-07-11-semantic-core-design.md  narrowings (task 9)
```

---

### Task 1: The analyzed file set input

**Files:**
- Modify: `crates/celerrate_db/src/input.rs`
- Modify: `crates/celerrate_db/src/lib.rs`

**Interfaces:**
- Consumes: `SourceFile` (existing salsa input in the same file).
- Produces: `AnalyzedFileSet` salsa input with `files(db) -> &Vec<SourceFile>`,
  `AnalyzedFileSet::new(db, Vec<SourceFile>)`, `set_files(&mut db)`.
  Task 3's `source_symbol_table` iterates it; tasks 5-8 thread it through
  lookups.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module of `crates/celerrate_db/src/input.rs`:

```rust
    use crate::AnalyzedFileSet;

    #[test]
    fn the_analyzed_file_set_stores_and_updates_its_files() {
        let mut db = TestDatabase::default();
        let first = SourceFile::new(&db, FileId::new(0), b"<?php".to_vec());
        let second = SourceFile::new(&db, FileId::new(1), b"<?php".to_vec());
        let set = AnalyzedFileSet::new(&db, vec![first]);
        assert_eq!(set.files(&db), &vec![first]);

        set.set_files(&mut db).to(vec![first, second]);
        assert_eq!(set.files(&db), &vec![first, second]);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p celerrate_db the_analyzed_file_set -- --nocapture`
Expected: compilation FAILS with `cannot find type AnalyzedFileSet`.

- [ ] **Step 3: Write the minimal implementation**

Append to `crates/celerrate_db/src/input.rs` (below `SourceFile`, above the
`tests` module):

```rust
/// The analyzed file set: every file the current analysis covers, in
/// the deterministic order the composition root established. Whole-
/// project queries depend on this input, so membership changes (a file
/// created or deleted) invalidate them; editing one member's bytes
/// changes that file's input, never the set itself.
#[salsa::input]
pub struct AnalyzedFileSet {
    #[returns(ref)]
    pub files: Vec<SourceFile>,
}
```

Update the export line in `crates/celerrate_db/src/lib.rs`:

```rust
pub use input::{AnalyzedFileSet, SourceFile};
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p celerrate_db the_analyzed_file_set`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_db/src/input.rs crates/celerrate_db/src/lib.rs
git commit -m "✨ feat(db): add the analyzed file set input"
```

---

### Task 2: Symbol spaces and case-folded keys

**Files:**
- Create: `crates/celerrate_semantics/src/symbols.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `DeclarationKind` from `crate::items`.
- Produces:
  - `SymbolSpace` enum (`ClassLike`, `Function`, `Constant`), deriving
    `Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord`.
  - `SymbolSpace::of_declaration(kind: DeclarationKind) -> SymbolSpace`.
  - `fully_qualified_name(namespace: &str, name: &str) -> String`.
  - `folded_symbol_key(space: SymbolSpace, fully_qualified: &str) -> String`.

  (`SymbolSpace::of_stub_kind` joins in task 4, when the stubs dependency
  arrives.)

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/symbols.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::{SymbolSpace, folded_symbol_key, fully_qualified_name};
    use crate::items::DeclarationKind;

    #[test]
    fn every_declaration_kind_maps_to_its_space() {
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Class),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Interface),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Trait),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Enum),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Function),
            SymbolSpace::Function,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Constant),
            SymbolSpace::Constant,
        );
    }

    #[test]
    fn a_fully_qualified_name_joins_namespace_and_name() {
        assert_eq!(fully_qualified_name("", "Service"), "Service");
        assert_eq!(
            fully_qualified_name("App\\Domain", "Service"),
            "App\\Domain\\Service",
        );
    }

    #[test]
    fn class_and_function_keys_fold_the_whole_name() {
        assert_eq!(
            folded_symbol_key(SymbolSpace::ClassLike, "App\\Service"),
            "app\\service",
        );
        assert_eq!(
            folded_symbol_key(SymbolSpace::Function, "App\\Greet"),
            "app\\greet",
        );
    }

    #[test]
    fn constant_keys_keep_their_terminal_segment() {
        // Namespaces are case-insensitive even for constants; only the
        // constant's own name keeps its case.
        assert_eq!(
            folded_symbol_key(SymbolSpace::Constant, "App\\Sub\\Limit"),
            "app\\sub\\Limit",
        );
        assert_eq!(folded_symbol_key(SymbolSpace::Constant, "E_ALL"), "E_ALL");
    }

    #[test]
    fn folding_is_ascii_only() {
        // PHP's engine folds ASCII only; multibyte spellings stay
        // distinct.
        assert_eq!(
            folded_symbol_key(SymbolSpace::ClassLike, "App\\Éxception"),
            "app\\Éxception",
        );
    }
}
```

Wire the module in `crates/celerrate_semantics/src/lib.rs`:

```rust
mod ast_id;
mod item_nodes;
mod items;
mod queries;
mod symbols;

pub use ast_id::{AstId, AstIdMap};
pub use items::{Declaration, DeclarationKind, ImportKind, ItemTree, UseImport};
pub use queries::{ast_id_map, item_tree};
pub use symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics symbols`
Expected: compilation FAILS with `cannot find type SymbolSpace` (and the
missing functions).

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_semantics/src/symbols.rs` (above the tests):

```rust
//! The three PHP symbol spaces and the case-folded lookup keys of the
//! global symbol index. PHP resolves class and function names
//! case-insensitively and constant names case-sensitively; namespaces
//! are case-insensitive everywhere. Folding is ASCII-only, matching
//! the engine's own folding.

use crate::items::DeclarationKind;

/// The symbol space a name resolves in. Classes, interfaces, traits,
/// and enums share one space; functions and constants each have their
/// own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SymbolSpace {
    ClassLike,
    Function,
    Constant,
}

impl SymbolSpace {
    /// The space a declared symbol occupies.
    pub fn of_declaration(kind: DeclarationKind) -> Self {
        match kind {
            DeclarationKind::Class
            | DeclarationKind::Interface
            | DeclarationKind::Trait
            | DeclarationKind::Enum => Self::ClassLike,
            DeclarationKind::Function => Self::Function,
            DeclarationKind::Constant => Self::Constant,
        }
    }
}

/// Joins a namespace (`""` is global) and a name into the fully
/// qualified spelling, without a leading backslash.
pub fn fully_qualified_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}\\{name}")
    }
}

/// The case-folded lookup key of one fully qualified name: the whole
/// name for class-likes and functions, the namespace segments only for
/// constants (their terminal segment is case-sensitive).
pub fn folded_symbol_key(space: SymbolSpace, fully_qualified: &str) -> String {
    match space {
        SymbolSpace::ClassLike | SymbolSpace::Function => fully_qualified.to_ascii_lowercase(),
        SymbolSpace::Constant => match fully_qualified.rsplit_once('\\') {
            Some((namespace, name)) => format!("{}\\{name}", namespace.to_ascii_lowercase()),
            None => fully_qualified.to_owned(),
        },
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics symbols`
Expected: 5 tests PASS.

- [ ] **Step 5: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_semantics/src/symbols.rs crates/celerrate_semantics/src/lib.rs
git commit -m "✨ feat(semantics): add symbol spaces and case-folded keys"
```

---

### Task 3: The source symbol table

**Files:**
- Create: `crates/celerrate_semantics/src/index.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `AnalyzedFileSet` (task 1), `SymbolSpace`, `folded_symbol_key`,
  `fully_qualified_name` (task 2), `item_tree` query, `Declaration`,
  `DeclarationKind`, `AstId` (existing).
- Produces:
  - `SymbolEntry { space: SymbolSpace, key: String, original: String, kind: DeclarationKind, ast_id: AstId }`
    deriving `Debug, Clone, PartialEq, Eq, Hash`.
  - `SymbolTable` with `from_entries(Vec<SymbolEntry>) -> SymbolTable`,
    `lookup(&self, space: SymbolSpace, key: &str) -> Option<&SymbolEntry>`,
    `entries(&self) -> &[SymbolEntry]`; derives
    `Debug, Clone, Default, PartialEq, Eq`.
  - Tracked query
    `source_symbol_table(db: &dyn salsa::Database, files: AnalyzedFileSet) -> &SymbolTable`
    (`returns(ref)`).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/index.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_source::FileId;
    use salsa::Setter;

    use super::{SymbolTable, source_symbol_table};
    use crate::items::DeclarationKind;
    use crate::symbols::SymbolSpace;

    fn set_of(db: &TestDatabase, sources: &[&str]) -> AnalyzedFileSet {
        let files: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        AnalyzedFileSet::new(db, files)
    }

    fn table_of(sources: &[&str]) -> SymbolTable {
        let db = TestDatabase::default();
        source_symbol_table(&db, set_of(&db, sources)).clone()
    }

    #[test]
    fn every_declaration_is_indexed_under_its_folded_key() {
        let table = table_of(&[
            "<?php namespace App; class Service {} function greet() {} const LIMIT = 1;",
        ]);
        let class = table.lookup(SymbolSpace::ClassLike, "app\\service").unwrap();
        assert_eq!(class.original, "App\\Service");
        assert_eq!(class.kind, DeclarationKind::Class);
        assert_eq!(class.ast_id.file, FileId::new(0));

        let function = table.lookup(SymbolSpace::Function, "app\\greet").unwrap();
        assert_eq!(function.original, "App\\greet");

        let constant = table.lookup(SymbolSpace::Constant, "app\\LIMIT").unwrap();
        assert_eq!(constant.kind, DeclarationKind::Constant);
    }

    #[test]
    fn constant_lookups_are_case_sensitive() {
        let table = table_of(&["<?php namespace App; const LIMIT = 1;"]);
        assert!(table.lookup(SymbolSpace::Constant, "app\\limit").is_none());
        assert!(table.lookup(SymbolSpace::Constant, "app\\LIMIT").is_some());
    }

    #[test]
    fn spaces_never_bleed_into_each_other() {
        let table = table_of(&["<?php function shared() {} class Shared {}"]);
        assert_eq!(
            table.lookup(SymbolSpace::Function, "shared").map(|e| e.kind),
            Some(DeclarationKind::Function),
        );
        assert_eq!(
            table.lookup(SymbolSpace::ClassLike, "shared").map(|e| e.kind),
            Some(DeclarationKind::Class),
        );
        assert!(table.lookup(SymbolSpace::Constant, "shared").is_none());
    }

    #[test]
    fn duplicates_answer_the_first_declaration_in_set_order() {
        let table = table_of(&[
            "<?php namespace App; class Service {}",
            "<?php namespace App; class SERVICE {}",
        ]);
        let entry = table.lookup(SymbolSpace::ClassLike, "app\\service").unwrap();
        assert_eq!(entry.ast_id.file, FileId::new(0));
        assert_eq!(entry.original, "App\\Service");
    }

    #[test]
    fn entries_are_sorted_deterministically() {
        let table = table_of(&[
            "<?php class Zulu {} class Alpha {}",
            "<?php function zulu() {} const A = 1;",
        ]);
        let keys: Vec<(SymbolSpace, &str)> = table
            .entries()
            .iter()
            .map(|entry| (entry.space, entry.key.as_str()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn an_unknown_key_answers_none() {
        let table = table_of(&["<?php class Known {}"]);
        assert!(table.lookup(SymbolSpace::ClassLike, "unknown").is_none());
    }

    #[test]
    fn a_body_edit_never_rebuilds_the_table() {
        // The item tree's early cutoff pays off here: the table depends
        // on item trees only, and a body edit backdates the tree.
        let mut db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php function greet() { return 1; }".to_vec(),
        );
        let set = AnalyzedFileSet::new(&db, vec![file]);
        let _ = source_symbol_table(&db, set);
        db.take_executed();

        file.set_bytes(&mut db)
            .to(b"<?php function greet() { return 2; }".to_vec());
        let _ = source_symbol_table(&db, set);
        let log = db.take_executed();
        assert!(
            log.iter().any(|entry| entry.starts_with("item_tree")),
            "the edited file reprojects: {log:?}",
        );
        assert!(
            !log.iter()
                .any(|entry| entry.starts_with("source_symbol_table")),
            "a body edit must never rebuild the table: {log:?}",
        );
    }

    #[test]
    fn adding_a_file_to_the_set_rebuilds_the_table() {
        let mut db = TestDatabase::default();
        let first = SourceFile::new(&db, FileId::new(0), b"<?php class A {}".to_vec());
        let set = AnalyzedFileSet::new(&db, vec![first]);
        assert_eq!(source_symbol_table(&db, set).entries().len(), 1);

        let second = SourceFile::new(&db, FileId::new(1), b"<?php class B {}".to_vec());
        set.set_files(&mut db).to(vec![first, second]);
        assert_eq!(source_symbol_table(&db, set).entries().len(), 2);
    }
}
```

Wire the module in `crates/celerrate_semantics/src/lib.rs` (add to both
lists):

```rust
mod index;
```

```rust
pub use index::{SymbolEntry, SymbolTable, source_symbol_table};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics index`
Expected: compilation FAILS with `cannot find type SymbolTable` (and the
missing query).

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_semantics/src/index.rs` (above the tests):

```rust
//! The source half of the global symbol index: every declaration of
//! the analyzed file set under its case-folded key. The table is built
//! from item trees only (the invalidation boundary holds), sorted and
//! `Eq`-comparable so consumers backdate; lookups themselves go
//! through the per-name query in `crate::lookup`, never through a
//! direct dependency on the whole table.

use celerrate_db::AnalyzedFileSet;

use crate::ast_id::AstId;
use crate::items::DeclarationKind;
use crate::queries::item_tree;
use crate::symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};

/// One declared symbol: its lookup key, the original spelling (the
/// "did you mean" diagnostics of sub-project 4 will need it), and the
/// declaration it names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolEntry {
    pub space: SymbolSpace,
    pub key: String,
    pub original: String,
    pub kind: DeclarationKind,
    pub ast_id: AstId,
}

/// The declared symbols of the analyzed file set, sorted by
/// (space, key) with declaration identity breaking ties: duplicates
/// keep a deterministic order and lookup answers the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolTable {
    entries: Vec<SymbolEntry>,
}

impl SymbolTable {
    /// Builds the table: a deterministic sort, duplicates retained.
    pub fn from_entries(mut entries: Vec<SymbolEntry>) -> Self {
        entries.sort_by(|left, right| {
            (left.space, left.key.as_str(), left.ast_id, left.original.as_str()).cmp(&(
                right.space,
                right.key.as_str(),
                right.ast_id,
                right.original.as_str(),
            ))
        });
        Self { entries }
    }

    /// The first entry under `key` in `space`, when one exists.
    pub fn lookup(&self, space: SymbolSpace, key: &str) -> Option<&SymbolEntry> {
        let start = self
            .entries
            .partition_point(|entry| (entry.space, entry.key.as_str()) < (space, key));
        self.entries
            .get(start)
            .filter(|entry| entry.space == space && entry.key == key)
    }

    pub fn entries(&self) -> &[SymbolEntry] {
        &self.entries
    }
}

/// The source symbol table of the analyzed file set. Depends on every
/// member's item tree: a signature change anywhere rebuilds it (the
/// merge is cheap), and the per-name lookups behind it backdate for
/// every name whose answer did not change.
#[salsa::tracked(returns(ref))]
pub fn source_symbol_table(db: &dyn salsa::Database, files: AnalyzedFileSet) -> SymbolTable {
    let mut entries = Vec::new();
    for &file in files.files(db) {
        for declaration in &item_tree(db, file).declarations {
            let space = SymbolSpace::of_declaration(declaration.kind);
            let original = fully_qualified_name(&declaration.namespace, &declaration.name);
            entries.push(SymbolEntry {
                space,
                key: folded_symbol_key(space, &original),
                original,
                kind: declaration.kind,
                ast_id: declaration.ast_id,
            });
        }
    }
    SymbolTable::from_entries(entries)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics index`
Expected: 8 tests PASS.

- [ ] **Step 5: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_semantics/src/index.rs crates/celerrate_semantics/src/lib.rs
git commit -m "✨ feat(semantics): index source declarations by folded key"
```

---

### Task 4: The stub symbol table

**Files:**
- Modify: `crates/celerrate_semantics/Cargo.toml`
- Modify: `crates/celerrate_semantics/src/symbols.rs`
- Modify: `crates/celerrate_semantics/src/index.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `StubIndexInput`, `StubIndex`, `StubSymbol`, `StubSymbolKind`,
  `StubAvailability`, `stubs_in_range` from `celerrate_stubs`;
  `ProjectConfiguration`, `PhpVersion`, `PhpVersionRange` from
  `celerrate_project`.
- Produces:
  - `SymbolSpace::of_stub_kind(kind: StubSymbolKind) -> SymbolSpace`.
  - `StubSymbolEntry { space: SymbolSpace, key: String, symbol: StubSymbol }`
    deriving `Debug, Clone, PartialEq, Eq`.
  - `StubSymbolTable` with `from_entries`, `lookup`, `entries` (same shapes
    as `SymbolTable`); derives `Debug, Clone, Default, PartialEq, Eq`.
  - Tracked query
    `stub_symbol_table(db, stubs: StubIndexInput, configuration: ProjectConfiguration) -> &StubSymbolTable`
    (`returns(ref)`).

- [ ] **Step 1: Add the dependencies**

In `crates/celerrate_semantics/Cargo.toml`, extend `[dependencies]` (keep
alphabetical order):

```toml
[dependencies]
celerrate_db = { path = "../celerrate_db" }
celerrate_project = { path = "../celerrate_project" }
celerrate_source = { path = "../celerrate_source" }
celerrate_stubs = { path = "../celerrate_stubs" }
celerrate_syntax = { path = "../celerrate_syntax" }
salsa = { workspace = true }
```

- [ ] **Step 2: Write the failing tests**

Append to the `tests` module of `crates/celerrate_semantics/src/symbols.rs`:

```rust
    use celerrate_stubs::StubSymbolKind;

    #[test]
    fn every_stub_kind_maps_to_its_space() {
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Class),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Interface),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Trait),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Enum),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Function),
            SymbolSpace::Function,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Constant),
            SymbolSpace::Constant,
        );
    }
```

Append to the `tests` module of `crates/celerrate_semantics/src/index.rs`:

```rust
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::stub_symbol_table;

    fn stub_input(db: &TestDatabase) -> StubIndexInput {
        let index = StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Random\\Randomizer".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 2)),
                    ..StubAvailability::ALWAYS
                },
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "E_ALL".to_owned(),
                kind: StubSymbolKind::Constant,
                availability: StubAvailability::ALWAYS,
            },
        ]);
        StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(db)
    }

    fn configuration_of(db: &TestDatabase, minimum: (u8, u8), maximum: (u8, u8)) -> ProjectConfiguration {
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(minimum.0, minimum.1),
            PhpVersion::new(maximum.0, maximum.1),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db)
    }

    #[test]
    fn stub_symbols_are_indexed_under_their_folded_keys() {
        let db = TestDatabase::default();
        let table = stub_symbol_table(&db, stub_input(&db), configuration_of(&db, (8, 1), (8, 5)));
        let class = table
            .lookup(SymbolSpace::ClassLike, "random\\randomizer")
            .unwrap();
        assert_eq!(class.symbol.name, "Random\\Randomizer");
        assert_eq!(
            class.symbol.availability.introduced,
            Some(PhpVersion::new(8, 2)),
        );
        assert!(table.lookup(SymbolSpace::Function, "strlen").is_some());
        assert!(table.lookup(SymbolSpace::Constant, "E_ALL").is_some());
        assert!(
            table.lookup(SymbolSpace::Constant, "e_all").is_none(),
            "constants stay case-sensitive",
        );
    }

    #[test]
    fn the_table_respects_the_version_range() {
        let mut db = TestDatabase::default();
        let stubs = stub_input(&db);
        let configuration = configuration_of(&db, (8, 0), (8, 1));
        let table = stub_symbol_table(&db, stubs, configuration);
        assert!(
            table
                .lookup(SymbolSpace::ClassLike, "random\\randomizer")
                .is_none(),
            "introduced after the maximum: filtered out",
        );

        configuration
            .set_php_version_range(&mut db)
            .to(PhpVersionRange::new(
                PhpVersion::new(8, 1),
                PhpVersion::new(8, 5),
            ));
        let widened = stub_symbol_table(&db, stubs, configuration);
        assert!(
            widened
                .lookup(SymbolSpace::ClassLike, "random\\randomizer")
                .is_some(),
        );
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics stub`
Expected: compilation FAILS with `no function of_stub_kind` /
`cannot find stub_symbol_table`.

- [ ] **Step 4: Write the minimal implementation**

In `crates/celerrate_semantics/src/symbols.rs`, add below
`of_declaration` (inside `impl SymbolSpace`) and add the import:

```rust
use celerrate_stubs::StubSymbolKind;
```

```rust
    /// The space a compiled stub symbol occupies.
    pub fn of_stub_kind(kind: StubSymbolKind) -> Self {
        match kind {
            StubSymbolKind::Class
            | StubSymbolKind::Interface
            | StubSymbolKind::Trait
            | StubSymbolKind::Enum => Self::ClassLike,
            StubSymbolKind::Function => Self::Function,
            StubSymbolKind::Constant => Self::Constant,
        }
    }
```

In `crates/celerrate_semantics/src/index.rs`, add the imports and append
below `source_symbol_table`:

```rust
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::{StubIndexInput, StubSymbol, stubs_in_range};
```

```rust
/// One stub symbol under its lookup key. The whole `StubSymbol` rides
/// along: part 6's version gating reads its availability window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubSymbolEntry {
    pub space: SymbolSpace,
    pub key: String,
    pub symbol: StubSymbol,
}

/// The stub half of the global symbol index: the version-filtered stub
/// view under case-folded keys. Rebuilt only when the stubs or the
/// configuration change, never on a source edit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StubSymbolTable {
    entries: Vec<StubSymbolEntry>,
}

impl StubSymbolTable {
    /// Builds the table: a deterministic sort, duplicates retained.
    pub fn from_entries(mut entries: Vec<StubSymbolEntry>) -> Self {
        entries.sort_by(|left, right| {
            (left.space, left.key.as_str(), left.symbol.name.as_str(), left.symbol.kind).cmp(&(
                right.space,
                right.key.as_str(),
                right.symbol.name.as_str(),
                right.symbol.kind,
            ))
        });
        Self { entries }
    }

    /// The first entry under `key` in `space`, when one exists.
    pub fn lookup(&self, space: SymbolSpace, key: &str) -> Option<&StubSymbolEntry> {
        let start = self
            .entries
            .partition_point(|entry| (entry.space, entry.key.as_str()) < (space, key));
        self.entries
            .get(start)
            .filter(|entry| entry.space == space && entry.key == key)
    }

    pub fn entries(&self) -> &[StubSymbolEntry] {
        &self.entries
    }
}

/// The stub symbol table over the version-filtered view.
#[salsa::tracked(returns(ref))]
pub fn stub_symbol_table(
    db: &dyn salsa::Database,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> StubSymbolTable {
    StubSymbolTable::from_entries(
        stubs_in_range(db, stubs, configuration)
            .symbols()
            .iter()
            .map(|symbol| {
                let space = SymbolSpace::of_stub_kind(symbol.kind);
                StubSymbolEntry {
                    space,
                    key: folded_symbol_key(space, &symbol.name),
                    symbol: symbol.clone(),
                }
            })
            .collect(),
    )
}
```

Update the exports in `crates/celerrate_semantics/src/lib.rs`:

```rust
pub use index::{
    StubSymbolEntry, StubSymbolTable, SymbolEntry, SymbolTable, source_symbol_table,
    stub_symbol_table,
};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics`
Expected: all tests PASS (including the new `stub` ones).

- [ ] **Step 6: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_semantics/Cargo.toml crates/celerrate_semantics/src/symbols.rs crates/celerrate_semantics/src/index.rs crates/celerrate_semantics/src/lib.rs Cargo.lock
git commit -m "✨ feat(semantics): index version-filtered stub symbols"
```

---

### Task 5: The per-name lookup query

**Files:**
- Create: `crates/celerrate_semantics/src/lookup.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `AnalyzedFileSet` (task 1), `source_symbol_table`,
  `stub_symbol_table` (tasks 3-4), `SymbolSpace` (task 2),
  `DeclarationKind`, `StubSymbolKind`, `StubAvailability`,
  `StubIndexInput`, `ProjectConfiguration`.
- Produces:
  - Interned `SymbolQuery<'db>` with fields `space: SymbolSpace` and
    `key: String` (`#[returns(ref)]`); constructed with
    `SymbolQuery::new(db, space, key)` where `key` is **pre-folded**.
  - `SymbolResolution` enum (Copy):
    `Source { kind: DeclarationKind }` and
    `Stub { kind: StubSymbolKind, availability: StubAvailability }`.
  - Tracked query
    `lookup_symbol<'db>(db, files: AnalyzedFileSet, stubs: StubIndexInput, configuration: ProjectConfiguration, query: SymbolQuery<'db>) -> Option<SymbolResolution>`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/lookup.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::{SymbolQuery, SymbolResolution, lookup_symbol};
    use crate::items::DeclarationKind;
    use crate::symbols::{SymbolSpace, folded_symbol_key};

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    fn fixture(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Exception".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
        ]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    fn resolve(fixture: &Fixture, space: SymbolSpace, written: &str) -> Option<SymbolResolution> {
        let query = SymbolQuery::new(
            &fixture.db,
            space,
            folded_symbol_key(space, written),
        );
        lookup_symbol(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
    }

    #[test]
    fn a_project_declaration_resolves() {
        let fixture = fixture(&["<?php namespace App; class Service {}"]);
        assert_eq!(
            resolve(&fixture, SymbolSpace::ClassLike, "APP\\SERVICE"),
            Some(SymbolResolution::Source {
                kind: DeclarationKind::Class,
            }),
        );
    }

    #[test]
    fn a_stub_symbol_resolves_with_its_availability() {
        let fixture = fixture(&["<?php"]);
        let resolution = resolve(&fixture, SymbolSpace::Function, "strlen");
        assert_eq!(
            resolution,
            Some(SymbolResolution::Stub {
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            }),
        );
    }

    #[test]
    fn a_project_declaration_shadows_the_stub() {
        // A polyfill declaring a standard symbol wins over the stubs.
        let fixture = fixture(&["<?php class Exception {}"]);
        assert_eq!(
            resolve(&fixture, SymbolSpace::ClassLike, "Exception"),
            Some(SymbolResolution::Source {
                kind: DeclarationKind::Class,
            }),
        );
    }

    #[test]
    fn an_unknown_name_answers_none() {
        let fixture = fixture(&["<?php class Known {}"]);
        assert_eq!(resolve(&fixture, SymbolSpace::ClassLike, "Unknown"), None);
        assert_eq!(resolve(&fixture, SymbolSpace::Function, "unknown"), None);
    }
}
```

Wire the module in `crates/celerrate_semantics/src/lib.rs` (add to both
lists):

```rust
mod lookup;
```

```rust
pub use lookup::{SymbolQuery, SymbolResolution, lookup_symbol};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics lookup`
Expected: compilation FAILS with `cannot find SymbolQuery` (and the
missing query).

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_semantics/src/lookup.rs` (above the tests):

```rust
//! The per-name lookup: the invalidation firewall of the symbol index.
//! Consumers never depend on a whole table; they ask for one interned
//! name. When signatures change somewhere, the table query re-runs and
//! every cached lookup re-runs too, but a lookup is a binary search:
//! cheap, and backdated whenever its answer did not change, so the
//! consumers behind it are spared. Adding a symbol in one file never
//! re-analyzes files that do not reference it.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::{StubAvailability, StubIndexInput, StubSymbolKind};

use crate::index::{source_symbol_table, stub_symbol_table};
use crate::items::DeclarationKind;
use crate::symbols::SymbolSpace;

/// One name to look up: its space and its **pre-folded** key (fold
/// with [`crate::folded_symbol_key`] before interning, so spelling
/// variants of one name share one memo).
#[salsa::interned(debug)]
pub struct SymbolQuery<'db> {
    pub space: SymbolSpace,
    #[returns(ref)]
    pub key: String,
}

/// Where a name resolved: a declaration of the analyzed file set, or a
/// compiled stub with its availability window (part 6's version gating
/// reads it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolResolution {
    Source {
        kind: DeclarationKind,
    },
    Stub {
        kind: StubSymbolKind,
        availability: StubAvailability,
    },
}

/// Resolves one folded key against the global index: the source table
/// first (a project declaration shadows a stub), the stubs second.
#[salsa::tracked]
pub fn lookup_symbol<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: SymbolQuery<'db>,
) -> Option<SymbolResolution> {
    let space = query.space(db);
    let key = query.key(db);
    if let Some(entry) = source_symbol_table(db, files).lookup(space, key) {
        return Some(SymbolResolution::Source { kind: entry.kind });
    }
    stub_symbol_table(db, stubs, configuration)
        .lookup(space, key)
        .map(|entry| SymbolResolution::Stub {
            kind: entry.symbol.kind,
            availability: entry.symbol.availability,
        })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics lookup`
Expected: 4 tests PASS.

- [ ] **Step 5: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_semantics/src/lookup.rs crates/celerrate_semantics/src/lib.rs
git commit -m "✨ feat(semantics): resolve per-name lookups through the index"
```

---

### Task 6: The PHP name resolution rules

**Files:**
- Create: `crates/celerrate_semantics/src/resolve.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`

**Interfaces:**
- Consumes: `ItemTree`, `ImportKind` (existing), `SymbolSpace`,
  `fully_qualified_name` (task 2).
- Produces:
  - `UseTables` with
    `UseTables::for_namespace(tree: &ItemTree, namespace: &str) -> UseTables`;
    derives `Debug, Clone, Default, PartialEq, Eq`.
  - `resolve_candidates(written: &str, space: SymbolSpace, namespace: &str, tables: &UseTables) -> Vec<String>`
    returning candidate fully qualified names in PHP's resolution order
    (no leading backslashes).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/resolve.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use celerrate_source::FileId;

    use super::{UseTables, resolve_candidates};
    use crate::items::ItemTree;
    use crate::symbols::SymbolSpace;

    fn candidates(source: &str, namespace: &str, written: &str, space: SymbolSpace) -> Vec<String> {
        let tree = ItemTree::from_root(FileId::new(0), &celerrate_syntax::parse(source).tree());
        let tables = UseTables::for_namespace(&tree, namespace);
        resolve_candidates(written, space, namespace, &tables)
    }

    #[test]
    fn a_fully_qualified_name_is_its_own_only_candidate() {
        assert_eq!(
            candidates("<?php namespace App;", "App", "\\Core\\Base", SymbolSpace::ClassLike),
            vec!["Core\\Base".to_owned()],
        );
        assert_eq!(
            candidates("<?php namespace App;", "App", "\\strlen", SymbolSpace::Function),
            vec!["strlen".to_owned()],
        );
    }

    #[test]
    fn the_namespace_keyword_prefix_is_relative_to_the_current_namespace() {
        assert_eq!(
            candidates("<?php namespace App;", "App", "namespace\\Child\\Node", SymbolSpace::ClassLike),
            vec!["App\\Child\\Node".to_owned()],
        );
        // Case-insensitive keyword, global namespace collapses it.
        assert_eq!(
            candidates("<?php", "", "NAMESPACE\\Node", SymbolSpace::ClassLike),
            vec!["Node".to_owned()],
        );
    }

    #[test]
    fn a_qualified_name_prefixes_the_current_namespace_and_never_falls_back() {
        assert_eq!(
            candidates("<?php namespace App;", "App", "Sub\\Helper", SymbolSpace::ClassLike),
            vec!["App\\Sub\\Helper".to_owned()],
        );
        // Qualified function names do not fall back to the global
        // namespace either: one candidate only.
        assert_eq!(
            candidates("<?php namespace App;", "App", "Sub\\greet", SymbolSpace::Function),
            vec!["App\\Sub\\greet".to_owned()],
        );
    }

    #[test]
    fn a_qualified_first_segment_resolves_through_the_class_imports() {
        let source = "<?php namespace App; use Lib\\Collections as Col;";
        assert_eq!(
            candidates(source, "App", "Col\\ArrayList", SymbolSpace::ClassLike),
            vec!["Lib\\Collections\\ArrayList".to_owned()],
        );
        // Alias matching is case-insensitive.
        assert_eq!(
            candidates(source, "App", "col\\ArrayList", SymbolSpace::ClassLike),
            vec!["Lib\\Collections\\ArrayList".to_owned()],
        );
        // The class table serves qualified names of every space.
        assert_eq!(
            candidates(source, "App", "Col\\format", SymbolSpace::Function),
            vec!["Lib\\Collections\\format".to_owned()],
        );
    }

    #[test]
    fn an_unqualified_class_import_wins_outright() {
        let source = "<?php namespace App; use Lib\\Helper;";
        assert_eq!(
            candidates(source, "App", "Helper", SymbolSpace::ClassLike),
            vec!["Lib\\Helper".to_owned()],
        );
        assert_eq!(
            candidates(source, "App", "HELPER", SymbolSpace::ClassLike),
            vec!["Lib\\Helper".to_owned()],
        );
    }

    #[test]
    fn an_unqualified_class_has_no_global_fallback() {
        assert_eq!(
            candidates("<?php namespace App;", "App", "Helper", SymbolSpace::ClassLike),
            vec!["App\\Helper".to_owned()],
        );
    }

    #[test]
    fn unqualified_functions_and_constants_fall_back_to_the_global_namespace() {
        assert_eq!(
            candidates("<?php namespace App;", "App", "greet", SymbolSpace::Function),
            vec!["App\\greet".to_owned(), "greet".to_owned()],
        );
        assert_eq!(
            candidates("<?php namespace App;", "App", "LIMIT", SymbolSpace::Constant),
            vec!["App\\LIMIT".to_owned(), "LIMIT".to_owned()],
        );
    }

    #[test]
    fn in_the_global_namespace_the_fallback_collapses_to_one_candidate() {
        assert_eq!(
            candidates("<?php", "", "greet", SymbolSpace::Function),
            vec!["greet".to_owned()],
        );
    }

    #[test]
    fn function_imports_match_case_insensitively() {
        let source = "<?php namespace App; use function Lib\\greet as hello;";
        assert_eq!(
            candidates(source, "App", "HELLO", SymbolSpace::Function),
            vec!["Lib\\greet".to_owned()],
        );
    }

    #[test]
    fn constant_imports_match_case_sensitively() {
        let source = "<?php namespace App; use const Lib\\LIMIT as L;";
        assert_eq!(
            candidates(source, "App", "L", SymbolSpace::Constant),
            vec!["Lib\\LIMIT".to_owned()],
        );
        // The lowercase spelling misses the import and takes the
        // normal fallback path.
        assert_eq!(
            candidates(source, "App", "l", SymbolSpace::Constant),
            vec!["App\\l".to_owned(), "l".to_owned()],
        );
    }

    #[test]
    fn imports_of_another_namespace_do_not_apply() {
        let source = "<?php namespace First { use Lib\\Helper; } namespace Second {}";
        assert_eq!(
            candidates(source, "Second", "Helper", SymbolSpace::ClassLike),
            vec!["Second\\Helper".to_owned()],
        );
    }

    #[test]
    fn imports_of_every_space_stay_separate() {
        // A class import never answers a function reference.
        let source = "<?php namespace App; use Lib\\Helper;";
        assert_eq!(
            candidates(source, "App", "Helper", SymbolSpace::Function),
            vec!["App\\Helper".to_owned(), "Helper".to_owned()],
        );
    }

    #[test]
    fn wreckage_produces_no_candidates() {
        assert_eq!(
            candidates("<?php", "App", "", SymbolSpace::ClassLike),
            Vec::<String>::new(),
        );
        assert_eq!(
            candidates("<?php", "App", "\\", SymbolSpace::ClassLike),
            Vec::<String>::new(),
        );
    }
}
```

Wire the module in `crates/celerrate_semantics/src/lib.rs` (add to both
lists):

```rust
mod resolve;
```

```rust
pub use resolve::{UseTables, resolve_candidates};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics resolve`
Expected: compilation FAILS with `cannot find UseTables`.

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_semantics/src/resolve.rs` (above the tests):

```rust
//! Top-level PHP name resolution: from a written reference to the
//! candidate fully qualified names, in the order PHP tries them. The
//! rules are the real ones: a leading backslash is absolute, a
//! `namespace\` prefix is relative, a qualified name resolves its
//! first segment through the class imports and never falls back to the
//! global namespace, an unqualified name resolves through its own
//! space's imports, then the current namespace, with a global fallback
//! for functions and constants only.

use std::collections::HashMap;

use crate::items::{ImportKind, ItemTree};
use crate::symbols::{SymbolSpace, fully_qualified_name};

/// The import tables of one namespace within one file: alias to
/// written absolute target, one map per symbol space. Class and
/// function aliases match case-insensitively (the map keys are
/// folded); constant aliases match case-sensitively (verbatim keys). A
/// duplicate alias keeps the last import: PHP rejects the redefinition
/// outright, tolerance picks a deterministic winner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UseTables {
    classes: HashMap<String, String>,
    functions: HashMap<String, String>,
    constants: HashMap<String, String>,
}

impl UseTables {
    /// The tables of `namespace`: every import the item tree carries
    /// for it. Imports apply to their whole namespace block; position
    /// within the block does not matter.
    pub fn for_namespace(tree: &ItemTree, namespace: &str) -> Self {
        let mut tables = Self::default();
        for import in tree
            .imports
            .iter()
            .filter(|import| import.namespace == namespace)
        {
            match import.kind {
                ImportKind::Class => {
                    tables
                        .classes
                        .insert(import.alias.to_ascii_lowercase(), import.target.clone());
                }
                ImportKind::Function => {
                    tables
                        .functions
                        .insert(import.alias.to_ascii_lowercase(), import.target.clone());
                }
                ImportKind::Constant => {
                    tables
                        .constants
                        .insert(import.alias.clone(), import.target.clone());
                }
            }
        }
        tables
    }

    /// The target a class-space alias names, case-insensitively. The
    /// class table also resolves the first segment of qualified names
    /// of every space: `use` imports name classes or namespaces.
    fn class_target(&self, alias: &str) -> Option<&str> {
        self.classes
            .get(&alias.to_ascii_lowercase())
            .map(String::as_str)
    }

    fn function_target(&self, alias: &str) -> Option<&str> {
        self.functions
            .get(&alias.to_ascii_lowercase())
            .map(String::as_str)
    }

    fn constant_target(&self, alias: &str) -> Option<&str> {
        self.constants.get(alias).map(String::as_str)
    }
}

/// The candidate fully qualified names of one written reference, in
/// the order PHP tries them. Empty input (error-recovery wreckage)
/// produces no candidates.
pub fn resolve_candidates(
    written: &str,
    space: SymbolSpace,
    namespace: &str,
    tables: &UseTables,
) -> Vec<String> {
    if written.is_empty() {
        return Vec::new();
    }
    if let Some(absolute) = written.strip_prefix('\\') {
        return if absolute.is_empty() {
            Vec::new()
        } else {
            vec![absolute.to_owned()]
        };
    }
    match written.split_once('\\') {
        Some((first, rest)) => {
            if first.eq_ignore_ascii_case("namespace") {
                return vec![fully_qualified_name(namespace, rest)];
            }
            match tables.class_target(first) {
                Some(target) => vec![format!("{target}\\{rest}")],
                None => vec![fully_qualified_name(namespace, written)],
            }
        }
        None => {
            let imported = match space {
                SymbolSpace::ClassLike => tables.class_target(written),
                SymbolSpace::Function => tables.function_target(written),
                SymbolSpace::Constant => tables.constant_target(written),
            };
            if let Some(target) = imported {
                return vec![target.to_owned()];
            }
            let in_namespace = fully_qualified_name(namespace, written);
            match space {
                SymbolSpace::ClassLike => vec![in_namespace],
                SymbolSpace::Function | SymbolSpace::Constant => {
                    if namespace.is_empty() {
                        vec![in_namespace]
                    } else {
                        vec![in_namespace, written.to_owned()]
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics resolve`
Expected: 13 tests PASS.

- [ ] **Step 5: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_semantics/src/resolve.rs crates/celerrate_semantics/src/lib.rs
git commit -m "✨ feat(semantics): apply the PHP name resolution rules"
```

---

### Task 7: End-to-end resolution and the firewall proof

**Files:**
- Modify: `crates/celerrate_semantics/src/resolve.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: everything from tasks 1-6.
- Produces:
  - `SymbolSources { files: AnalyzedFileSet, stubs: StubIndexInput, configuration: ProjectConfiguration }`
    deriving `Debug, Clone, Copy` (a plain bundle so consumers thread one
    handle set; tracked functions take the three handles separately because
    salsa arguments must be salsa structs).
  - `resolve_name(db: &dyn salsa::Database, sources: SymbolSources, namespace: &str, tables: &UseTables, written: &str, space: SymbolSpace) -> Option<SymbolResolution>`
    (candidates in PHP order, each looked up through the per-name firewall,
    first hit wins). This is part 6's entry point for every collected
    reference.

- [ ] **Step 1: Write the failing unit tests**

Append to the `tests` module of `crates/celerrate_semantics/src/resolve.rs`:

```rust
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::{SymbolSources, resolve_name};
    use crate::items::DeclarationKind;
    use crate::lookup::SymbolResolution;
    use crate::queries::item_tree;

    fn sources_of(db: &TestDatabase, sources: &[&str]) -> SymbolSources {
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        SymbolSources {
            files: AnalyzedFileSet::new(db, handles),
            stubs: StubIndexInput::builder(StubIndex::from_symbols(vec![StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            }]))
            .durability(salsa::Durability::HIGH)
            .new(db),
            configuration: ProjectConfiguration::builder(PhpVersionRange::new(
                PhpVersion::new(8, 1),
                PhpVersion::new(8, 5),
            ))
            .durability(salsa::Durability::MEDIUM)
            .new(db),
        }
    }

    #[test]
    fn a_reference_resolves_through_its_import_to_the_declaration() {
        let db = TestDatabase::default();
        let sources = sources_of(
            &db,
            &[
                "<?php namespace App; use Lib\\Helper;",
                "<?php namespace Lib; class Helper {}",
            ],
        );
        let file = *sources.files.files(&db).first().unwrap();
        let tables = UseTables::for_namespace(item_tree(&db, file), "App");
        assert_eq!(
            resolve_name(&db, sources, "App", &tables, "Helper", SymbolSpace::ClassLike),
            Some(SymbolResolution::Source {
                kind: DeclarationKind::Class,
            }),
        );
    }

    #[test]
    fn an_unqualified_function_falls_back_to_the_global_stub() {
        let db = TestDatabase::default();
        let sources = sources_of(&db, &["<?php namespace App;"]);
        assert_eq!(
            resolve_name(
                &db,
                sources,
                "App",
                &UseTables::default(),
                "strlen",
                SymbolSpace::Function,
            ),
            Some(SymbolResolution::Stub {
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            }),
        );
    }

    #[test]
    fn an_unresolvable_reference_answers_none() {
        let db = TestDatabase::default();
        let sources = sources_of(&db, &["<?php namespace App;"]);
        assert_eq!(
            resolve_name(
                &db,
                sources,
                "App",
                &UseTables::default(),
                "Missing",
                SymbolSpace::ClassLike,
            ),
            None,
        );
    }
```

Note: the inner attribute `#![allow(clippy::unwrap_used)]` must be the
first line inside the `mod tests` block; place it there when the module
gains these tests (the module previously needed no allowance).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics resolve`
Expected: compilation FAILS with `cannot find SymbolSources` /
`cannot find resolve_name`.

- [ ] **Step 3: Write the minimal implementation**

Append to `crates/celerrate_semantics/src/resolve.rs` (above the tests),
and add the imports:

```rust
use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::lookup::{SymbolQuery, SymbolResolution, lookup_symbol};
use crate::symbols::folded_symbol_key;
```

```rust
/// The three inputs the global symbol index reads, bundled so
/// consumers thread one handle set through resolution calls. A plain
/// value, not a salsa struct: tracked functions take the three handles
/// separately.
#[derive(Debug, Clone, Copy)]
pub struct SymbolSources {
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
}

/// Resolves one written reference: candidate names in PHP's order,
/// each looked up through the per-name firewall; the first hit wins.
pub fn resolve_name(
    db: &dyn salsa::Database,
    sources: SymbolSources,
    namespace: &str,
    tables: &UseTables,
    written: &str,
    space: SymbolSpace,
) -> Option<SymbolResolution> {
    resolve_candidates(written, space, namespace, tables)
        .into_iter()
        .find_map(|candidate| {
            let query = SymbolQuery::new(db, space, folded_symbol_key(space, &candidate));
            lookup_symbol(db, sources.files, sources.stubs, sources.configuration, query)
        })
}
```

Update the exports in `crates/celerrate_semantics/src/lib.rs`:

```rust
pub use resolve::{SymbolSources, UseTables, resolve_candidates, resolve_name};
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `cargo test -p celerrate_semantics resolve`
Expected: all resolve tests PASS (13 from task 6 plus 3 new).

- [ ] **Step 5: Commit the entry point**

```bash
git add crates/celerrate_semantics/src/resolve.rs crates/celerrate_semantics/src/lib.rs
git commit -m "✨ feat(semantics): resolve references end to end"
```

- [ ] **Step 6: Write the failing invalidation-scope tests**

Append to `crates/celerrate_semantics/tests/invalidation_scope.rs`. First
extend the imports at the top of the file:

```rust
use celerrate_db::AnalyzedFileSet;
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    SymbolSources, SymbolSpace, UseTables, resolve_name,
};
use celerrate_stubs::{StubIndex, StubIndexInput};
```

Then append the consumer query and the tests:

```rust
/// A stand-in for part 6's checks: the inheritance names of one file
/// that do not resolve. It reads the file's item tree and the
/// per-name lookups, nothing else; the firewall must spare it from
/// every unrelated change.
#[salsa::tracked]
fn unresolved_inheritance_names(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
) -> Vec<String> {
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let tree = item_tree(db, file);
    let mut unresolved = Vec::new();
    for declaration in &tree.declarations {
        let tables = UseTables::for_namespace(tree, &declaration.namespace);
        for name in declaration
            .extends
            .iter()
            .chain(&declaration.implements)
            .chain(&declaration.trait_uses)
        {
            let resolution = resolve_name(
                db,
                sources,
                &declaration.namespace,
                &tables,
                name,
                SymbolSpace::ClassLike,
            );
            if resolution.is_none() {
                unresolved.push(name.clone());
            }
        }
    }
    unresolved
}

struct ResolutionFixture {
    db: TestDatabase,
    files: Vec<SourceFile>,
    set: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

impl ResolutionFixture {
    fn new(sources: &[&str]) -> Self {
        let db = TestDatabase::default();
        let files: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let set = AnalyzedFileSet::new(&db, files.clone());
        let stubs = StubIndexInput::builder(StubIndex::default())
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Self {
            db,
            files,
            set,
            stubs,
            configuration,
        }
    }

    fn unresolved(&self, file_index: usize) -> Vec<String> {
        unresolved_inheritance_names(
            &self.db,
            self.set,
            self.stubs,
            self.configuration,
            self.files[file_index],
        )
    }
}

#[test]
fn adding_an_unrelated_symbol_spares_every_consumer() {
    // The spec's firewall sentence, observed directly: adding a symbol
    // in one file does not invalidate the checks of files that never
    // reference it.
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base {}",
        "<?php namespace Elsewhere; class Unrelated {}",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.files[2]
        .set_bytes(&mut fixture.db)
        .to(b"<?php namespace Elsewhere; class Unrelated {} class Another {}".to_vec());
    let _ = fixture.unresolved(0);

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        1,
        "a new signature rebuilds the table: {log:?}",
    );
    assert!(
        executions_of(&log, "lookup_symbol") >= 1,
        "the cached lookups re-run cheaply: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        0,
        "an unchanged lookup answer must backdate, sparing the consumer: {log:?}",
    );
}

#[test]
fn a_body_edit_in_another_file_stops_at_its_item_tree() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base { public function greet() { return 1; } }",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.files[1].set_bytes(&mut fixture.db).to(
        b"<?php namespace App; class Base { public function greet() { return 2; } }".to_vec(),
    );
    let _ = fixture.unresolved(0);

    let log = fixture.db.take_executed();
    assert_eq!(executions_of(&log, "item_tree"), 1, "{log:?}");
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        0,
        "an identical item tree must never rebuild the table: {log:?}",
    );
    assert_eq!(executions_of(&log, "lookup_symbol"), 0, "{log:?}");
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        0,
        "{log:?}",
    );
}

#[test]
fn deleting_the_referenced_declaration_reaches_the_consumer() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base {}",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture.files[1]
        .set_bytes(&mut fixture.db)
        .to(b"<?php namespace App;".to_vec());
    assert_eq!(fixture.unresolved(0), vec!["Base".to_owned()]);

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        1,
        "a changed lookup answer must reach the consumer: {log:?}",
    );
}

#[test]
fn declaring_a_previously_missing_symbol_reaches_the_consumer() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App;",
    ]);
    assert_eq!(fixture.unresolved(0), vec!["Base".to_owned()]);
    fixture.db.take_executed();

    fixture.files[1]
        .set_bytes(&mut fixture.db)
        .to(b"<?php namespace App; class Base {}".to_vec());
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        1,
        "{log:?}",
    );
}

#[test]
fn a_version_range_change_never_touches_the_source_table() {
    let mut fixture = ResolutionFixture::new(&[
        "<?php namespace App; class Consumer extends Base {}",
        "<?php namespace App; class Base {}",
    ]);
    assert_eq!(fixture.unresolved(0), Vec::<String>::new());
    fixture.db.take_executed();

    fixture
        .configuration
        .set_php_version_range(&mut fixture.db)
        .to(PhpVersionRange::point(PhpVersion::new(8, 3)));
    let _ = fixture.unresolved(0);

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "stub_symbol_table"),
        1,
        "the stub side recomputes: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "source_symbol_table"),
        0,
        "the source side must not: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "unresolved_inheritance_names"),
        0,
        "unchanged stub answers backdate: {log:?}",
    );
}
```

Note: this integration test file indexes `fixture.files[...]`; the file's
existing `#![allow(clippy::indexing_slicing, ...)]` header (extend the
existing inner attribute list if `indexing_slicing` is not yet allowed
there) keeps clippy green.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics --test invalidation_scope`
Expected: all tests PASS (7 existing plus 5 new). If
`adding_an_unrelated_symbol_spares_every_consumer` fails on the consumer
count, the firewall is broken: debug the lookup query's dependencies
before touching the assertions.

- [ ] **Step 8: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_semantics/tests/invalidation_scope.rs
git commit -m "✅ test(semantics): pin the resolution invalidation scope"
```

---

### Task 8: The incremental consistency replay

**Files:**
- Modify: `crates/celerrate_db/src/testing.rs`
- Modify: `crates/celerrate_semantics/tests/incremental_consistency.rs`

**Interfaces:**
- Consumes: the existing `TestDatabase`, `SourceFile`,
  `assert_incremental_consistency_with`; everything from tasks 1-7.
- Produces:
  - `assert_incremental_consistency_with_context<Context>(initial: &[&[u8]], edits: &[(usize, &[u8])], make_context: &dyn Fn(&TestDatabase, &[SourceFile]) -> Context, assert_state_matches: &dyn Fn(&TestDatabase, &Context, &TestDatabase, &Context))`
    in `celerrate_db::testing`: the state-level form of the replay for
    whole-project inputs. The existing per-file
    `assert_incremental_consistency_with` is reimplemented on top of it
    (behavior unchanged).

- [ ] **Step 1: Write the failing test**

Replace the header comment and extend
`crates/celerrate_semantics/tests/incremental_consistency.rs`. Add the
imports:

```rust
use celerrate_db::testing::assert_incremental_consistency_with_context;
use celerrate_db::AnalyzedFileSet;
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    SymbolResolution, SymbolSources, SymbolSpace, UseTables, resolve_name, source_symbol_table,
};
use celerrate_stubs::{
    StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
};
```

Append the test:

```rust
type ResolutionContext = (AnalyzedFileSet, StubIndexInput, ProjectConfiguration);

fn resolution_context(
    db: &celerrate_db::testing::TestDatabase,
    files: &[SourceFile],
) -> ResolutionContext {
    (
        AnalyzedFileSet::new(db, files.to_vec()),
        StubIndexInput::builder(StubIndex::from_symbols(vec![StubSymbol {
            name: "strlen".to_owned(),
            kind: StubSymbolKind::Function,
            availability: StubAvailability::ALWAYS,
        }]))
        .durability(salsa::Durability::HIGH)
        .new(db),
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db),
    )
}

/// Every inheritance name of every file, resolved: the resolution
/// traffic a real check would produce, in deterministic order.
fn resolution_answers(
    db: &celerrate_db::testing::TestDatabase,
    context: &ResolutionContext,
) -> Vec<(String, Option<SymbolResolution>)> {
    let (files, stubs, configuration) = *context;
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let mut answers = Vec::new();
    for &file in files.files(db) {
        let tree = celerrate_semantics::item_tree(db, file);
        for declaration in &tree.declarations {
            let tables = UseTables::for_namespace(tree, &declaration.namespace);
            for name in declaration
                .extends
                .iter()
                .chain(&declaration.implements)
                .chain(&declaration.trait_uses)
            {
                answers.push((
                    name.clone(),
                    resolve_name(
                        db,
                        sources,
                        &declaration.namespace,
                        &tables,
                        name,
                        SymbolSpace::ClassLike,
                    ),
                ));
            }
        }
    }
    answers
}

#[test]
fn resolution_matches_a_from_scratch_analysis_after_every_edit() {
    assert_incremental_consistency_with_context(
        &[
            b"<?php namespace App; use Lib\\Helper; class Consumer extends Helper implements Contract {}",
            b"<?php namespace Lib; class Helper {}",
            b"<?php namespace App; interface Contract {}",
        ],
        &[
            // A body edit: nothing observable changes.
            (1, b"<?php namespace Lib; class Helper { public function noop() {} }"),
            // The referenced declaration disappears.
            (1, b"<?php namespace Lib;"),
            // It returns under a different spelling: class lookups are
            // case-insensitive, so it resolves again.
            (1, b"<?php namespace Lib; class HELPER {}"),
            // The import is re-aliased: the reference now misses it.
            (0, b"<?php namespace App; use Lib\\Helper as Aid; class Consumer extends Helper implements Contract {}"),
            // A new file-set-neutral edit: an unrelated declaration.
            (2, b"<?php namespace App; interface Contract {} interface Extra {}"),
        ],
        &resolution_context,
        &|incremental, context, from_scratch, fresh_context| {
            assert_eq!(
                source_symbol_table(incremental, context.0),
                source_symbol_table(from_scratch, fresh_context.0),
                "the symbol tables diverged",
            );
            assert_eq!(
                resolution_answers(incremental, context),
                resolution_answers(from_scratch, fresh_context),
                "the resolution answers diverged",
            );
        },
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p celerrate_semantics --test incremental_consistency`
Expected: compilation FAILS with
`cannot find assert_incremental_consistency_with_context`.

- [ ] **Step 3: Write the harness extension**

In `crates/celerrate_db/src/testing.rs`, add the state-level form and
reimplement the per-file form on top of it (replace the bodies of
`assert_incremental_consistency_with` and delete the now-unused
`assert_matches_from_scratch` helper):

```rust
/// The state-level form of the replay: `make_context` creates the
/// whole-project inputs (the analyzed file set, the stub index, the
/// configuration) once for the incremental database and once per
/// from-scratch database, and `assert_state_matches` compares the two
/// databases after the initial state and after every edit.
pub fn assert_incremental_consistency_with_context<Context>(
    initial: &[&[u8]],
    edits: &[(usize, &[u8])],
    make_context: &dyn Fn(&TestDatabase, &[SourceFile]) -> Context,
    assert_state_matches: &dyn Fn(&TestDatabase, &Context, &TestDatabase, &Context),
) {
    let mut incremental = TestDatabase::default();
    let mut current: Vec<Vec<u8>> = initial.iter().map(|bytes| bytes.to_vec()).collect();
    let files: Vec<SourceFile> = current
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            SourceFile::new(&incremental, FileId::new(index as u32), bytes.clone())
        })
        .collect();
    let context = make_context(&incremental, &files);

    assert_state_against_scratch(&incremental, &context, &current, make_context, assert_state_matches);
    for &(file_index, new_bytes) in edits {
        assert!(
            file_index < files.len(),
            "edit targets unknown file index {file_index}",
        );
        let (Some(slot), Some(file)) = (current.get_mut(file_index), files.get(file_index)) else {
            // Unreachable: guarded by the assertion above.
            return;
        };
        *slot = new_bytes.to_vec();
        file.set_bytes(&mut incremental).to(new_bytes.to_vec());
        assert_state_against_scratch(
            &incremental,
            &context,
            &current,
            make_context,
            assert_state_matches,
        );
    }
}

fn assert_state_against_scratch<Context>(
    incremental: &TestDatabase,
    context: &Context,
    current: &[Vec<u8>],
    make_context: &dyn Fn(&TestDatabase, &[SourceFile]) -> Context,
    assert_state_matches: &dyn Fn(&TestDatabase, &Context, &TestDatabase, &Context),
) {
    let from_scratch = TestDatabase::default();
    let fresh_files: Vec<SourceFile> = current
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            SourceFile::new(&from_scratch, FileId::new(index as u32), bytes.clone())
        })
        .collect();
    let fresh_context = make_context(&from_scratch, &fresh_files);
    assert_state_matches(incremental, context, &from_scratch, &fresh_context);
}
```

And the per-file form becomes a thin wrapper (its signature and behavior
do not change; existing callers stay untouched):

```rust
/// The closure form of the replay: upper layers extend the same
/// harness with their own per-file comparison, without any dependency
/// from this crate upward. The closure receives the incremental
/// database and its file handle, the from-scratch database and its
/// fresh handle, and the file index; it runs for every file after the
/// initial state and after every edit.
pub fn assert_incremental_consistency_with(
    initial: &[&[u8]],
    edits: &[(usize, &[u8])],
    assert_file_matches: &dyn Fn(&TestDatabase, SourceFile, &TestDatabase, SourceFile, usize),
) {
    assert_incremental_consistency_with_context(
        initial,
        edits,
        &|_, files| files.to_vec(),
        &|incremental, files, from_scratch, fresh_files| {
            for (index, (file, fresh_file)) in files.iter().zip(fresh_files).enumerate() {
                assert_file_matches(incremental, *file, from_scratch, *fresh_file, index);
            }
        },
    );
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_db && cargo test -p celerrate_semantics --test incremental_consistency`
Expected: PASS, including every pre-existing consistency test (the
per-file wrapper must behave identically).

- [ ] **Step 5: Gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_db/src/testing.rs crates/celerrate_semantics/tests/incremental_consistency.rs
git commit -m "✅ test(semantics): replay resolution against from-scratch runs"
```

---

### Task 9: Record the narrowings and close the part

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-11-semantic-core-design.md`

- [ ] **Step 1: Record the deliberate narrowings in the design spec**

In section 6, after the "Name resolution" paragraph (the one ending with
"are part of the `ItemTree`.") and before the "Syntax gating" paragraph,
insert:

```markdown
Deliberate narrowings and shapes, recorded here after implementation
review (part 5). The global index is realized as two tables: the source
table over the analyzed file set's `ItemTree`s and the stub table over
the version-filtered stub view, consulted in that order by the per-name
lookup — a project declaration shadows a stub, and a source edit never
re-copies the stub side. Case folding is ASCII (the engine's own
folding), and a constant folds its namespace segments while keeping its
terminal segment case-sensitive. Import tables group by the item tree's
namespace field (a whole namespace block sees its imports, position
within the block does not matter); class and function aliases match
case-insensitively, constant aliases case-sensitively, and a duplicate
alias keeps the last import. Duplicate declarations of one name resolve
to the deterministic first entry (file set order, then tree order);
duplicate-declaration diagnostics are later work. The analyzed file set
lives in `celerrate_db` as the section 2 input list names it.
```

Adjust the wording if the implementation deviated during tasks 1-8; the
spec must describe what was actually built.

- [ ] **Step 2: Verify the full gate one last time**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

- [ ] **Step 3: Commit**

```bash
git add .claude/superpowers/specs/2026-07-11-semantic-core-design.md
git commit -m "📝 docs(specs): record the resolution narrowings"
```

---

## Self-Review

- **Spec coverage (section 6, part 5 scope):** merged FQN-to-symbol index
  over project, vendor, and stubs → tasks 3-5; per-name lookup queries with
  the invalidation firewall → tasks 5 and 7; case-insensitive classes and
  functions, case-sensitive constants, folded keys with original spelling
  retained → tasks 2-4; full top-level resolution rules (fully qualified,
  relative qualified, unqualified with per-space fallbacks, use tables with
  aliases and groups) → task 6; spec section 9's harness growth and
  invalidation-scope tests → tasks 7-8. Reference collection and
  diagnostics are deliberately part 6.
- **Type consistency:** `SymbolSpace` / `folded_symbol_key` /
  `fully_qualified_name` (task 2) are used with identical signatures in
  tasks 3-7; `SymbolTable::lookup(space, key)` and
  `StubSymbolTable::lookup(space, key)` share their shape; `SymbolQuery`
  keys are pre-folded everywhere (`resolve` in task 5's tests,
  `resolve_name` in task 7); `resolve_name(db, sources, namespace, tables,
  written, space)` is called with the same argument order in tasks 7-8.
- **Placeholders:** none; every step carries its complete code and exact
  commands.
