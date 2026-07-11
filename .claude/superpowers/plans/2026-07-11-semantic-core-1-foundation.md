# Semantic Core Part 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the shared diagnostic model (`celerrate_diagnostics`), the virtual file system (`celerrate_vfs`), and the salsa base layer (`celerrate_db`) with `source_text`, `parse`, `line_index`, and `file_diagnostics` as queries, plus the invalidation-instrumentation and incremental-consistency skeletons.

**Architecture:** Three new crates below and above the existing syntax layer, per the spec
`.claude/superpowers/specs/2026-07-11-semantic-core-design.md` (sections 2, 3, 7, 9)
and the parent layering. `celerrate_diagnostics` sits between `celerrate_source` and
`celerrate_syntax`; `celerrate_vfs` knows nothing about salsa (state is pumped into
inputs at the composition root, which does not exist yet); `celerrate_db` holds the
`SourceFile` input and the foundational queries. The concrete production database
arrives with the CLI (part 7); this part ships an instrumented `TestDatabase` in a
public `testing` module that also hosts the incremental harness.

**Tech Stack:** Rust edition 2024, salsa 0.27, rowan 0.16 (existing), insta (existing).

## Global Constraints

- Zero panic, mechanically enforced: workspace denies `unwrap_used`, `expect_used`,
  `indexing_slicing`, `panic`; `unsafe_code` is forbidden. Only test modules and
  integration-test files may `#[allow]` / `#![allow]` these lints.
- Non-test code returns `Result` or total functions; a poisoned mutex is recovered
  with `unwrap_or_else(std::sync::PoisonError::into_inner)`, never unwrapped.
- Strict layering, DAG with no upward edges:
  `celerrate_source` → `celerrate_diagnostics` → `celerrate_syntax` → `celerrate_vfs` → `celerrate_db`.
  `celerrate_db` does NOT depend on `celerrate_vfs` (state pumping happens at the
  composition root, a later part). `celerrate_vfs` depends only on `celerrate_source`.
- TDD: every task starts with a failing test.
- Everything in English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits, repository-configured identity, no
  Claude attribution anywhere.
- Every new crate's `Cargo.toml` uses the workspace inheritance pattern of
  `crates/celerrate_syntax/Cargo.toml` (version/edition/license/authors/repository
  `.workspace = true`, `[lints] workspace = true`).
- Verification commands for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`.

## Stable diagnostic identifiers allocated by this plan

These are permanent once published; do not renumber:

| Identifier | Owner | Meaning |
| --- | --- | --- |
| CEL0001 | `celerrate_db` | Source too large (decode failure) |
| CEL0002 | `celerrate_syntax` | Lexer: unexpected character |
| CEL0003 | `celerrate_syntax` | Lexer: unterminated block comment |
| CEL0004 | `celerrate_syntax` | Lexer: unterminated string |
| CEL0005 | `celerrate_syntax` | Lexer: unterminated heredoc |
| CEL0006 | `celerrate_syntax` | Lexer: unterminated interpolation |
| CEL0007 | `celerrate_syntax` | Parser: expected expression |
| CEL0008 | `celerrate_syntax` | Parser: expected semicolon |
| CEL0009 | `celerrate_syntax` | Parser: expected a specific token |
| CEL0010 | `celerrate_syntax` | Parser: unexpected token |
| CEL0011 | `celerrate_syntax` | Parser: nesting too deep |
| CEL0012 | `celerrate_syntax` | Parser: non-associative operator chained |
| CEL0013 | `celerrate_syntax` | Parser: no progress (internal, survivable) |
| CEL0014 | `celerrate_syntax` | Parser: expected member name |
| CEL0015 | `celerrate_syntax` | Parser: expected statement |
| CEL0016 | `celerrate_syntax` | Parser: expected type |
| CEL0017 | `celerrate_syntax` | Parser: expected declaration |

All syntax-family severities are `Severity::Error`.

---

### Task 1: The `celerrate_diagnostics` crate

**Files:**
- Create: `crates/celerrate_diagnostics/Cargo.toml`
- Create: `crates/celerrate_diagnostics/src/lib.rs`
- Create: `crates/celerrate_diagnostics/src/identifier.rs`
- Create: `crates/celerrate_diagnostics/src/severity.rs`
- Create: `crates/celerrate_diagnostics/src/diagnostic.rs`

**Interfaces:**
- Consumes: `celerrate_source::{FileId, TextRange}`.
- Produces: `DiagnosticId` (`const fn new(&'static str)`, `fn as_str(&self) -> &'static str`),
  `Severity` (`Warning`, `Error`; `Warning < Error`),
  `Diagnostic { pub id: DiagnosticId, pub severity: Severity, pub file: FileId, pub range: TextRange }`
  with a total `Ord` keyed on `(file, range.start(), range.end(), id, severity)`.

- [ ] **Step 1: Create the crate manifest**

`crates/celerrate_diagnostics/Cargo.toml`:

```toml
[package]
name = "celerrate_diagnostics"
description = "Shared diagnostic data model for the Celerrate toolchain"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_source = { path = "../celerrate_source" }

[lints]
workspace = true
```

- [ ] **Step 2: Write the failing tests**

`crates/celerrate_diagnostics/src/diagnostic.rs` (tests only for now; the module
does not compile until Step 4 adds the types, which is the failure we want):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, TextRange, TextSize};

    use crate::{Diagnostic, DiagnosticId, Severity};

    fn diagnostic(file: u32, start: u32, end: u32, id: &'static str) -> Diagnostic {
        Diagnostic {
            id: DiagnosticId::new(id),
            severity: Severity::Error,
            file: FileId::new(file),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
        }
    }

    #[test]
    fn identifier_round_trips() {
        assert_eq!(DiagnosticId::new("CEL0001").as_str(), "CEL0001");
    }

    #[test]
    fn severity_orders_warning_below_error() {
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn diagnostics_order_by_file_then_range_then_identifier() {
        let mut diagnostics = vec![
            diagnostic(1, 0, 1, "CEL0002"),
            diagnostic(0, 5, 9, "CEL0002"),
            diagnostic(0, 0, 4, "CEL0003"),
            diagnostic(0, 0, 4, "CEL0002"),
        ];
        diagnostics.sort();
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.file.as_u32(), diagnostic.id.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "CEL0002"), (0, "CEL0003"), (0, "CEL0002"), (1, "CEL0002")],
        );
    }

    #[test]
    fn equal_diagnostics_compare_equal() {
        assert_eq!(diagnostic(0, 0, 1, "CEL0002"), diagnostic(0, 0, 1, "CEL0002"));
    }
}
```

Create the module skeletons so the crate exists:

`crates/celerrate_diagnostics/src/lib.rs`:

```rust
//! The shared diagnostic data model.
//!
//! Every layer that reports, from the parser up, projects its structured
//! findings into this model: a stable identifier, a severity, and a
//! primary span. The rich anatomy (annotated spans, notes, structured
//! suggestions) arrives with the diagnostics-and-fixes sub-project;
//! rendering is always an upper layer's business.

mod diagnostic;
mod identifier;
mod severity;

pub use diagnostic::Diagnostic;
pub use identifier::DiagnosticId;
pub use severity::Severity;
```

`crates/celerrate_diagnostics/src/identifier.rs` and `severity.rs`: leave each with
only a placeholder comment `// Implemented in the next step.` so compilation fails.

- [ ] **Step 3: Run the tests to verify failure**

Run: `cargo test -p celerrate_diagnostics`
Expected: FAIL to compile with unresolved imports `Diagnostic`, `DiagnosticId`, `Severity`.

- [ ] **Step 4: Write the implementation**

`crates/celerrate_diagnostics/src/identifier.rs`:

```rust
/// A stable, documented diagnostic identifier, `CEL0001`-style.
///
/// Identifiers are permanent once published: users script against them
/// and suppress by them, so renumbering is a breaking change. Each
/// producing crate owns the identifiers of its own diagnostic kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiagnosticId(&'static str);

impl DiagnosticId {
    pub const fn new(identifier: &'static str) -> Self {
        Self(identifier)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}
```

`crates/celerrate_diagnostics/src/severity.rs`:

```rust
/// How serious a diagnostic is. Ordered: `Warning < Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Warning,
    Error,
}
```

`crates/celerrate_diagnostics/src/diagnostic.rs` (above the test module):

```rust
use celerrate_source::{FileId, TextRange};

