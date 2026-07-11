# Semantic Core Part 3: Stubs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `celerrate_stubs`: a pinned phpstorm-stubs snapshot compiled by a
feature-gated compiler binary into a committed, hand-written versioned binary blob
(top-level symbols with per-version availability metadata), embedded in the binary,
loaded as a high-durability salsa input, and filtered by the project's PHP version
range through a tracked query. Also discharges the part 2 deferral: the project
configuration becomes a salsa input in `celerrate_project`.

**Architecture:** Per the spec `.claude/superpowers/specs/2026-07-11-semantic-core-design.md`
(section 5) and the approved part 3 design discussion. Decisions fixed there:

- **Snapshot pinning:** a committed pin file (`xtask/phpstorm-stubs.pin`) carries the
  phpstorm-stubs commit SHA; `cargo xtask fetch-stubs` fetches that commit with git
  into `target/phpstorm-stubs/<sha>/` (gitignored via `target/`). Refinement over the
  design discussion: the fetch is git-based (`git fetch --depth 1 <repository> <sha>`)
  instead of a tarball download with a separate sha256, because a git commit SHA is
  already cryptographic and GitHub archive bytes are not stable over time. No network
  in any normal build: the compiled blob is committed.
- **Compiler placement:** the umbrella spec says "compiled by xtask", but xtask has a
  documented invariant (it depends on no `celerrate_*` crate, so a broken generated
  file can never prevent regenerating it) and the compiler must parse PHP with
  `celerrate_syntax`. Resolution, faithful to the parent spec ("the build-time stub
  compiler is owned by `celerrate_stubs`"): the compiler lives in `celerrate_stubs`
  as a `required-features = ["compiler"]` binary with `celerrate_syntax` as an
  optional dependency (and a dev-dependency, so extraction tests run in the default
  `cargo test --workspace`). `cargo xtask compile-stubs` orchestrates: fetch if
  absent, then `cargo run --package celerrate_stubs --features compiler --bin
  stub-compiler`. xtask itself only does network and process spawning.
- **Blob format:** hand-written, zero-dependency: magic + format version u32 +
  checksum (FNV-1a 64) + section table + payload. One live section (the symbol
  table) and two reserved section identifiers (signature payload for sub-project 3,
  overlay merge for refinements and plugin stubs). Unknown section identifiers are
  skipped by the reader (additive evolution without a version bump); the format
  version changes only on incompatible layout changes. The reader is tolerant and
  zero-panic: truncation, bad checksum, or an unknown version produce a clean error,
  never a panic. Output is byte-deterministic: symbols sorted, duplicates merged.
- **Salsa placement:** the umbrella's "the salsa inputs live in `celerrate_db`"
  names the base-db *role*; the concrete inputs whose field types live above
  `celerrate_db` are defined in their domain crates, consistent with "higher-level
  query definitions live in their domain crates". `ProjectConfiguration` (carrying
  the `PhpVersionRange`) is a salsa input in `celerrate_project` (which gains the
  `salsa` dependency, closing part 2's "the configuration becomes a salsa input in
  part 3" note); `StubIndexInput` and the tracked `stubs_in_range` query live in
  `celerrate_stubs`.
- **Filtering semantics:** "filtered by the project's version range" is a derived
  view, not destruction: the input holds the full decoded index; the tracked query
  `stubs_in_range` keeps every symbol that exists somewhere in `[minimum, maximum]`
  and drops the rest. Availability metadata stays on the survivors — part 6's
  version-gating family needs it.
- **Freshness:** on the codegen model, adapted to the network: a unit test compares
  the committed blob to a recompilation when the pinned snapshot is present locally
  (and self-reports as skipped otherwise), and CI gains a `stubs` job running
  `cargo xtask compile-stubs --check` (fetch + recompile + compare).

**Tech Stack:** Rust edition 2024, salsa 0.27 (existing workspace dependency; newly
used by `celerrate_project` and `celerrate_stubs`), existing crates
`celerrate_project`, `celerrate_syntax` (compiler feature + dev-dependency),
`celerrate_db` (dev-only). No new external dependencies anywhere.

## Global Constraints

- Zero panic, mechanically enforced: the workspace denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` is forbidden. Only test
  modules may `#[allow]` / `#![allow]` these lints. Use `.get()`, `split_at_checked`,
  `checked_add`, `is_none_or`, `unwrap_or_default` — never indexing or unwrap in
  production code. The `stub-compiler` binary and xtask are production code too:
  same rules (`eprintln!`/`println!` are fine, panics are not).
- Strict layering, DAG with no upward edges. `celerrate_stubs` depends on
  `celerrate_project` and `salsa`, plus `celerrate_syntax` behind the `compiler`
  feature (optional dependency) and as a dev-dependency. `celerrate_project` gains
  `salsa`. xtask keeps depending on no `celerrate_*` crate.
- Error resilience: no input may crash or fail the tool. A malformed stub file
  produces a compiler warning and partial extraction; a corrupted blob produces a
  clean `StubBlobError`, never a panic.
- Determinism: the same snapshot produces the same blob, byte for byte. Sorted
  walks, sorted symbols, merged duplicates, no wall-clock time, no randomness, no
  environment reads inside queries.
- TDD throughout: every step of behavior starts from a failing test.
- Everything in English, full words, no abbreviated names (standard acronyms fine).
- Commits: gitmoji + Conventional Commits, repository-configured identity, no
  Claude attribution anywhere.
- Local commands that must stay green after every task:
  `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `cargo deny check`.
  Additionally for this part: `cargo clippy --package celerrate_stubs --features
  compiler --all-targets -- -D warnings` (the compiler binary is skipped by the
  workspace-wide invocation because of `required-features`).

## File Structure

```
crates/celerrate_stubs/
  Cargo.toml                 crate manifest: compiler feature, stub-compiler binary
  src/lib.rs                 crate root, exports, embedded blob (task 8)
  src/symbol.rs              StubSymbolKind, StubAvailability, StubDeprecation, StubSymbol
  src/index.rs               StubIndex: sorted, duplicate-merged container
  src/blob.rs                the hand-written blob format: encode, decode, errors
  src/query.rs               StubIndexInput + stubs_in_range (task 8)
  src/compiler/mod.rs        cfg(any(test, feature = "compiler")) gate, freshness test (task 9)
  src/compiler/extract.rs    parsed stub file -> symbols with availability
  src/compiler/snapshot.rs   deterministic snapshot walk, pinned-directory lookup
  src/bin/stub-compiler.rs   the compiler binary (required-features = ["compiler"])
  src/stubs.bin              the committed blob (generated, task 7)
crates/celerrate_project/
  Cargo.toml                 gains salsa
  src/input.rs               ProjectConfiguration salsa input (task 8)
  src/lib.rs                 gains `mod input;` + export
xtask/
  phpstorm-stubs.pin         pinned snapshot: repository + commit SHA (task 7)
  src/lib.rs                 gains `pub mod stubs;`
  src/main.rs                gains fetch-stubs / compile-stubs arms
  src/stubs.rs               pin parsing, git fetch, compiler orchestration
.github/workflows/ci.yml    gains the stubs job (task 9)
CHANGELOG.md                 gains the part 3 entry (task 9)
```

Task order: 1 symbol model, 2 index, 3 blob format, 4 declaration extraction,
5 availability metadata, 6 snapshot walk + compiler binary, 7 xtask + pin +
committed blob, 8 embedding + salsa wiring, 9 freshness + CI + changelog.

---

### Task 1: The crate and the symbol model

**Files:**
- Create: `crates/celerrate_stubs/Cargo.toml`
- Create: `crates/celerrate_stubs/src/lib.rs`
- Create: `crates/celerrate_stubs/src/symbol.rs`

**Interfaces:**
- Consumes: `celerrate_project::{PhpVersion, PhpVersionRange}` (existing:
  `PhpVersion::new(major: u8, minor: u8)`, `PhpVersionRange { minimum, maximum }`,
  `PhpVersionRange::new(minimum, maximum)`).
- Produces (later tasks rely on these exact shapes):
  - `pub enum StubSymbolKind { Class, Interface, Trait, Enum, Function, Constant }`
    with `pub const fn as_u8(self) -> u8` and
    `pub const fn from_u8(value: u8) -> Option<Self>` (discriminants 0..=5 in the
    order above).
  - `pub struct StubDeprecation { pub since: Option<PhpVersion> }`
  - `pub struct StubAvailability { pub introduced: Option<PhpVersion>, pub removed:
    Option<PhpVersion>, pub deprecated: Option<StubDeprecation> }` with
    `pub const ALWAYS: Self` and `pub fn exists_in(&self, range: PhpVersionRange) -> bool`.
  - `pub struct StubSymbol { pub name: String, pub kind: StubSymbolKind,
    pub availability: StubAvailability }`
  - All model types derive `Debug, Clone, PartialEq, Eq, Hash` (`Copy` on the three
    small ones: `StubSymbolKind`, `StubDeprecation`, `StubAvailability`).

- [ ] **Step 1: Create the crate manifest and an empty library root**

`crates/celerrate_stubs/Cargo.toml` (the workspace `members = ["crates/*", "xtask"]`
glob picks it up automatically):

```toml
[package]
name = "celerrate_stubs"
description = "Compiled phpstorm-stubs: the embedded standard-library symbol index for the Celerrate toolchain"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_project = { path = "../celerrate_project" }

[lints]
workspace = true
```

`crates/celerrate_stubs/src/lib.rs`:

```rust
//! Compiled phpstorm-stubs: the embedded index of standard-library and
//! extension symbols with per-version availability metadata.
//!
//! A pinned snapshot of phpstorm-stubs is compiled by the feature-gated
//! `stub-compiler` binary (driven by `cargo xtask compile-stubs`) into a
//! committed, versioned binary blob. At runtime the embedded blob loads
//! as a high-durability salsa input and a tracked query filters it by
//! the project's PHP version range.

mod symbol;

pub use symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};
```

Run: `cargo build --package celerrate_stubs`
Expected: FAIL with "unresolved import" / "file not found for module `symbol`"
(the module does not exist yet — that is this task's red state; proceed).

- [ ] **Step 2: Write the failing tests for the symbol model**

`crates/celerrate_stubs/src/symbol.rs`:

```rust
#[cfg(test)]
mod tests {
    use celerrate_project::{PhpVersion, PhpVersionRange};

    use super::{StubAvailability, StubDeprecation, StubSymbolKind};

    #[test]
    fn kinds_round_trip_through_their_blob_discriminants() {
        let kinds = [
            StubSymbolKind::Class,
            StubSymbolKind::Interface,
            StubSymbolKind::Trait,
            StubSymbolKind::Enum,
            StubSymbolKind::Function,
            StubSymbolKind::Constant,
        ];
        for (expected_discriminant, kind) in kinds.into_iter().enumerate() {
            assert_eq!(usize::from(kind.as_u8()), expected_discriminant);
            assert_eq!(StubSymbolKind::from_u8(kind.as_u8()), Some(kind));
        }
        assert_eq!(StubSymbolKind::from_u8(6), None);
    }

    #[test]
    fn an_unconstrained_symbol_exists_in_every_range() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        assert!(StubAvailability::ALWAYS.exists_in(range));
    }

    #[test]
    fn a_symbol_introduced_inside_the_range_exists() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let availability = StubAvailability {
            introduced: Some(PhpVersion::new(8, 3)),
            ..StubAvailability::ALWAYS
        };
        assert!(availability.exists_in(range));
    }

    #[test]
    fn a_symbol_introduced_after_the_maximum_does_not_exist() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 2));
        let availability = StubAvailability {
            introduced: Some(PhpVersion::new(8, 3)),
            ..StubAvailability::ALWAYS
        };
        assert!(!availability.exists_in(range));
    }

    #[test]
    fn a_symbol_removed_at_or_before_the_minimum_does_not_exist() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let removed_before = StubAvailability {
            removed: Some(PhpVersion::new(8, 0)),
            ..StubAvailability::ALWAYS
        };
        let removed_at_minimum = StubAvailability {
            removed: Some(PhpVersion::new(8, 1)),
            ..StubAvailability::ALWAYS
        };
        assert!(!removed_before.exists_in(range));
        assert!(!removed_at_minimum.exists_in(range));
    }

    #[test]
    fn a_symbol_removed_inside_the_range_still_exists_at_the_minimum() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let availability = StubAvailability {
            removed: Some(PhpVersion::new(8, 3)),
            ..StubAvailability::ALWAYS
        };
        assert!(availability.exists_in(range));
    }

    #[test]
    fn deprecation_never_affects_existence() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let availability = StubAvailability {
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
            ..StubAvailability::ALWAYS
        };
        assert!(availability.exists_in(range));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs`
Expected: FAIL to compile ("cannot find type `StubAvailability`" and friends).

- [ ] **Step 4: Write the symbol model**

Prepend to `crates/celerrate_stubs/src/symbol.rs` (above the test module):

```rust
//! The stub symbol model: what one top-level declaration of the
//! compiled phpstorm-stubs snapshot looks like at runtime.

