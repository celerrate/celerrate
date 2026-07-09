# Foundations Part 2: Source-File Representation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete `celerrate_source` with the two primitives the engine references files through: an opaque `FileId` and a `SourceText` that decodes raw bytes into engine-ready UTF-8 with provenance (BOM, replacement ranges) and one data-carrying failure (oversized input).

**Architecture:** Pure representation layer, no disk I/O (spec section 1: file contents are future salsa inputs fed from outside). `FileId` is a meaningless 4-byte key assigned by upper layers. `SourceText::from_bytes` strips and records a UTF-8 BOM, replaces invalid UTF-8 with U+FFFD while recording each replacement's range in the normalized text, and rejects texts beyond the 4 GiB `TextSize` cap with a typed error. `LineIndex` stays separate and unchanged.

**Tech Stack:** Rust (edition 2024), `text-size` crate, standard library only.

**Spec:** `.claude/superpowers/specs/2026-07-10-foundations-2-source-files-design.md` (read it before starting).

## Global Constraints

- Zero-panic policy: workspace lints already deny `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` is forbidden. Never index or slice with `[]`; use `get`/`strip_prefix`/iterators. Test files may carry a file-level `#![allow(clippy::expect_used)]` with a reason comment.
- All files in English, full words for names (standard acronyms fine; `u32` in `as_u32` is a type name, acceptable).
- Commits: gitmoji + Conventional Commits, scope `source`. No AI attribution lines. Verify `git config user.email` prints `5817251+jh3ady@users.noreply.github.com` before the first commit.
- TDD strictly: failing test → run to see it fail → minimal implementation → run to see it pass → commit.
- Work happens on branch `foundations-2-source-files` cut from `main` (the executor's worktree skill handles creation; if working inline, `git switch -c foundations-2-source-files` first).
- No disk I/O, no path types, no new dependencies anywhere in this plan.
- Verification commands (run from the repository root): `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

## File Structure

- `crates/celerrate_source/src/file_id.rs` — new: the `FileId` handle.
- `crates/celerrate_source/src/source_text.rs` — new: `SourceText`, `SourceTooLarge`, decoding.
- `crates/celerrate_source/src/lib.rs` — modified: module declarations, re-exports, crate docs refresh.
- `crates/celerrate_source/tests/file_id.rs` — new: `FileId` behavior tests.
- `crates/celerrate_source/tests/source_text.rs` — new: decoding behavior tests.
- `CHANGELOG.md` — modified: Unreleased entry.

---

### Task 1: `FileId` opaque handle

**Files:**
- Create: `crates/celerrate_source/tests/file_id.rs`
- Create: `crates/celerrate_source/src/file_id.rs`
- Modify: `crates/celerrate_source/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub struct FileId` with `pub const fn new(raw: u32) -> FileId` and `pub const fn as_u32(self) -> u32`, deriving `Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord`. Re-exported from the crate root.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_source/tests/file_id.rs`:

```rust
use std::collections::HashSet;

use celerrate_source::FileId;

#[test]
fn round_trips_its_raw_value() {
    assert_eq!(FileId::new(42).as_u32(), 42);
}

#[test]
fn ordering_follows_the_raw_value() {
    assert!(FileId::new(1) < FileId::new(2));
    assert_eq!(FileId::new(7), FileId::new(7));
}

#[test]
fn works_as_a_hash_map_key() {
    let mut set = HashSet::new();
    set.insert(FileId::new(1));
    set.insert(FileId::new(1));
    set.insert(FileId::new(2));
    assert_eq!(set.len(), 2);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_source --test file_id`
Expected: compilation error, `FileId` not found in `celerrate_source`.

- [ ] **Step 3: Write the implementation**

Create `crates/celerrate_source/src/file_id.rs`:

```rust
/// An opaque, compact handle identifying one source file.
///
/// The crate attaches no meaning to the value: identifiers are assigned by
/// the layer that discovers files (the query database, in a later
/// sub-project) and serve as cheap `Copy` keys everywhere below it. The
/// mapping between identifiers and paths lives with the assigner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

impl FileId {
    /// Wraps a raw identifier assigned by the caller.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw identifier, for the layer that assigned it.
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}
```

Modify `crates/celerrate_source/src/lib.rs`: keep the existing `//!` crate doc comment untouched, and update the items below it so modules and re-exports stay alphabetical:

```rust
pub use text_size::{TextRange, TextSize};

mod file_id;
mod line_index;

pub use file_id::FileId;
pub use line_index::{LineColumn, LineIndex};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_source --test file_id`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_source/tests/file_id.rs crates/celerrate_source/src/file_id.rs crates/celerrate_source/src/lib.rs
git commit -m "✨ feat(source): add opaque FileId handle"
```

---

### Task 2: `SourceText` — valid UTF-8, BOM handling, size cap

**Files:**
- Create: `crates/celerrate_source/tests/source_text.rs`
- Create: `crates/celerrate_source/src/source_text.rs`
- Modify: `crates/celerrate_source/src/lib.rs`

**Interfaces:**
- Consumes: `TextRange`, `TextSize` from the crate root.
- Produces: `pub struct SourceText` (derives `Debug, Clone, PartialEq, Eq`) with:
  - `pub fn from_bytes(bytes: &[u8]) -> Result<SourceText, SourceTooLarge>`
  - `pub fn text(&self) -> &str`
  - `pub fn had_utf8_bom(&self) -> bool`
  - `pub fn replacements(&self) -> &[TextRange]`
  - `pub fn is_pristine(&self) -> bool`
  - `pub struct SourceTooLarge { pub decoded_length: usize }` (derives `Debug, Clone, Copy, PartialEq, Eq`, implements `Display` and `std::error::Error`)

  Task 3 relies on the private helper `fn text_size_of(length: usize) -> Result<TextSize, SourceTooLarge>` defined here.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_source/tests/source_text.rs`:

```rust
//! Behavior tests for `SourceText` decoding. `expect` is allowed here:
//! failing loudly is exactly what a test should do.
#![allow(clippy::expect_used)]

use celerrate_source::SourceText;

#[test]
fn plain_ascii_decodes_unchanged() {
    let source = SourceText::from_bytes(b"<?php echo 1;").expect("fits the cap");
    assert_eq!(source.text(), "<?php echo 1;");
    assert!(!source.had_utf8_bom());
    assert!(source.replacements().is_empty());
    assert!(source.is_pristine());
}

#[test]
fn multibyte_utf8_decodes_unchanged() {
    let source = SourceText::from_bytes("héllo 🐘".as_bytes()).expect("fits the cap");
    assert_eq!(source.text(), "héllo 🐘");
    assert!(source.is_pristine());
}

#[test]
fn empty_input_decodes_to_empty_pristine_text() {
    let source = SourceText::from_bytes(b"").expect("fits the cap");
    assert_eq!(source.text(), "");
    assert!(source.is_pristine());
}

#[test]
fn utf8_bom_is_stripped_and_recorded() {
    let source = SourceText::from_bytes(b"\xEF\xBB\xBF<?php").expect("fits the cap");
    assert_eq!(source.text(), "<?php");
    assert!(source.had_utf8_bom());
    assert!(source.replacements().is_empty());
    assert!(!source.is_pristine());
}

#[test]
fn utf8_bom_alone_decodes_to_empty_text() {
    let source = SourceText::from_bytes(b"\xEF\xBB\xBF").expect("fits the cap");
    assert_eq!(source.text(), "");
    assert!(source.had_utf8_bom());
    assert!(!source.is_pristine());
}

#[test]
fn bom_bytes_after_the_start_are_kept_as_text() {
    let source = SourceText::from_bytes(b"a\xEF\xBB\xBFb").expect("fits the cap");
    // U+FEFF in the middle of the text is a zero-width no-break space,
    // not a byte-order mark: it stays in the text.
    assert_eq!(source.text(), "a\u{FEFF}b");
    assert!(!source.had_utf8_bom());
}

#[test]
fn line_endings_and_nul_bytes_pass_through() {
    let source = SourceText::from_bytes(b"a\r\nb\0c").expect("fits the cap");
    assert_eq!(source.text(), "a\r\nb\0c");
    assert!(source.is_pristine());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_source --test source_text`
Expected: compilation error, `SourceText` not found.

- [ ] **Step 3: Write the implementation**

Create `crates/celerrate_source/src/source_text.rs`:

```rust
use text_size::{TextRange, TextSize};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// The decoded text would exceed the 4 GiB cap of [`TextSize`].
///
/// This is the only way decoding fails; the caller renders it as a
/// diagnostic. Everything else — invalid bytes, a byte-order mark — is
/// provenance data on the decoded [`SourceText`], not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceTooLarge {
    /// Byte length the decoded text would have reached.
    pub decoded_length: usize,
}

impl core::fmt::Display for SourceTooLarge {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "decoded source text would be {} bytes, beyond the 4 GiB maximum",
            self.decoded_length
        )
    }
}

impl std::error::Error for SourceTooLarge {}

/// Source bytes decoded into engine-ready UTF-8 text, with provenance.
///
/// Decoding strips a leading UTF-8 byte-order mark (recorded in
/// [`had_utf8_bom`](Self::had_utf8_bom)) and replaces invalid UTF-8
/// sequences with U+FFFD (each replacement's range in the decoded text is
/// recorded in [`replacements`](Self::replacements)). No other
/// normalization happens: line endings, tabs, and NUL bytes pass through
/// untouched, and the lossless syntax-tree guarantee is measured against
/// this decoded text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceText {
    text: String,
    had_utf8_bom: bool,
    replacements: Vec<TextRange>,
}

impl SourceText {
    /// Decodes raw file bytes. The only failure is [`SourceTooLarge`];
    /// every byte sequence otherwise decodes to a usable text.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceTooLarge> {
        let (had_utf8_bom, content) = match bytes.strip_prefix(UTF8_BOM) {
            Some(rest) => (true, rest),
            None => (false, bytes),
        };
        // The decoded text is never shorter than the input (valid bytes
        // copy one to one; invalid sequences of at most three bytes become
        // a three-byte U+FFFD), so oversized inputs fail before decoding.
        text_size_of(content.len())?;
        let (text, replacements) = decode_lossy(content)?;
        text_size_of(text.len())?;
        Ok(Self {
            text,
            had_utf8_bom,
            replacements,
        })
    }

    /// The decoded UTF-8 text, byte-order mark stripped.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether a leading UTF-8 byte-order mark was stripped. Writers can
    /// re-emit it; a byte-order mark before `<?php` is also a PHP hazard
    /// (bytes before the opening tag are sent to output) worth a future
    /// lint.
    pub fn had_utf8_bom(&self) -> bool {
        self.had_utf8_bom
    }

    /// Ranges in [`text`](Self::text) where invalid bytes were replaced
    /// with U+FFFD. Distinguishes real corruption from a literal U+FFFD
    /// present in the file.
    pub fn replacements(&self) -> &[TextRange] {
        &self.replacements
    }

    /// True when the decoded text is byte-for-byte the input: no
    /// byte-order mark, no replacements. Upper layers must consult this
    /// before writing autofixes back to disk.
    pub fn is_pristine(&self) -> bool {
        !self.had_utf8_bom && self.replacements.is_empty()
    }
}

/// Converts a byte length within the decoded text into a [`TextSize`],
/// rejecting lengths beyond the 4 GiB cap.
fn text_size_of(length: usize) -> Result<TextSize, SourceTooLarge> {
    u32::try_from(length)
        .map(TextSize::from)
        .map_err(|_| SourceTooLarge {
            decoded_length: length,
        })
}

/// Decodes bytes to UTF-8, replacing invalid sequences with U+FFFD.
/// Replacement-range tracking arrives with the invalid-input tests; for
/// valid input the lossy conversion is a borrowed pass-through.
fn decode_lossy(bytes: &[u8]) -> Result<(String, Vec<TextRange>), SourceTooLarge> {
    Ok((String::from_utf8_lossy(bytes).into_owned(), Vec::new()))
}
```

Modify `crates/celerrate_source/src/lib.rs`: keep the existing `//!` crate doc comment untouched (Task 4 refreshes it), and update the items below it:

```rust
pub use text_size::{TextRange, TextSize};

mod file_id;
mod line_index;
mod source_text;

pub use file_id::FileId;
pub use line_index::{LineColumn, LineIndex};
pub use source_text::{SourceText, SourceTooLarge};
```

- [ ] **Step 4: Add the unit test for the cap helper**

The 4 GiB failure path cannot allocate 4 GiB in CI, so it is tested through the helper, in a unit-test module at the end of `crates/celerrate_source/src/source_text.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{SourceTooLarge, text_size_of};

    #[test]
    fn lengths_within_the_cap_convert() {
        assert!(text_size_of(0).is_ok());
        assert!(text_size_of(u32::MAX as usize).is_ok());
    }

    #[test]
    fn lengths_beyond_the_cap_are_rejected() {
        let length = u32::MAX as usize + 1;
        assert_eq!(
            text_size_of(length),
            Err(SourceTooLarge {
                decoded_length: length
            })
        );
    }
}
```

Note: on a 32-bit target `u32::MAX as usize + 1` would overflow, but the workspace only targets 64-bit platforms (spec section 7 of the parent design: Linux/macOS x64+arm64, Windows x64).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_source`
Expected: all tests pass (7 in `source_text.rs`, 2 unit tests, plus existing `file_id` and `line_index` tests).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_source/tests/source_text.rs crates/celerrate_source/src/source_text.rs crates/celerrate_source/src/lib.rs
git commit -m "✨ feat(source): add SourceText decoding with BOM handling"
```

---

### Task 3: Invalid UTF-8 replacement tracking

**Files:**
- Modify: `crates/celerrate_source/tests/source_text.rs`
- Modify: `crates/celerrate_source/src/source_text.rs` (the `decode_lossy` function only)

**Interfaces:**
- Consumes: `SourceText::from_bytes`, `text_size_of`, and the `SourceText` accessors from Task 2 (exact signatures in Task 2's Produces block).
- Produces: `decode_lossy` now returns the actual replacement ranges; the public API is unchanged.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_source/tests/source_text.rs`:

```rust
use celerrate_source::{TextRange, TextSize};

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

#[test]
fn invalid_byte_at_start_is_replaced_and_recorded() {
    let source = SourceText::from_bytes(b"\xFFabc").expect("fits the cap");
    assert_eq!(source.text(), "\u{FFFD}abc");
    assert_eq!(source.replacements(), &[range(0, 3)]);
    assert!(!source.is_pristine());
}

#[test]
fn invalid_byte_in_the_middle_is_replaced_and_recorded() {
    let source = SourceText::from_bytes(b"ab\xFFcd").expect("fits the cap");
    assert_eq!(source.text(), "ab\u{FFFD}cd");
    assert_eq!(source.replacements(), &[range(2, 5)]);
}

#[test]
fn invalid_byte_at_the_end_is_replaced_and_recorded() {
    let source = SourceText::from_bytes(b"ab\xFF").expect("fits the cap");
    assert_eq!(source.text(), "ab\u{FFFD}");
    assert_eq!(source.replacements(), &[range(2, 5)]);
}

#[test]
fn consecutive_invalid_bytes_each_get_a_replacement() {
    let source = SourceText::from_bytes(b"a\xFF\xFEb").expect("fits the cap");
    assert_eq!(source.text(), "a\u{FFFD}\u{FFFD}b");
    assert_eq!(source.replacements(), &[range(1, 4), range(4, 7)]);
}

#[test]
fn truncated_multibyte_character_at_the_end_is_one_replacement() {
    // "é" is C3 A9; the input stops after C3.
    let source = SourceText::from_bytes(b"caf\xC3").expect("fits the cap");
    assert_eq!(source.text(), "caf\u{FFFD}");
    assert_eq!(source.replacements(), &[range(3, 6)]);
}

#[test]
fn literal_replacement_character_in_valid_input_is_not_recorded() {
    let source = SourceText::from_bytes("a\u{FFFD}b".as_bytes()).expect("fits the cap");
    assert_eq!(source.text(), "a\u{FFFD}b");
    assert!(source.replacements().is_empty());
    assert!(source.is_pristine());
}

#[test]
fn bom_and_replacements_combine_in_pristine() {
    let source = SourceText::from_bytes(b"\xEF\xBB\xBFa\xFF").expect("fits the cap");
    assert_eq!(source.text(), "a\u{FFFD}");
    assert!(source.had_utf8_bom());
    assert_eq!(source.replacements(), &[range(1, 4)]);
    assert!(!source.is_pristine());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_source --test source_text`
Expected: the new tests fail on `replacements()` assertions (the decoded text is already correct through `from_utf8_lossy`; the ranges are missing). The Task 2 tests still pass.

- [ ] **Step 3: Replace `decode_lossy` with the range-tracking loop**

In `crates/celerrate_source/src/source_text.rs`, replace the whole `decode_lossy` function (keep `text_size_of` and everything else unchanged):

```rust
const REPLACEMENT_CHARACTER: char = '\u{FFFD}';

/// Decodes bytes to UTF-8. Each invalid sequence (a maximal invalid
/// subpart, per the Unicode substitution recommendation `core::str`
/// follows) becomes one U+FFFD, and its range in the decoded text is
/// recorded.
fn decode_lossy(bytes: &[u8]) -> Result<(String, Vec<TextRange>), SourceTooLarge> {
    let mut text = String::new();
    let mut replacements = Vec::new();
    let mut remaining = bytes;
    loop {
        match core::str::from_utf8(remaining) {
            Ok(valid) => {
                text.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if let Some(Ok(valid)) = remaining
                    .get(..valid_up_to)
                    .map(core::str::from_utf8)
                {
                    text.push_str(valid);
                }
                let start = text_size_of(text.len())?;
                text.push(REPLACEMENT_CHARACTER);
                let end = text_size_of(text.len())?;
                replacements.push(TextRange::new(start, end));
                let skip = match error.error_len() {
                    Some(invalid_length) => valid_up_to + invalid_length,
                    // A truncated character at the end of input: nothing
                    // left to decode after it.
                    None => remaining.len(),
                };
                remaining = remaining.get(skip..).unwrap_or_default();
                if remaining.is_empty() {
                    break;
                }
            }
        }
    }
    Ok((text, replacements))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_source`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_source/tests/source_text.rs crates/celerrate_source/src/source_text.rs
git commit -m "✨ feat(source): track invalid UTF-8 replacements"
```

---

### Task 4: Crate documentation refresh, changelog, full verification

**Files:**
- Modify: `crates/celerrate_source/src/lib.rs` (doc comment only)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: everything shipped by Tasks 1-3.
- Produces: nothing new; documentation and a green workspace.

- [ ] **Step 1: Refresh the crate documentation**

In `crates/celerrate_source/src/lib.rs`, replace the doc comment (the note that loading "is added by a later plan" is now stale):

```rust
//! Source text primitives for the Celerrate toolchain: file identifiers,
//! decoded source text, text sizes, ranges, and line/column indexing.
//! This is the bottom layer of the workspace: it depends on no other
//! Celerrate crate and performs no I/O — file contents arrive as bytes
//! from whoever discovers files (command-line walk, editor buffers,
//! tests) and are decoded by [`SourceText::from_bytes`].
//!
//! Offsets and ranges are byte-based and use the `text-size` types, which
//! cap file size at 4 GiB; decoding rejects larger inputs (as
//! [`SourceTooLarge`]) before offsets are ever constructed.
```

- [ ] **Step 2: Update the changelog**

In `CHANGELOG.md`, replace the `celerrate_source` bullet under `## [Unreleased]` / `### Added`:

```markdown
- `celerrate_source`: source text primitives (spans, line/column index,
  file identifiers, byte decoding with BOM and invalid-UTF-8 provenance).
```

- [ ] **Step 3: Run the full verification suite**

Run, from the repository root:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: all three succeed with no output changes required. If `cargo fmt` reports differences, apply `cargo fmt --all` and re-run the tests.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_source/src/lib.rs CHANGELOG.md
git commit -m "📝 docs(source): document decoding in crate docs and changelog"
```

---

## Completion

When all tasks are done and the verification suite is green, use superpowers:finishing-a-development-branch to integrate `foundations-2-source-files` (Part 1 went through a pull request to `main`; expect the same here).