use crate::identifier::DiagnosticId;
use crate::severity::Severity;

/// One reported finding: a stable identifier, a severity, and the
/// primary span it points at. The minimal shared shape every producer
/// projects into; ordering is total and deterministic so diagnostic
/// lists can be sorted and compared byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: DiagnosticId,
    pub severity: Severity,
    pub file: FileId,
    pub range: TextRange,
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.file,
            self.range.start(),
            self.range.end(),
            self.id,
            self.severity,
        )
            .cmp(&(
                other.file,
                other.range.start(),
                other.range.end(),
                other.id,
                other.severity,
            ))
    }
}

impl PartialOrd for Diagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
```

Note: `celerrate_source` must re-export `TextSize` (it already does, per
`crates/celerrate_source/src/lib.rs`).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_diagnostics`
Expected: PASS (4 tests).

- [ ] **Step 6: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_diagnostics
git commit -m "✨ feat(diagnostics): create the shared diagnostic data model"
```

---

### Task 2: Project syntax diagnostics into the shared model

**Files:**
- Modify: `crates/celerrate_syntax/Cargo.toml` (add the `celerrate_diagnostics` dependency)
- Modify: `crates/celerrate_syntax/src/diagnostic.rs` (identifier tables and projection)
- Modify: `crates/celerrate_syntax/src/parse.rs` (derive `PartialEq, Eq` on `Parse`)

**Interfaces:**
- Consumes: Task 1's `Diagnostic`, `DiagnosticId`, `Severity`; the existing
  `LexerDiagnosticKind`, `ParserDiagnosticKind`, `SyntaxDiagnostic` in
  `crates/celerrate_syntax/src/diagnostic.rs`.
- Produces: `LexerDiagnosticKind::diagnostic_id(self) -> DiagnosticId`,
  `ParserDiagnosticKind::diagnostic_id(self) -> DiagnosticId`,
  `SyntaxDiagnostic::diagnostic_id(&self) -> DiagnosticId`,
  `SyntaxDiagnostic::to_diagnostic(&self, file: FileId) -> Diagnostic`.
  `Parse` additionally derives `PartialEq, Eq` (salsa backdating needs it in Task 5).

- [ ] **Step 1: Add the dependency**

In `crates/celerrate_syntax/Cargo.toml`, add under `[dependencies]`:

```toml
celerrate_diagnostics = { path = "../celerrate_diagnostics" }
```

- [ ] **Step 2: Write the failing tests**

Append to the test module of `crates/celerrate_syntax/src/diagnostic.rs` (create the
`#[cfg(test)] mod tests` if the file has none):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_diagnostics::Severity;
    use celerrate_source::{FileId, TextRange, TextSize};

    use super::*;

    #[test]
    fn lexer_kinds_map_to_their_stable_identifiers() {
        let expected = [
            (LexerDiagnosticKind::UnexpectedCharacter, "CEL0002"),
            (LexerDiagnosticKind::UnterminatedBlockComment, "CEL0003"),
            (LexerDiagnosticKind::UnterminatedString, "CEL0004"),
            (LexerDiagnosticKind::UnterminatedHeredoc, "CEL0005"),
            (LexerDiagnosticKind::UnterminatedInterpolation, "CEL0006"),
        ];
        for (kind, identifier) in expected {
            assert_eq!(kind.diagnostic_id().as_str(), identifier);
        }
    }

    #[test]
    fn parser_kinds_map_to_their_stable_identifiers() {
        use crate::syntax_kind::SyntaxKind;
        let expected = [
            (ParserDiagnosticKind::ExpectedExpression, "CEL0007"),
            (ParserDiagnosticKind::ExpectedSemicolon, "CEL0008"),
            (ParserDiagnosticKind::Expected(SyntaxKind::Semicolon), "CEL0009"),
            (ParserDiagnosticKind::UnexpectedToken, "CEL0010"),
            (ParserDiagnosticKind::NestingTooDeep, "CEL0011"),
            (ParserDiagnosticKind::NonAssociativeOperator, "CEL0012"),
            (ParserDiagnosticKind::NoProgress, "CEL0013"),
            (ParserDiagnosticKind::ExpectedMemberName, "CEL0014"),
            (ParserDiagnosticKind::ExpectedStatement, "CEL0015"),
            (ParserDiagnosticKind::ExpectedType, "CEL0016"),
            (ParserDiagnosticKind::ExpectedDeclaration, "CEL0017"),
        ];
        for (kind, identifier) in expected {
            assert_eq!(kind.diagnostic_id().as_str(), identifier);
        }
    }

    #[test]
    fn projection_carries_identifier_severity_file_and_range() {
        let syntax_diagnostic = SyntaxDiagnostic {
            kind: SyntaxDiagnosticKind::Lexer(LexerDiagnosticKind::UnterminatedString),
            range: TextRange::new(TextSize::from(3), TextSize::from(8)),
        };
        let diagnostic = syntax_diagnostic.to_diagnostic(FileId::new(7));
        assert_eq!(diagnostic.id.as_str(), "CEL0004");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.file, FileId::new(7));
        assert_eq!(diagnostic.range, syntax_diagnostic.range);
    }

    #[test]
    fn parses_compare_equal_to_themselves() {
        let parse = crate::parse("<?php echo 1;");
        assert_eq!(parse.clone(), parse);
    }
}
```

Note for the implementer: if `crates/celerrate_syntax/src/diagnostic.rs` already has
a test module, merge these tests into it instead of adding a second module.
`SyntaxKind::Semicolon` is a real token kind (verified in
`crates/celerrate_syntax/src/syntax_kind/generated.rs`); any token kind works
for this test.

- [ ] **Step 3: Run the tests to verify failure**

Run: `cargo test -p celerrate_syntax diagnostic`
Expected: FAIL to compile with "no method named `diagnostic_id`".

- [ ] **Step 4: Write the implementation**

In `crates/celerrate_syntax/src/diagnostic.rs`, add at the top:

```rust
use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_source::FileId;
```

Then the identifier tables and projection (after the existing type definitions):

```rust
impl LexerDiagnosticKind {
    /// The stable identifier of this kind. Permanent once published.
    pub const fn diagnostic_id(self) -> DiagnosticId {
        match self {
            Self::UnexpectedCharacter => DiagnosticId::new("CEL0002"),
            Self::UnterminatedBlockComment => DiagnosticId::new("CEL0003"),
            Self::UnterminatedString => DiagnosticId::new("CEL0004"),
            Self::UnterminatedHeredoc => DiagnosticId::new("CEL0005"),
            Self::UnterminatedInterpolation => DiagnosticId::new("CEL0006"),
        }
    }
}