use celerrate_project::{PhpVersion, PhpVersionRange};

/// The kind of a top-level stub symbol. The discriminants are the blob
/// encoding: fixed forever once a blob format version has shipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StubSymbolKind {
    Class = 0,
    Interface = 1,
    Trait = 2,
    Enum = 3,
    Function = 4,
    Constant = 5,
}

impl StubSymbolKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Class),
            1 => Some(Self::Interface),
            2 => Some(Self::Trait),
            3 => Some(Self::Enum),
            4 => Some(Self::Function),
            5 => Some(Self::Constant),
            _ => None,
        }
    }
}

/// A deprecation mark. `since` is the version that deprecated the
/// symbol; `None` when the stubs mark a deprecation without a version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StubDeprecation {
    pub since: Option<PhpVersion>,
}

/// Per-version availability of one symbol. `None` means "no
/// constraint". `removed` is the first version in which the symbol no
/// longer exists (the `@removed` convention of phpstorm-stubs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StubAvailability {
    pub introduced: Option<PhpVersion>,
    pub removed: Option<PhpVersion>,
    pub deprecated: Option<StubDeprecation>,
}

impl StubAvailability {
    pub const ALWAYS: Self = Self {
        introduced: None,
        removed: None,
        deprecated: None,
    };

    /// Whether the symbol exists anywhere in `range`: introduced no
    /// later than the maximum and not yet removed at the minimum.
    /// Deprecation never affects existence.
    pub fn exists_in(&self, range: PhpVersionRange) -> bool {
        self.introduced
            .is_none_or(|version| version <= range.maximum)
            && self.removed.is_none_or(|version| version > range.minimum)
    }
}

/// One top-level symbol compiled from the stubs: the fully qualified
/// name (original spelling, no leading backslash), its kind, and its
/// availability window.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StubSymbol {
    pub name: String,
    pub kind: StubSymbolKind,
    pub availability: StubAvailability,
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs`
Expected: PASS (7 tests).

- [ ] **Step 6: Full workspace gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: all green (run `cargo fmt --all` first if formatting complains).

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): model stub symbols and availability"
```

---

### Task 2: The deterministic stub index

**Files:**
- Create: `crates/celerrate_stubs/src/index.rs`
- Modify: `crates/celerrate_stubs/src/lib.rs` (add `mod index;` + export)

**Interfaces:**
- Consumes: `StubSymbol`, `StubSymbolKind`, `StubAvailability`, `StubDeprecation`
  (task 1).
- Produces:
  - `pub struct StubIndex` deriving `Debug, Clone, Default, PartialEq, Eq, Hash`.
  - `pub fn from_symbols(symbols: Vec<StubSymbol>) -> StubIndex` — sorts by
    `(name, kind)` and merges duplicates into their availability union.
  - `pub fn symbols(&self) -> &[StubSymbol]`, `pub fn len(&self) -> usize`,
    `pub fn is_empty(&self) -> bool`.

- [ ] **Step 1: Write the failing tests**

`crates/celerrate_stubs/src/index.rs`:

```rust
#[cfg(test)]
mod tests {
    use celerrate_project::PhpVersion;

    use super::StubIndex;
    use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

    fn symbol(name: &str, kind: StubSymbolKind, availability: StubAvailability) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability,
        }
    }

    #[test]
    fn the_default_index_is_empty() {
        let index = StubIndex::default();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn symbols_sort_by_name_then_kind() {
        let index = StubIndex::from_symbols(vec![
            symbol("strlen", StubSymbolKind::Function, StubAvailability::ALWAYS),
            symbol("Countable", StubSymbolKind::Interface, StubAvailability::ALWAYS),
            symbol("Countable", StubSymbolKind::Class, StubAvailability::ALWAYS),
        ]);
        let names: Vec<(&str, StubSymbolKind)> = index
            .symbols()
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Countable", StubSymbolKind::Class),
                ("Countable", StubSymbolKind::Interface),
                ("strlen", StubSymbolKind::Function),
            ],
        );
    }

    #[test]
    fn duplicate_declarations_merge_into_the_widest_window() {
        // phpstorm-stubs declares some symbols several times with
        // different availability guards: the union wins.
        let first = StubAvailability {
            introduced: Some(PhpVersion::new(8, 0)),
            removed: Some(PhpVersion::new(8, 2)),
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
        };
        let second = StubAvailability {
            introduced: Some(PhpVersion::new(7, 4)),
            removed: Some(PhpVersion::new(8, 4)),
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 0)),
            }),
        };
        let index = StubIndex::from_symbols(vec![
            symbol("foo", StubSymbolKind::Function, first),
            symbol("foo", StubSymbolKind::Function, second),
        ]);
        assert_eq!(index.len(), 1);
        let merged = index.symbols().first().map(|entry| entry.availability);
        assert_eq!(
            merged,
            Some(StubAvailability {
                introduced: Some(PhpVersion::new(7, 4)),
                removed: Some(PhpVersion::new(8, 4)),
                deprecated: Some(StubDeprecation {
                    since: Some(PhpVersion::new(8, 0)),
                }),
            }),
        );
    }

    #[test]
    fn no_constraint_absorbs_any_bound_when_merging() {
        let bounded = StubAvailability {
            introduced: Some(PhpVersion::new(8, 0)),
            removed: Some(PhpVersion::new(8, 2)),
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
        };
        let index = StubIndex::from_symbols(vec![
            symbol("foo", StubSymbolKind::Function, bounded),
            symbol("foo", StubSymbolKind::Function, StubAvailability::ALWAYS),
        ]);
        assert_eq!(
            index.symbols().first().map(|entry| entry.availability),
            Some(StubAvailability::ALWAYS),
        );
    }

    #[test]
    fn same_name_different_kinds_stay_separate() {
        let index = StubIndex::from_symbols(vec![
            symbol("Stringable", StubSymbolKind::Interface, StubAvailability::ALWAYS),
            symbol("Stringable", StubSymbolKind::Class, StubAvailability::ALWAYS),
        ]);
        assert_eq!(index.len(), 2);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs` (after adding `mod index;` and
`pub use index::StubIndex;` to `lib.rs`).
Expected: FAIL to compile ("cannot find struct `StubIndex`").

- [ ] **Step 3: Write the index**

Prepend to `crates/celerrate_stubs/src/index.rs`:

```rust
//! The compiled stub index: every top-level symbol, deterministically
//! sorted, duplicates merged.

use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol};

/// The compiled stub index, sorted by `(name, kind)`. `Eq`-comparable
/// so derived queries over it backdate (salsa early cutoff).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubIndex {
    symbols: Vec<StubSymbol>,
}

impl StubIndex {
    /// Builds the index: sorts by `(name, kind)` and merges duplicate
    /// declarations (phpstorm-stubs declares some symbols several
    /// times, with different availability guards) into their union.
    pub fn from_symbols(mut symbols: Vec<StubSymbol>) -> Self {
        symbols.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.kind.cmp(&right.kind))
        });
        let mut merged: Vec<StubSymbol> = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            match merged.last_mut() {
                Some(last) if last.name == symbol.name && last.kind == symbol.kind => {
                    last.availability =
                        merge_availability(last.availability, symbol.availability);
                }
                _ => merged.push(symbol),
            }
        }
        Self { symbols: merged }
    }

    pub fn symbols(&self) -> &[StubSymbol] {
        &self.symbols
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

/// The union of two availability windows: the widest wins. `None`
/// means "no constraint" and absorbs any bound; the merge is
/// deprecated only when every duplicate is deprecated.
fn merge_availability(left: StubAvailability, right: StubAvailability) -> StubAvailability {
    StubAvailability {
        introduced: match (left.introduced, right.introduced) {
            (Some(first), Some(second)) => Some(first.min(second)),
            _ => None,
        },
        removed: match (left.removed, right.removed) {
            (Some(first), Some(second)) => Some(first.max(second)),
            _ => None,
        },
        deprecated: match (left.deprecated, right.deprecated) {
            (Some(first), Some(second)) => Some(merge_deprecation(first, second)),
            _ => None,
        },
    }
}

fn merge_deprecation(left: StubDeprecation, right: StubDeprecation) -> StubDeprecation {
    StubDeprecation {
        since: match (left.since, right.since) {
            (Some(first), Some(second)) => Some(first.min(second)),
            _ => None,
        },
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs`
Expected: PASS.

- [ ] **Step 5: Full workspace gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): build the deterministic stub index"
```

---

### Task 3: The blob format

**Files:**
- Create: `crates/celerrate_stubs/src/blob.rs`
- Modify: `crates/celerrate_stubs/src/lib.rs` (add `mod blob;` + exports)

**Interfaces:**
- Consumes: `StubIndex::{from_symbols, symbols, len}`, the symbol model.
- Produces:
  - `pub fn encode(index: &StubIndex) -> Vec<u8>`
  - `pub fn decode(blob: &[u8]) -> Result<StubIndex, StubBlobError>`
  - `pub enum StubBlobError { TooShort, BadMagic, UnsupportedFormatVersion(u32),
    ChecksumMismatch, MissingSymbolTable, MalformedSection }` deriving
    `Debug, Clone, Copy, PartialEq, Eq` and implementing `Display` + `Error`.
  - `pub const BLOB_MAGIC: [u8; 8]`, `pub const BLOB_FORMAT_VERSION: u32`,
    `pub const SECTION_SYMBOL_TABLE: u32`, `pub const SECTION_SIGNATURES: u32`
    (reserved), `pub const SECTION_OVERLAYS: u32` (reserved).

**Format specification** (fixed here; every byte is little-endian):

```
offset 0..8    magic  b"CELSTUBS"
offset 8..12   format version u32              (currently 1)
offset 12..20  checksum u64                    (FNV-1a 64 of bytes 20..end)
offset 20..24  section count u32
offset 24..    section table: count entries of (identifier u32, offset u64, length u64)
then           section payloads, offsets absolute from blob start

section identifier 1 = symbol table:
  count u32
  then, per symbol, sorted by (name bytes, kind discriminant):
    kind u8                                    (StubSymbolKind::as_u8)
    flags u8   bit 0: introduced present       bit 1: removed present
               bit 2: deprecated               bit 3: deprecation since present
    introduced (major u8, minor u8)            if bit 0
    removed    (major u8, minor u8)            if bit 1
    deprecation since (major u8, minor u8)     if bit 3
    name length u32, then that many UTF-8 bytes

