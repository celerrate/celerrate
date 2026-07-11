# Semantic Core Part 2: Project Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `celerrate_project` (zero-configuration Composer discovery: tolerant
`composer.json` and `installed.json` readers, autoload rules, the PHP version range,
project/vendor classification, structured notices) and give `celerrate_vfs` the disk
walk those autoload rules drive (path normalization, deterministic PHP-file
enumeration, disk loading).

**Architecture:** Per the spec `.claude/superpowers/specs/2026-07-11-semantic-core-design.md`
(sections 3 and 4) and the parent spec's version range model (section 2). The VFS owns
the walk mechanics (`normalize_path`, `enumerate_php_files`, `Vfs::load_from_disk`);
`celerrate_project` derives what gets walked. `celerrate_project` is pure — no salsa:
the configuration becomes a salsa input in part 3, when the stub index (its first
query consumer) exists. Discovery findings stay structured (`ProjectNotice`, the
producing-crate narrowing recorded in spec section 7); the preview renderer projects
them into the shared `Diagnostic` model in part 7. Their `CEL####` identifiers are
allocated now, because they are permanent once published.

**Tech Stack:** Rust edition 2024, `serde_json` 1 (new workspace dependency),
`tempfile` 3 (new workspace dev-dependency), existing crates
`celerrate_source`, `celerrate_diagnostics`, `celerrate_vfs`, `celerrate_db` (dev-only).

## Global Constraints

- Zero panic, mechanically enforced: workspace denies `unwrap_used`, `expect_used`,
  `indexing_slicing`, `panic`; `unsafe_code` is forbidden. Only test modules and
  integration-test files may `#[allow]` / `#![allow]` these lints. Use `.get()`,
  `checked_add`, `is_none_or`, `unwrap_or_default` — never indexing or unwrap in
  production code.
- Strict layering, DAG with no upward edges. `celerrate_project` depends on
  `celerrate_diagnostics`, `celerrate_vfs`, and `serde_json` — NOT on
  `celerrate_syntax` or `celerrate_db` (`celerrate_db` appears only as a
  dev-dependency for the end-to-end test). `celerrate_vfs` keeps depending only on
  `celerrate_source`.
- Error resilience: no user input may crash or fail the tool. A corrupted or missing
  `composer.json` produces a notice and defaults; an unreadable directory is skipped;
  discovery and enumeration never return errors to their callers.
- Determinism: same disk state in, byte-identical results out. Every collection that
  crosses an API boundary is sorted and deduplicated (`BTreeSet` before `Vec`).
  `serde_json` is used WITHOUT the `preserve_order` feature, so JSON object iteration
  is key-sorted and deterministic.
- TDD: every task starts with a failing test.
- Everything in English, full words, no abbreviated names (standard acronyms fine).
- Commits: gitmoji + Conventional Commits, repository-configured identity, no Claude
  attribution anywhere.
- New crate `Cargo.toml`s use the workspace inheritance pattern
  (version/edition/license/authors/repository `.workspace = true`,
  `[lints] workspace = true`).
- Verification for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`.
  Tasks that add dependencies also run `cargo deny check` (if a new permissive
  license appears in the tree, extend the `deny.toml` allow list with a source
  comment, following the existing Zlib/Unicode-3.0 pattern).

## Stable diagnostic identifiers allocated by this plan

Part 1 allocated CEL0001–CEL0017. These are next; permanent once published, do not
renumber:

| Identifier | Owner | Meaning | Severity |
| --- | --- | --- | --- |
| CEL0018 | `celerrate_project` | No `composer.json`: analyzing the root directory with defaults | Warning |
| CEL0019 | `celerrate_project` | `composer.json` invalid (not a JSON object): defaults used | Warning |
| CEL0020 | `celerrate_project` | No PHP version configured: latest supported stable assumed | Warning |
| CEL0021 | `celerrate_project` | A PHP version constraint is unparseable or admits no supported version: fallback used | Warning |
| CEL0022 | `celerrate_project` | `installed.json` invalid: vendor autoload skipped | Warning |

## Decisions fixed by this plan

Semantics an implementer must not re-litigate mid-task:

- **Minor-precision versions.** `PhpVersion` is `major.minor`; patch components are
  truncated on input. Availability metadata and version gating are minor-granular.
  Supported window: 8.1 through 8.5; the zero-configuration fallback is the point
  range [8.5, 8.5].
- **Constraint truncation rules.** At minor precision, `>` behaves as `>=` (Composer's
  `>8.1.0` still admits `8.1.1`, which is minor 8.1); `<X.Y` and `<X.Y.0` exclude
  minor X.Y while `<X.Y.Z` with Z > 0 includes it; `<=` always includes the named
  minor. A bare major (`"8"`) is treated as the major wildcard `8.*` — a deliberate,
  documented deviation from Composer's exact-version reading, matching author intent
  in `require.php`.
- **Version detection precedence** (parent spec section 2, minus `celerrate.toml`):
  `config.platform.php` (a concrete version, collapsing to a point range, clamped
  into the supported window) → `require.php` interpreted as a range → fallback
  [latest, latest] with CEL0020. An unparseable stage emits CEL0021 and falls
  through; CEL0020 fires only when NO version signal existed at all, so a project
  never gets two version notices.
- **Notice economy.** A missing or invalid manifest emits exactly one notice (CEL0018
  or CEL0019); the version fallback it implies is not separately reported. A missing
  `installed.json` is silent (a project without installed dependencies is normal); an
  invalid one emits CEL0022.
- **Autoload scope.** For the project, `autoload` and `autoload-dev` merge (test code
  is project code). For vendor packages (from `installed.json`), only `autoload`
  (that file carries no dev autoload), and ALL listed packages count, including dev
  packages — consistent with the "declared anywhere counts as declared" stance of
  spec section 7. `config.vendor-dir` is honored (default `vendor`).
- **Walk rules.** A directory root is walked recursively for `.php` files
  (ASCII-case-insensitive extension match); an explicit file root (autoload `files` /
  `classmap` entries may name single files of any extension) is included as-is.
  Symbolic links to directories are followed with cycle protection (visited set of
  canonicalized directories); unreadable entries and missing declared roots are
  skipped silently. Results are sorted and deduplicated.
- **Classification.** A path under the vendor root is `FileOrigin::Vendor`, everything
  else `FileOrigin::Project`. This feeds salsa durability at the composition root
  (part 7); part 2 only provides the API.
- **Discovery reads disk exactly twice** (`composer.json`, then
  `<vendor>/composer/installed.json`) at push time, never inside a query. The pure
  core `discover_from_sources` takes the two texts as parameters so unit tests never
  touch the file system; `discover` is the thin disk-reading wrapper. Callers pass an
  absolute project root.

---

### Task 1: Path normalization in `celerrate_vfs`

**Files:**
- Create: `crates/celerrate_vfs/src/path.rs`
- Modify: `crates/celerrate_vfs/src/lib.rs`

**Interfaces:**
- Consumes: nothing new (`std::path` only).
- Produces: `celerrate_vfs::normalize_path(path: &Path, base: &Path) -> PathBuf` —
  joins `path` onto `base` when relative, then removes `.` and resolves `..`
  lexically. No file-system access. `base` must be absolute; the result is then
  absolute.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_vfs/src/path.rs` with only the test module (the module will
not compile until Step 3 adds the function, which is the failure we want):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::normalize_path;

    #[test]
    fn a_relative_path_joins_onto_the_base() {
        assert_eq!(
            normalize_path(Path::new("src/App.php"), Path::new("/project")),
            PathBuf::from("/project/src/App.php"),
        );
    }

    #[test]
    fn an_absolute_path_ignores_the_base() {
        assert_eq!(
            normalize_path(Path::new("/elsewhere/lib"), Path::new("/project")),
            PathBuf::from("/elsewhere/lib"),
        );
    }

    #[test]
    fn current_directory_components_are_removed() {
        assert_eq!(
            normalize_path(Path::new("./src/./sub"), Path::new("/project")),
            PathBuf::from("/project/src/sub"),
        );
    }

    #[test]
    fn parent_components_resolve_lexically() {
        assert_eq!(
            normalize_path(Path::new("../acme/library"), Path::new("/project/vendor/composer")),
            PathBuf::from("/project/vendor/acme/library"),
        );
    }

    #[test]
    fn excess_parents_on_an_absolute_path_stop_at_the_root() {
        assert_eq!(
            normalize_path(Path::new("../../../.."), Path::new("/project")),
            PathBuf::from("/"),
        );
    }

    #[test]
    fn an_already_normalized_path_is_unchanged() {
        assert_eq!(
            normalize_path(Path::new("/project/src"), Path::new("/project")),
            PathBuf::from("/project/src"),
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_vfs`
Expected: compile error — `normalize_path` not found. (First register the module:
in `crates/celerrate_vfs/src/lib.rs` add `mod path;` above `mod vfs;` and
`pub use path::normalize_path;` above the existing `pub use`.)

- [ ] **Step 3: Write the implementation**

Prepend to `crates/celerrate_vfs/src/path.rs` (above the test module):

```rust
use std::path::{Component, Path, PathBuf};

/// Joins `path` onto `base` when it is relative, then removes `.` and
/// resolves `..` lexically. No file-system access happens: symbolic
/// links are not resolved, so the result is a pure function of its
/// inputs. `base` must be absolute; the result is then absolute.
pub fn normalize_path(path: &Path, base: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Popping fails only at a filesystem root, where an
                // excess `..` stays at the root; a relative prefix
                // (impossible under an absolute base) would keep it.
                if !normalized.pop() && !joined.is_absolute() {
                    normalized.push(Component::ParentDir.as_os_str());
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
```

The final `crates/celerrate_vfs/src/lib.rs`:

```rust
//! File loading and in-memory overlays.
//!
//! The virtual file system is the bridge between the outside world and
//! the salsa inputs: it owns the `FileId ↔ path` mapping, holds the
//! current byte contents of every known file (disk state shadowed by
//! editor-style overlays), and reports what changed so the composition
//! root can pump new states into the database. It never reads anything
//! during a query: it pushes states, salsa pulls derivations.
//!
//! Callers pass absolute, already-normalized paths to the map:
//! [`normalize_path`] produces them, and the discovery layer above
//! decides what gets walked.

mod path;
mod vfs;

pub use path::normalize_path;
pub use vfs::{ChangedFile, Vfs};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_vfs`
Expected: PASS (6 new tests, existing tests unchanged)

- [ ] **Step 5: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: no warnings, all tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_vfs
git commit -m "✨ feat(vfs): add lexical path normalization"
```

---

### Task 2: The disk walk and disk loading in `celerrate_vfs`

**Files:**
- Create: `crates/celerrate_vfs/src/walk.rs`
- Modify: `crates/celerrate_vfs/src/lib.rs`
- Modify: `Cargo.toml` (workspace root: add `tempfile` to `[workspace.dependencies]`)
- Modify: `crates/celerrate_vfs/Cargo.toml` (add `[dev-dependencies]`)

**Interfaces:**
- Consumes: `Vfs::set_file_contents(&mut self, path: &Path, contents: Option<Vec<u8>>) -> FileId` (Task 1 of part 1); `celerrate_source::FileId`.
- Produces:
  `celerrate_vfs::enumerate_php_files(roots: &[PathBuf]) -> Vec<PathBuf>` — sorted,
  deduplicated; directory roots walked recursively for `.php`, file roots included
  as-is, missing/unreadable entries skipped, symbolic-link cycles guarded;
  `Vfs::load_from_disk(&mut self, path: &Path) -> std::io::Result<FileId>` — reads
  the file's bytes into the disk state.

- [ ] **Step 1: Add the `tempfile` dev-dependency**

In the root `Cargo.toml`, `[workspace.dependencies]` becomes (alphabetical):

```toml
[workspace.dependencies]
insta = { version = "1", features = ["glob"] }
rowan = "0.16"
salsa = "0.27"
tempfile = "3"
text-size = "1"
ungrammar = "1"
```

In `crates/celerrate_vfs/Cargo.toml`, insert between `[dependencies]` and `[lints]`:

```toml
[dev-dependencies]
tempfile = { workspace = true }
```

Run: `cargo deny check`
Expected: PASS (tempfile and its tree are MIT/Apache-2.0-compatible; if a new
permissive license is reported, extend `deny.toml`'s allow list with a comment).

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_vfs/src/walk.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::enumerate_php_files;
    use crate::Vfs;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn directories_are_walked_recursively_for_php_files_only() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php");
        write(&root.join("src/nested/B.php"), "<?php");
        write(&root.join("src/notes.txt"), "not code");
        write(&root.join("outside/C.php"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("src")]),
            vec![root.join("src/A.php"), root.join("src/nested/B.php")],
        );
    }

    #[test]
    fn the_extension_match_ignores_ascii_case() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/Legacy.PHP"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("src")]),
            vec![root.join("src/Legacy.PHP")],
        );
    }

    #[test]
    fn an_explicit_file_root_is_included_regardless_of_extension() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("bootstrap.inc"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("bootstrap.inc")]),
            vec![root.join("bootstrap.inc")],
        );
    }

    #[test]
    fn results_are_sorted_and_deduplicated_across_overlapping_roots() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/B.php"), "<?php");
        write(&root.join("src/nested/A.php"), "<?php");
        let roots = vec![
            root.join("src"),
            root.join("src/nested"),
            root.join("src/B.php"),
        ];
        assert_eq!(
            enumerate_php_files(&roots),
            vec![root.join("src/B.php"), root.join("src/nested/A.php")],
        );
    }

    #[test]
    fn missing_roots_are_skipped_silently() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        assert_eq!(
            enumerate_php_files(&[root.join("does-not-exist")]),
            Vec::<PathBuf>::new(),
        );
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_cycles_terminate() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php");
        std::os::unix::fs::symlink(root.join("src"), root.join("src/loop")).unwrap();
        let files = enumerate_php_files(&[root.join("src")]);
        assert_eq!(files.first(), Some(&root.join("src/A.php")));
    }

    #[test]
    fn load_from_disk_reads_the_bytes_into_the_disk_state() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php echo 1;");
        let mut vfs = Vfs::default();
        let file = vfs.load_from_disk(&root.join("src/A.php")).unwrap();
        assert_eq!(vfs.contents(file), Some(b"<?php echo 1;".as_slice()));
        assert!(vfs.load_from_disk(&root.join("missing.php")).is_err());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Register the module in `crates/celerrate_vfs/src/lib.rs`: add `mod walk;` after