impl ParserDiagnosticKind {
    /// The stable identifier of this kind. The `Expected` family shares
    /// one identifier: the identifier names the problem class, not the
    /// missing token. Permanent once published.
    pub const fn diagnostic_id(self) -> DiagnosticId {
        match self {
            Self::ExpectedExpression => DiagnosticId::new("CEL0007"),
            Self::ExpectedSemicolon => DiagnosticId::new("CEL0008"),
            Self::Expected(_) => DiagnosticId::new("CEL0009"),
            Self::UnexpectedToken => DiagnosticId::new("CEL0010"),
            Self::NestingTooDeep => DiagnosticId::new("CEL0011"),
            Self::NonAssociativeOperator => DiagnosticId::new("CEL0012"),
            Self::NoProgress => DiagnosticId::new("CEL0013"),
            Self::ExpectedMemberName => DiagnosticId::new("CEL0014"),
            Self::ExpectedStatement => DiagnosticId::new("CEL0015"),
            Self::ExpectedType => DiagnosticId::new("CEL0016"),
            Self::ExpectedDeclaration => DiagnosticId::new("CEL0017"),
        }
    }
}

impl SyntaxDiagnostic {
    /// The stable identifier of this diagnostic's kind.
    pub const fn diagnostic_id(&self) -> DiagnosticId {
        match self.kind {
            SyntaxDiagnosticKind::Lexer(kind) => kind.diagnostic_id(),
            SyntaxDiagnosticKind::Parser(kind) => kind.diagnostic_id(),
        }
    }

    /// Projects this syntax diagnostic into the shared model. Every
    /// syntax finding is an error: the file does not parse as written.
    pub fn to_diagnostic(&self, file: FileId) -> Diagnostic {
        Diagnostic {
            id: self.diagnostic_id(),
            severity: Severity::Error,
            file,
            range: self.range,
        }
    }
}
```

In `crates/celerrate_syntax/src/parse.rs`, extend the `Parse` derive:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
```

(`rowan::GreenNode` equality is structural — header plus children, recursively,
short-circuiting at the first divergence — not pointer-based; salsa uses the
comparison for backdating in Task 5, where structural equality is the
semantically correct choice.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, including the whole existing suite.

- [ ] **Step 6: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): project syntax diagnostics into the shared model"
```

---

### Task 3: The `celerrate_vfs` crate

**Files:**
- Create: `crates/celerrate_vfs/Cargo.toml`
- Create: `crates/celerrate_vfs/src/lib.rs`
- Create: `crates/celerrate_vfs/src/vfs.rs`

**Interfaces:**
- Consumes: `celerrate_source::FileId`.
- Produces:
  `Vfs::default()`,
  `Vfs::file_id(&mut self, path: &Path) -> FileId` (interning),
  `Vfs::path(&self, file_id: FileId) -> Option<&Path>`,
  `Vfs::set_file_contents(&mut self, path: &Path, contents: Option<Vec<u8>>) -> FileId`,
  `Vfs::set_overlay(&mut self, path: &Path, contents: Vec<u8>) -> FileId`,
  `Vfs::clear_overlay(&mut self, file_id: FileId)`,
  `Vfs::contents(&self, file_id: FileId) -> Option<&[u8]>` (overlay wins over disk state),
  `Vfs::take_changes(&mut self) -> Vec<ChangedFile>` (sorted by `FileId`, deduplicated),
  `ChangedFile { pub file_id: FileId, pub contents: Option<Vec<u8>> }`.
- Scope note: no disk walking, no watcher, no path normalization in this part. The
  walk arrives with `celerrate_project` (part 2), the watcher with the CLI (part 7).
  The contract, documented on `Vfs`: callers pass absolute, already-normalized paths;
  normalization policy is owned by the discovery layer.

- [ ] **Step 1: Create the crate manifest**

`crates/celerrate_vfs/Cargo.toml`:

```toml
[package]
name = "celerrate_vfs"
description = "File loading and in-memory overlays for the Celerrate toolchain"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_source = { path = "../celerrate_source" }