section identifiers 2 (per-version signature payload, sub-project 3) and
3 (overlay merge: Celerrate refinements, plugin stubs) are reserved.
Unknown section identifiers are skipped by the reader: sections can be
added without a format version bump; the version changes only on
incompatible layout changes.
```

- [ ] **Step 1: Write the failing round-trip and corruption tests**

`crates/celerrate_stubs/src/blob.rs` (test module first):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_project::PhpVersion;

    use super::{
        BLOB_FORMAT_VERSION, BLOB_MAGIC, SECTION_SYMBOL_TABLE, StubBlobError, decode, encode,
        fnv1a64,
    };
    use crate::index::StubIndex;
    use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

    fn sample_index() -> StubIndex {
        StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Random\\Randomizer".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 2)),
                    removed: None,
                    deprecated: None,
                },
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "utf8_encode".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability {
                    introduced: None,
                    removed: Some(PhpVersion::new(8, 4)),
                    deprecated: Some(StubDeprecation {
                        since: Some(PhpVersion::new(8, 2)),
                    }),
                },
            },
            StubSymbol {
                name: "E_ALL".to_owned(),
                kind: StubSymbolKind::Constant,
                availability: StubAvailability {
                    introduced: None,
                    removed: None,
                    deprecated: Some(StubDeprecation { since: None }),
                },
            },
        ])
    }

    #[test]
    fn an_index_round_trips_through_the_blob() {
        let index = sample_index();
        assert_eq!(decode(&encode(&index)), Ok(index));
    }

    #[test]
    fn the_empty_index_round_trips() {
        let index = StubIndex::default();
        assert_eq!(decode(&encode(&index)), Ok(index));
    }

    #[test]
    fn encoding_is_deterministic() {
        assert_eq!(encode(&sample_index()), encode(&sample_index()));
    }

    #[test]
    fn the_blob_starts_with_magic_and_format_version() {
        let blob = encode(&StubIndex::default());
        assert_eq!(blob[0..8], BLOB_MAGIC);
        assert_eq!(blob[8..12], BLOB_FORMAT_VERSION.to_le_bytes());
    }

    #[test]
    fn an_empty_input_is_too_short() {
        assert_eq!(decode(&[]), Err(StubBlobError::TooShort));
    }

    #[test]
    fn a_foreign_blob_is_rejected_by_magic() {
        let mut blob = encode(&sample_index());
        blob[0] = b'X';
        assert_eq!(decode(&blob), Err(StubBlobError::BadMagic));
    }

    #[test]
    fn an_unknown_format_version_is_rejected_before_anything_else_is_read() {
        let mut blob = encode(&sample_index());
        blob[8..12].copy_from_slice(&999u32.to_le_bytes());
        assert_eq!(
            decode(&blob),
            Err(StubBlobError::UnsupportedFormatVersion(999)),
        );
    }

    #[test]
    fn a_flipped_payload_byte_fails_the_checksum() {
        let mut blob = encode(&sample_index());
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert_eq!(decode(&blob), Err(StubBlobError::ChecksumMismatch));
    }

    #[test]
    fn a_truncated_blob_never_panics() {
        let blob = encode(&sample_index());
        for length in 0..blob.len() {
            // Every prefix decodes to a clean error, never a panic.
            assert!(decode(&blob[..length]).is_err(), "prefix length {length}");
        }
    }

    #[test]
    fn unknown_sections_are_skipped_for_forward_compatibility() {
        // Hand-build a version-1 blob whose table carries an unknown
        // section before the symbol table.
        let symbol_table = {
            let encoded = encode(&sample_index());
            // The symbol table of a freshly encoded blob starts right
            // after the header (24) plus one 20-byte table entry.
            encoded[44..].to_vec()
        };
        let unknown_payload = b"future data";
        let table_entries = 2u32;
        let unknown_offset = 24u64 + u64::from(table_entries) * 20;
        let symbol_offset = unknown_offset + unknown_payload.len() as u64;
        let mut blob = Vec::new();
        blob.extend_from_slice(&BLOB_MAGIC);
        blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        blob.extend_from_slice(&[0; 8]);
        blob.extend_from_slice(&table_entries.to_le_bytes());
        blob.extend_from_slice(&777u32.to_le_bytes());
        blob.extend_from_slice(&unknown_offset.to_le_bytes());
        blob.extend_from_slice(&(unknown_payload.len() as u64).to_le_bytes());
        blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
        blob.extend_from_slice(&symbol_offset.to_le_bytes());
        blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
        blob.extend_from_slice(unknown_payload);
        blob.extend_from_slice(&symbol_table);
        let checksum = fnv1a64(&blob[20..]);
        blob[12..20].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(decode(&blob), Ok(sample_index()));
    }

    #[test]
    fn a_blob_without_a_symbol_table_reports_it() {
        let mut blob = Vec::new();
        blob.extend_from_slice(&BLOB_MAGIC);
        blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        blob.extend_from_slice(&[0; 8]);
        blob.extend_from_slice(&0u32.to_le_bytes());
        let checksum = fnv1a64(&blob[20..]);
        blob[12..20].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(decode(&blob), Err(StubBlobError::MissingSymbolTable));
    }

    #[test]
    fn errors_render_for_humans() {
        assert_eq!(
            StubBlobError::UnsupportedFormatVersion(7).to_string(),
            "unsupported stub blob format version 7",
        );
        assert!(!StubBlobError::ChecksumMismatch.to_string().is_empty());
    }
}
```

Note for the implementer: the `44` in the forward-compatibility test is
`24` (header + section count) + `20` (one table entry); if the header layout
ever changes, this test changes with it — that is the point.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs` (after adding `mod blob;` and
`pub use blob::{BLOB_FORMAT_VERSION, BLOB_MAGIC, SECTION_OVERLAYS, SECTION_SIGNATURES, SECTION_SYMBOL_TABLE, StubBlobError, decode, encode};`
to `lib.rs`).
Expected: FAIL to compile.

- [ ] **Step 3: Write the format**

Prepend to `crates/celerrate_stubs/src/blob.rs`:

```rust
//! The hand-written stub blob format: versioned, checksummed,
//! sectioned, byte-deterministic. The reader is tolerant end to end:
//! corruption yields a [`StubBlobError`], never a panic.

use core::fmt;

use celerrate_project::PhpVersion;

use crate::index::StubIndex;
use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

pub const BLOB_MAGIC: [u8; 8] = *b"CELSTUBS";

/// Bumped only on incompatible layout changes. Additive evolution goes
/// through new sections, which old readers skip.
pub const BLOB_FORMAT_VERSION: u32 = 1;

/// The top-level symbol table: the one live section.
pub const SECTION_SYMBOL_TABLE: u32 = 1;

/// Reserved: per-version signature deltas (sub-project 3).
pub const SECTION_SIGNATURES: u32 = 2;

/// Reserved: the overlay merge point (Celerrate refinements, plugin
/// stubs).
pub const SECTION_OVERLAYS: u32 = 3;

/// Why a blob failed to decode. Every variant is a clean rejection:
/// the composition root falls back to an empty index and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubBlobError {
    TooShort,
    BadMagic,
    UnsupportedFormatVersion(u32),
    ChecksumMismatch,
    MissingSymbolTable,
    MalformedSection,
}

impl fmt::Display for StubBlobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(formatter, "stub blob is truncated"),
            Self::BadMagic => write!(formatter, "not a Celerrate stub blob"),
            Self::UnsupportedFormatVersion(version) => {
                write!(formatter, "unsupported stub blob format version {version}")
            }
            Self::ChecksumMismatch => write!(formatter, "stub blob checksum mismatch"),
            Self::MissingSymbolTable => {
                write!(formatter, "stub blob carries no symbol table section")
            }
            Self::MalformedSection => write!(formatter, "malformed stub blob section"),
        }
    }
}

impl std::error::Error for StubBlobError {}

/// Encodes the index. Deterministic: the same index always produces
/// the same bytes (the index is already sorted and merged).
pub fn encode(index: &StubIndex) -> Vec<u8> {
    let symbol_table = encode_symbol_table(index);
    let table_entries = 1u32;
    let payload_offset = 24u64 + u64::from(table_entries) * 20;
    let mut blob = Vec::with_capacity(symbol_table.len() + 64);
    blob.extend_from_slice(&BLOB_MAGIC);
    blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
    blob.extend_from_slice(&[0; 8]); // checksum, patched below
    blob.extend_from_slice(&table_entries.to_le_bytes());
    blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
    blob.extend_from_slice(&payload_offset.to_le_bytes());
    blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
    blob.extend_from_slice(&symbol_table);
    let checksum = fnv1a64(blob.get(20..).unwrap_or_default());
    if let Some(slot) = blob.get_mut(12..20) {
        slot.copy_from_slice(&checksum.to_le_bytes());
    }
    blob
}

fn encode_symbol_table(index: &StubIndex) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(index.len() as u32).to_le_bytes());
    for symbol in index.symbols() {
        bytes.push(symbol.kind.as_u8());
        let availability = symbol.availability;
        let since = availability.deprecated.and_then(|deprecation| deprecation.since);
        let mut flags = 0u8;
        if availability.introduced.is_some() {
            flags |= 1;
        }
        if availability.removed.is_some() {
            flags |= 1 << 1;
        }
        if availability.deprecated.is_some() {
            flags |= 1 << 2;
        }
        if since.is_some() {
            flags |= 1 << 3;
        }
        bytes.push(flags);
        for version in [availability.introduced, availability.removed, since]
            .into_iter()
            .flatten()
        {
            bytes.extend_from_slice(&[version.major, version.minor]);
        }
        bytes.extend_from_slice(&(symbol.name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(symbol.name.as_bytes());
    }
    bytes
}

/// Decodes a blob. Tolerant: every malformation is an error value.
pub fn decode(blob: &[u8]) -> Result<StubIndex, StubBlobError> {
    let mut header = Reader::new(blob);
    let magic = header.take(8).ok_or(StubBlobError::TooShort)?;
    if magic != BLOB_MAGIC {
        return Err(StubBlobError::BadMagic);
    }
    let format_version = header.u32().ok_or(StubBlobError::TooShort)?;
    if format_version != BLOB_FORMAT_VERSION {
        return Err(StubBlobError::UnsupportedFormatVersion(format_version));
    }
    let checksum = header.u64().ok_or(StubBlobError::TooShort)?;
    let checksummed = blob.get(20..).ok_or(StubBlobError::TooShort)?;
    if fnv1a64(checksummed) != checksum {
        return Err(StubBlobError::ChecksumMismatch);
    }
    let section_count = header.u32().ok_or(StubBlobError::TooShort)?;
    let mut symbol_table: Option<&[u8]> = None;
    for _ in 0..section_count {
        let identifier = header.u32().ok_or(StubBlobError::TooShort)?;
        let offset = header.u64().ok_or(StubBlobError::TooShort)?;
        let length = header.u64().ok_or(StubBlobError::TooShort)?;
        let end = offset.checked_add(length).ok_or(StubBlobError::MalformedSection)?;
        let start = usize::try_from(offset).map_err(|_| StubBlobError::MalformedSection)?;
        let end = usize::try_from(end).map_err(|_| StubBlobError::MalformedSection)?;
        let section = blob.get(start..end).ok_or(StubBlobError::MalformedSection)?;
        if identifier == SECTION_SYMBOL_TABLE {
            symbol_table = Some(section);
        }
        // Unknown identifiers are skipped: newer blobs that only add
        // sections stay readable without a format version bump.
    }
    decode_symbol_table(symbol_table.ok_or(StubBlobError::MissingSymbolTable)?)
}

fn decode_symbol_table(bytes: &[u8]) -> Result<StubIndex, StubBlobError> {
    let mut reader = Reader::new(bytes);
    let count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut symbols = Vec::new();
    for _ in 0..count {
        let kind = reader
            .u8()
            .and_then(StubSymbolKind::from_u8)
            .ok_or(StubBlobError::MalformedSection)?;
        let flags = reader.u8().ok_or(StubBlobError::MalformedSection)?;
        let introduced = if flags & 1 != 0 {
            Some(reader.version().ok_or(StubBlobError::MalformedSection)?)
        } else {
            None
        };
        let removed = if flags & (1 << 1) != 0 {
            Some(reader.version().ok_or(StubBlobError::MalformedSection)?)
        } else {
            None
        };
        let since = if flags & (1 << 3) != 0 {
            Some(reader.version().ok_or(StubBlobError::MalformedSection)?)
        } else {
            None
        };
        let deprecated = (flags & (1 << 2) != 0).then_some(StubDeprecation { since });
        let name_length = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let name_length =
            usize::try_from(name_length).map_err(|_| StubBlobError::MalformedSection)?;
        let name_bytes = reader.take(name_length).ok_or(StubBlobError::MalformedSection)?;
        let name = core::str::from_utf8(name_bytes)
            .map_err(|_| StubBlobError::MalformedSection)?
            .to_owned();
        symbols.push(StubSymbol {
            name,
            kind,
            availability: StubAvailability {
                introduced,
                removed,
                deprecated,
            },
        });
    }
    Ok(StubIndex::from_symbols(symbols))
}

/// FNV-1a, 64-bit: six lines beat a checksum dependency.
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A cursor over borrowed bytes; every read is checked, no indexing.
struct Reader<'blob> {
    bytes: &'blob [u8],
}