`mod vfs;` and `pub use walk::enumerate_php_files;` after the `vfs` re-export.

Run: `cargo test --package celerrate_vfs`
Expected: compile error — `enumerate_php_files` and `load_from_disk` not found.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/celerrate_vfs/src/walk.rs`:

```rust
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use celerrate_source::FileId;

use crate::Vfs;

/// Enumerates the PHP files under a set of walk roots, sorted and
/// deduplicated. A root that is a file is included as-is (autoload
/// `files` and `classmap` entries may name single files of any
/// extension); a root that is a directory is walked recursively for
/// `.php` files (ASCII-case-insensitive). Symbolic links to
/// directories are followed with cycle protection; missing roots and
/// unreadable entries are skipped: enumeration never fails.
pub fn enumerate_php_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    let mut visited_directories = BTreeSet::new();
    for root in roots {
        if root.is_file() {
            files.insert(root.clone());
        } else if root.is_dir() {
            walk_directory(root, &mut files, &mut visited_directories);
        }
    }
    files.into_iter().collect()
}

fn walk_directory(
    directory: &Path,
    files: &mut BTreeSet<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
) {
    // The canonical form only guards against symbolic-link cycles;
    // reported paths keep the shape the walk reached them under.
    let Ok(canonical) = fs::canonicalize(directory) else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_directory(&path, files, visited);
        } else if has_php_extension(&path) && path.is_file() {
            files.insert(path);
        }
    }
}

fn has_php_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("php"))
}

impl Vfs {
    /// Loads a file's current disk bytes into the disk state. This is
    /// push-time work for the composition root, never called during a
    /// query.
    pub fn load_from_disk(&mut self, path: &Path) -> std::io::Result<FileId> {
        let bytes = fs::read(path)?;
        Ok(self.set_file_contents(path, Some(bytes)))
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_vfs`
Expected: PASS (7 new tests; the symbolic-link test terminating at all proves the
cycle guard).

- [ ] **Step 6: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace && cargo deny check`
Expected: no warnings, all green.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/celerrate_vfs
git commit -m "✨ feat(vfs): enumerate PHP files under declared walk roots"
```

---

### Task 3: The `celerrate_project` crate and the PHP version model

**Files:**
- Create: `crates/celerrate_project/Cargo.toml`
- Create: `crates/celerrate_project/src/lib.rs`
- Create: `crates/celerrate_project/src/version.rs`

**Interfaces:**
- Consumes: nothing yet.
- Produces:
  `PhpVersion { pub major: u8, pub minor: u8 }` (`const fn new(major, minor)`,
  `Display` as `"8.1"`, full `Ord`, `fn clamped_to_supported(self) -> Self`);
  `SUPPORTED_VERSIONS: [PhpVersion; 5]` (8.1 through 8.5, ascending);
  `OLDEST_SUPPORTED_VERSION`, `LATEST_STABLE_VERSION`;
  `PhpVersionRange { pub minimum: PhpVersion, pub maximum: PhpVersion }`
  (`const fn new(minimum, maximum)`, `const fn point(version)`,
  `const fn fallback()` = point at latest stable).

- [ ] **Step 1: Create the crate manifest**

`crates/celerrate_project/Cargo.toml`:

```toml
[package]
name = "celerrate_project"
description = "Composer discovery, autoload rules, and the PHP version range for the Celerrate toolchain"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[lints]
workspace = true
```

`crates/celerrate_project/src/lib.rs`:

```rust
//! Composer project discovery.
//!
//! Zero-configuration detection: `composer.json` is located and read
//! tolerantly (a corrupted or missing file produces a notice and
//! defaults, never a failure), the autoload rules it and
//! `vendor/composer/installed.json` declare drive the disk walk and
//! classify every file as project or vendor, and the PHP version range
//! follows the parent spec's detection precedence. This crate is pure:
//! the configuration becomes a salsa input at the composition root.

mod version;

pub use version::{
    LATEST_STABLE_VERSION, OLDEST_SUPPORTED_VERSION, PhpVersion, PhpVersionRange,
    SUPPORTED_VERSIONS,
};
```

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_project/src/version.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{
        LATEST_STABLE_VERSION, OLDEST_SUPPORTED_VERSION, PhpVersion, PhpVersionRange,
        SUPPORTED_VERSIONS,
    };

    #[test]
    fn a_version_displays_as_major_dot_minor() {
        assert_eq!(PhpVersion::new(8, 1).to_string(), "8.1");
    }

    #[test]
    fn versions_order_by_major_then_minor() {
        assert!(PhpVersion::new(8, 1) < PhpVersion::new(8, 2));
        assert!(PhpVersion::new(7, 9) < PhpVersion::new(8, 0));
    }

    #[test]
    fn the_supported_window_is_php_8_1_through_8_5_ascending() {
        assert_eq!(SUPPORTED_VERSIONS.first(), Some(&OLDEST_SUPPORTED_VERSION));
        assert_eq!(SUPPORTED_VERSIONS.last(), Some(&LATEST_STABLE_VERSION));
        assert_eq!(OLDEST_SUPPORTED_VERSION, PhpVersion::new(8, 1));
        assert_eq!(LATEST_STABLE_VERSION, PhpVersion::new(8, 5));
        assert!(SUPPORTED_VERSIONS.is_sorted());
    }

    #[test]
    fn clamping_folds_into_the_supported_window() {
        assert_eq!(
            PhpVersion::new(7, 4).clamped_to_supported(),
            PhpVersion::new(8, 1),
        );
        assert_eq!(
            PhpVersion::new(9, 0).clamped_to_supported(),
            PhpVersion::new(8, 5),
        );
        assert_eq!(
            PhpVersion::new(8, 3).clamped_to_supported(),
            PhpVersion::new(8, 3),
        );
    }

    #[test]
    fn a_point_range_pins_both_ends_and_fallback_is_the_latest_stable() {
        let point = PhpVersionRange::point(PhpVersion::new(8, 2));
        assert_eq!(point.minimum, PhpVersion::new(8, 2));
        assert_eq!(point.maximum, PhpVersion::new(8, 2));
        assert_eq!(
            PhpVersionRange::fallback(),
            PhpVersionRange::point(LATEST_STABLE_VERSION),
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_project`
Expected: compile error — the types do not exist yet.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/celerrate_project/src/version.rs`:

```rust
use core::fmt;

/// A PHP version at the granularity the engine reasons about:
/// `major.minor`. Patch components are truncated on input; version
/// gating and availability metadata are minor-granular.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhpVersion {
    pub major: u8,
    pub minor: u8,
}

impl PhpVersion {
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }

    /// Folds into the supported window: older than the oldest
    /// supported becomes the oldest, newer than the latest stable
    /// becomes the latest.
    pub fn clamped_to_supported(self) -> Self {
        self.clamp(OLDEST_SUPPORTED_VERSION, LATEST_STABLE_VERSION)
    }
}

impl fmt::Display for PhpVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

/// The versions this binary knows, oldest to newest. Bumped
/// deliberately when a new PHP minor becomes supported, like the
/// pinned corpus and stub snapshots.
pub const SUPPORTED_VERSIONS: [PhpVersion; 5] = [
    PhpVersion::new(8, 1),
    PhpVersion::new(8, 2),
    PhpVersion::new(8, 3),
    PhpVersion::new(8, 4),
    PhpVersion::new(8, 5),
];

/// The oldest version the engine supports (parent spec: PHP 8.1+).
pub const OLDEST_SUPPORTED_VERSION: PhpVersion = PhpVersion::new(8, 1);

/// The newest supported stable version: the zero-configuration
/// fallback when no version signal exists.
pub const LATEST_STABLE_VERSION: PhpVersion = PhpVersion::new(8, 5);

/// The supported PHP version range `[minimum, maximum]`, inclusive.
/// Availability checks run at the minimum, removal and deprecation
/// checks at the maximum (parent spec's range rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhpVersionRange {
    pub minimum: PhpVersion,
    pub maximum: PhpVersion,
}

impl PhpVersionRange {
    pub const fn new(minimum: PhpVersion, maximum: PhpVersion) -> Self {
        Self { minimum, maximum }
    }

    /// A range collapsed to a single version.
    pub const fn point(version: PhpVersion) -> Self {
        Self::new(version, version)
    }