[lints]
workspace = true
```

- [ ] **Step 2: Write the failing tests**

`crates/celerrate_vfs/src/vfs.rs`, test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use super::*;

    #[test]
    fn interning_is_stable() {
        let mut vfs = Vfs::default();
        let first = vfs.file_id(Path::new("/project/a.php"));
        let second = vfs.file_id(Path::new("/project/b.php"));
        assert_ne!(first, second);
        assert_eq!(vfs.file_id(Path::new("/project/a.php")), first);
        assert_eq!(vfs.path(first), Some(Path::new("/project/a.php")));
    }

    #[test]
    fn contents_round_trip_through_disk_state() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"<?php".to_vec()));
        assert_eq!(vfs.contents(file), Some(b"<?php".as_slice()));
    }

    #[test]
    fn overlays_shadow_disk_state_and_clear_back() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"disk".to_vec()));
        vfs.set_overlay(Path::new("/project/a.php"), b"overlay".to_vec());
        assert_eq!(vfs.contents(file), Some(b"overlay".as_slice()));
        vfs.clear_overlay(file);
        assert_eq!(vfs.contents(file), Some(b"disk".as_slice()));
    }

    #[test]
    fn changes_report_effective_contents_sorted_and_deduplicated() {
        let mut vfs = Vfs::default();
        // Interned first, so `file_b` receives the lower identifier.
        let file_b = vfs.set_file_contents(Path::new("/project/b.php"), Some(b"2".to_vec()));
        let file_a = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"1".to_vec()));
        vfs.set_file_contents(Path::new("/project/a.php"), Some(b"3".to_vec()));
        assert!(file_b < file_a);
        let changes = vfs.take_changes();
        assert_eq!(
            changes,
            vec![
                ChangedFile {
                    file_id: file_b,
                    contents: Some(b"2".to_vec()),
                },
                ChangedFile {
                    file_id: file_a,
                    contents: Some(b"3".to_vec()),
                },
            ],
        );
        assert!(vfs.take_changes().is_empty());
    }

    #[test]
    fn unchanged_effective_contents_do_not_report_a_change() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"same".to_vec()));
        vfs.take_changes();
        vfs.set_file_contents(Path::new("/project/a.php"), Some(b"same".to_vec()));
        vfs.set_overlay(Path::new("/project/a.php"), b"same".to_vec());
        assert!(vfs.take_changes().is_empty());
        let _ = file;
    }

    #[test]
    fn deleting_under_an_overlay_keeps_the_effective_contents() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"disk".to_vec()));
        vfs.set_overlay(Path::new("/project/a.php"), b"overlay".to_vec());
        vfs.take_changes();
        vfs.set_file_contents(Path::new("/project/a.php"), None);
        assert!(vfs.take_changes().is_empty());
        vfs.clear_overlay(file);
        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes.first().unwrap().contents, None);
    }
}
```

`crates/celerrate_vfs/src/lib.rs`:

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
//! Callers pass absolute, already-normalized paths: normalization
//! policy (separators, case, symlinks) is owned by the discovery layer
//! that walks the disk, not by the map that interns its results.

mod vfs;

pub use vfs::{ChangedFile, Vfs};
```

- [ ] **Step 3: Run the tests to verify failure**

Run: `cargo test -p celerrate_vfs`
Expected: FAIL to compile with unresolved `Vfs` / `ChangedFile`.

- [ ] **Step 4: Write the implementation**

`crates/celerrate_vfs/src/vfs.rs` (above the test module):

```rust
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use celerrate_source::FileId;

/// One file whose effective contents changed since the last
/// [`Vfs::take_changes`]. `contents` is the current effective state:
/// `None` means the file no longer exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub file_id: FileId,
    pub contents: Option<Vec<u8>>,
}

/// The in-memory file state: interned paths, disk contents, overlays.
///
/// The effective contents of a file are its overlay when one is set,
/// otherwise its disk state. A mutation is reported as a change only
/// when it alters the effective contents, so redundant writes never
/// reach the database.
#[derive(Debug, Default)]
pub struct Vfs {
    paths: Vec<PathBuf>,
    identifiers: HashMap<PathBuf, FileId>,
    disk: HashMap<FileId, Vec<u8>>,
    overlays: HashMap<FileId, Vec<u8>>,
    changed: BTreeSet<FileId>,
}

impl Vfs {
    /// Interns a path, assigning the next identifier on first sight.
    pub fn file_id(&mut self, path: &Path) -> FileId {
        if let Some(&existing) = self.identifiers.get(path) {
            return existing;
        }
        let assigned = FileId::new(self.paths.len() as u32);
        self.paths.push(path.to_path_buf());
        self.identifiers.insert(path.to_path_buf(), assigned);
        assigned
    }

    /// The path a file identifier was interned under.
    pub fn path(&self, file_id: FileId) -> Option<&Path> {
        self.paths
            .get(file_id.as_u32() as usize)
            .map(PathBuf::as_path)
    }

    /// Sets or deletes (`None`) the disk state of a file.
    pub fn set_file_contents(&mut self, path: &Path, contents: Option<Vec<u8>>) -> FileId {
        let file_id = self.file_id(path);
        let before = self.effective(file_id).map(<[u8]>::to_vec);
        match contents {
            Some(bytes) => {
                self.disk.insert(file_id, bytes);
            }
            None => {
                self.disk.remove(&file_id);
            }
        }
        self.record_if_changed(file_id, before);
        file_id
    }

    /// Sets an overlay shadowing the disk state of a file.
    pub fn set_overlay(&mut self, path: &Path, contents: Vec<u8>) -> FileId {
        let file_id = self.file_id(path);
        let before = self.effective(file_id).map(<[u8]>::to_vec);
        self.overlays.insert(file_id, contents);
        self.record_if_changed(file_id, before);
        file_id
    }

    /// Removes a file's overlay, revealing its disk state again.
    pub fn clear_overlay(&mut self, file_id: FileId) {
        let before = self.effective(file_id).map(<[u8]>::to_vec);
        self.overlays.remove(&file_id);
        self.record_if_changed(file_id, before);
    }