impl<'blob> Reader<'blob> {
    fn new(bytes: &'blob [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, count: usize) -> Option<&'blob [u8]> {
        let (head, tail) = self.bytes.split_at_checked(count)?;
        self.bytes = tail;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn version(&mut self) -> Option<PhpVersion> {
        let bytes = self.take(2)?;
        Some(PhpVersion::new(*bytes.first()?, *bytes.get(1)?))
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs`
Expected: PASS (including the exhaustive truncated-prefix sweep).

- [ ] **Step 5: Full workspace gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): encode and decode the versioned stub blob"
```

---

### Task 4: Extract top-level declarations from stub files

**Files:**
- Modify: `crates/celerrate_stubs/Cargo.toml` (compiler feature, optional +
  dev dependency on `celerrate_syntax`)
- Create: `crates/celerrate_stubs/src/compiler/mod.rs`
- Create: `crates/celerrate_stubs/src/compiler/extract.rs`
- Modify: `crates/celerrate_stubs/src/lib.rs` (gated `pub mod compiler;`)

**Interfaces:**
- Consumes: `celerrate_syntax::{parse, SyntaxKind, SyntaxNode, SyntaxToken}`,
  `celerrate_syntax::ast` (typed AST: `SourceFile::statements()`,
  `Statement` enum, `Block::statements()`, `NamespaceDeclaration::{name, block}`,
  `ClassDeclaration::name_token()`, `InterfaceDeclaration::name_token()`,
  `TraitDeclaration::name_token()`, `EnumDeclaration::name_token()`,
  `FunctionDeclaration::name_token()`, `ConstantDeclaration::constant_elements()`,
  `ConstantElement::name_token()`, `ExpressionStatement::expression()`,
  `CallExpression::{callee, argument_list}`, `NameExpression::name()`,
  `ArgumentList::arguments()`, `Argument::{label_token, expression}`,
  `AstNode::{cast, syntax}`), plus `StubSymbol`/`StubSymbolKind` (task 1).
- Produces:
  - `pub mod compiler` gated by `#[cfg(any(test, feature = "compiler"))]`.
  - `pub struct Extraction { pub symbols: Vec<StubSymbol>, pub had_parse_errors: bool }`
  - `pub fn extract(text: &str) -> Extraction` in `compiler::extract`.
  - Availability metadata is task 5: in this task every extracted symbol carries
    `StubAvailability::ALWAYS` through a private
    `fn availability_of(node: &SyntaxNode) -> StubAvailability` stub that task 5
    replaces.

- [ ] **Step 1: Wire the feature and the gated module**

In `crates/celerrate_stubs/Cargo.toml`, replace the `[dependencies]` section with:

```toml
[features]
# The stub compiler: parses the pinned phpstorm-stubs snapshot. Behind a
# feature so the runtime dependency graph stays free of the parser; the
# same code is a dev-dependency so its tests run in the default suite.
compiler = ["dep:celerrate_syntax"]

[dependencies]
celerrate_project = { path = "../celerrate_project" }
celerrate_syntax = { path = "../celerrate_syntax", optional = true }

[dev-dependencies]
celerrate_syntax = { path = "../celerrate_syntax" }
```

In `crates/celerrate_stubs/src/lib.rs`, after the existing `mod` items add:

```rust
#[cfg(any(test, feature = "compiler"))]
pub mod compiler;
```

`crates/celerrate_stubs/src/compiler/mod.rs`:

```rust
//! The stub compiler: turns the pinned phpstorm-stubs snapshot into
//! the committed blob. Compiled only for tests and under the
//! `compiler` feature — the runtime never parses PHP.

pub mod extract;
```

- [ ] **Step 2: Write the failing extraction tests**

`crates/celerrate_stubs/src/compiler/extract.rs` (test module first):

```rust
#[cfg(test)]
mod tests {
    use super::{Extraction, extract};
    use crate::symbol::StubSymbolKind;

    fn names_and_kinds(extraction: &Extraction) -> Vec<(String, StubSymbolKind)> {
        extraction
            .symbols
            .iter()
            .map(|symbol| (symbol.name.clone(), symbol.kind))
            .collect()
    }

    #[test]
    fn every_top_level_declaration_kind_is_extracted() {
        let extraction = extract(
            "<?php\n\
             class Exception {}\n\
             interface Traversable {}\n\
             trait Helper {}\n\
             enum Suit {}\n\
             function strlen(string $string): int {}\n\
             const PHP_EOL = \"\\n\";\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("Exception".to_owned(), StubSymbolKind::Class),
                ("Traversable".to_owned(), StubSymbolKind::Interface),
                ("Helper".to_owned(), StubSymbolKind::Trait),
                ("Suit".to_owned(), StubSymbolKind::Enum),
                ("strlen".to_owned(), StubSymbolKind::Function),
                ("PHP_EOL".to_owned(), StubSymbolKind::Constant),
            ],
        );
        assert!(!extraction.had_parse_errors);
    }

    #[test]
    fn a_statement_form_namespace_qualifies_everything_after_it() {
        let extraction = extract(
            "<?php\n\
             namespace Random;\n\
             class Randomizer {}\n\
             const SEED = 1;\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("Random\\Randomizer".to_owned(), StubSymbolKind::Class),
                ("Random\\SEED".to_owned(), StubSymbolKind::Constant),
            ],
        );
    }

    #[test]
    fn brace_form_namespaces_scope_their_block_only() {
        let extraction = extract(
            "<?php\n\
             namespace Ds { class Vector {} }\n\
             namespace { function outside() {} }\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("Ds\\Vector".to_owned(), StubSymbolKind::Class),
                ("outside".to_owned(), StubSymbolKind::Function),
            ],
        );
    }

    #[test]
    fn sequential_statement_form_namespaces_switch_the_prefix() {
        let extraction = extract(
            "<?php\n\
             namespace First;\n\
             function one() {}\n\
             namespace Second;\n\
             function two() {}\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("First\\one".to_owned(), StubSymbolKind::Function),
                ("Second\\two".to_owned(), StubSymbolKind::Function),
            ],
        );
    }

    #[test]
    fn a_grouped_constant_declaration_yields_one_symbol_per_element() {
        let extraction = extract("<?php const A = 1, B = 2;");
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("A".to_owned(), StubSymbolKind::Constant),
                ("B".to_owned(), StubSymbolKind::Constant),
            ],
        );
    }

    #[test]
    fn define_calls_with_a_literal_name_declare_global_constants() {
        let extraction = extract(
            "<?php\n\
             namespace Ignored;\n\
             define('E_ALL', 32767);\n\
             define(\"E_STRICT\", 2048);\n\
             define($dynamic, 1);\n\
             define(E_ALL, 1);\n",
        );
        // define() names the constant absolutely, whatever the current
        // namespace; dynamic names are skipped.
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("E_ALL".to_owned(), StubSymbolKind::Constant),
                ("E_STRICT".to_owned(), StubSymbolKind::Constant),
            ],
        );
    }

    #[test]
    fn nested_and_conditional_declarations_are_not_top_level() {
        let extraction = extract(
            "<?php\n\
             class Outer { public function method(): void {} }\n\
             if (true) { function guarded() {} }\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![("Outer".to_owned(), StubSymbolKind::Class)],
        );
    }

    #[test]
    fn malformed_input_extracts_what_the_parser_recovered_and_reports_errors() {
        let extraction = extract("<?php class Broken { function ok() {}");
        assert!(extraction.had_parse_errors);
        assert_eq!(
            names_and_kinds(&extraction),
            vec![("Broken".to_owned(), StubSymbolKind::Class)],
        );
    }

    #[test]
    fn empty_and_html_only_files_extract_nothing() {
        assert!(extract("").symbols.is_empty());
        assert!(extract("plain text, no PHP").symbols.is_empty());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs`
Expected: FAIL to compile ("cannot find function `extract`").

- [ ] **Step 4: Write the extraction walker**

Prepend to `crates/celerrate_stubs/src/compiler/extract.rs`:

```rust
//! Extraction: one stub file's text in, its top-level symbols out.
//! Tolerant end to end: malformed PHP still yields whatever
//! declarations the error-resilient parser recovered.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxNode, SyntaxToken};

use crate::symbol::{StubAvailability, StubSymbol, StubSymbolKind};

/// The result of extracting one stub file. `had_parse_errors` lets the
/// compiler count warnings without ever failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extraction {
    pub symbols: Vec<StubSymbol>,
    pub had_parse_errors: bool,
}

/// Extracts every top-level symbol of one stub file.
pub fn extract(text: &str) -> Extraction {
    let parse = celerrate_syntax::parse(text);
    let mut symbols = Vec::new();
    if let Some(file) = ast::SourceFile::cast(parse.tree()) {
        collect(file.statements(), "", &mut symbols);
    }
    Extraction {
        symbols,
        had_parse_errors: !parse.diagnostics().is_empty(),
    }
}

fn collect(
    statements: ast::AstChildren<ast::Statement>,
    initial_namespace: &str,
    symbols: &mut Vec<StubSymbol>,
) {
    // Statement-form `namespace Foo;` switches the prefix for the
    // statements that follow it, so the namespace is walk state.
    let mut namespace = initial_namespace.to_owned();
    for statement in statements {
        match statement {
            ast::Statement::NamespaceDeclaration(declaration) => {
                let name = declaration
                    .name()
                    .map(|name| name_text(&name))
                    .unwrap_or_default();
                match declaration.block() {
                    Some(block) => collect(block.statements(), &name, symbols),
                    None => namespace = name,
                }
            }
            ast::Statement::ClassDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Class,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::InterfaceDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Interface,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::TraitDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Trait,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::EnumDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Enum,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::FunctionDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Function,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::ConstantDeclaration(declaration) => {
                let availability = availability_of(declaration.syntax());
                for element in declaration.constant_elements() {
                    if let Some(name_token) = element.name_token() {
                        symbols.push(StubSymbol {
                            name: qualify(&namespace, name_token.text()),
                            kind: StubSymbolKind::Constant,
                            availability,
                        });
                    }
                }
            }
            ast::Statement::ExpressionStatement(statement) => {
                if let Some(symbol) = define_constant(&statement) {
                    symbols.push(symbol);
                }
            }
            _ => {}
        }
    }
}

fn push_named(
    symbols: &mut Vec<StubSymbol>,
    namespace: &str,
    kind: StubSymbolKind,
    name_token: Option<SyntaxToken>,
    node: &SyntaxNode,
) {
    let Some(name_token) = name_token else { return };
    symbols.push(StubSymbol {
        name: qualify(namespace, name_token.text()),
        kind,
        availability: availability_of(node),
    });
}

/// A `define('NAME', ...)` statement with a literal string name: a
/// global constant declaration, whatever the current namespace.
/// Dynamic names are out of scope, like every dynamic reference.
fn define_constant(statement: &ast::ExpressionStatement) -> Option<StubSymbol> {
    let ast::Expression::CallExpression(call) = statement.expression()? else {
        return None;
    };
    let ast::Expression::NameExpression(callee) = call.callee()? else {
        return None;
    };
    // Function names are case-insensitive in PHP.
    let callee_name = name_text(&callee.name()?);
    if !callee_name.trim_start_matches('\\').eq_ignore_ascii_case("define") {
        return None;
    }
    let first_argument = call.argument_list()?.arguments().next()?;
    let name = string_literal(&first_argument.expression()?)?;
    Some(StubSymbol {
        name: name.trim_start_matches('\\').to_owned(),
        kind: StubSymbolKind::Constant,
        availability: availability_of(statement.syntax()),
    })
}

/// The text of a `Name` node with any interior trivia stripped.
fn name_text(name: &ast::Name) -> String {
    name.syntax()
        .text()
        .to_string()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// The content of a simple single- or double-quoted string literal;
/// anything else (interpolation, heredoc, concatenation) is `None`.
fn string_literal(expression: &ast::Expression) -> Option<String> {
    let ast::Expression::Literal(literal) = expression else {
        return None;
    };
    let text = literal.syntax().text().to_string();
    let trimmed = text.trim();
    let unquoted = trimmed
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .or_else(|| {
            trimmed
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })?;
    Some(unquoted.to_owned())
}

fn qualify(namespace: &str, name: &str) -> String {
    let name = name.trim_start_matches('\\');
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}\\{name}")
    }
}

/// Availability metadata arrives with the next task; until then every
/// symbol is unconstrained.
fn availability_of(_node: &SyntaxNode) -> StubAvailability {
    StubAvailability::ALWAYS
}
```

Implementation notes for this step:

- `ast::Expression`, `ast::Statement` are generated enums; the `let ... else`
  destructuring above compiles because their variants wrap the node structs.
- If `name_text` on `NameExpression` needs a different route (for example the
  callee is parsed as something other than `NameExpression` for a bare
  `define(...)` call), inspect the actual tree with
  `celerrate_syntax::parse("<?php define('X', 1);").tree()` debug output in a
  scratch test and adjust the destructuring — keep the tests as written; they
  define the behavior.
- `availability_of` intentionally ignores its argument for now; task 5 gives it
  its real body. Silence the unused-parameter warning with the underscore name,
  exactly as written.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs`
Expected: PASS.

- [ ] **Step 6: Verify the feature-gated build also compiles**

Run: `cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Full workspace gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): extract top-level declarations from stub files"
```

---

### Task 5: Availability metadata from doc tags and attributes

**Files:**
- Modify: `crates/celerrate_stubs/src/compiler/extract.rs` (replace the
  `availability_of` stub, add the metadata readers and their tests)

**Interfaces:**
- Consumes: `SyntaxKind::{DocComment, Whitespace, LineComment, BlockComment}`,
  `ast::{AttributeGroup, Attribute, Argument}`, `StubAvailability`,
  `StubDeprecation`, `PhpVersion` (task 1).
- Produces: the real `fn availability_of(node: &SyntaxNode) -> StubAvailability`,
  reading, in this order (doc tags first, attributes override nothing — each
  field is set once, first source wins):
  - Doc tags on the declaration's leading `/** ... */` comment:
    `@since X.Y` → `introduced`, `@removed X.Y` → `removed`,
    `@deprecated [X.Y]` → `deprecated` (version optional).
  - Attributes on the declaration: `#[PhpStormStubsElementAvailable(from: 'X.Y',
    to: 'X.Y')]` (labeled or positional: first positional is `from`, second is
    `to`) → `introduced` / `removed = successor(to)`;
    `#[Deprecated(since: 'X.Y')]` (JetBrains) → `deprecated`.
    Attribute names match on their last segment, ASCII-case-insensitively
    (class names are case-insensitive in PHP).

- [ ] **Step 1: Write the failing metadata tests**

Append to the test module of `crates/celerrate_stubs/src/compiler/extract.rs`:

```rust
    use celerrate_project::PhpVersion;

    use crate::symbol::{StubAvailability, StubDeprecation};

    fn only_availability(source: &str) -> StubAvailability {
        let extraction = extract(source);
        assert_eq!(extraction.symbols.len(), 1, "expected one symbol in {source}");
        extraction
            .symbols
            .first()
            .map(|symbol| symbol.availability)
            .unwrap_or(StubAvailability::ALWAYS)
    }

    #[test]
    fn doc_tags_set_the_availability_window() {
        let availability = only_availability(
            "<?php\n\
             /**\n\
              * Frobnicates.\n\
              * @since 8.1\n\
              * @deprecated 8.3\n\
              */\n\
             function frobnicate() {}\n",
        );
        assert_eq!(
            availability,
            StubAvailability {
                introduced: Some(PhpVersion::new(8, 1)),
                removed: None,
                deprecated: Some(StubDeprecation {
                    since: Some(PhpVersion::new(8, 3)),
                }),
            },
        );
    }

    #[test]
    fn a_removed_tag_sets_the_removal_version() {
        let availability = only_availability(
            "<?php\n/** @removed 8.0 */\nfunction create_function() {}\n",
        );
        assert_eq!(availability.removed, Some(PhpVersion::new(8, 0)));
    }

    #[test]
    fn a_deprecation_without_a_version_is_recorded_as_unversioned() {
        let availability =
            only_availability("<?php\n/** @deprecated */\nfunction old_thing() {}\n");
        assert_eq!(availability.deprecated, Some(StubDeprecation { since: None }));
    }

    #[test]
    fn patch_components_and_suffixes_are_truncated() {
        let availability = only_availability(
            "<?php\n/** @since 5.3.0 */\nfunction with_patch() {}\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(5, 3)));
    }

    #[test]
    fn unparseable_versions_are_ignored_not_fatal() {
        let availability = only_availability(
            "<?php\n/** @since forever */\nfunction murky() {}\n",
        );
        assert_eq!(availability, StubAvailability::ALWAYS);
    }

    #[test]
    fn a_doc_comment_only_binds_to_the_declaration_that_follows_it() {
        let extraction = extract(
            "<?php\n\
             /** @since 8.1 */\n\
             function first() {}\n\
             function second() {}\n",
        );
        let introduced: Vec<Option<PhpVersion>> = extraction
            .symbols
            .iter()
            .map(|symbol| symbol.availability.introduced)
            .collect();
        assert_eq!(introduced, vec![Some(PhpVersion::new(8, 1)), None]);
    }

    #[test]
    fn the_availability_attribute_sets_the_window_with_labels() {
        let availability = only_availability(
            "<?php\n\
             #[PhpStormStubsElementAvailable(from: '8.2')]\n\
             function fresh() {}\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 2)));
    }

    #[test]
    fn the_availability_attribute_accepts_positional_arguments() {
        let availability = only_availability(
            "<?php\n\
             #[PhpStormStubsElementAvailable('7.0', '7.4')]\n\
             function spanned() {}\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(7, 0)));
        // `to: 7.4` means present up to 7.4: gone in the successor, 8.0.
        assert_eq!(availability.removed, Some(PhpVersion::new(8, 0)));
    }

    #[test]
    fn the_to_bound_uses_the_real_php_release_line() {
        let availability = only_availability(
            "<?php\n\
             #[PhpStormStubsElementAvailable(from: '8.0', to: '8.1')]\n\
             function narrow() {}\n",
        );
        assert_eq!(availability.removed, Some(PhpVersion::new(8, 2)));
    }

    #[test]
    fn the_deprecated_attribute_matches_by_last_segment_and_reads_since() {
        let availability = only_availability(
            "<?php\n\
             #[\\JetBrains\\PhpStorm\\Deprecated(reason: 'use something else', since: '8.1')]\n\
             function dated() {}\n",
        );
        assert_eq!(
            availability.deprecated,
            Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
        );
    }

    #[test]
    fn a_doc_comment_reaches_its_declaration_across_the_attributes() {
        let availability = only_availability(
            "<?php\n\
             /** @since 8.1 */\n\
             #[PhpStormStubsElementAvailable(from: '8.2')]\n\
             function both() {}\n",
        );
        // Each field is set once, first source wins: the doc tag came first.
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 1)));
    }

    #[test]
    fn a_define_call_carries_its_leading_doc_metadata() {
        let availability = only_availability(
            "<?php\n/** @since 8.4 */\ndefine('BRAND_NEW', 1);\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 4)));
    }

    #[test]
    fn line_comments_between_doc_and_declaration_do_not_break_the_binding() {
        let availability = only_availability(
            "<?php\n\
             /** @since 8.1 */\n\
             // implementation note\n\
             function commented() {}\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 1)));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs`
Expected: the new tests FAIL (every availability comes back `ALWAYS`), the
task 4 tests still pass.

- [ ] **Step 3: Implement the metadata readers**

In `crates/celerrate_stubs/src/compiler/extract.rs`, replace the
`availability_of` stub with the real implementation and add its helpers:

```rust
use celerrate_project::PhpVersion;
use celerrate_syntax::SyntaxKind;

use crate::symbol::StubDeprecation;

/// Availability from the declaration's own metadata: leading doc tags,
/// then attributes. Each field is set once; the first source wins.
fn availability_of(node: &SyntaxNode) -> StubAvailability {
    let mut availability = doc_availability(node);
    apply_attributes(node, &mut availability);
    availability
}

fn doc_availability(node: &SyntaxNode) -> StubAvailability {
    let mut availability = StubAvailability::ALWAYS;
    let Some(comment) = leading_doc_comment(node) else {
        return availability;
    };
    for line in comment.text().lines() {
        let line = line.trim_start_matches(['/', '*', ' ', '\t']).trim_end();
        if let Some(rest) = line.strip_prefix("@since") {
            if availability.introduced.is_none() {
                availability.introduced = parse_version(rest);
            }
        } else if let Some(rest) = line.strip_prefix("@removed") {
            if availability.removed.is_none() {
                availability.removed = parse_version(rest);
            }
        } else if let Some(rest) = line.strip_prefix("@deprecated") {
            if availability.deprecated.is_none() {
                availability.deprecated = Some(StubDeprecation {
                    since: parse_version(rest),
                });
            }
        }
    }
    availability
}

/// The closest `/** ... */` before the node, separated from it only by
/// trivia. Robust to either trivia attachment (inside the node's
/// leading trivia or before the node): the walk starts at the first
/// meaningful token and goes backwards.
fn leading_doc_comment(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut token = first_meaningful_token(node)?.prev_token();
    while let Some(current) = token {
        match current.kind() {
            SyntaxKind::DocComment => return Some(current),
            SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment => {
                token = current.prev_token();
            }
            _ => return None,
        }
    }
    None
}

fn first_meaningful_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut token = node.first_token()?;
    while matches!(
        token.kind(),
        SyntaxKind::Whitespace
            | SyntaxKind::LineComment
            | SyntaxKind::BlockComment
            | SyntaxKind::DocComment
    ) {
        token = token.next_token()?;
    }
    Some(token)
}

fn apply_attributes(node: &SyntaxNode, availability: &mut StubAvailability) {
    for group in node.children().filter_map(ast::AttributeGroup::cast) {
        for attribute in group.attributes() {
            let Some(name) = attribute.name() else { continue };
            let name = name_text(&name);
            let simple = name.rsplit('\\').next().unwrap_or(&name);
            if simple.eq_ignore_ascii_case("PhpStormStubsElementAvailable") {
                apply_element_available(&attribute, availability);
            } else if simple.eq_ignore_ascii_case("Deprecated") {
                if availability.deprecated.is_none() {
                    availability.deprecated = Some(StubDeprecation {
                        since: labeled_version(&attribute, "since"),
                    });
                }
            }
        }
    }
}

/// `#[PhpStormStubsElementAvailable(from:, to:)]`: labeled or
/// positional (first positional is `from`, second is `to`). `to` is
/// the last version that still has the symbol, so removal is its
/// successor.
fn apply_element_available(attribute: &ast::Attribute, availability: &mut StubAvailability) {
    let Some(argument_list) = attribute.argument_list() else {
        return;
    };
    let mut positional_index = 0usize;
    for argument in argument_list.arguments() {
        let label = argument.label_token().map(|token| token.text().to_owned());
        let version = argument
            .expression()
            .as_ref()
            .and_then(string_literal)
            .as_deref()
            .and_then(parse_version);
        let role = match label.as_deref() {
            Some("from") => Some(0),
            Some("to") => Some(1),
            Some(_) => None,
            None => {
                let role = positional_index;
                positional_index += 1;
                (role < 2).then_some(role)
            }
        };
        match (role, version) {
            (Some(0), Some(version)) if availability.introduced.is_none() => {
                availability.introduced = Some(version);
            }
            (Some(1), Some(version)) if availability.removed.is_none() => {
                availability.removed = Some(successor(version));
            }
            _ => {}
        }
    }
}

fn labeled_version(attribute: &ast::Attribute, label: &str) -> Option<PhpVersion> {
    attribute
        .argument_list()?
        .arguments()
        .find(|argument| {
            argument
                .label_token()
                .is_some_and(|token| token.text() == label)
        })?
        .expression()
        .as_ref()
        .and_then(string_literal)
        .as_deref()
        .and_then(parse_version)
}

/// Parses `8.1`, `8.1.2`, `8.1RC1`, or `8` into a major.minor version;
/// anything unparseable is `None`, never an error.
fn parse_version(text: &str) -> Option<PhpVersion> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse::<u8>().ok()?;
    let minor = parts.next().map_or(0, leading_digits);
    Some(PhpVersion::new(major, minor))
}