    /// The zero-configuration fallback: the latest supported stable
    /// version, as a point.
    pub const fn fallback() -> Self {
        Self::point(LATEST_STABLE_VERSION)
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_project`
Expected: PASS (5 tests)

- [ ] **Step 6: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: no warnings, all green.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/celerrate_project
git commit -m "✨ feat(project): model PHP versions and the supported range"
```

---

### Task 4: Composer version constraints

**Files:**
- Create: `crates/celerrate_project/src/constraint.rs`
- Modify: `crates/celerrate_project/src/lib.rs`

**Interfaces:**
- Consumes: `PhpVersion`, `PhpVersionRange`, `SUPPORTED_VERSIONS` (Task 3).
- Produces:
  `version_range_for_constraint(constraint: &str) -> Option<PhpVersionRange>` —
  interprets a Composer constraint as the supported versions it admits; `None` when
  unparseable OR when no supported version satisfies it (the caller reports CEL0021
  and falls back);
  `php_version_from_text(text: &str) -> Option<PhpVersion>` — a plain version
  literal (`8.1`, `8.1.2`, `v8.3-dev`) at minor precision, for `config.platform.php`;
  bare majors and wildcards are rejected (a platform names one concrete runtime).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_project/src/constraint.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{php_version_from_text, version_range_for_constraint};
    use crate::version::{PhpVersion, PhpVersionRange};

    fn range(
        minimum_major: u8,
        minimum_minor: u8,
        maximum_major: u8,
        maximum_minor: u8,
    ) -> PhpVersionRange {
        PhpVersionRange::new(
            PhpVersion::new(minimum_major, minimum_minor),
            PhpVersion::new(maximum_major, maximum_minor),
        )
    }

    #[test]
    fn caret_spans_to_the_end_of_the_major() {
        assert_eq!(version_range_for_constraint("^8.1"), Some(range(8, 1, 8, 5)));
        assert_eq!(
            version_range_for_constraint("^8.2.3"),
            Some(range(8, 2, 8, 5)),
        );
    }

    #[test]
    fn exact_versions_and_minor_wildcards_pin_the_minor() {
        assert_eq!(version_range_for_constraint("8.3"), Some(range(8, 3, 8, 3)));
        assert_eq!(
            version_range_for_constraint("8.2.7"),
            Some(range(8, 2, 8, 2)),
        );
        assert_eq!(
            version_range_for_constraint("8.2.*"),
            Some(range(8, 2, 8, 2)),
        );
    }

    #[test]
    fn a_bare_major_and_its_wildcard_cover_the_major() {
        assert_eq!(version_range_for_constraint("8"), Some(range(8, 1, 8, 5)));
        assert_eq!(version_range_for_constraint("8.*"), Some(range(8, 1, 8, 5)));
        assert_eq!(version_range_for_constraint("*"), Some(range(8, 1, 8, 5)));
    }

    #[test]
    fn comparisons_combine_with_spaces_or_commas_as_and() {
        assert_eq!(
            version_range_for_constraint(">=8.2 <8.5"),
            Some(range(8, 2, 8, 4)),
        );
        assert_eq!(
            version_range_for_constraint(">=8.2,<=8.4"),
            Some(range(8, 2, 8, 4)),
        );
    }

    #[test]
    fn strict_bounds_follow_the_minor_precision_truncation_rules() {
        // `>8.1.0` admits `8.1.1`, so minor 8.1 stays in.
        assert_eq!(version_range_for_constraint(">8.1"), Some(range(8, 1, 8, 5)));
        // `<8.4.0` excludes all of minor 8.4 ...
        assert_eq!(version_range_for_constraint("<8.4"), Some(range(8, 1, 8, 3)));
        // ... but `<8.4.3` still admits `8.4.0`.
        assert_eq!(
            version_range_for_constraint("<8.4.3"),
            Some(range(8, 1, 8, 4)),
        );
    }

    #[test]
    fn tilde_bumps_the_component_below_the_last_named_one() {
        assert_eq!(version_range_for_constraint("~8.1"), Some(range(8, 1, 8, 5)));
        assert_eq!(
            version_range_for_constraint("~8.1.0"),
            Some(range(8, 1, 8, 1)),
        );
    }

    #[test]
    fn hyphen_ranges_complete_a_partial_upper_bound_with_a_wildcard() {
        assert_eq!(
            version_range_for_constraint("8.1 - 8.3"),
            Some(range(8, 1, 8, 3)),
        );
        assert_eq!(
            version_range_for_constraint("8.2 - 8"),
            Some(range(8, 2, 8, 5)),
        );
    }

    #[test]
    fn alternatives_take_the_union() {
        assert_eq!(
            version_range_for_constraint("^7.4 || ^8.0"),
            Some(range(8, 1, 8, 5)),
        );
    }

    #[test]
    fn stability_flags_and_prefixes_are_ignored() {
        assert_eq!(
            version_range_for_constraint("^8.1@dev"),
            Some(range(8, 1, 8, 5)),
        );
        assert_eq!(
            version_range_for_constraint("v8.2"),
            Some(range(8, 2, 8, 2)),
        );
        assert_eq!(
            version_range_for_constraint("8.1.0-beta1"),
            Some(range(8, 1, 8, 1)),
        );
    }

    #[test]
    fn unparseable_and_unsatisfiable_constraints_are_rejected() {
        assert_eq!(version_range_for_constraint("banana"), None);
        assert_eq!(version_range_for_constraint(""), None);
        assert_eq!(version_range_for_constraint("!=8.1"), None);
        // Parseable, but admits no supported version.
        assert_eq!(version_range_for_constraint("7.4.*"), None);
    }

    #[test]
    fn a_platform_version_is_a_concrete_literal() {
        assert_eq!(php_version_from_text("8.1.2"), Some(PhpVersion::new(8, 1)));
        assert_eq!(php_version_from_text("v8.3-dev"), Some(PhpVersion::new(8, 3)));
        assert_eq!(php_version_from_text("8"), None);
        assert_eq!(php_version_from_text("8.*"), None);
        assert_eq!(php_version_from_text("^8.1"), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Register the module in `crates/celerrate_project/src/lib.rs`: add `mod constraint;`
above `mod version;` and
`pub use constraint::{php_version_from_text, version_range_for_constraint};` above
the `version` re-export.

Run: `cargo test --package celerrate_project`
Expected: compile error — the functions do not exist yet.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/celerrate_project/src/constraint.rs`:

```rust
//! Composer version constraints, interpreted at minor precision.
//!
//! The engine reasons in `major.minor`, so every constraint is
//! truncated to that granularity with Composer's semantics preserved:
//! `>8.1.0` still admits `8.1.1` (minor 8.1 stays in), `<8.4.3` still
//! admits `8.4.0` (minor 8.4 stays in), `<8.4` excludes minor 8.4.
//! One documented deviation: a bare major (`"8"`) reads as the major
//! wildcard `8.*`, matching author intent in `require.php` rather than
//! Composer's exact-version normalization.

use crate::version::{PhpVersion, PhpVersionRange, SUPPORTED_VERSIONS};

/// Interprets a Composer constraint as the supported range it admits:
/// the lowest and highest supported versions satisfying it. `None`
/// when the constraint cannot be parsed or admits no supported
/// version; the caller decides the fallback and the notice.
pub fn version_range_for_constraint(constraint: &str) -> Option<PhpVersionRange> {
    let alternatives = parse_alternatives(constraint)?;
    let satisfied: Vec<PhpVersion> = SUPPORTED_VERSIONS
        .iter()
        .copied()
        .filter(|version| {
            alternatives.iter().any(|conjunction| {
                conjunction.iter().all(|interval| interval.contains(*version))
            })
        })
        .collect();
    Some(PhpVersionRange::new(
        *satisfied.first()?,
        *satisfied.last()?,
    ))
}

/// Parses a plain version literal (`8.1`, `8.1.2`, `v8.3-dev`) at
/// minor precision, for `config.platform.php`. Bare majors, wildcards,
/// and operators are rejected: a platform names one concrete runtime.
pub fn php_version_from_text(text: &str) -> Option<PhpVersion> {
    let literal = parse_version_literal(text.trim())?;
    let minor = literal.minor?;
    Some(PhpVersion::new(literal.major, minor))
}

/// One inclusive-lower, exclusive-upper interval at minor precision.
/// `None` bounds are unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MinorInterval {
    lower_inclusive: Option<PhpVersion>,
    upper_exclusive: Option<PhpVersion>,
}

impl MinorInterval {
    const UNBOUNDED: Self = Self {
        lower_inclusive: None,
        upper_exclusive: None,
    };

    fn lower(version: PhpVersion) -> Self {
        Self {
            lower_inclusive: Some(version),
            upper_exclusive: None,
        }
    }

    fn upper(version: PhpVersion) -> Self {
        Self {
            lower_inclusive: None,
            upper_exclusive: Some(version),
        }
    }

    fn between(lower: PhpVersion, upper: PhpVersion) -> Self {
        Self {
            lower_inclusive: Some(lower),
            upper_exclusive: Some(upper),
        }
    }

    fn contains(self, version: PhpVersion) -> bool {
        self.lower_inclusive.is_none_or(|lower| version >= lower)
            && self.upper_exclusive.is_none_or(|upper| version < upper)
    }
}

/// A version literal: `8`, `8.1`, `8.1.2`, `v8.1`, `8.1.*`,
/// `8.1.2-beta1`. Pre-release and build suffixes are dropped;
/// components beyond the patch (Composer allows four) are ignored.
struct VersionLiteral {
    major: u8,
    /// `None`: bare major or a major-level wildcard.
    minor: Option<u8>,
    /// `None`: absent or a wildcard.
    patch: Option<u32>,
}

impl VersionLiteral {
    fn minor_version(&self) -> PhpVersion {
        PhpVersion::new(self.major, self.minor.unwrap_or(0))
    }
}

fn parse_version_literal(text: &str) -> Option<VersionLiteral> {
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let text = text.split(['-', '+']).next()?;
    let mut parts = text.split('.');
    let major: u8 = parts.next()?.parse().ok()?;
    let minor = match parts.next() {
        None | Some("*" | "x" | "X") => None,
        Some(part) => Some(part.parse::<u8>().ok()?),
    };
    let patch = match parts.next() {
        None | Some("*" | "x" | "X") => None,
        Some(part) => Some(part.parse::<u32>().ok()?),
    };
    Some(VersionLiteral { major, minor, patch })
}

fn next_minor(version: PhpVersion) -> Option<PhpVersion> {
    version
        .minor
        .checked_add(1)
        .map(|minor| PhpVersion::new(version.major, minor))
}

fn next_major(version: PhpVersion) -> Option<PhpVersion> {
    version
        .major
        .checked_add(1)
        .map(|major| PhpVersion::new(major, 0))
}

/// The alternatives (`||`, and legacy single `|`) of conjunctions.
fn parse_alternatives(constraint: &str) -> Option<Vec<Vec<MinorInterval>>> {
    let groups: Vec<&str> = constraint
        .split('|')
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .collect();
    if groups.is_empty() {
        return None;
    }
    groups.into_iter().map(parse_conjunction).collect()
}

/// One AND group: simples separated by whitespace or commas, with
/// spaced hyphen ranges (`8.1 - 8.3`) folded into single intervals.
fn parse_conjunction(text: &str) -> Option<Vec<MinorInterval>> {
    let replaced = text.replace(',', " ");
    let tokens: Vec<&str> = replaced.split_whitespace().collect();
    let mut intervals = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens.get(index)?;
        if tokens.get(index + 1).copied() == Some("-") {
            intervals.push(hyphen_range(token, tokens.get(index + 2)?)?);
            index += 3;
        } else {
            intervals.push(parse_simple(token)?);
            index += 1;
        }
    }
    if intervals.is_empty() { None } else { Some(intervals) }
}

fn hyphen_range(lower: &str, upper: &str) -> Option<MinorInterval> {
    let lower_literal = parse_version_literal(strip_stability(lower)?)?;
    let upper_literal = parse_version_literal(strip_stability(upper)?)?;
    let upper_version = upper_literal.minor_version();
    // A partial upper bound is completed by a wildcard, per Composer:
    // `8.1 - 8.3` is `>=8.1 <8.4`, `8.2 - 8` is `>=8.2 <9.0`.
    let upper_exclusive = if upper_literal.minor.is_some() {
        next_minor(upper_version)?
    } else {
        next_major(upper_version)?
    };
    Some(MinorInterval::between(
        lower_literal.minor_version(),
        upper_exclusive,
    ))
}

fn strip_stability(token: &str) -> Option<&str> {
    token.split('@').next()
}

fn parse_simple(token: &str) -> Option<MinorInterval> {
    let token = strip_stability(token)?;
    if token.is_empty() {
        return None;
    }
    if matches!(token, "*" | "x" | "X") {
        return Some(MinorInterval::UNBOUNDED);
    }
    if let Some(rest) = token.strip_prefix(">=") {
        return Some(MinorInterval::lower(
            parse_version_literal(rest)?.minor_version(),
        ));
    }
    if let Some(rest) = token.strip_prefix("<=") {
        let version = parse_version_literal(rest)?.minor_version();
        return Some(MinorInterval::upper(next_minor(version)?));
    }
    if let Some(rest) = token.strip_prefix('>') {
        // Composer's `>8.1.0` still admits `8.1.1`, so at minor
        // precision `>` behaves as `>=`.
        return Some(MinorInterval::lower(
            parse_version_literal(rest)?.minor_version(),
        ));
    }
    if let Some(rest) = token.strip_prefix('<') {
        let literal = parse_version_literal(rest)?;
        let version = literal.minor_version();
        let upper = if literal.patch.unwrap_or(0) > 0 {
            // `<8.4.3` still admits `8.4.0`.
            next_minor(version)?
        } else {
            version
        };
        return Some(MinorInterval::upper(upper));
    }
    if let Some(rest) = token.strip_prefix('^') {
        let version = parse_version_literal(rest)?.minor_version();
        return Some(MinorInterval::between(version, next_major(version)?));
    }
    if let Some(rest) = token.strip_prefix('~') {
        let literal = parse_version_literal(rest)?;
        let version = literal.minor_version();
        let upper = if literal.patch.is_some() {
            next_minor(version)?
        } else {
            next_major(version)?
        };
        return Some(MinorInterval::between(version, upper));
    }
    // A plain literal: `8` and `8.*` cover the major; `8.1` and
    // `8.1.2` pin the minor.
    let literal = parse_version_literal(token)?;
    let version = literal.minor_version();
    let upper = if literal.minor.is_some() {
        next_minor(version)?
    } else {
        next_major(version)?
    };
    Some(MinorInterval::between(version, upper))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_project`
Expected: PASS (11 new tests)

- [ ] **Step 5: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: no warnings, all green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_project
git commit -m "✨ feat(project): interpret Composer version constraints as ranges"
```

---

### Task 5: Autoload rules

**Files:**
- Create: `crates/celerrate_project/src/autoload.rs`
- Modify: `crates/celerrate_project/src/lib.rs`
- Modify: `crates/celerrate_project/Cargo.toml` (add `serde_json`, `celerrate_vfs`)
- Modify: `Cargo.toml` (workspace root: add `serde_json` to `[workspace.dependencies]`)

**Interfaces:**
- Consumes: `celerrate_vfs::normalize_path(path: &Path, base: &Path) -> PathBuf`
  (Task 1); `serde_json::Value`.
- Produces:
  `AutoloadRules { pub psr4: Vec<NamespaceMapping>, pub psr0: Vec<NamespaceMapping>, pub classmap: Vec<String>, pub files: Vec<String> }`
  with `fn from_json(value: Option<&serde_json::Value>) -> Self` (tolerant: absent or
  mistyped sections yield empty rules), `fn merged(self, other: Self) -> Self`,
  `fn is_empty(&self) -> bool`,
  `fn walk_roots(&self, base: &Path) -> Vec<PathBuf>` (resolved, normalized, sorted,
  deduplicated);
  `NamespaceMapping { pub prefix: String, pub directories: Vec<String> }`.

- [ ] **Step 1: Add the dependencies**

In the root `Cargo.toml`, `[workspace.dependencies]` becomes (alphabetical):

```toml
[workspace.dependencies]
insta = { version = "1", features = ["glob"] }
rowan = "0.16"
salsa = "0.27"
serde_json = "1"
tempfile = "3"
text-size = "1"
ungrammar = "1"
```

In `crates/celerrate_project/Cargo.toml`, insert between `[package]` and `[lints]`:

```toml
[dependencies]
celerrate_vfs = { path = "../celerrate_vfs" }
serde_json = { workspace = true }
```

Run: `cargo deny check`
Expected: PASS (`serde_json` pulls `serde`, `itoa`, `ryu`, `memchr` — all satisfy the
allow list via MIT or Apache-2.0).

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_project/src/autoload.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::{AutoloadRules, NamespaceMapping};

    fn rules(json: &str) -> AutoloadRules {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        AutoloadRules::from_json(Some(&value))
    }

    #[test]
    fn every_section_is_read_with_string_and_array_directory_forms() {
        let rules = rules(
            r#"{
                "psr-4": { "App\\": "src/", "Tools\\": ["tools/a", "tools/b"] },
                "psr-0": { "Legacy_": "legacy/" },
                "classmap": ["database/seeds", "lib/Single.php"],
                "files": ["helpers.php"]
            }"#,
        );
        assert_eq!(
            rules.psr4,
            vec![
                NamespaceMapping {
                    prefix: String::from("App\\"),
                    directories: vec![String::from("src/")],
                },
                NamespaceMapping {
                    prefix: String::from("Tools\\"),
                    directories: vec![String::from("tools/a"), String::from("tools/b")],
                },
            ],
        );
        assert_eq!(
            rules.psr0,
            vec![NamespaceMapping {
                prefix: String::from("Legacy_"),
                directories: vec![String::from("legacy/")],
            }],
        );
        assert_eq!(
            rules.classmap,
            vec![String::from("database/seeds"), String::from("lib/Single.php")],
        );
        assert_eq!(rules.files, vec![String::from("helpers.php")]);
    }

    #[test]
    fn absent_and_mistyped_sections_yield_empty_rules() {
        assert!(AutoloadRules::from_json(None).is_empty());
        assert!(rules("{}").is_empty());
        assert!(rules(r#"{ "psr-4": "not an object", "classmap": 3 }"#).is_empty());
        assert!(rules(r#"{ "classmap": [1, true] }"#).is_empty());
    }

    #[test]
    fn merging_appends_the_other_rules() {
        let merged = rules(r#"{ "psr-4": { "App\\": "src/" } }"#)
            .merged(rules(r#"{ "psr-4": { "Tests\\": "tests/" }, "files": ["dev.php"] }"#));
        assert_eq!(merged.psr4.len(), 2);
        assert_eq!(merged.files, vec![String::from("dev.php")]);
    }

    #[test]
    fn walk_roots_resolve_normalize_sort_and_deduplicate() {
        let rules = rules(
            r#"{
                "psr-4": { "App\\": "src/", "Again\\": "./src" },
                "classmap": ["lib/Single.php"],
                "files": ["helpers.php"]
            }"#,
        );
        assert_eq!(
            rules.walk_roots(Path::new("/project")),
            vec![
                PathBuf::from("/project/helpers.php"),
                PathBuf::from("/project/lib/Single.php"),
                PathBuf::from("/project/src"),
            ],
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Register the module in `crates/celerrate_project/src/lib.rs`: add `mod autoload;`
above `mod constraint;` and
`pub use autoload::{AutoloadRules, NamespaceMapping};` above the `constraint`
re-export.

Run: `cargo test --package celerrate_project`
Expected: compile error — the types do not exist yet.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/celerrate_project/src/autoload.rs`:

```rust
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use celerrate_vfs::normalize_path;

/// The autoload rules one `composer.json` (or one installed package)
/// declares. Directories and files are kept as declared, relative to
/// the declaring package's root; [`AutoloadRules::walk_roots`]
/// resolves them. Namespace prefixes are retained for the resolution
/// layers of later parts; the walk flattens them away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoloadRules {
    pub psr4: Vec<NamespaceMapping>,
    pub psr0: Vec<NamespaceMapping>,
    pub classmap: Vec<String>,
    pub files: Vec<String>,
}

/// One PSR-4 or PSR-0 entry: a namespace prefix and the directories
/// that serve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceMapping {
    pub prefix: String,
    pub directories: Vec<String>,
}

impl AutoloadRules {
    /// Reads one `autoload`-shaped JSON object tolerantly: an absent
    /// or mistyped section yields empty rules, never a failure.
    pub fn from_json(value: Option<&serde_json::Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        Self {
            psr4: namespace_mappings(value.get("psr-4")),
            psr0: namespace_mappings(value.get("psr-0")),
            classmap: string_list(value.get("classmap")),
            files: string_list(value.get("files")),
        }
    }

    /// Appends the other rules; used to fold `autoload-dev` into
    /// `autoload` (test code is project code).
    pub fn merged(mut self, other: Self) -> Self {
        self.psr4.extend(other.psr4);
        self.psr0.extend(other.psr0);
        self.classmap.extend(other.classmap);
        self.files.extend(other.files);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.psr4.is_empty()
            && self.psr0.is_empty()
            && self.classmap.is_empty()
            && self.files.is_empty()
    }

    /// Every declared directory and file, resolved against the
    /// declaring root, normalized, deduplicated, and sorted.
    pub fn walk_roots(&self, base: &Path) -> Vec<PathBuf> {
        let mut roots = BTreeSet::new();
        for mapping in self.psr4.iter().chain(&self.psr0) {
            for directory in &mapping.directories {
                roots.insert(normalize_path(Path::new(directory), base));
            }
        }
        for declared in self.classmap.iter().chain(&self.files) {
            roots.insert(normalize_path(Path::new(declared), base));
        }
        roots.into_iter().collect()
    }
}

fn namespace_mappings(value: Option<&serde_json::Value>) -> Vec<NamespaceMapping> {
    let Some(object) = value.and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    object
        .iter()
        .map(|(prefix, directories)| NamespaceMapping {
            prefix: prefix.clone(),
            directories: match directories {
                serde_json::Value::String(directory) => vec![directory.clone()],
                serde_json::Value::Array(entries) => entries
                    .iter()
                    .filter_map(|entry| entry.as_str().map(str::to_owned))
                    .collect(),
                _ => Vec::new(),
            },
        })
        .collect()
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
```

Note: `serde_json`'s default map is `BTreeMap` (no `preserve_order` feature), so
`object.iter()` is key-sorted — the mapping order is deterministic.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_project`
Expected: PASS (4 new tests)

- [ ] **Step 6: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace && cargo deny check`
Expected: no warnings, all green.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/celerrate_project
git commit -m "✨ feat(project): model autoload rules and their walk roots"
```

---

### Task 6: The `composer.json` reader

**Files:**
- Create: `crates/celerrate_project/src/manifest.rs`
- Modify: `crates/celerrate_project/src/lib.rs`

**Interfaces:**
- Consumes: `AutoloadRules::from_json`, `AutoloadRules::merged` (Task 5).
- Produces:
  `ComposerManifest { pub platform_php: Option<String>, pub require_php: Option<String>, pub vendor_directory: Option<String>, pub autoload: AutoloadRules }`;
  `parse_manifest(text: &str) -> Option<ComposerManifest>` — `None` only when the
  text is not a JSON object at all; every field inside is read tolerantly.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_project/src/manifest.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::parse_manifest;

    #[test]
    fn the_consumed_fields_are_extracted() {
        let manifest = parse_manifest(
            r#"{
                "require": { "php": "^8.1", "acme/library": "^2.0" },
                "config": { "platform": { "php": "8.1.2" }, "vendor-dir": "third-party" },
                "autoload": { "psr-4": { "App\\": "src/" } },
                "autoload-dev": { "psr-4": { "Tests\\": "tests/" } }
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.require_php.as_deref(), Some("^8.1"));
        assert_eq!(manifest.platform_php.as_deref(), Some("8.1.2"));
        assert_eq!(manifest.vendor_directory.as_deref(), Some("third-party"));
        let prefixes: Vec<&str> = manifest
            .autoload
            .psr4
            .iter()
            .map(|mapping| mapping.prefix.as_str())
            .collect();
        assert_eq!(prefixes, vec!["App\\", "Tests\\"]);
    }

    #[test]
    fn absent_and_mistyped_fields_fall_back_to_defaults() {
        let manifest = parse_manifest(
            r#"{ "require": { "php": 8 }, "config": "nope", "autoload": [] }"#,
        )
        .unwrap();
        assert_eq!(manifest.require_php, None);
        assert_eq!(manifest.platform_php, None);
        assert_eq!(manifest.vendor_directory, None);
        assert!(manifest.autoload.is_empty());
        assert!(parse_manifest("{}").unwrap().autoload.is_empty());
    }

    #[test]
    fn non_object_documents_are_rejected() {
        assert_eq!(parse_manifest("not json at all"), None);
        assert_eq!(parse_manifest("[1, 2]"), None);
        assert_eq!(parse_manifest("\"just a string\""), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Register the module in `crates/celerrate_project/src/lib.rs`: add `mod manifest;`
after `mod constraint;` and
`pub use manifest::{ComposerManifest, parse_manifest};` after the `constraint`
re-export.

Run: `cargo test --package celerrate_project`
Expected: compile error — the type and function do not exist yet.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/celerrate_project/src/manifest.rs`:

```rust
use crate::autoload::AutoloadRules;

/// The fields of `composer.json` the engine consumes, read
/// tolerantly: an absent or mistyped field falls back to its default,
/// never a failure. `autoload` and `autoload-dev` arrive merged (test
/// code is project code).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposerManifest {
    /// `config.platform.php`: the concrete runtime the project pins.
    pub platform_php: Option<String>,
    /// `require.php`: the constraint the project declares.
    pub require_php: Option<String>,
    /// `config.vendor-dir`: where installed dependencies live
    /// (default `vendor`).
    pub vendor_directory: Option<String>,
    pub autoload: AutoloadRules,
}

/// Parses `composer.json`. `None` only when the text is not a JSON
/// object at all; the caller reports the notice and falls back.
pub fn parse_manifest(text: &str) -> Option<ComposerManifest> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    let config = object.get("config");
    Some(ComposerManifest {
        platform_php: config
            .and_then(|config| config.get("platform"))
            .and_then(|platform| platform.get("php"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        require_php: object
            .get("require")
            .and_then(|require| require.get("php"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        vendor_directory: config
            .and_then(|config| config.get("vendor-dir"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        autoload: AutoloadRules::from_json(object.get("autoload"))
            .merged(AutoloadRules::from_json(object.get("autoload-dev"))),
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_project`
Expected: PASS (3 new tests)

- [ ] **Step 5: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: no warnings, all green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_project
git commit -m "✨ feat(project): read composer.json tolerantly"
```

---

### Task 7: The `installed.json` reader

**Files:**
- Create: `crates/celerrate_project/src/installed.rs`
- Modify: `crates/celerrate_project/src/lib.rs`

**Interfaces:**
- Consumes: `AutoloadRules::from_json` (Task 5),
  `celerrate_vfs::normalize_path` (Task 1).
- Produces:
  `VendorPackage { pub name: String, pub root: PathBuf, pub autoload: AutoloadRules }`
  (`root` absolute and normalized);
  `parse_installed_packages(text: &str, composer_directory: &Path) -> Option<Vec<VendorPackage>>`
  — `composer_directory` is the directory containing the file
  (`<vendor>/composer`), which `install-path` entries are relative to. Accepts the
  Composer 2 object form (`{"packages": [...]}`) and the Composer 1 bare-array form;
  `None` when the text is valid in neither shape.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_project/src/installed.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::parse_installed_packages;

    const COMPOSER_DIRECTORY: &str = "/project/vendor/composer";

    #[test]
    fn composer_2_packages_resolve_their_install_paths() {
        let packages = parse_installed_packages(
            r#"{
                "packages": [
                    {
                        "name": "acme/library",
                        "install-path": "../acme/library",
                        "autoload": { "psr-4": { "Acme\\": "src/" } }
                    }
                ],
                "dev": true,
                "dev-package-names": []
            }"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(packages.len(), 1);
        let package = packages.first().unwrap();
        assert_eq!(package.name, "acme/library");
        assert_eq!(package.root, PathBuf::from("/project/vendor/acme/library"));
        assert_eq!(
            package.autoload.walk_roots(&package.root),
            vec![PathBuf::from("/project/vendor/acme/library/src")],
        );
    }

    #[test]
    fn a_missing_install_path_defaults_to_the_vendor_slash_name_layout() {
        let packages = parse_installed_packages(
            r#"{ "packages": [ { "name": "acme/library" } ] }"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(
            packages.first().unwrap().root,
            PathBuf::from("/project/vendor/acme/library"),
        );
    }

    #[test]
    fn the_composer_1_bare_array_form_is_accepted() {
        let packages = parse_installed_packages(
            r#"[ { "name": "acme/library", "autoload": { "files": ["functions.php"] } } ]"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(
            packages.first().unwrap().root,
            PathBuf::from("/project/vendor/acme/library"),
        );
    }

    #[test]
    fn nameless_entries_are_skipped_and_broken_documents_rejected() {
        let packages = parse_installed_packages(
            r#"{ "packages": [ { "install-path": "../x/y" }, { "name": "kept/one" } ] }"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages.first().unwrap().name, "kept/one");

        assert!(parse_installed_packages("not json", Path::new(COMPOSER_DIRECTORY)).is_none());
        assert!(parse_installed_packages("3", Path::new(COMPOSER_DIRECTORY)).is_none());
        assert!(
            parse_installed_packages(r#"{ "packages": 3 }"#, Path::new(COMPOSER_DIRECTORY))
                .is_none()
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Register the module in `crates/celerrate_project/src/lib.rs`: add `mod installed;`
after `mod constraint;` and
`pub use installed::{VendorPackage, parse_installed_packages};` after the
`constraint` re-export.

Run: `cargo test --package celerrate_project`
Expected: compile error — the type and function do not exist yet.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/celerrate_project/src/installed.rs`:

```rust
use std::path::{Path, PathBuf};

use celerrate_vfs::normalize_path;

use crate::autoload::AutoloadRules;

/// One installed dependency: its name, its resolved package root, and
/// the autoload rules it declares. All listed packages count,
/// including dev packages: a symbol declared anywhere in vendor is
/// declared (spec section 7's conservative stance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorPackage {
    pub name: String,
    /// Absolute, normalized package root.
    pub root: PathBuf,
    pub autoload: AutoloadRules,
}

/// Parses `installed.json`. `composer_directory` is the directory
/// containing the file (`<vendor>/composer`), which `install-path`
/// entries are relative to. Accepts the Composer 2 object form
/// (`{"packages": [...]}`) and the Composer 1 bare-array form; `None`
/// when the text is valid in neither shape. Entries without a name
/// are skipped, never failures.
pub fn parse_installed_packages(
    text: &str,
    composer_directory: &Path,
) -> Option<Vec<VendorPackage>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let packages = match &value {
        serde_json::Value::Array(entries) => entries,
        serde_json::Value::Object(object) => object.get("packages")?.as_array()?,
        _ => return None,
    };
    Some(
        packages
            .iter()
            .filter_map(|package| vendor_package(package, composer_directory))
            .collect(),
    )
}

fn vendor_package(
    package: &serde_json::Value,
    composer_directory: &Path,
) -> Option<VendorPackage> {
    let name = package.get("name")?.as_str()?.to_owned();
    // Composer 1 entries carry no `install-path`; the package then
    // lives at `<vendor>/<name>`, one level above `composer/`.
    let relative_root = package
        .get("install-path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("../{name}"));
    Some(VendorPackage {
        root: normalize_path(Path::new(&relative_root), composer_directory),
        autoload: AutoloadRules::from_json(package.get("autoload")),
        name,
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_project`
Expected: PASS (4 new tests)

- [ ] **Step 5: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: no warnings, all green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_project
git commit -m "✨ feat(project): read installed.json into vendor packages"
```

---

### Task 8: Notices and discovery

**Files:**
- Create: `crates/celerrate_project/src/notice.rs`
- Create: `crates/celerrate_project/src/discovery.rs`
- Modify: `crates/celerrate_project/src/lib.rs`
- Modify: `crates/celerrate_project/Cargo.toml` (add `celerrate_diagnostics`)

**Interfaces:**
- Consumes: everything from Tasks 3–7;
  `celerrate_diagnostics::{DiagnosticId, Severity}`;
  `celerrate_vfs::normalize_path`.
- Produces:
  `ProjectNotice` (enum: `MissingComposerManifest`, `InvalidComposerManifest`,
  `PhpVersionFallback`, `InvalidPhpVersionConstraint { constraint: String }`,
  `InvalidInstalledPackages`) with `fn identifier(&self) -> DiagnosticId` and
  `fn severity(&self) -> Severity` (always `Warning`);
  the identifier constants `MISSING_COMPOSER_MANIFEST` (CEL0018),
  `INVALID_COMPOSER_MANIFEST` (CEL0019), `PHP_VERSION_FALLBACK` (CEL0020),
  `INVALID_PHP_VERSION_CONSTRAINT` (CEL0021), `INVALID_INSTALLED_PACKAGES` (CEL0022);
  `FileOrigin` (enum: `Project`, `Vendor`);
  `ProjectDiscovery { pub root: PathBuf, pub vendor_root: PathBuf, pub php_version_range: PhpVersionRange, pub project_walk_roots: Vec<PathBuf>, pub vendor_walk_roots: Vec<PathBuf>, pub notices: Vec<ProjectNotice> }`
  with `fn classify(&self, path: &Path) -> FileOrigin` and
  `fn walk_roots(&self) -> Vec<PathBuf>` (project roots then vendor roots);
  `discover_from_sources(root: &Path, manifest_text: Option<&str>, installed_text: Option<&str>) -> ProjectDiscovery` (pure);
  `discover(root: &Path) -> ProjectDiscovery` (reads the two files; `root` must be
  absolute).

- [ ] **Step 1: Add the dependency**

In `crates/celerrate_project/Cargo.toml`, `[dependencies]` becomes:

```toml
[dependencies]
celerrate_diagnostics = { path = "../celerrate_diagnostics" }
celerrate_vfs = { path = "../celerrate_vfs" }
serde_json = { workspace = true }
```

- [ ] **Step 2: Write the failing notice tests**

Create `crates/celerrate_project/src/notice.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_diagnostics::Severity;

    use super::ProjectNotice;

    #[test]
    fn identifiers_are_stable() {
        let cases = [
            (ProjectNotice::MissingComposerManifest, "CEL0018"),
            (ProjectNotice::InvalidComposerManifest, "CEL0019"),
            (ProjectNotice::PhpVersionFallback, "CEL0020"),
            (
                ProjectNotice::InvalidPhpVersionConstraint {
                    constraint: String::from("banana"),
                },
                "CEL0021",
            ),
            (ProjectNotice::InvalidInstalledPackages, "CEL0022"),
        ];
        for (notice, identifier) in cases {
            assert_eq!(notice.identifier().as_str(), identifier);
            assert_eq!(notice.severity(), Severity::Warning);
        }
    }
}
```

- [ ] **Step 3: Run the notice tests to verify they fail**

Register the modules in `crates/celerrate_project/src/lib.rs`: the final file is

```rust
//! Composer project discovery.
//!
//! Zero-configuration detection: `composer.json` is located and read
//! tolerantly (a corrupted or missing file produces a notice and
//! defaults, never a failure), the autoload rules it and
//! `vendor/composer/installed.json` declare drive the disk walk and
//! classify every file as project or vendor, and the PHP version range
//! follows the parent spec's detection precedence. This crate is pure:
//! the configuration becomes a salsa input at the composition root.

mod autoload;
mod constraint;
mod discovery;
mod installed;
mod manifest;
mod notice;
mod version;

pub use autoload::{AutoloadRules, NamespaceMapping};
pub use constraint::{php_version_from_text, version_range_for_constraint};
pub use discovery::{FileOrigin, ProjectDiscovery, discover, discover_from_sources};
pub use installed::{VendorPackage, parse_installed_packages};
pub use manifest::{ComposerManifest, parse_manifest};
pub use notice::{
    INVALID_COMPOSER_MANIFEST, INVALID_INSTALLED_PACKAGES, INVALID_PHP_VERSION_CONSTRAINT,
    MISSING_COMPOSER_MANIFEST, PHP_VERSION_FALLBACK, ProjectNotice,
};
pub use version::{
    LATEST_STABLE_VERSION, OLDEST_SUPPORTED_VERSION, PhpVersion, PhpVersionRange,
    SUPPORTED_VERSIONS,
};
```

Create `crates/celerrate_project/src/discovery.rs` as an empty file for now so the
module tree compiles up to the missing types.

Run: `cargo test --package celerrate_project`
Expected: compile error — `ProjectNotice` and the discovery items do not exist yet.

- [ ] **Step 4: Implement the notices**

Prepend to `crates/celerrate_project/src/notice.rs`:

```rust
use celerrate_diagnostics::{DiagnosticId, Severity};

/// No `composer.json`: the root directory is analyzed with defaults.
pub const MISSING_COMPOSER_MANIFEST: DiagnosticId = DiagnosticId::new("CEL0018");
/// `composer.json` is not a JSON object: defaults are used.
pub const INVALID_COMPOSER_MANIFEST: DiagnosticId = DiagnosticId::new("CEL0019");
/// No PHP version configured: the latest supported stable is assumed.
pub const PHP_VERSION_FALLBACK: DiagnosticId = DiagnosticId::new("CEL0020");
/// A version constraint is unparseable or admits no supported version.
pub const INVALID_PHP_VERSION_CONSTRAINT: DiagnosticId = DiagnosticId::new("CEL0021");
/// `installed.json` is unreadable: vendor autoload is skipped.
pub const INVALID_INSTALLED_PACKAGES: DiagnosticId = DiagnosticId::new("CEL0022");

/// One discovery finding, structured. The kind stays with this
/// producing crate (the narrowing recorded in the semantic-core spec,
/// section 7); the preview renderer projects it into the shared
/// diagnostic model when part 7 consumes it. Zero-configuration never
/// blocks: every notice is a warning attached to a fallback already
/// taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectNotice {
    MissingComposerManifest,
    InvalidComposerManifest,
    PhpVersionFallback,
    InvalidPhpVersionConstraint { constraint: String },
    InvalidInstalledPackages,
}

impl ProjectNotice {
    pub fn identifier(&self) -> DiagnosticId {
        match self {
            Self::MissingComposerManifest => MISSING_COMPOSER_MANIFEST,
            Self::InvalidComposerManifest => INVALID_COMPOSER_MANIFEST,
            Self::PhpVersionFallback => PHP_VERSION_FALLBACK,
            Self::InvalidPhpVersionConstraint { .. } => INVALID_PHP_VERSION_CONSTRAINT,
            Self::InvalidInstalledPackages => INVALID_INSTALLED_PACKAGES,
        }
    }

    pub fn severity(&self) -> Severity {
        Severity::Warning
    }
}
```

- [ ] **Step 5: Write the failing discovery tests**

Fill `crates/celerrate_project/src/discovery.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::{FileOrigin, discover_from_sources};
    use crate::notice::ProjectNotice;
    use crate::version::{PhpVersion, PhpVersionRange};

    const ROOT: &str = "/project";

    #[test]
    fn without_a_manifest_the_root_is_analyzed_with_defaults_and_one_notice() {
        let discovery = discover_from_sources(Path::new(ROOT), None, None);
        assert_eq!(discovery.notices, vec![ProjectNotice::MissingComposerManifest]);
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
        assert_eq!(discovery.vendor_walk_roots, Vec::<PathBuf>::new());
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(discovery.vendor_root, PathBuf::from("/project/vendor"));
    }

    #[test]
    fn an_invalid_manifest_behaves_like_a_missing_one_with_its_own_notice() {
        let discovery = discover_from_sources(Path::new(ROOT), Some("not json"), None);
        assert_eq!(discovery.notices, vec![ProjectNotice::InvalidComposerManifest]);
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
    }

    #[test]
    fn the_platform_version_wins_over_the_require_constraint() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(
                r#"{
                    "require": { "php": "^8.1" },
                    "config": { "platform": { "php": "8.2.7" } }
                }"#,
            ),
            None,
        );
        assert_eq!(
            discovery.php_version_range,
            PhpVersionRange::point(PhpVersion::new(8, 2)),
        );
        assert_eq!(discovery.notices, Vec::<ProjectNotice>::new());
    }

    #[test]
    fn an_unsupported_platform_version_is_clamped() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "config": { "platform": { "php": "7.4.33" } } }"#),
            None,
        );
        assert_eq!(
            discovery.php_version_range,
            PhpVersionRange::point(PhpVersion::new(8, 1)),
        );
    }

    #[test]
    fn an_invalid_platform_falls_through_to_the_require_constraint() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(
                r#"{
                    "require": { "php": ">=8.2 <8.5" },
                    "config": { "platform": { "php": "eight" } }
                }"#,
            ),
            None,
        );
        assert_eq!(
            discovery.php_version_range,
            PhpVersionRange::new(PhpVersion::new(8, 2), PhpVersion::new(8, 4)),
        );
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidPhpVersionConstraint {
                constraint: String::from("eight"),
            }],
        );
    }

    #[test]
    fn no_version_signal_at_all_falls_back_with_one_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#),
            None,
        );
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(discovery.notices, vec![ProjectNotice::PhpVersionFallback]);
    }

    #[test]
    fn an_unsatisfiable_require_constraint_falls_back_with_one_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "require": { "php": "7.4.*" } }"#),
            None,
        );
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidPhpVersionConstraint {
                constraint: String::from("7.4.*"),
            }],
        );
    }

    #[test]
    fn declared_autoload_replaces_the_root_walk_and_vendor_joins_in() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(
                r#"{
                    "require": { "php": "^8.1" },
                    "autoload": { "psr-4": { "App\\": "src/" } },
                    "autoload-dev": { "psr-4": { "Tests\\": "tests/" } }
                }"#,
            ),
            Some(
                r#"{
                    "packages": [
                        {
                            "name": "acme/library",
                            "install-path": "../acme/library",
                            "autoload": { "psr-4": { "Acme\\": "src/" } }
                        }
                    ]
                }"#,
            ),
        );
        assert_eq!(
            discovery.project_walk_roots,
            vec![PathBuf::from("/project/src"), PathBuf::from("/project/tests")],
        );
        assert_eq!(
            discovery.vendor_walk_roots,
            vec![PathBuf::from("/project/vendor/acme/library/src")],
        );
        assert_eq!(
            discovery.walk_roots(),
            vec![
                PathBuf::from("/project/src"),
                PathBuf::from("/project/tests"),
                PathBuf::from("/project/vendor/acme/library/src"),
            ],
        );
    }

    #[test]
    fn a_manifest_without_autoload_still_walks_the_root() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "require": { "php": "^8.1" } }"#),
            None,
        );
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
    }

    #[test]
    fn the_vendor_directory_override_moves_the_boundary() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "require": { "php": "^8.1" }, "config": { "vendor-dir": "third-party" } }"#),
            None,
        );
        assert_eq!(discovery.vendor_root, PathBuf::from("/project/third-party"));
        assert_eq!(
            discovery.classify(Path::new("/project/third-party/acme/src/A.php")),
            FileOrigin::Vendor,
        );
        assert_eq!(
            discovery.classify(Path::new("/project/src/A.php")),
            FileOrigin::Project,
        );
    }

    #[test]
    fn invalid_installed_packages_skip_vendor_with_a_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "require": { "php": "^8.1" } }"#),
            Some("not json"),
        );
        assert_eq!(discovery.vendor_walk_roots, Vec::<PathBuf>::new());
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidInstalledPackages],
        );
    }
}
```

- [ ] **Step 6: Run the discovery tests to verify they fail**

Run: `cargo test --package celerrate_project`
Expected: compile error — the discovery items do not exist yet.

- [ ] **Step 7: Implement discovery**

Prepend to `crates/celerrate_project/src/discovery.rs`:

```rust
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use celerrate_vfs::normalize_path;

use crate::constraint::{php_version_from_text, version_range_for_constraint};
use crate::installed::parse_installed_packages;
use crate::manifest::{ComposerManifest, parse_manifest};
use crate::notice::ProjectNotice;
use crate::version::PhpVersionRange;

/// Whether a file belongs to the project or to an installed
/// dependency. Vendor is the high-durability tier: at the composition
/// root its inputs are invalidated wholesale only when the lock file
/// changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOrigin {
    Project,
    Vendor,
}

/// Everything zero-configuration discovery derives from a project
/// root: the version range, the walk roots, the vendor boundary, and
/// the notices explaining every fallback taken. Discovery never
/// fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiscovery {
    pub root: PathBuf,
    pub vendor_root: PathBuf,
    pub php_version_range: PhpVersionRange,
    pub project_walk_roots: Vec<PathBuf>,
    pub vendor_walk_roots: Vec<PathBuf>,
    pub notices: Vec<ProjectNotice>,
}

impl ProjectDiscovery {
    /// Project roots then vendor roots, each set sorted.
    pub fn walk_roots(&self) -> Vec<PathBuf> {
        self.project_walk_roots
            .iter()
            .chain(&self.vendor_walk_roots)
            .cloned()
            .collect()
    }

    pub fn classify(&self, path: &Path) -> FileOrigin {
        if path.starts_with(&self.vendor_root) {
            FileOrigin::Vendor
        } else {
            FileOrigin::Project
        }
    }
}

/// Discovers the project at an absolute `root`, reading
/// `composer.json` and `<vendor>/composer/installed.json` from disk.
/// This is push-time work for the composition root, never called
/// during a query.
pub fn discover(root: &Path) -> ProjectDiscovery {
    let manifest_text = fs::read_to_string(root.join("composer.json")).ok();
    // The manifest decides where the vendor directory lives, so it is
    // parsed once here just to locate `installed.json`; the pure core
    // re-derives everything from the texts.
    let manifest = manifest_text.as_deref().and_then(parse_manifest);
    let vendor = vendor_root(root, manifest.as_ref());
    let installed_text = fs::read_to_string(vendor.join("composer/installed.json")).ok();
    discover_from_sources(root, manifest_text.as_deref(), installed_text.as_deref())
}

/// The pure core of [`discover`]: derives the configuration from the
/// two file texts (`None` = the file does not exist). `root` must be
/// absolute.
pub fn discover_from_sources(
    root: &Path,
    manifest_text: Option<&str>,
    installed_text: Option<&str>,
) -> ProjectDiscovery {
    let mut notices = Vec::new();
    let manifest = match manifest_text {
        None => {
            notices.push(ProjectNotice::MissingComposerManifest);
            None
        }
        Some(text) => match parse_manifest(text) {
            None => {
                notices.push(ProjectNotice::InvalidComposerManifest);
                None
            }
            Some(manifest) => Some(manifest),
        },
    };
    let vendor_root = vendor_root(root, manifest.as_ref());
    // A missing or invalid manifest already carries its own notice;
    // the version fallback it implies is not separately reported.
    let php_version_range = match &manifest {
        None => PhpVersionRange::fallback(),
        Some(manifest) => resolve_version_range(manifest, &mut notices),
    };
    let project_walk_roots = match &manifest {
        Some(manifest) if !manifest.autoload.is_empty() => manifest.autoload.walk_roots(root),
        // Zero-configuration never blocks: no declared autoload means
        // the whole root is analyzed.
        _ => vec![normalize_path(root, root)],
    };
    let vendor_walk_roots = match installed_text {
        None => Vec::new(),
        Some(text) => match parse_installed_packages(text, &vendor_root.join("composer")) {
            None => {
                notices.push(ProjectNotice::InvalidInstalledPackages);
                Vec::new()
            }
            Some(packages) => {
                let mut roots = BTreeSet::new();
                for package in packages {
                    roots.extend(package.autoload.walk_roots(&package.root));
                }
                roots.into_iter().collect()
            }
        },
    };
    ProjectDiscovery {
        root: normalize_path(root, root),
        vendor_root,
        php_version_range,
        project_walk_roots,
        vendor_walk_roots,
        notices,
    }
}

fn vendor_root(root: &Path, manifest: Option<&ComposerManifest>) -> PathBuf {
    let declared = manifest
        .and_then(|manifest| manifest.vendor_directory.clone())
        .unwrap_or_else(|| String::from("vendor"));
    normalize_path(Path::new(&declared), root)
}

/// The parent spec's detection precedence, minus its `celerrate.toml`
/// first stage: `config.platform.php` (a point, clamped), then
/// `require.php` as a range, then the latest stable with a warning.
/// An unparseable stage reports itself and falls through; the plain
/// fallback notice fires only when no version signal existed at all.
fn resolve_version_range(
    manifest: &ComposerManifest,
    notices: &mut Vec<ProjectNotice>,
) -> PhpVersionRange {
    if let Some(platform) = &manifest.platform_php {
        if let Some(version) = php_version_from_text(platform) {
            return PhpVersionRange::point(version.clamped_to_supported());
        }
        notices.push(ProjectNotice::InvalidPhpVersionConstraint {
            constraint: platform.clone(),
        });
    }
    if let Some(require) = &manifest.require_php {
        if let Some(range) = version_range_for_constraint(require) {
            return range;
        }
        notices.push(ProjectNotice::InvalidPhpVersionConstraint {
            constraint: require.clone(),
        });
        return PhpVersionRange::fallback();
    }
    if manifest.platform_php.is_none() {
        notices.push(ProjectNotice::PhpVersionFallback);
    }
    PhpVersionRange::fallback()
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --package celerrate_project`
Expected: PASS (12 new tests: 1 notice + 11 discovery)

- [ ] **Step 9: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: no warnings, all green.

- [ ] **Step 10: Commit**

```bash
git add crates/celerrate_project
git commit -m "✨ feat(project): derive the project configuration with notices"
```

---

### Task 9: End-to-end determinism and the changelog

**Files:**
- Create: `crates/celerrate_project/tests/discovery_end_to_end.rs`
- Modify: `crates/celerrate_project/Cargo.toml` (add `[dev-dependencies]`)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `discover`, `ProjectDiscovery::walk_roots`, `classify` (Task 8);
  `enumerate_php_files`, `Vfs::load_from_disk`, `Vfs::contents` (Task 2);
  `celerrate_db::{SourceFile, file_diagnostics}` and
  `celerrate_db::testing::TestDatabase` (part 1).
- Produces: the part-level integration proof — the whole front door (discovery →
  walk → VFS → database) is deterministic: two from-scratch runs over the same disk
  state produce byte-identical results, undeclared files stay out, classification
  and diagnostics land where expected. (No new salsa queries exist in this part, so
  no new invalidation-scope assertions are due; the configuration joins the
  instrumented tests in part 3, when it becomes a salsa input with its first query
  consumer.)

- [ ] **Step 1: Add the dev-dependencies**

In `crates/celerrate_project/Cargo.toml`, insert between `[dependencies]` and
`[lints]`:

```toml
[dev-dependencies]
celerrate_db = { path = "../celerrate_db" }
tempfile = { workspace = true }
```

(`celerrate_db` sits below `celerrate_project` in the layering, so a dev-dependency
on it introduces no upward edge.)

- [ ] **Step 2: Write the failing test**

`crates/celerrate_project/tests/discovery_end_to_end.rs`:

```rust
//! The part-level proof: the whole front door — discovery, walk,
//! virtual file system, database — is a pure function of the disk
//! state.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use celerrate_db::testing::TestDatabase;
use celerrate_db::{SourceFile, file_diagnostics};
use celerrate_project::{FileOrigin, PhpVersion, PhpVersionRange, discover};
use celerrate_vfs::{Vfs, enumerate_php_files};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// One analyzed file, reduced to comparable facts: its root-relative
/// path, its origin, and its diagnostic count.
fn analyze(root: &Path) -> Vec<(String, FileOrigin, usize)> {
    let discovery = discover(root);
    assert_eq!(
        discovery.php_version_range,
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
    );
    assert!(discovery.notices.is_empty());
    let files = enumerate_php_files(&discovery.walk_roots());
    let mut vfs = Vfs::default();
    let db = TestDatabase::default();
    files
        .iter()
        .map(|path| {
            let file_id = vfs.load_from_disk(path).unwrap();
            let bytes = vfs.contents(file_id).unwrap().to_vec();
            let source = SourceFile::new(&db, file_id, bytes);
            (
                path.strip_prefix(root).unwrap().display().to_string(),
                discovery.classify(path),
                file_diagnostics(&db, source).len(),
            )
        })
        .collect()
}

#[test]
fn discovery_walk_and_analysis_are_deterministic_end_to_end() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(
        &root.join("composer.json"),
        r#"{
            "require": { "php": "^8.1" },
            "autoload": { "psr-4": { "App\\": "src/" } }
        }"#,
    );
    write(&root.join("src/App.php"), "<?php class App {}");
    write(&root.join("src/Broken.php"), "<?php class {");
    write(&root.join("stray.php"), "<?php this file is not declared");
    write(
        &root.join("vendor/composer/installed.json"),
        r#"{
            "packages": [
                {
                    "name": "acme/library",
                    "install-path": "../acme/library",
                    "autoload": { "psr-4": { "Acme\\": "src/" } }
                }
            ]
        }"#,
    );
    write(
        &root.join("vendor/acme/library/src/Library.php"),
        "<?php class Library {}",
    );

    let first = analyze(root);
    let second = analyze(root);
    assert_eq!(first, second, "two from-scratch runs diverged");

    let summary: Vec<(&str, FileOrigin, bool)> = first
        .iter()
        .map(|(name, origin, diagnostics)| (name.as_str(), *origin, *diagnostics > 0))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("src/App.php", FileOrigin::Project, false),
            ("src/Broken.php", FileOrigin::Project, true),
            (
                "vendor/acme/library/src/Library.php",
                FileOrigin::Vendor,
                false,
            ),
        ],
        "walked set, origins, or diagnostics changed: \
         the undeclared stray.php must stay out",
    );
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test --package celerrate_project --test discovery_end_to_end`
Expected: FAIL only if any earlier task left a gap — with Tasks 1–8 complete this
test should PASS immediately. If it fails, the failure is a real integration bug in
an earlier task: fix it there (with a unit test) rather than adapting this test.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --package celerrate_project --test discovery_end_to_end`
Expected: PASS

- [ ] **Step 5: Update the changelog**

In `CHANGELOG.md`, append to the `### Added` list under `## [Unreleased]`:

```markdown
- `celerrate_vfs`: lexical path normalization and the deterministic
  disk walk (PHP files under declared roots, explicit file entries,
  symbolic-link cycle protection, disk loading into the file state).
- `celerrate_project`: zero-configuration Composer discovery — tolerant
  `composer.json` and `installed.json` readers, autoload rules (PSR-4,
  PSR-0, classmap, files) deriving the walk roots and the
  project/vendor classification, the PHP version range with the
  parent-spec detection precedence at minor precision, and the
  structured discovery notices (`CEL0018` through `CEL0022`).
```

- [ ] **Step 6: Verify workspace health**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace && cargo deny check`
Expected: no warnings, all green.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_project Cargo.lock
git commit -m "✅ test(project): pin end-to-end discovery determinism"
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record project discovery"
```