    /// The effective contents: the overlay when set, the disk state
    /// otherwise, `None` when the file does not exist.
    pub fn contents(&self, file_id: FileId) -> Option<&[u8]> {
        self.effective(file_id)
    }

    /// Drains the accumulated changes, sorted by file identifier, each
    /// carrying the file's current effective contents.
    pub fn take_changes(&mut self) -> Vec<ChangedFile> {
        let changed = core::mem::take(&mut self.changed);
        changed
            .into_iter()
            .map(|file_id| ChangedFile {
                file_id,
                contents: self.effective(file_id).map(<[u8]>::to_vec),
            })
            .collect()
    }

    fn effective(&self, file_id: FileId) -> Option<&[u8]> {
        self.overlays
            .get(&file_id)
            .or_else(|| self.disk.get(&file_id))
            .map(Vec::as_slice)
    }

    fn record_if_changed(&mut self, file_id: FileId, before: Option<Vec<u8>>) {
        if self.effective(file_id) != before.as_deref() {
            self.changed.insert(file_id);
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_vfs`
Expected: PASS (6 tests).

- [ ] **Step 6: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_vfs
git commit -m "✨ feat(vfs): create the virtual file system crate"
```

---

### Task 4: The `celerrate_db` skeleton with an instrumented test database

**Files:**
- Modify: `Cargo.toml` (workspace: add `salsa = "0.27"` to `[workspace.dependencies]`)
- Create: `crates/celerrate_db/Cargo.toml`
- Create: `crates/celerrate_db/src/lib.rs`
- Create: `crates/celerrate_db/src/input.rs`
- Create: `crates/celerrate_db/src/testing.rs`

**Interfaces:**
- Consumes: `celerrate_source::FileId`; salsa 0.27 (`salsa::Storage`, `#[salsa::db]`,
  `#[salsa::input]`, `salsa::Event`, `salsa::EventKind::WillExecute`).
- Produces:
  `SourceFile` salsa input with fields `file_id: FileId` and `#[returns(ref)] bytes: Vec<u8>`
  (generated API: `SourceFile::new(&db, file_id, bytes)`, `source_file.file_id(&db)`,
  `source_file.bytes(&db) -> &Vec<u8>`, `source_file.set_bytes(&mut db).to(bytes)`);
  `testing::TestDatabase` (`Default`, `Clone`) with
  `TestDatabase::take_executed(&self) -> Vec<String>` returning the Debug strings of
  every `WillExecute` event since the last call.
- Layering note: `celerrate_db` depends on `celerrate_source`, `celerrate_syntax`,
  `celerrate_diagnostics`, and salsa. It does NOT depend on `celerrate_vfs`.

- [ ] **Step 1: Add salsa to the workspace**

In the root `Cargo.toml`, `[workspace.dependencies]` gains:

```toml
salsa = "0.27"
```

- [ ] **Step 2: Create the crate manifest**

`crates/celerrate_db/Cargo.toml`:

```toml
[package]
name = "celerrate_db"
description = "Salsa inputs and foundational queries for the Celerrate toolchain"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_diagnostics = { path = "../celerrate_diagnostics" }
celerrate_source = { path = "../celerrate_source" }
celerrate_syntax = { path = "../celerrate_syntax" }
salsa = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 3: Write the failing test**

`crates/celerrate_db/src/input.rs`, test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::FileId;

    use crate::SourceFile;
    use crate::testing::TestDatabase;

    #[test]
    fn a_source_file_stores_its_identifier_and_bytes() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(3), b"<?php".to_vec());
        assert_eq!(file.file_id(&db), FileId::new(3));
        assert_eq!(file.bytes(&db), b"<?php");
    }

    #[test]
    fn setting_bytes_replaces_them() {
        let mut db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"before".to_vec());
        file.set_bytes(&mut db).to(b"after".to_vec());
        assert_eq!(file.bytes(&db), b"after");
    }
}
```

`crates/celerrate_db/src/lib.rs`:

```rust
//! Salsa inputs and foundational queries: the base-db layer.
//!
//! This crate defines the inputs (file contents keyed by [`FileId`])
//! and the queries every layer shares. Higher-level query definitions
//! live in their domain crates; the concrete production database is
//! assembled at the composition root (the CLI binary, a later part).
//!
//! [`FileId`]: celerrate_source::FileId

mod input;
pub mod testing;

pub use input::SourceFile;
```

- [ ] **Step 4: Run the test to verify failure**

Run: `cargo test -p celerrate_db`
Expected: FAIL to compile (no `SourceFile`, no `testing::TestDatabase`).

- [ ] **Step 5: Write the implementation**

`crates/celerrate_db/src/input.rs` (above the test module):

```rust
use celerrate_source::FileId;

/// One analyzed file: its identifier (assigned by the virtual file
/// system) and its raw bytes. Decoding is a derived query, so decode
/// provenance and failures stay incremental, not input state.
#[salsa::input]
pub struct SourceFile {
    pub file_id: FileId,
    #[returns(ref)]
    pub bytes: Vec<u8>,
}
```

`crates/celerrate_db/src/testing.rs`:

```rust
//! Test support: an instrumented database for this crate's tests, the
//! invalidation-scope tests, and the incremental harness. The concrete
//! production database is assembled at the composition root (the CLI
//! binary, a later part), not here.

use std::sync::{Arc, Mutex, PoisonError};

/// A salsa database that records every query execution.
///
/// Each `WillExecute` event is captured as its Debug rendering (for
/// example `parse(Id(400))`); invalidation-scope tests assert on those
/// strings to pin exactly which queries re-ran after an edit.
#[salsa::db]
#[derive(Clone)]
pub struct TestDatabase {
    storage: salsa::Storage<Self>,
    executed: Arc<Mutex<Vec<String>>>,
}

impl Default for TestDatabase {
    fn default() -> Self {
        let executed: Arc<Mutex<Vec<String>>> = Arc::default();
        let storage = salsa::Storage::new(Some(Box::new({
            let executed = executed.clone();
            move |event: salsa::Event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    let mut log = executed
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    log.push(format!("{database_key:?}"));
                }
            }
        })));
        Self { storage, executed }
    }
}

impl TestDatabase {
    /// Drains the executions recorded since the last call.
    pub fn take_executed(&self) -> Vec<String> {
        let mut log = self
            .executed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        core::mem::take(&mut *log)
    }
}

#[salsa::db]
impl salsa::Database for TestDatabase {}
```

Implementer note: this is the salsa 0.27 API surface (verified against the salsa
book and the `calc` example). If the pinned salsa release differs on a detail
(`Storage::new` signature, event field names), adapt mechanically; the contract to
preserve is: `TestDatabase` is `Default + Clone` and `take_executed` returns the
Debug strings of the `WillExecute` events since the last call.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p celerrate_db`
Expected: PASS (2 tests).

- [ ] **Step 7: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace && cargo deny check`
Expected: clean; `cargo deny check` accepts salsa's dependency tree (if a license or
advisory surfaces, stop and report rather than editing `deny.toml` silently).

```bash
git add Cargo.toml Cargo.lock crates/celerrate_db
git commit -m "✨ feat(db): create the salsa base layer with an instrumented test database"
```

---

### Task 5: The `source_text`, `parse`, and `line_index` queries

**Files:**
- Modify: `crates/celerrate_source/src/line_index.rs` (derive `PartialEq, Eq` on `LineIndex`)
- Create: `crates/celerrate_db/src/queries.rs`
- Modify: `crates/celerrate_db/src/lib.rs` (module and re-exports)

**Interfaces:**
- Consumes: Task 4's `SourceFile` and `TestDatabase`;
  `celerrate_source::{SourceText, SourceTooLarge, LineIndex}`;
  `celerrate_syntax::{parse as parse_source_text, Parse}` (the existing
  `celerrate_syntax::parse(source: &str) -> Parse`).
- Produces (all `#[salsa::tracked(returns(ref))]`, first parameter `db: &dyn salsa::Database`):
  `source_text(db, file: SourceFile) -> Result<SourceText, SourceTooLarge>`,
  `parse(db, file: SourceFile) -> Parse` (decode failure parses the empty string),
  `line_index(db, file: SourceFile) -> LineIndex` (decode failure indexes the empty string).
  Call sites receive references: `source_text(db, file)` is `&Result<SourceText, SourceTooLarge>`.

- [ ] **Step 1: Derive equality on `LineIndex`**

In `crates/celerrate_source/src/line_index.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
```

(Cheap, deterministic equality over the line-start vector: this is what lets salsa
backdate `line_index` results when an edit does not move any line boundary.)

- [ ] **Step 2: Write the failing tests**

`crates/celerrate_db/src/queries.rs`, test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, LineColumn, TextSize};