fn leading_digits(part: &str) -> u8 {
    let digits: String = part
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

/// The first version after `version` on PHP's actual release line:
/// minors increment, except the two historical jumps (5.6 → 7.0,
/// PHP 6 never shipped, and 7.4 → 8.0).
fn successor(version: PhpVersion) -> PhpVersion {
    match (version.major, version.minor) {
        (5, 6) => PhpVersion::new(7, 0),
        (7, 4) => PhpVersion::new(8, 0),
        (major, minor) => PhpVersion::new(major, minor.saturating_add(1)),
    }
}
```

Adjust the existing `string_literal` signature if needed so both call sites
agree (`fn string_literal(expression: &ast::Expression) -> Option<String>` as
written in task 4 — the task 4 `define_constant` call site already passes a
reference). `parse_version` takes `&str`: the `@since` doc-tag call sites pass
the tag's trailing text directly (`parse_version(rest)` works because `rest`
starts with whitespace and `parse_version` trims). If
`char::is_ascii_digit` does not satisfy `take_while`'s argument type, use the
closure form `|character: &char| character.is_ascii_digit()`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs`
Expected: PASS (all extraction tests, old and new).

If `a_doc_comment_reaches_its_declaration_across_the_attributes` or the
doc-binding tests fail because trivia attachment differs from the assumption
(the doc comment could sit inside the declaration node or before it), fix
`leading_doc_comment` / `first_meaningful_token` until the tests pass — the
tests define the contract, do not weaken them.

- [ ] **Step 5: Full workspace gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings && cargo fmt --all -- --check`

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): read availability metadata from doc tags and attributes"
```

---

### Task 6: The snapshot walk and the stub-compiler binary

**Files:**
- Create: `crates/celerrate_stubs/src/compiler/snapshot.rs`
- Create: `crates/celerrate_stubs/src/bin/stub-compiler.rs`
- Modify: `crates/celerrate_stubs/src/compiler/mod.rs` (add `pub mod snapshot;`)
- Modify: `crates/celerrate_stubs/Cargo.toml` (declare the binary)
- Modify: `crates/celerrate_stubs/Cargo.toml` dev-dependencies (add `tempfile`)

**Interfaces:**
- Consumes: `compiler::extract::extract` (task 4/5), `StubIndex::from_symbols`,
  `blob::encode`.
- Produces:
  - `pub fn stub_files(snapshot: &Path) -> std::io::Result<Vec<PathBuf>>` in
    `compiler::snapshot`: every `*.php` file under `snapshot` (ASCII-case-
    insensitive extension), recursively, skipping any directory named `.git`,
    `.github`, `.idea`, or `tests`, sorted by their relative path components
    (platform-independent order).
  - The `stub-compiler` binary:
    `stub-compiler <snapshot-directory> <output-blob-path> [--check]`
    - without `--check`: compiles and writes the blob, prints a one-line summary
      (files, symbols, warnings, bytes), exit 0; exit 1 on any hard error
      (unreadable snapshot directory, no stub files found, unwritable output).
    - with `--check`: compiles and byte-compares against the existing file;
      exit 0 when identical, exit 1 with a "stale blob" message otherwise.
    - a stub file that fails to read or parses with diagnostics produces a
      warning on stderr and partial extraction, never a failure.

- [ ] **Step 1: Write the failing walk tests**

`crates/celerrate_stubs/src/compiler/snapshot.rs`:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::fs;
    use std::path::Path;

    use super::stub_files;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn the_walk_finds_php_files_recursively_in_sorted_order() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("standard/basic.php"), "<?php");
        write(&root.path().join("Core/Core.php"), "<?php");
        write(&root.path().join("Core/deep/nested.PHP"), "<?php");
        write(&root.path().join("README.md"), "not php");
        let files = stub_files(root.path()).unwrap();
        let relative: Vec<String> = files
            .iter()
            .map(|path| {
                path.strip_prefix(root.path())
                    .unwrap()
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        assert_eq!(
            relative,
            vec!["Core/Core.php", "Core/deep/nested.PHP", "standard/basic.php"],
        );
    }

    #[test]
    fn tool_and_test_directories_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("standard/basic.php"), "<?php");
        write(&root.path().join(".git/objects/fake.php"), "<?php");
        write(&root.path().join(".github/workflow.php"), "<?php");
        write(&root.path().join(".idea/config.php"), "<?php");
        write(&root.path().join("tests/StubsTest.php"), "<?php");
        let files = stub_files(root.path()).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn a_missing_root_is_an_error_not_a_panic() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("absent");
        assert!(stub_files(&missing).is_err());
    }
}
```

Add `tempfile = { workspace = true }` to `[dev-dependencies]` in
`crates/celerrate_stubs/Cargo.toml` (it is already a workspace dependency), and
`pub mod snapshot;` to `crates/celerrate_stubs/src/compiler/mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs`
Expected: FAIL to compile ("cannot find function `stub_files`").

- [ ] **Step 3: Write the walk**

Prepend to `crates/celerrate_stubs/src/compiler/snapshot.rs`:

```rust
//! The snapshot walk: every stub file of the pinned phpstorm-stubs
//! checkout, in an order that is deterministic across platforms.

use std::path::{Path, PathBuf};

/// Directories that carry no stubs: the repository's own tooling.
const SKIPPED_DIRECTORIES: [&str; 4] = [".git", ".github", ".idea", "tests"];

/// Every `*.php` file under `snapshot` (extension matched ASCII-case-
/// insensitively), recursively, skipping tooling directories, sorted
/// by path components so the walk order never depends on the platform.
pub fn stub_files(snapshot: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk(snapshot, &mut files)?;
    files.sort_by(|left, right| {
        left.components()
            .map(|component| component.as_os_str().to_owned())
            .cmp(right.components().map(|component| component.as_os_str().to_owned()))
    });
    Ok(files)
}

fn walk(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let skipped = path
                .file_name()
                .is_some_and(|name| SKIPPED_DIRECTORIES.iter().any(|skip| name == *skip));
            if !skipped {
                walk(&path, files)?;
            }
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("php"))
        {
            files.push(path);
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs`
Expected: PASS.

- [ ] **Step 5: Declare and write the binary**

Append to `crates/celerrate_stubs/Cargo.toml`:

```toml
[[bin]]
name = "stub-compiler"
path = "src/bin/stub-compiler.rs"
required-features = ["compiler"]
```

`crates/celerrate_stubs/src/bin/stub-compiler.rs`:

```rust
//! The stub compiler: pinned snapshot in, committed blob out. Driven
//! by `cargo xtask compile-stubs`, never by a build script. Malformed
//! stub files produce warnings and partial extraction; only a missing
//! snapshot or an unwritable output fails the run.

use std::path::PathBuf;
use std::process::ExitCode;

use celerrate_stubs::compiler::extract::extract;
use celerrate_stubs::compiler::snapshot::stub_files;
use celerrate_stubs::{StubIndex, encode};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let (snapshot, output, check) = match (
        arguments.next(),
        arguments.next(),
        arguments.next().as_deref(),
        arguments.next(),
    ) {
        (Some(snapshot), Some(output), None, None) => {
            (PathBuf::from(snapshot), PathBuf::from(output), false)
        }
        (Some(snapshot), Some(output), Some("--check"), None) => {
            (PathBuf::from(snapshot), PathBuf::from(output), true)
        }
        _ => {
            return Err("usage: stub-compiler <snapshot-directory> <output-blob-path> [--check]".into());
        }
    };

    let files = stub_files(&snapshot)
        .map_err(|error| format!("cannot walk {}: {error}", snapshot.display()))?;
    if files.is_empty() {
        // A wrong path must never silently produce an empty blob.
        return Err(format!("no stub files under {}", snapshot.display()).into());
    }

    let mut symbols = Vec::new();
    let mut warnings = 0usize;
    for path in &files {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("warning: skipping {}: {error}", path.display());
                warnings += 1;
                continue;
            }
        };
        let extraction = extract(&text);
        if extraction.had_parse_errors {
            eprintln!("warning: parse diagnostics in {}", path.display());
            warnings += 1;
        }
        symbols.extend(extraction.symbols);
    }

    let index = StubIndex::from_symbols(symbols);
    let blob = encode(&index);
    println!(
        "{} stub files, {} symbols, {} warnings, {} bytes",
        files.len(),
        index.len(),
        warnings,
        blob.len(),
    );

    if check {
        let committed = std::fs::read(&output)
            .map_err(|error| format!("cannot read {}: {error}", output.display()))?;
        if committed != blob {
            return Err(format!(
                "{} is stale: run `cargo xtask compile-stubs` and commit the result",
                output.display(),
            )
            .into());
        }
        println!("{} is up to date", output.display());
    } else {
        std::fs::write(&output, &blob)
            .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
        println!("wrote {}", output.display());
    }
    Ok(())
}
```

- [ ] **Step 6: Exercise the binary on a miniature snapshot**

```bash
mkdir -p target/stub-smoke/snapshot/standard
printf '<?php\n/** @since 8.1 */\nfunction smoke_test() {}\n' > target/stub-smoke/snapshot/standard/basic.php
cargo run --package celerrate_stubs --features compiler --bin stub-compiler -- target/stub-smoke/snapshot target/stub-smoke/smoke.bin
cargo run --package celerrate_stubs --features compiler --bin stub-compiler -- target/stub-smoke/snapshot target/stub-smoke/smoke.bin --check
cargo run --package celerrate_stubs --features compiler --bin stub-compiler -- target/stub-smoke/absent target/stub-smoke/nope.bin; echo "exit: $?"
rm -r target/stub-smoke
```

Expected: first run prints `1 stub files, 1 symbols, 0 warnings, ... bytes` and
`wrote ...`; the `--check` run prints `... is up to date`; the last run prints a
clean `error: cannot walk ...` with `exit: 1`.

- [ ] **Step 7: Full gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings && cargo fmt --all -- --check`

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): add the snapshot walk and the stub-compiler binary"
```

---

### Task 7: xtask orchestration, the pin file, and the committed blob

**Files:**
- Create: `xtask/phpstorm-stubs.pin`
- Create: `xtask/src/stubs.rs`
- Modify: `xtask/src/lib.rs` (add `pub mod stubs;`)
- Modify: `xtask/src/main.rs` (new command arms)
- Create (generated): `crates/celerrate_stubs/src/stubs.bin`

**Interfaces:**
- Consumes: the `stub-compiler` binary (task 6), `crate::workspace_root()`
  (existing in xtask).
- Produces:
  - `pub struct StubsPin { pub repository: String, pub commit: String }`
  - `pub fn parse_pin(text: &str) -> Result<StubsPin>` (xtask's existing
    `Result` alias), `pub fn pin() -> Result<StubsPin>` (reads
    `xtask/phpstorm-stubs.pin`), `pub fn snapshot_directory() -> Result<PathBuf>`
    (`<workspace>/target/phpstorm-stubs/<commit>`), `pub fn fetch() -> Result<()>`,
    `pub fn compile(check: bool) -> Result<()>`.
  - CLI: `cargo xtask fetch-stubs`, `cargo xtask compile-stubs`,
    `cargo xtask compile-stubs --check` (the `.cargo/config.toml` alias
    `xtask = "run --package xtask --"` already exists).
  - The committed blob at `crates/celerrate_stubs/src/stubs.bin`.

- [ ] **Step 1: Write the failing pin-parsing tests**

`xtask/src/stubs.rs` (test module first):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::parse_pin;

    const VALID_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_valid_pin_parses() {
        let pin = parse_pin(&format!(
            "# a comment\n\
             repository = https://github.com/JetBrains/phpstorm-stubs\n\
             \n\
             commit = {VALID_SHA}\n",
        ))
        .unwrap();
        assert_eq!(pin.repository, "https://github.com/JetBrains/phpstorm-stubs");
        assert_eq!(pin.commit, VALID_SHA);
    }

    #[test]
    fn a_missing_key_is_rejected() {
        assert!(parse_pin("repository = https://example.com/repo").is_err());
        assert!(parse_pin(&format!("commit = {VALID_SHA}")).is_err());
    }

    #[test]
    fn a_short_or_non_hexadecimal_commit_is_rejected() {
        assert!(parse_pin("repository = r\ncommit = abc123").is_err());
        assert!(
            parse_pin("repository = r\ncommit = zzzz456789abcdef0123456789abcdef01234567")
                .is_err()
        );
    }

    #[test]
    fn unknown_keys_are_rejected_to_catch_typos() {
        assert!(parse_pin(&format!("repo = r\ncommit = {VALID_SHA}")).is_err());
    }
}
```

Add `pub mod stubs;` to `xtask/src/lib.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package xtask`
Expected: FAIL to compile.

- [ ] **Step 3: Write the pin, fetch, and compile orchestration**

Prepend to `xtask/src/stubs.rs`:

```rust
//! The pinned phpstorm-stubs snapshot: fetch it at the pinned commit
//! and drive the stub compiler. Network happens only here — never in
//! a build script, never in a query. The pin is bumped deliberately,
//! like the corpus SHAs.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// The parsed `xtask/phpstorm-stubs.pin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubsPin {
    pub repository: String,
    pub commit: String,
}

/// Reads and parses the committed pin file.
pub fn pin() -> Result<StubsPin> {
    let path = crate::workspace_root()?.join("xtask/phpstorm-stubs.pin");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse_pin(&text)
}