    use crate::testing::TestDatabase;
    use crate::{SourceFile, line_index, parse, source_text};

    #[test]
    fn source_text_decodes_the_bytes() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"\xEF\xBB\xBF<?php".to_vec());
        let decoded = source_text(&db, file).as_ref().unwrap();
        assert_eq!(decoded.text(), "<?php");
        assert!(decoded.had_utf8_bom());
    }

    #[test]
    fn parse_produces_a_lossless_tree_over_the_decoded_text() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        let parsed = parse(&db, file);
        assert_eq!(parsed.tree().text().to_string(), "<?php echo 1;");
    }

    #[test]
    fn line_index_maps_offsets_over_the_decoded_text() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php\necho 1;".to_vec());
        let index = line_index(&db, file);
        assert_eq!(
            index.line_column(TextSize::from(6)),
            LineColumn { line: 1, column: 0 },
        );
    }

    #[test]
    fn editing_bytes_reparses() {
        let mut db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        assert_eq!(parse(&db, file).tree().text().to_string(), "<?php echo 1;");
        file.set_bytes(&mut db).to(b"<?php echo 2;".to_vec());
        assert_eq!(parse(&db, file).tree().text().to_string(), "<?php echo 2;");
    }
}
```

- [ ] **Step 3: Run the tests to verify failure**

Run: `cargo test -p celerrate_db`
Expected: FAIL to compile (no `queries` module).

- [ ] **Step 4: Write the implementation**

`crates/celerrate_db/src/queries.rs` (above the test module):

```rust
use celerrate_source::{LineIndex, SourceText, SourceTooLarge};
use celerrate_syntax::Parse;

use crate::input::SourceFile;