/// Parses the pin file: `key = value` lines, `#` comments, both
/// `repository` and a full-length hexadecimal `commit` required.
pub fn parse_pin(text: &str) -> Result<StubsPin> {
    let mut repository = None;
    let mut commit = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("malformed pin line: {line}").into());
        };
        match key.trim() {
            "repository" => repository = Some(value.trim().to_owned()),
            "commit" => commit = Some(value.trim().to_owned()),
            unknown => return Err(format!("unknown pin key: {unknown}").into()),
        }
    }
    let repository = repository.ok_or("pin file misses the repository key")?;
    let commit = commit.ok_or("pin file misses the commit key")?;
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the pinned commit must be a full 40-character SHA".into());
    }
    Ok(StubsPin { repository, commit })
}

/// Where the pinned snapshot lives: under `target/`, so it is already
/// gitignored and swept by `cargo clean`.
pub fn snapshot_directory() -> Result<PathBuf> {
    Ok(crate::workspace_root()?
        .join("target/phpstorm-stubs")
        .join(pin()?.commit))
}

/// Fetches the pinned snapshot if it is not already present. The
/// checkout lands in a staging directory first and is renamed only
/// when complete, so an interrupted fetch never masquerades as a
/// snapshot.
pub fn fetch() -> Result<()> {
    let pin = pin()?;
    let directory = snapshot_directory()?;
    if directory.exists() {
        println!("snapshot already present at {}", directory.display());
        return Ok(());
    }
    let staging = directory.with_file_name(format!("{}.staging", pin.commit));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    run_git(&staging, &["init", "--quiet"])?;
    run_git(
        &staging,
        &["fetch", "--quiet", "--depth", "1", &pin.repository, &pin.commit],
    )?;
    run_git(&staging, &["checkout", "--quiet", "--detach", "FETCH_HEAD"])?;
    std::fs::rename(&staging, &directory)?;
    println!("fetched {} at {}", pin.repository, pin.commit);
    Ok(())
}

/// Fetches if needed, then runs the stub compiler over the snapshot.
/// `check` compares against the committed blob instead of writing it.
pub fn compile(check: bool) -> Result<()> {
    fetch()?;
    let root = crate::workspace_root()?;
    let snapshot = snapshot_directory()?;
    let blob = root.join("crates/celerrate_stubs/src/stubs.bin");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut command = Command::new(cargo);
    command
        .current_dir(&root)
        .args([
            "run",
            "--release",
            "--package",
            "celerrate_stubs",
            "--features",
            "compiler",
            "--bin",
            "stub-compiler",
            "--",
        ])
        .arg(&snapshot)
        .arg(&blob);
    if check {
        command.arg("--check");
    }
    let status = command.status()?;
    if !status.success() {
        return Err("stub compilation failed".into());
    }
    Ok(())
}

fn run_git(directory: &Path, arguments: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .status()?;
    if !status.success() {
        return Err(format!("git {} failed", arguments.join(" ")).into());
    }
    Ok(())
}
```

Update `xtask/src/main.rs` to route the new commands (replace the whole `match`):

```rust
fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let outcome = match (arguments.next().as_deref(), arguments.next().as_deref()) {
        (Some("codegen"), None) => xtask::codegen::run(),
        (Some("fetch-stubs"), None) => xtask::stubs::fetch(),
        (Some("compile-stubs"), None) => xtask::stubs::compile(false),
        (Some("compile-stubs"), Some("--check")) => xtask::stubs::compile(true),
        _ => {
            eprintln!("usage: cargo xtask <codegen | fetch-stubs | compile-stubs [--check]>");
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
```

Update the doc comment at the top of `xtask/src/lib.rs` so it no longer claims
codegen is the only command (keep the no-`celerrate_*`-dependency invariant
sentence — it still holds: `stubs.rs` only spawns `git` and `cargo`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package xtask`
Expected: PASS.

- [ ] **Step 5: Pin the snapshot**

Get the current phpstorm-stubs master commit:

```bash
git ls-remote https://github.com/JetBrains/phpstorm-stubs.git refs/heads/master
```

Expected: one line, `<40-hex-sha>\trefs/heads/master`. Write
`xtask/phpstorm-stubs.pin` with that SHA:

```
# The pinned phpstorm-stubs snapshot. Bump deliberately: change the
# commit, run `cargo xtask compile-stubs`, and commit the regenerated
# blob together with this file.
repository = https://github.com/JetBrains/phpstorm-stubs
commit = <the SHA from ls-remote>
```

- [ ] **Step 6: Fetch, compile, and sanity-check the blob**

```bash
cargo xtask compile-stubs
```

Expected: a fetch (the first time), then a summary in the shape
`<thousands> stub files, <tens of thousands> symbols, <n> warnings, <n> bytes`
and `wrote .../crates/celerrate_stubs/src/stubs.bin`. Warnings are acceptable
(they flag stub files our parser recovers on — record the count); a hard error
is not.

Determinism and freshness sanity checks:

```bash
shasum -a 256 crates/celerrate_stubs/src/stubs.bin
cargo xtask compile-stubs
shasum -a 256 crates/celerrate_stubs/src/stubs.bin   # identical hash
cargo xtask compile-stubs --check                     # "is up to date", exit 0
ls -la crates/celerrate_stubs/src/stubs.bin           # expect single-digit MB
```

Expected: the two hashes are identical; `--check` passes. If the blob exceeds
~16 MB, stop and reconsider the encoding with the human partner before
committing.

- [ ] **Step 7: Full gates, then commit (pin + xtask + blob together)**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`

```bash
git add xtask crates/celerrate_stubs/src/stubs.bin
git commit -m "🔧 chore(xtask): fetch the pinned phpstorm-stubs snapshot and compile the blob"
```

---

### Task 8: Embedding and the salsa wiring

**Files:**
- Modify: `crates/celerrate_project/Cargo.toml` (add `salsa`)
- Create: `crates/celerrate_project/src/input.rs`
- Modify: `crates/celerrate_project/src/lib.rs` (add `mod input;` + export)
- Modify: `crates/celerrate_stubs/Cargo.toml` (add `salsa`; dev-dependencies
  `celerrate_db`, `celerrate_source`)
- Create: `crates/celerrate_stubs/src/query.rs`
- Modify: `crates/celerrate_stubs/src/lib.rs` (embedded blob, `mod query;`, exports)

**Interfaces:**
- Consumes: `salsa` 0.27 (input builder: `Type::builder(fields...)
  .durability(salsa::Durability::HIGH).new(&db)`; setters:
  `value.set_field(&mut db).to(...)`), `celerrate_db::testing::TestDatabase`
  (`take_executed()` returns the Debug renderings of executed queries),
  `celerrate_db::SourceFile`, `celerrate_source::FileId`, `StubIndex`,
  `blob::decode` (task 3), the committed `src/stubs.bin` (task 7).
- Produces:
  - In `celerrate_project`:
    `#[salsa::input] pub struct ProjectConfiguration { pub php_version_range: PhpVersionRange }`
    — this closes part 2's "the configuration becomes a salsa input in part 3"
    deferral. Created at the composition root (part 7) from a
    `ProjectDiscovery`, with `salsa::Durability::MEDIUM` (configuration changes
    are rarer than file edits, more frequent than stub bumps).
  - In `celerrate_stubs`:
    - `pub const EMBEDDED_STUB_BLOB: &[u8]`
    - `pub fn embedded_stub_index() -> Result<StubIndex, StubBlobError>`
    - `#[salsa::input] pub struct StubIndexInput { #[returns(ref)] pub index: StubIndex }`
      — created once at the composition root with `salsa::Durability::HIGH`
      (the index changes only when the binary changes).
    - `#[salsa::tracked(returns(ref))] pub fn stubs_in_range(db, stubs:
      StubIndexInput, configuration: ProjectConfiguration) -> StubIndex` —
      the version-filtered view every later consumer reads (part 5's merged
      symbol index, part 6's checks).

- [ ] **Step 1: Write the failing ProjectConfiguration test**

Add `salsa = { workspace = true }` to `[dependencies]` in
`crates/celerrate_project/Cargo.toml`.

`crates/celerrate_project/src/input.rs`:

```rust
//! The analysis configuration as a salsa input: the slice of project
//! discovery that queries are allowed to read. Everything else in
//! `ProjectDiscovery` (walk roots, notices) is push-time state for the
//! composition root, not query-visible input.

use crate::version::PhpVersionRange;

/// Created at the composition root from a [`ProjectDiscovery`], with
/// `salsa::Durability::MEDIUM`: configuration changes are rarer than
/// file edits and more frequent than stub bumps.
///
/// [`ProjectDiscovery`]: crate::ProjectDiscovery
#[salsa::input]
pub struct ProjectConfiguration {
    pub php_version_range: PhpVersionRange,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use salsa::Setter;

    use super::ProjectConfiguration;
    use crate::version::{PhpVersion, PhpVersionRange};

    #[test]
    fn the_configuration_stores_and_updates_the_version_range() {
        let mut db = TestDatabase::default();
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let configuration = ProjectConfiguration::builder(range)
            .durability(salsa::Durability::MEDIUM)
            .new(&db);
        assert_eq!(configuration.php_version_range(&db), range);

        let narrowed = PhpVersionRange::point(PhpVersion::new(8, 2));
        configuration
            .set_php_version_range(&mut db)
            .to(narrowed);
        assert_eq!(configuration.php_version_range(&db), narrowed);
    }
}
```

Add to `crates/celerrate_project/src/lib.rs`: `mod input;` and
`pub use input::ProjectConfiguration;`.

- [ ] **Step 2: Run, verify red, then green**

Run: `cargo test --package celerrate_project`
Expected: compiles and passes once the module and manifest edits above are in
place (the input struct and the test land together; the red state here is the
missing module if the test file is written first). If the builder API differs
(`builder(...)` not found), check the salsa 0.27 generated surface with
`cargo doc --package salsa` — the input macro generates `builder(...)` with a
`durability(...)` method and a terminal `new(&db)`.

- [ ] **Step 3: Write the failing embedding and query tests**

Manifest edits in `crates/celerrate_stubs/Cargo.toml`: add
`salsa = { workspace = true }` to `[dependencies]`; add
`celerrate_db = { path = "../celerrate_db" }` and
`celerrate_source = { path = "../celerrate_source" }` to `[dev-dependencies]`.

`crates/celerrate_stubs/src/query.rs`:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use salsa::Setter;

    use super::{StubIndexInput, stubs_in_range};
    use crate::index::StubIndex;
    use crate::symbol::{StubAvailability, StubSymbol, StubSymbolKind};

    fn sample_input(db: &TestDatabase) -> StubIndexInput {
        let index = StubIndex::from_symbols(vec![
            StubSymbol {
                name: "always_there".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "born_in_php_84".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 4)),
                    ..StubAvailability::ALWAYS
                },
            },
            StubSymbol {
                name: "gone_in_php_80".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability {
                    removed: Some(PhpVersion::new(8, 0)),
                    ..StubAvailability::ALWAYS
                },
            },
        ]);
        StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(db)
    }

    fn configuration(db: &TestDatabase, minimum: (u8, u8), maximum: (u8, u8)) -> ProjectConfiguration {
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(minimum.0, minimum.1),
            PhpVersion::new(maximum.0, maximum.1),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db)
    }

    fn filtered_names(db: &TestDatabase, stubs: StubIndexInput, configuration: ProjectConfiguration) -> Vec<String> {
        stubs_in_range(db, stubs, configuration)
            .symbols()
            .iter()
            .map(|symbol| symbol.name.clone())
            .collect()
    }

    #[test]
    fn the_filtered_view_keeps_only_symbols_that_exist_in_the_range() {
        let db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 3));
        assert_eq!(
            filtered_names(&db, stubs, configuration),
            vec!["always_there".to_owned()],
        );
    }

    #[test]
    fn widening_the_range_reveals_more_symbols() {
        let db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 5));
        assert_eq!(
            filtered_names(&db, stubs, configuration),
            vec!["always_there".to_owned(), "born_in_php_84".to_owned()],
        );
    }

    #[test]
    fn changing_the_version_range_recomputes_the_filtered_view() {
        let mut db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 3));
        let _ = stubs_in_range(&db, stubs, configuration);
        db.take_executed();

        configuration
            .set_php_version_range(&mut db)
            .to(PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)));
        let names = filtered_names(&db, stubs, configuration);
        let executed = db.take_executed();
        assert!(
            executed.iter().any(|entry| entry.starts_with("stubs_in_range")),
            "expected a recomputation, saw {executed:?}",
        );
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn editing_a_source_file_leaves_the_filtered_view_untouched() {
        // The invalidation-scope assertion for this part: file edits
        // (the low-durability input) never touch the stub derivation.
        let mut db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 5));
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        let _ = stubs_in_range(&db, stubs, configuration);
        let _ = celerrate_db::parse(&db, file);
        db.take_executed();

        file.set_bytes(&mut db).to(b"<?php echo 2;".to_vec());
        let _ = celerrate_db::parse(&db, file);
        let _ = stubs_in_range(&db, stubs, configuration);
        let executed = db.take_executed();
        assert!(
            executed.iter().any(|entry| entry.starts_with("parse")),
            "the edit reparses, saw {executed:?}",
        );
        assert!(
            !executed.iter().any(|entry| entry.starts_with("stubs_in_range")),
            "the stub view must not recompute on a file edit, saw {executed:?}",
        );
    }
}
```

Embedding tests, appended to the test module in `crates/celerrate_stubs/src/lib.rs`
(create the module if the crate root has none):

```rust
#[cfg(test)]
mod tests {
    use crate::{StubSymbolKind, embedded_stub_index};

    #[test]
    fn the_embedded_blob_decodes() {
        let index = embedded_stub_index();
        let index = match index {
            Ok(index) => index,
            Err(error) => panic!("the committed blob must decode: {error}"),
        };
        // Tens of thousands of symbols; a low floor keeps the test
        // honest without chasing the snapshot's exact count.
        assert!(index.len() > 10_000, "only {} symbols", index.len());
    }

    #[test]
    fn well_known_symbols_are_present_with_their_kinds() {
        let index = embedded_stub_index().unwrap_or_default();
        let find = |name: &str, kind: StubSymbolKind| {
            index
                .symbols()
                .iter()
                .any(|symbol| symbol.name == name && symbol.kind == kind)
        };
        assert!(find("strlen", StubSymbolKind::Function));
        assert!(find("Exception", StubSymbolKind::Class));
        assert!(find("Traversable", StubSymbolKind::Interface));
        assert!(find("E_ALL", StubSymbolKind::Constant), "define() extraction");
        assert!(
            find("Random\\Randomizer", StubSymbolKind::Class),
            "namespaced extraction",
        );
    }
}
```

(`panic!` and `unwrap_or_default` in a test module are fine — the module-level
`#![allow]` is not even needed for `panic!` in tests if clippy complains, add
`#![allow(clippy::panic)]` locally.)

Note on witnesses: these names are stable across phpstorm-stubs history. If one
is genuinely absent from the pinned snapshot, inspect
`target/phpstorm-stubs/<sha>/` for the actual declaration, pick an equivalent
witness, and keep the assertion shape (a function, a class, an interface, a
`define()` constant, a namespaced class).

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs`
Expected: FAIL to compile (`stubs_in_range`, `embedded_stub_index` missing).

- [ ] **Step 5: Implement the embedding and the query**

Prepend to `crates/celerrate_stubs/src/query.rs`:

```rust
//! The salsa surface of the stubs: the index as a high-durability
//! input and the version-filtered view as a tracked query.

use celerrate_project::ProjectConfiguration;

use crate::index::StubIndex;

/// The decoded stub index as a salsa input. Created once at the
/// composition root with `salsa::Durability::HIGH`: it changes only
/// when the binary (or, later, an overlay) changes.
#[salsa::input]
pub struct StubIndexInput {
    #[returns(ref)]
    pub index: StubIndex,
}

/// The stub symbols that exist somewhere in the configured version
/// range. Symbols removed before the minimum or introduced after the
/// maximum are invisible; availability metadata stays on the
/// survivors — part 6's version-gating checks read it.
#[salsa::tracked(returns(ref))]
pub fn stubs_in_range(
    db: &dyn salsa::Database,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> StubIndex {
    let range = configuration.php_version_range(db);
    StubIndex::from_symbols(
        stubs
            .index(db)
            .symbols()
            .iter()
            .filter(|symbol| symbol.availability.exists_in(range))
            .cloned()
            .collect(),
    )
}
```

In `crates/celerrate_stubs/src/lib.rs`, add the module, the embedded blob, and
the exports:

```rust
mod query;

pub use query::{StubIndexInput, stubs_in_range};

/// The committed stub blob, embedded at compile time. Regenerated by
/// `cargo xtask compile-stubs`; the freshness test and the CI `stubs`
/// job keep it in step with the pinned snapshot.
pub const EMBEDDED_STUB_BLOB: &[u8] = include_bytes!("stubs.bin");

/// Decodes the embedded blob. In a healthy build this cannot fail (the
/// freshness test compares the committed bytes to a recompilation); a
/// failure is surfaced as a value for the composition root to report,
/// never a panic.
pub fn embedded_stub_index() -> Result<StubIndex, StubBlobError> {
    blob::decode(EMBEDDED_STUB_BLOB)
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs && cargo test --package celerrate_project`
Expected: PASS, including both invalidation-scope tests and the embedded-blob
tests against the real committed blob.

- [ ] **Step 7: Full gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`

```bash
git add crates/celerrate_project crates/celerrate_stubs
git commit -m "✨ feat(stubs): embed the blob behind a version-filtered salsa view"
```

---

### Task 9: Freshness, CI, and the changelog

**Files:**
- Modify: `crates/celerrate_stubs/src/compiler/snapshot.rs` (pinned-directory
  lookup + freshness test)
- Modify: `.github/workflows/ci.yml` (the `stubs` job)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `stub_files`, `extract`, `StubIndex::from_symbols`, `encode`,
  `EMBEDDED_STUB_BLOB`, `blob::fnv1a64` (already `pub(crate)`).
- Produces:
  - `pub fn pinned_snapshot_directory() -> Option<PathBuf>` in
    `compiler::snapshot`: reads `xtask/phpstorm-stubs.pin` relative to the
    crate (`CARGO_MANIFEST_DIR/../../xtask/phpstorm-stubs.pin`), returns
    `<workspace>/target/phpstorm-stubs/<commit>` when the pin parses and the
    directory exists, `None` otherwise. (Deliberate small duplication of the
    xtask pin parsing: xtask must not be depended on by any `celerrate_*`
    crate, and the test must not depend on xtask either.)
  - The freshness unit test and the CI job.

- [ ] **Step 1: Add the pinned-directory lookup and the freshness test**

Append to `crates/celerrate_stubs/src/compiler/snapshot.rs` (production part):

```rust
/// The fetched pinned snapshot, if present: reads the pin file
/// committed next to xtask and points into `target/`. `None` when the
/// snapshot has not been fetched (or the pin is unreadable): callers
/// treat that as "nothing to compare against", never as an error.
pub fn pinned_snapshot_directory() -> Option<PathBuf> {
    let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_directory.parent()?.parent()?;
    let pin_text = std::fs::read_to_string(workspace_root.join("xtask/phpstorm-stubs.pin")).ok()?;
    let commit = pin_text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "commit").then(|| value.trim().to_owned())
    })?;
    let directory = workspace_root.join("target/phpstorm-stubs").join(commit);
    directory.is_dir().then_some(directory)
}
```

And the freshness test, in the same file's test module:

```rust
    use crate::blob::{encode, fnv1a64};
    use crate::compiler::extract::extract;
    use crate::index::StubIndex;

    /// The committed blob must be exactly what the pinned snapshot
    /// compiles to. Runs only when the snapshot has been fetched
    /// (`cargo xtask fetch-stubs`); CI enforces it unconditionally
    /// through `cargo xtask compile-stubs --check`. Debug-build note:
    /// this parses the whole snapshot and can take a few minutes.
    #[test]
    fn the_committed_blob_matches_a_recompilation_of_the_pinned_snapshot() {
        let Some(snapshot) = super::pinned_snapshot_directory() else {
            eprintln!(
                "skipped: pinned snapshot not fetched; run `cargo xtask fetch-stubs` to enable this test",
            );
            return;
        };
        let files = match super::stub_files(&snapshot) {
            Ok(files) => files,
            Err(error) => panic!("cannot walk the snapshot: {error}"),
        };
        let mut symbols = Vec::new();
        for path in &files {
            if let Ok(text) = std::fs::read_to_string(path) {
                symbols.extend(extract(&text).symbols);
            }
        }
        let recompiled = encode(&StubIndex::from_symbols(symbols));
        let committed = crate::EMBEDDED_STUB_BLOB;
        // Compare via length + hash: a byte-for-byte assert_eq would
        // dump megabytes on failure.
        assert!(
            recompiled.len() == committed.len() && fnv1a64(&recompiled) == fnv1a64(committed),
            "src/stubs.bin is stale: run `cargo xtask compile-stubs` and commit the result",
        );
    }
```

(Add `#![allow(clippy::panic)]` to the test module attributes if not already
covered by the existing allow list.)

- [ ] **Step 2: Run the freshness test both ways**

```bash
cargo test --package celerrate_stubs the_committed_blob_matches -- --nocapture
```

Expected: PASS. With the snapshot fetched (it is, since task 7), it really
compares; to see the skip path, temporarily rename
`target/phpstorm-stubs/<sha>` and re-run (expect the "skipped:" line), then
rename it back.

- [ ] **Step 3: Add the CI job**

Append to `.github/workflows/ci.yml` under `jobs:` (sibling of `fuzz`):

```yaml
  stubs:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings
      - run: cargo xtask compile-stubs --check
```

(The clippy line exists because `required-features` hides the binary from the
workspace-wide clippy in the `lint` job; this job is also the only one with the
network fetch.)

Validation: CI cannot run locally; check the YAML parses (for example
`python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/ci.yml'))"`)
and rely on the pull-request run for the rest.

- [ ] **Step 4: Record the part in the changelog**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, after the
`celerrate_project` entry (keep the list in layering order), add:

```markdown
- `celerrate_stubs`: the pinned phpstorm-stubs snapshot compiled by
  `cargo xtask compile-stubs` into a committed, versioned binary blob
  (top-level symbols with per-version availability metadata), embedded
  in the binary and exposed as a high-durability salsa input with a
  version-range-filtered view; the project configuration becomes a
  salsa input in `celerrate_project`.
```

- [ ] **Step 5: Record the compiler-placement narrowing in the spec**

Precedent: the part 2 narrowing was recorded in the spec after implementation
review. In `.claude/superpowers/specs/2026-07-11-semantic-core-design.md`,
section 5, first bullet, replace:

```
- Compiled by `xtask`, not by a `build.rs`, consistent with the
  typed-AST sourcegen pattern: the blob is committed and a freshness
  test asserts it matches the pinned snapshot. The compiler uses
  `celerrate_syntax` to parse the stubs (a separate dependency graph in
  Cargo, no layering violation).
```

with:

```
- Compiled by a dedicated compiler, not by a `build.rs`, consistent
  with the typed-AST sourcegen pattern: the blob is committed and a
  freshness test asserts it matches the pinned snapshot. Placement,
  recorded here after implementation review: the compiler is a
  feature-gated binary owned by `celerrate_stubs` (parent-spec
  ownership), because it parses PHP with `celerrate_syntax` while
  xtask's invariant — no dependency on any `celerrate_*` crate, so a
  broken generated file can never prevent regenerating it — must
  survive. `cargo xtask compile-stubs` remains the entry point: xtask
  fetches the pinned snapshot (git, network only here) and spawns the
  compiler.
```

- [ ] **Step 6: Full gates, then commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`

```bash
git add crates/celerrate_stubs .github/workflows/ci.yml CHANGELOG.md
git commit -m "✅ test(stubs): pin blob freshness in tests and CI"
git add .claude/superpowers/specs/2026-07-11-semantic-core-design.md
git commit -m "📝 docs(specs): record the stub compiler placement narrowing"
```

---

## Definition of done

- [ ] `cargo test --workspace` green, including the invalidation-scope tests
  (version-range change recomputes `stubs_in_range`; a file edit does not) and
  the embedded-blob tests against the real committed blob.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings`
  both clean; `cargo fmt --all -- --check` and `cargo deny check` clean.
- [ ] `cargo xtask compile-stubs --check` passes against the committed
  `crates/celerrate_stubs/src/stubs.bin`; running `compile-stubs` twice
  produces byte-identical blobs.
- [ ] The blob decodes to more than 10,000 symbols including function, class,
  interface, `define()` constant, and namespaced witnesses.
- [ ] CI carries the `stubs` job; the changelog records the part.
- [ ] No new external dependencies; xtask still depends on no `celerrate_*`
  crate; the runtime dependency graph of `celerrate_stubs` is
  `celerrate_project` + `salsa` only.