/// Decodes a file's bytes into engine-ready text. The only failure is
/// an oversized input; everything else (byte-order mark, invalid
/// UTF-8) is provenance on the decoded text.
#[salsa::tracked(returns(ref))]
pub fn source_text(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> Result<SourceText, SourceTooLarge> {
    SourceText::from_bytes(file.bytes(db))
}

/// Parses a file's decoded text into the lossless syntax tree. A file
/// that fails to decode parses as empty: the decode failure itself is
/// reported by `file_diagnostics`, and every consumer still receives a
/// well-formed tree.
#[salsa::tracked(returns(ref))]
pub fn parse(db: &dyn salsa::Database, file: SourceFile) -> Parse {
    match source_text(db, file) {
        Ok(text) => celerrate_syntax::parse(text.text()),
        Err(_) => celerrate_syntax::parse(""),
    }
}

/// The line/column index of a file's decoded text. A file that fails
/// to decode indexes as empty, mirroring `parse`.
#[salsa::tracked(returns(ref))]
pub fn line_index(db: &dyn salsa::Database, file: SourceFile) -> LineIndex {
    match source_text(db, file) {
        Ok(text) => LineIndex::new(text.text()),
        Err(_) => LineIndex::new(""),
    }
}
```

`crates/celerrate_db/src/lib.rs` gains:

```rust
mod queries;

pub use queries::{line_index, parse, source_text};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_db`
Expected: PASS (6 tests).

- [ ] **Step 6: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_source crates/celerrate_db
git commit -m "✨ feat(db): derive source text, parse, and line index as queries"
```

---

### Task 6: The `file_diagnostics` query and CEL0001

**Files:**
- Modify: `crates/celerrate_db/src/queries.rs`
- Modify: `crates/celerrate_db/src/lib.rs` (re-exports)

**Interfaces:**
- Consumes: Task 5's queries; Task 2's `SyntaxDiagnostic::to_diagnostic`;
  `celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity}`.
- Produces:
  `pub const SOURCE_TOO_LARGE: DiagnosticId` (value `CEL0001`, owned by this crate);
  `file_diagnostics(db, file: SourceFile) -> Vec<Diagnostic>`
  (`#[salsa::tracked(returns(ref))]`; deterministic order: the `Parse` diagnostics
  are already sorted by range, and all entries share one file).

- [ ] **Step 1: Write the failing tests**

Append to the test module of `crates/celerrate_db/src/queries.rs`:

```rust
    use crate::{SOURCE_TOO_LARGE, file_diagnostics};

    #[test]
    fn clean_files_have_no_diagnostics() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        assert!(file_diagnostics(&db, file).is_empty());
    }

    #[test]
    fn syntax_diagnostics_project_with_the_file_identifier() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(9), b"<?php echo ;".to_vec());
        let diagnostics = file_diagnostics(&db, file);
        assert!(!diagnostics.is_empty());
        for diagnostic in diagnostics {
            assert_eq!(diagnostic.file, FileId::new(9));
            assert_eq!(diagnostic.severity, celerrate_diagnostics::Severity::Error);
            assert!(diagnostic.id.as_str().starts_with("CEL"));
        }
    }

    #[test]
    fn source_too_large_is_stable() {
        assert_eq!(SOURCE_TOO_LARGE.as_str(), "CEL0001");
    }
```

Note: the `<?php echo ;` fixture must produce at least one parser diagnostic
(`ExpectedExpression`). If it does not, pick any fixture from the existing
`celerrate_syntax` error corpus that produces diagnostics.

The decode-failure path (`Err(SourceTooLarge)` becomes one `CEL0001` diagnostic
with an empty range at offset zero) cannot be exercised without allocating more
than 4 GiB, so it is covered by construction, not by test, mirroring how
`celerrate_source` tests its own cap through `text_size_of`.

- [ ] **Step 2: Run the tests to verify failure**

Run: `cargo test -p celerrate_db`
Expected: FAIL to compile (no `file_diagnostics`, no `SOURCE_TOO_LARGE`).

- [ ] **Step 3: Write the implementation**

In `crates/celerrate_db/src/queries.rs`, extend the imports and add:

```rust
use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_source::{TextRange, TextSize};

/// The file's decoded bytes would exceed the 4 GiB engine cap.
pub const SOURCE_TOO_LARGE: DiagnosticId = DiagnosticId::new("CEL0001");

/// Every diagnostic of one file, in deterministic source order: the
/// decode failure when the file could not be decoded, the projected
/// syntax diagnostics otherwise. Semantic families join in later parts.
#[salsa::tracked(returns(ref))]
pub fn file_diagnostics(db: &dyn salsa::Database, file: SourceFile) -> Vec<Diagnostic> {
    let file_id = file.file_id(db);
    match source_text(db, file) {
        Err(_) => vec![Diagnostic {
            id: SOURCE_TOO_LARGE,
            severity: Severity::Error,
            file: file_id,
            range: TextRange::empty(TextSize::from(0)),
        }],
        Ok(_) => parse(db, file)
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.to_diagnostic(file_id))
            .collect(),
    }
}
```

`crates/celerrate_db/src/lib.rs` re-exports gain:

```rust
pub use queries::{SOURCE_TOO_LARGE, file_diagnostics, line_index, parse, source_text};
```

(replacing the previous `queries` re-export line).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_db`
Expected: PASS (9 tests).

- [ ] **Step 5: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_db
git commit -m "✨ feat(db): assemble per-file diagnostics with stable identifiers"
```

---

### Task 7: Invalidation-scope tests

**Files:**
- Create: `crates/celerrate_db/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: everything public from `celerrate_db` (Tasks 4 through 6), especially
  `testing::TestDatabase::take_executed`.
- Produces: the first two entries of the invalidation-scope suite the spec
  (section 9) requires. Later parts extend this file with their own edit classes.

- [ ] **Step 1: Write the tests (they must pass immediately: they pin behavior the
  previous tasks built; a failure here is a bug in Tasks 4 through 6)**

`crates/celerrate_db/tests/invalidation_scope.rs`:

```rust
//! Invalidation-scope tests: after each canonical edit class, assert
//! exactly which queries re-executed. The incremental-consistency
//! harness verifies the result; these tests verify how little work
//! produced it, which is what the published incremental targets
//! depend on (parent spec, section 3).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{SourceFile, file_diagnostics, parse};
use celerrate_source::FileId;

fn executions_of(log: &[String], query: &str) -> usize {
    let prefix = format!("{query}(");
    log.iter()
        .filter(|entry| entry.contains(prefix.as_str()))
        .count()
}

#[test]
fn editing_one_file_reanalyzes_only_that_file() {
    let mut db = TestDatabase::default();
    let edited = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
    let untouched = SourceFile::new(&db, FileId::new(1), b"<?php echo 2;".to_vec());
    let _ = file_diagnostics(&db, edited);
    let _ = file_diagnostics(&db, untouched);
    db.take_executed();

    edited.set_bytes(&mut db).to(b"<?php echo 3;".to_vec());
    let _ = file_diagnostics(&db, edited);
    let _ = file_diagnostics(&db, untouched);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "parse"),
        1,
        "only the edited file reparses: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "file_diagnostics"),
        1,
        "only the edited file recomputes diagnostics: {log:?}",
    );
}

#[test]
fn an_equal_decode_backdates_and_skips_the_reparse() {
    // `\xFF` and `\xFE` are both single invalid bytes: each decodes to
    // one U+FFFD at the same range, so the decoded `SourceText` is
    // identical. Salsa backdates the equal `source_text` result and
    // `parse` never re-executes: early cutoff, observed directly.
    let mut db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), b"<?php echo \xFF;".to_vec());
    let _ = parse(&db, file);
    db.take_executed();

    file.set_bytes(&mut db).to(b"<?php echo \xFE;".to_vec());
    let _ = parse(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "source_text"),
        1,
        "the decode re-runs on new bytes: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "parse"),
        0,
        "an identical decode must backdate, sparing the reparse: {log:?}",
    );
}
```

Implementer note: the assertions parse the Debug rendering of salsa's
`WillExecute` keys (for example `parse(Id(400))`). If the pinned salsa release
renders keys differently, adjust `executions_of` accordingly; the behavioral
contract asserted by these tests must not be weakened.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p celerrate_db --test invalidation_scope`
Expected: PASS (2 tests). If `an_equal_decode_backdates_and_skips_the_reparse`
fails with one `parse` execution, backdating did not fire: check that
`SourceText` equality holds across the two fixtures and that the tracked
functions do not accidentally capture the raw bytes.

- [ ] **Step 3: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_db/tests/invalidation_scope.rs
git commit -m "✅ test(db): pin the invalidation scope of file edits"
```

---

### Task 8: The incremental-consistency harness skeleton

**Files:**
- Modify: `crates/celerrate_db/src/testing.rs` (add the harness function)
- Create: `crates/celerrate_db/tests/incremental_consistency.rs`

**Interfaces:**
- Consumes: Tasks 4 through 6.
- Produces:
  `testing::assert_incremental_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])])`:
  builds one incremental database over the initial files, applies the edits one by
  one, and after each edit asserts that every file's tree text and diagnostics are
  byte-for-byte identical to a from-scratch database built on the current state.
  Later parts extend this skeleton (more compared queries, corpus replay, thread
  variation); its signature and its "compare against fresh after every edit"
  contract are the fixed points.

- [ ] **Step 1: Write the failing test**

`crates/celerrate_db/tests/incremental_consistency.rs`:

```rust
//! The incremental correctness harness, skeleton form: after any edit
//! sequence, the incremental result must be byte-for-byte identical to
//! a from-scratch analysis (parent spec, section 9, tier 3).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use celerrate_db::testing::assert_incremental_consistency;

#[test]
fn edit_sequences_match_from_scratch_analysis() {
    assert_incremental_consistency(
        &[b"<?php echo 1;", b"<?php function f() { return 2; }"],
        &[
            (0, b"<?php echo 10;"),
            (1, b"<?php function f() { return"),
            (0, b"<?php echo ;"),
            (1, b"<?php function f() { return 2; }"),
        ],
    );
}

#[test]
fn degenerate_bytes_stay_consistent() {
    assert_incremental_consistency(
        &[b"\xFF\xFE<?php echo \xFF;"],
        &[(0, b"<?php"), (0, b"\xEF\xBB\xBF<?php echo 1;")],
    );
}
```

- [ ] **Step 2: Run the test to verify failure**

Run: `cargo test -p celerrate_db --test incremental_consistency`
Expected: FAIL to compile (no `assert_incremental_consistency`).

- [ ] **Step 3: Write the implementation**

Append to `crates/celerrate_db/src/testing.rs`:

```rust
use celerrate_source::FileId;

use crate::{SourceFile, file_diagnostics, parse};

/// Replays an edit sequence against one incremental database and, after
/// every edit, asserts each file's analysis is byte-for-byte identical
/// to a from-scratch database built on the current state.
///
/// `initial` provides the starting bytes of file 0, 1, 2, ...; each
/// edit is `(file index, new bytes)`. Panics (test-style assertions)
/// on any divergence or out-of-range file index.
pub fn assert_incremental_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])]) {
    let mut incremental = TestDatabase::default();
    let mut current: Vec<Vec<u8>> = initial.iter().map(|bytes| bytes.to_vec()).collect();
    let files: Vec<SourceFile> = current
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            SourceFile::new(&incremental, FileId::new(index as u32), bytes.clone())
        })
        .collect();

    assert_matches_from_scratch(&incremental, &files, &current);
    for &(file_index, new_bytes) in edits {
        assert!(
            file_index < files.len(),
            "edit targets unknown file index {file_index}",
        );
        let (Some(slot), Some(file)) = (current.get_mut(file_index), files.get(file_index))
        else {
            // Unreachable: guarded by the assertion above.
            return;
        };
        *slot = new_bytes.to_vec();
        file.set_bytes(&mut incremental).to(new_bytes.to_vec());
        assert_matches_from_scratch(&incremental, &files, &current);
    }
}

fn assert_matches_from_scratch(
    incremental: &TestDatabase,
    files: &[SourceFile],
    current: &[Vec<u8>],
) {
    let from_scratch = TestDatabase::default();
    for (index, (file, bytes)) in files.iter().zip(current).enumerate() {
        let fresh_file =
            SourceFile::new(&from_scratch, FileId::new(index as u32), bytes.clone());
        assert_eq!(
            parse(incremental, *file).tree().text().to_string(),
            parse(&from_scratch, fresh_file).tree().text().to_string(),
            "tree text diverged for file {index}",
        );
        assert_eq!(
            file_diagnostics(incremental, *file),
            file_diagnostics(&from_scratch, fresh_file),
            "diagnostics diverged for file {index}",
        );
    }
}
```

Lint note: `assert!` and `assert_eq!` are permitted by the workspace lint set
(only `panic!`, `unwrap`, `expect`, and indexing are denied); the harness is
test infrastructure whose job is to fail loudly.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_db --test incremental_consistency`
Expected: PASS (2 tests).

- [ ] **Step 5: Verify workspace health and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_db
git commit -m "✅ test(db): add the incremental consistency harness skeleton"
```

---

### Task 9: Closure — changelog and full verification

**Files:**
- Modify: `CHANGELOG.md` (Unreleased / Added)

**Interfaces:**
- Consumes: everything above.
- Produces: the recorded changelog entry and a fully verified workspace.

- [ ] **Step 1: Update the changelog**

In `CHANGELOG.md`, append to the `### Added` list under `## [Unreleased]`:

```markdown
- `celerrate_diagnostics`: the shared diagnostic data model (stable
  `CEL####` identifiers, severity, primary span); lexer and parser
  diagnostics project into it.
- `celerrate_vfs`: the virtual file system (interned file identifiers,
  disk state, in-memory overlays, change draining).
- `celerrate_db`: the salsa base layer (`SourceFile` input;
  `source_text`, `parse`, `line_index`, and `file_diagnostics` as
  incremental queries), with the invalidation-scope tests and the
  incremental-consistency harness skeleton. This opens the semantic
  core sub-project.
```

- [ ] **Step 2: Full verification**

Run, in order, expecting every one clean:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

If `cargo fmt --all -- --check` reports differences, run `cargo fmt --all` and
include the changes in the commit.

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the semantic core foundation"
```
