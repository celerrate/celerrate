# Diagnostics and Fixes Part 2: `celerrate_edit` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the `celerrate_edit` crate: structured edits expressed against the lossless syntax tree, compiled into deterministic, sorted, conflict-free `TextEdit` sets, plus the application primitive that splices such a set into source text, guarded by a fuzz target.

**Architecture:** `celerrate_edit` sits directly above `celerrate_syntax` and depends on `celerrate_source` and `celerrate_syntax`, nothing higher (design section 2). An `EditBuilder` records operations against nodes and tokens of one file's tree and `finish()`es into a sorted `Vec<TextEdit>` (the `celerrate_source` shape landed in part 1); a free function `apply` splices a finalized set into source text. Overlapping edits are a `Result` error in both places, never a silent resolution — the application layer (part 6, the autofix engine) decides what to do with conflicts.

**Deliberate minimalism (a design rule, not an omission):** the design says "The API grows only at the pace of shipped fixes: token replacement and comment insertion with indentation are what this sub-project needs." This plan therefore builds exactly two structured operations — kind-checked token replacement and own-line comment insertion that reproduces indentation — plus conflict detection and application. `insert_after`, `delete`, node replacement, and element constructors arrive with their first real clients (the style group, the formatter, migrations). Do not add them here.

**Tech Stack:** Rust (edition 2024, workspace toolchain pin), rowan-backed syntax tree via `celerrate_syntax` aliases, `text-size` ranges via `celerrate_source` re-exports, `cargo-fuzz`/libFuzzer with the `arbitrary` crate for the fuzz target.

## Global Constraints

Copied from the project's non-negotiable rules; every task implicitly includes them.

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result`. Test modules may locally `#[allow]` these lints (existing convention: `#![allow(clippy::unwrap_used)]` or `expect_used` at the top of the `tests` module, with a one-line justification comment).
- TDD: failing test first, minimal implementation, refactor. No production code without a test that demanded it.
- Strict layering: `celerrate_edit` depends only on `celerrate_source` and `celerrate_syntax`. No salsa, no `celerrate_db`, no `celerrate_diagnostics`.
- Determinism: compiled edit sets are sorted by the `TextEdit` total order; `apply` sorts its input so the result never depends on input order.
- Everything in English, full words, no abbreviated names (`replacement`, not `repl`).
- Commits: gitmoji + Conventional Commits, e.g. `✨ feat(edit): ...`, authored with the repository-configured identity (never override it).
- Local gates: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`, `cargo deny check`. Run test + clippy + fmt before every commit.

## Existing surface this plan consumes (verified against the code)

- `celerrate_source` re-exports: `FileId` (`FileId::new(u32)`, `.as_u32()`), `TextRange` (`TextRange::new(start, end)`, `TextRange::empty(offset)`, `.start()`, `.end()`, `.is_empty()`), `TextSize` (`TextSize::from(u32)`, `usize::from(size)`), and `TextEdit { file: FileId, range: TextRange, replacement: String }` with a total `Ord` by `(file, range.start(), range.end(), replacement)`.
- `celerrate_syntax` exports: `parse(source: &str) -> Parse` (`.tree() -> SyntaxNode`), `lex(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>)` where `Token { kind: SyntaxKind, length: TextSize }`, `SyntaxKind` (with `.is_trivia()`), `SyntaxNode`, `SyntaxToken` (rowan-backed: `.kind()`, `.text() -> &str`, `.text_range() -> TextRange`, `.first_token()`, `.prev_token()`, `.descendants()`, `.descendants_with_tokens()`).
- The lexer starts in inline-HTML mode: `lex("foo")` yields an `InlineHtml` token, not an `Identifier`. Validating a replacement therefore lexes it behind a synthetic `"<?php "` prefix.
- `TextRange::new` panics when `start > end` — the fuzz harness must normalize arbitrary pairs before constructing ranges.
- The workspace member glob `crates/*` picks the new crate up automatically; the xtask `dependency-shape` check governs only plugin crates and needs no change.
- `fuzz/` is its own Cargo workspace; its `Cargo.lock` is gitignored. CI (`.github/workflows/fuzz.yml`) enumerates fuzz targets explicitly, one `cargo +nightly fuzz run <target>` step each; committed seeds live in `fuzz/corpus/<target>/`.

## File structure

- Create: `crates/celerrate_edit/Cargo.toml` — crate manifest.
- Create: `crates/celerrate_edit/src/lib.rs` — crate documentation and re-exports only.
- Create: `crates/celerrate_edit/src/conflict.rs` — `EditConflict` and the shared conflict predicate.
- Create: `crates/celerrate_edit/src/apply.rs` — `apply` and `ApplyError`.
- Create: `crates/celerrate_edit/src/builder.rs` — `EditBuilder` and `EditError`.
- Create: `fuzz/fuzz_targets/edit_apply.rs` — the edit-application fuzz target.
- Create: `fuzz/corpus/edit_apply/seed_basic` — one committed seed input.
- Modify: `fuzz/Cargo.toml` — the `celerrate_edit` and `arbitrary` dependencies, the `edit_apply` binary.
- Modify: `.github/workflows/fuzz.yml` — the `edit_apply` run step.
- Modify: `CHANGELOG.md` — the Unreleased entry.

---

### Task 1: Crate scaffold and conflict detection

**Files:**
- Create: `crates/celerrate_edit/Cargo.toml`
- Create: `crates/celerrate_edit/src/lib.rs`
- Create: `crates/celerrate_edit/src/conflict.rs`

**Interfaces:**
- Consumes: `celerrate_source::{TextEdit, FileId, TextRange, TextSize}`.
- Produces: `pub struct EditConflict { pub first: TextEdit, pub second: TextEdit }` (derives `Debug, Clone, PartialEq, Eq`) and `pub(crate) fn find_conflict(sorted: &[TextEdit]) -> Option<EditConflict>`, both in `conflict.rs`; `lib.rs` re-exports `EditConflict`. Tasks 2 and 3 call `find_conflict` and wrap `EditConflict` in their error types.

**Conflict rule (the exact semantics):** over a slice already sorted by the `TextEdit` total order, two adjacent same-file edits conflict when their ranges intersect over at least one byte (`first.range.end() > second.range.start()`), or when both are insertions at the same offset (equal empty ranges — their relative order would be arbitrary, and "never a silent resolution" forbids picking one). Touching at a boundary is not a conflict: an insertion at the start of a replaced range deterministically lands before the replacement. Checking adjacent pairs suffices because sorting groups intersecting ranges next to each other.

- [ ] **Step 1: Create the crate scaffold**

`crates/celerrate_edit/Cargo.toml`:

```toml
[package]
name = "celerrate_edit"
description = "Structured-edit library on the Celerrate syntax tree"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_source = { path = "../celerrate_source" }
celerrate_syntax = { path = "../celerrate_syntax" }

[lints]
workspace = true
```

`crates/celerrate_edit/src/lib.rs` (this reduced doc comment is deliberate: `EditBuilder` and `apply` do not exist yet, and task 3 rewrites the paragraph to name them once they do):

```rust
//! Structured edits on the Celerrate syntax tree, compiled into the
//! deterministic, sorted, conflict-free [`TextEdit`] set that
//! suggestions transport. Two overlapping edits are an error, never a
//! silent resolution.
//!
//! [`TextEdit`]: celerrate_source::TextEdit

mod conflict;

pub use conflict::EditConflict;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_edit/src/conflict.rs` with the test module only (no implementation yet):

```rust
#[cfg(test)]
mod tests {
    //! `unwrap` is fine here: failing loudly is what a test should do.
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use super::find_conflict;

    fn edit(file: u32, start: u32, end: u32, replacement: &str) -> TextEdit {
        TextEdit {
            file: FileId::new(file),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            replacement: replacement.to_owned(),
        }
    }

    #[test]
    fn disjoint_edits_do_not_conflict() {
        let edits = [edit(0, 0, 2, "a"), edit(0, 5, 9, "b")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn touching_edits_do_not_conflict() {
        let edits = [edit(0, 0, 5, "a"), edit(0, 5, 9, "b")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn an_insertion_at_the_start_of_a_replacement_does_not_conflict() {
        let edits = [edit(0, 5, 5, "inserted"), edit(0, 5, 9, "replaced")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn intersecting_edits_conflict() {
        let edits = [edit(0, 0, 5, "a"), edit(0, 4, 9, "b")];
        let conflict = find_conflict(&edits).unwrap();
        assert_eq!(conflict.first, edit(0, 0, 5, "a"));
        assert_eq!(conflict.second, edit(0, 4, 9, "b"));
    }

    #[test]
    fn a_replacement_containing_an_insertion_point_conflicts() {
        let edits = [edit(0, 0, 9, "a"), edit(0, 4, 4, "b")];
        assert!(find_conflict(&edits).is_some());
    }

    #[test]
    fn identical_edits_conflict() {
        let edits = [edit(0, 3, 5, "a"), edit(0, 3, 5, "a")];
        assert!(find_conflict(&edits).is_some());
    }

    #[test]
    fn two_insertions_at_the_same_offset_conflict() {
        let edits = [edit(0, 5, 5, "a"), edit(0, 5, 5, "b")];
        assert!(find_conflict(&edits).is_some());
    }

    #[test]
    fn same_ranges_in_different_files_do_not_conflict() {
        let edits = [edit(0, 0, 5, "a"), edit(1, 0, 5, "b")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn the_first_conflicting_pair_is_reported() {
        let edits = [
            edit(0, 0, 5, "a"),
            edit(0, 4, 6, "b"),
            edit(0, 5, 9, "c"),
        ];
        let conflict = find_conflict(&edits).unwrap();
        assert_eq!(conflict.first, edit(0, 0, 5, "a"));
        assert_eq!(conflict.second, edit(0, 4, 6, "b"));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_edit`
Expected: compilation error — `cannot find function find_conflict` (and the struct) in `super`.

- [ ] **Step 4: Write the minimal implementation**

Prepend to `crates/celerrate_edit/src/conflict.rs` (above the test module):

```rust
use celerrate_source::TextEdit;

/// Two edits that cannot coexist in one edit set: their ranges
/// intersect, or both insert at the same offset and their relative
/// order would be arbitrary. `first` precedes `second` in the total
/// edit order. Conflicts are reported, never silently resolved; the
/// application layer decides what to do with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditConflict {
    pub first: TextEdit,
    pub second: TextEdit,
}

/// Finds the first conflicting pair in an edit slice already sorted by
/// the [`TextEdit`] total order. Touching at a boundary is not a
/// conflict: an insertion at the start of a replaced range
/// deterministically lands before the replacement. Sorting groups
/// intersecting ranges next to each other, so adjacent pairs suffice.
pub(crate) fn find_conflict(sorted: &[TextEdit]) -> Option<EditConflict> {
    sorted.windows(2).find_map(|pair| {
        let [first, second] = pair else {
            return None;
        };
        if first.file != second.file {
            return None;
        }
        let intersects = first.range.end() > second.range.start();
        let racing_insertions = first.range == second.range && first.range.is_empty();
        (intersects || racing_insertions).then(|| EditConflict {
            first: first.clone(),
            second: second.clone(),
        })
    })
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_edit`
Expected: all 9 tests PASS.

- [ ] **Step 6: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean clippy, no formatting churn outside the new crate, all tests green.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_edit Cargo.lock
git commit -m "✨ feat(edit): create the crate with edit-conflict detection"
```

---

### Task 2: The application primitive

**Files:**
- Create: `crates/celerrate_edit/src/apply.rs`
- Modify: `crates/celerrate_edit/src/lib.rs`

**Interfaces:**
- Consumes: `EditConflict` and `find_conflict` from task 1; `celerrate_source::TextEdit`.
- Produces: `pub fn apply(source: &str, edits: &[TextEdit]) -> Result<String, ApplyError>` and `pub enum ApplyError { Conflict(EditConflict), MultipleFiles { edit: TextEdit }, RangeOutOfBounds { edit: TextEdit, source_length: usize }, RangeNotOnCharacterBoundary { edit: TextEdit } }` (derives `Debug, Clone, PartialEq, Eq`), both re-exported from `lib.rs`. Task 5's fuzz target and part 6's autofix engine consume exactly this signature — note that `apply` takes a plain `&[TextEdit]` (suggestions deserialized from the persistent cache arrive as plain edits, not through the builder), sorts a copy internally, and validates everything itself.

**Semantics:** sort a copy of the input by the `TextEdit` total order (result never depends on input order); every edit must target the same file (an empty slice is fine and returns the source unchanged); any conflict is `ApplyError::Conflict`; every range must satisfy `end <= source.len()` and start/end on `char` boundaries; splice front to back with a cursor. Nothing is ever silently dropped or resolved.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_edit/src/apply.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    //! `unwrap` is fine here: failing loudly is what a test should do.
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use super::{ApplyError, apply};

    fn edit(start: u32, end: u32, replacement: &str) -> TextEdit {
        TextEdit {
            file: FileId::new(0),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            replacement: replacement.to_owned(),
        }
    }

    #[test]
    fn no_edits_return_the_source_unchanged() {
        assert_eq!(apply("<?php echo 1;", &[]).unwrap(), "<?php echo 1;");
    }

    #[test]
    fn a_single_replacement_is_spliced() {
        // "<?php echo 1;" — replace "1" (offsets 11..12) with "2".
        assert_eq!(
            apply("<?php echo 1;", &[edit(11, 12, "2")]).unwrap(),
            "<?php echo 2;",
        );
    }

    #[test]
    fn an_insertion_and_a_deletion_compose() {
        // "abcdef": insert "X" at 2, delete "de" (3..5).
        assert_eq!(
            apply("abcdef", &[edit(2, 2, "X"), edit(3, 5, "")]).unwrap(),
            "abXcf",
        );
    }

    #[test]
    fn the_result_does_not_depend_on_input_order() {
        let forward = apply("abcdef", &[edit(0, 1, "X"), edit(3, 4, "Y")]).unwrap();
        let backward = apply("abcdef", &[edit(3, 4, "Y"), edit(0, 1, "X")]).unwrap();
        assert_eq!(forward, backward);
        assert_eq!(forward, "XbcYef");
    }

    #[test]
    fn an_insertion_at_the_end_of_the_source_is_valid() {
        assert_eq!(apply("abc", &[edit(3, 3, "!")]).unwrap(), "abc!");
    }

    #[test]
    fn intersecting_edits_are_a_conflict() {
        let error = apply("abcdef", &[edit(0, 4, "x"), edit(2, 6, "y")]).unwrap_err();
        assert!(matches!(error, ApplyError::Conflict(_)));
    }

    #[test]
    fn edits_in_different_files_are_rejected() {
        let foreign = TextEdit {
            file: FileId::new(1),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            replacement: "x".to_owned(),
        };
        let error = apply("abcdef", &[edit(3, 4, "y"), foreign.clone()]).unwrap_err();
        assert_eq!(error, ApplyError::MultipleFiles { edit: foreign });
    }

    #[test]
    fn a_range_past_the_end_of_the_source_is_rejected() {
        let error = apply("abc", &[edit(2, 9, "x")]).unwrap_err();
        assert_eq!(
            error,
            ApplyError::RangeOutOfBounds {
                edit: edit(2, 9, "x"),
                source_length: 3,
            },
        );
    }

    #[test]
    fn a_range_splitting_a_multibyte_character_is_rejected() {
        // "héllo": 'é' occupies bytes 1..3; offset 2 splits it.
        let error = apply("héllo", &[edit(2, 4, "x")]).unwrap_err();
        assert_eq!(
            error,
            ApplyError::RangeNotOnCharacterBoundary {
                edit: edit(2, 4, "x"),
            },
        );
    }

    #[test]
    fn multibyte_content_survives_splicing() {
        // "héllo": replace "llo" (bytes 3..6) with "ros".
        assert_eq!(apply("héllo", &[edit(3, 6, "ros")]).unwrap(), "héros");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_edit`
Expected: compilation error — `cannot find function apply` / `cannot find type ApplyError` in `super`.

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_edit/src/apply.rs`:

```rust
use celerrate_source::TextEdit;

use crate::conflict::{EditConflict, find_conflict};

/// Why an edit set could not be applied to a source text. Nothing is
/// ever dropped or resolved silently: the caller decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Two edits intersect, or race for the same insertion point.
    Conflict(EditConflict),
    /// One application targets one file; this edit belongs to another.
    MultipleFiles { edit: TextEdit },
    /// The edit's range does not fit the source text.
    RangeOutOfBounds { edit: TextEdit, source_length: usize },
    /// The edit's range splits a multi-byte character.
    RangeNotOnCharacterBoundary { edit: TextEdit },
}

/// Applies a finalized edit set to one file's source text.
///
/// The edits are sorted into the [`TextEdit`] total order first, so
/// the result never depends on input order; conflicts, foreign files,
/// and ill-fitting ranges are errors. The empty set returns the source
/// unchanged.
pub fn apply(source: &str, edits: &[TextEdit]) -> Result<String, ApplyError> {
    let mut sorted = edits.to_vec();
    sorted.sort();
    if let Some(first) = sorted.first() {
        if let Some(foreign) = sorted.iter().find(|edit| edit.file != first.file) {
            return Err(ApplyError::MultipleFiles {
                edit: foreign.clone(),
            });
        }
    }
    if let Some(conflict) = find_conflict(&sorted) {
        return Err(ApplyError::Conflict(conflict));
    }
    for edit in &sorted {
        let start = usize::from(edit.range.start());
        let end = usize::from(edit.range.end());
        if end > source.len() {
            return Err(ApplyError::RangeOutOfBounds {
                edit: edit.clone(),
                source_length: source.len(),
            });
        }
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            return Err(ApplyError::RangeNotOnCharacterBoundary { edit: edit.clone() });
        }
    }
    let mut patched = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in &sorted {
        let start = usize::from(edit.range.start());
        // Unreachable fallback: the ranges were just validated as
        // sorted, disjoint, in bounds, and on character boundaries.
        patched.push_str(source.get(cursor..start).unwrap_or(""));
        patched.push_str(&edit.replacement);
        cursor = usize::from(edit.range.end());
    }
    patched.push_str(source.get(cursor..).unwrap_or(""));
    Ok(patched)
}
```

Update `crates/celerrate_edit/src/lib.rs` (add the module and re-export):

```rust
mod apply;
mod conflict;

pub use apply::{ApplyError, apply};
pub use conflict::EditConflict;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_edit`
Expected: all tests PASS (9 from task 1 + 10 new).

- [ ] **Step 5: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_edit
git commit -m "✨ feat(edit): apply a finalized edit set to source text"
```

---

### Task 3: The builder and kind-checked token replacement

**Files:**
- Create: `crates/celerrate_edit/src/builder.rs`
- Modify: `crates/celerrate_edit/src/lib.rs`

**Interfaces:**
- Consumes: `find_conflict`/`EditConflict` from task 1; `apply` from task 2 (in tests); `celerrate_syntax::{lex, SyntaxKind, SyntaxToken}`; `celerrate_source::{FileId, TextEdit}`.
- Produces, re-exported from `lib.rs`, and consumed by task 4 and by part 6's fix construction:
  - `pub struct EditBuilder` with `pub fn new(file: FileId) -> EditBuilder`, `pub fn replace_token(&mut self, token: &SyntaxToken, replacement: &str) -> Result<(), EditError>`, `pub fn finish(self) -> Result<Vec<TextEdit>, EditConflict>`.
  - `pub enum EditError { ReplacementIsNotOneToken { replacement: String }, ReplacementChangesKind { expected: SyntaxKind, actual: SyntaxKind, replacement: String }, CommentTextBreaksOut { text: String } }` (derives `Debug, Clone, PartialEq, Eq`; the `CommentTextBreaksOut` variant is used by task 4 — declare it now so the enum does not change shape between tasks).

**The kind guarantee (what makes this "structured" rather than raw text):** the replacement must lex to exactly one clean token of the same kind as the token it replaces, so a rename can never smuggle structure ( `foo; drop()` ), turn an identifier into a keyword (`class`), leave trailing junk (`foo `), or carry a lexer error through an edit. Because the lexer starts in inline-HTML mode, validation lexes `"<?php {replacement}"` and skips the synthetic open tag plus trivia; the surviving token's byte length must equal the replacement's length.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_edit/src/builder.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use celerrate_source::FileId;
    use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

    use super::{EditBuilder, EditError};
    use crate::apply;

    fn parse_tree(source: &str) -> SyntaxNode {
        celerrate_syntax::parse(source).tree()
    }

    fn token_with_text(root: &SyntaxNode, text: &str) -> SyntaxToken {
        root.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.text() == text)
            .expect("the token under edit exists in the tree")
    }

    #[test]
    fn a_token_is_replaced_in_place() {
        let source = "<?php strlen($value);";
        let root = parse_tree(source);
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "strrev").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(apply(source, &edits).unwrap(), "<?php strrev($value);");
    }

    #[test]
    fn a_replacement_of_a_different_length_still_lands_exactly() {
        let source = "<?php userNme();";
        let root = parse_tree(source);
        let token = token_with_text(&root, "userNme");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "userName").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(apply(source, &edits).unwrap(), "<?php userName();");
    }

    #[test]
    fn surrounding_trivia_are_never_touched() {
        let source = "<?php  /* keep */  strlen($value);";
        let root = parse_tree(source);
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "strrev").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php  /* keep */  strrev($value);",
        );
    }

    #[test]
    fn a_replacement_that_is_two_tokens_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.replace_token(&token, "foo bar"),
            Err(EditError::ReplacementIsNotOneToken {
                replacement: "foo bar".to_owned(),
            }),
        );
    }

    #[test]
    fn a_replacement_with_trailing_whitespace_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.replace_token(&token, "foo "),
            Err(EditError::ReplacementIsNotOneToken {
                replacement: "foo ".to_owned(),
            }),
        );
    }

    #[test]
    fn an_empty_replacement_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.replace_token(&token, ""),
            Err(EditError::ReplacementIsNotOneToken {
                replacement: String::new(),
            }),
        );
    }

    #[test]
    fn a_replacement_with_a_lexer_error_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        assert!(matches!(
            builder.replace_token(&token, "\"unterminated"),
            Err(EditError::ReplacementIsNotOneToken { .. }),
        ));
    }

    #[test]
    fn renaming_an_identifier_to_a_keyword_is_rejected() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        let error = builder.replace_token(&token, "class").unwrap_err();
        assert!(matches!(
            error,
            EditError::ReplacementChangesKind {
                expected: SyntaxKind::Identifier,
                ..
            },
        ));
    }

    #[test]
    fn finish_sorts_the_edits_into_the_total_order() {
        let source = "<?php first(); second();";
        let root = parse_tree(source);
        let second = token_with_text(&root, "second");
        let first = token_with_text(&root, "first");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&second, "later").unwrap();
        builder.replace_token(&first, "early").unwrap();
        let edits = builder.finish().unwrap();
        assert!(edits[0].range.start() < edits[1].range.start());
        assert_eq!(apply(source, &edits).unwrap(), "<?php early(); later();");
    }

    #[test]
    fn replacing_the_same_token_twice_is_a_conflict() {
        let root = parse_tree("<?php strlen($value);");
        let token = token_with_text(&root, "strlen");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.replace_token(&token, "strrev").unwrap();
        builder.replace_token(&token, "strtolower").unwrap();
        assert!(builder.finish().is_err());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_edit`
Expected: compilation error — `cannot find EditBuilder` / `EditError`.

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_edit/src/builder.rs`:

```rust
use celerrate_source::{FileId, TextEdit};
use celerrate_syntax::{SyntaxKind, SyntaxToken, lex};

use crate::conflict::{EditConflict, find_conflict};

/// Why a structured operation could not be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The replacement text does not lex to exactly one clean token.
    ReplacementIsNotOneToken { replacement: String },
    /// The replacement lexes to a token of a different kind than the
    /// one it replaces, so the edit would change structure, not text.
    ReplacementChangesKind {
        expected: SyntaxKind,
        actual: SyntaxKind,
        replacement: String,
    },
    /// The comment text would terminate the comment early: it contains
    /// a line break or a PHP close tag.
    CommentTextBreaksOut { text: String },
}

/// Records structured operations against one file's syntax tree and
/// compiles them into the deterministic, sorted, conflict-free
/// [`TextEdit`] set. An operation never touches trivia it was not
/// aimed at.
pub struct EditBuilder {
    file: FileId,
    edits: Vec<TextEdit>,
}

impl EditBuilder {
    pub fn new(file: FileId) -> Self {
        Self {
            file,
            edits: Vec::new(),
        }
    }

    /// Replaces one token's text, keeping its kind: the replacement
    /// must lex to exactly one clean token of the same kind, so a
    /// rename can never smuggle structure through an edit.
    pub fn replace_token(
        &mut self,
        token: &SyntaxToken,
        replacement: &str,
    ) -> Result<(), EditError> {
        let actual = single_token_kind(replacement)?;
        if actual != token.kind() {
            return Err(EditError::ReplacementChangesKind {
                expected: token.kind(),
                actual,
                replacement: replacement.to_owned(),
            });
        }
        self.edits.push(TextEdit {
            file: self.file,
            range: token.text_range(),
            replacement: replacement.to_owned(),
        });
        Ok(())
    }

    /// Finalizes into the sorted edit set, or reports the first
    /// conflict. The set is the terminal, tree-free form suggestions
    /// transport and [`crate::apply`] consumes.
    pub fn finish(self) -> Result<Vec<TextEdit>, EditConflict> {
        let mut edits = self.edits;
        edits.sort();
        match find_conflict(&edits) {
            Some(conflict) => Err(conflict),
            None => Ok(edits),
        }
    }
}

/// Lexes `replacement` in scripting mode (behind a synthetic `<?php `
/// prefix, because the lexer starts in inline-HTML mode) and returns
/// its kind when it is exactly one clean token: no lexer diagnostics,
/// one non-trivia token past the open tag, and nothing else — the
/// length comparison rejects trailing trivia.
fn single_token_kind(replacement: &str) -> Result<SyntaxKind, EditError> {
    let not_one_token = || EditError::ReplacementIsNotOneToken {
        replacement: replacement.to_owned(),
    };
    let (tokens, diagnostics) = lex(&format!("<?php {replacement}"));
    if !diagnostics.is_empty() {
        return Err(not_one_token());
    }
    let mut meaningful = tokens
        .iter()
        .skip(1) // the synthetic open tag
        .filter(|token| !token.kind.is_trivia());
    let Some(first) = meaningful.next() else {
        return Err(not_one_token());
    };
    if meaningful.next().is_some() {
        return Err(not_one_token());
    }
    if usize::from(first.length) != replacement.len() {
        return Err(not_one_token());
    }
    Ok(first.kind)
}
```

Update `crates/celerrate_edit/src/lib.rs`:

```rust
//! Structured edits on the Celerrate syntax tree: [`EditBuilder`]
//! expresses edits as operations on nodes and tokens and compiles them
//! into the deterministic, sorted, conflict-free [`TextEdit`] set that
//! suggestions transport, and [`apply`] splices such a set into source
//! text. Two overlapping edits are an error, never a silent resolution,
//! and an edit never touches trivia it was not aimed at.
//!
//! [`TextEdit`]: celerrate_source::TextEdit

mod apply;
mod builder;
mod conflict;

pub use apply::{ApplyError, apply};
pub use builder::{EditBuilder, EditError};
pub use conflict::EditConflict;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_edit`
Expected: all tests PASS. If `renaming_an_identifier_to_a_keyword_is_rejected` fails because the token kind for `strlen` in a call position is not `Identifier`, inspect with a one-off `dbg!(token.kind())` — adjust the test's `expected:` pattern to the actual kind, not the implementation.

- [ ] **Step 5: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_edit
git commit -m "✨ feat(edit): the builder replaces tokens under a same-kind guarantee"
```

---

### Task 4: Line-comment insertion with indentation

**Files:**
- Modify: `crates/celerrate_edit/src/builder.rs`

**Interfaces:**
- Consumes: `EditBuilder` internals from task 3; `celerrate_syntax::SyntaxNode`; `celerrate_source::TextRange`.
- Produces: `pub fn insert_line_comment_before(&mut self, node: &SyntaxNode, text: &str) -> Result<(), EditError>` on `EditBuilder`. This is the design's "comment insertion with indentation"; part 5's native-directive work is its expected first client.

**Semantics:** inserts `// {text}` on its own line directly above `node`, reproducing the node's indentation. The edit is a pure insertion at the node's first byte — the replacement string is `"// {text}\n{indentation}"` — so the trivia already in front of the node are never touched (the design's style-preservation rule). The indentation is the run after the last line break in the whitespace token immediately preceding the node; a node with no preceding whitespace token (start of file, or glued to the previous token) gets no indentation; a mid-line node (preceding whitespace without a line break) reuses that whitespace as-is, which keeps the statement intact because the inserted comment ends with a line break before the node. The text is rejected (`EditError::CommentTextBreaksOut`) when it contains `\n`, `\r`, or `?>` — a PHP line comment is terminated by a close tag, so any of those would end the comment early and change program structure.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/celerrate_edit/src/builder.rs` (it already imports `apply`, `EditBuilder`, `EditError`, `parse_tree`, `FileId`; add a node-finding helper):

```rust
    fn first_node_of_kind(root: &SyntaxNode, kind: SyntaxKind) -> SyntaxNode {
        root.descendants()
            .find(|node| node.kind() == kind)
            .expect("the target node exists in the tree")
    }

    #[test]
    fn a_comment_is_inserted_above_an_indented_statement() {
        let source = "<?php\nfunction demo() {\n    echo 1;\n}\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder
            .insert_line_comment_before(&statement, "@celerrate-ignore CEL0018")
            .unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php\nfunction demo() {\n    // @celerrate-ignore CEL0018\n    echo 1;\n}\n",
        );
    }

    #[test]
    fn a_comment_above_a_top_level_statement_carries_no_indentation() {
        let source = "<?php\necho 1;\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.insert_line_comment_before(&statement, "note").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(apply(source, &edits).unwrap(), "<?php\n// note\necho 1;\n");
    }

    #[test]
    fn tab_indentation_is_reproduced() {
        let source = "<?php\nfunction demo() {\n\techo 1;\n}\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.insert_line_comment_before(&statement, "note").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php\nfunction demo() {\n\t// note\n\techo 1;\n}\n",
        );
    }

    #[test]
    fn a_mid_line_node_stays_intact_after_insertion() {
        // The comment ends with a line break before the node, so the
        // statement survives even when the node is not at a line start.
        let source = "<?php\necho 1; echo 2;\n";
        let root = parse_tree(source);
        let second = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::EchoStatement)
            .nth(1)
            .expect("the second echo statement");
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.insert_line_comment_before(&second, "note").unwrap();
        let edits = builder.finish().unwrap();
        assert_eq!(
            apply(source, &edits).unwrap(),
            "<?php\necho 1; // note\n echo 2;\n",
        );
    }

    #[test]
    fn comment_text_with_a_line_break_is_rejected() {
        let root = parse_tree("<?php\necho 1;\n");
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.insert_line_comment_before(&statement, "a\nb"),
            Err(EditError::CommentTextBreaksOut {
                text: "a\nb".to_owned(),
            }),
        );
    }

    #[test]
    fn comment_text_with_a_close_tag_is_rejected() {
        let root = parse_tree("<?php\necho 1;\n");
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        assert_eq!(
            builder.insert_line_comment_before(&statement, "a ?> b"),
            Err(EditError::CommentTextBreaksOut {
                text: "a ?> b".to_owned(),
            }),
        );
    }

    #[test]
    fn an_inserted_comment_reparses_as_a_comment() {
        // The guarantee behind the validation: the patched file lexes
        // with the inserted text inside a line comment, not as code.
        let source = "<?php\nfunction demo() {\n    echo 1;\n}\n";
        let root = parse_tree(source);
        let statement = first_node_of_kind(&root, SyntaxKind::EchoStatement);
        let mut builder = EditBuilder::new(FileId::new(0));
        builder.insert_line_comment_before(&statement, "note").unwrap();
        let edits = builder.finish().unwrap();
        let patched = apply(source, &edits).unwrap();
        let reparsed = parse_tree(&patched);
        let comment = reparsed
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::LineComment)
            .expect("the inserted comment lexes as a line comment");
        assert_eq!(comment.text(), "// note");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_edit`
Expected: compilation error — `no method named insert_line_comment_before`.

- [ ] **Step 3: Write the minimal implementation**

Add to the `impl EditBuilder` block in `crates/celerrate_edit/src/builder.rs` (and extend the `use` lines with `celerrate_source::TextRange` and `celerrate_syntax::SyntaxNode`):

```rust
    /// Inserts `// text` on its own line directly above `node`,
    /// reproducing the node's indentation. The edit is a pure insertion
    /// at the node's first byte, so the trivia already in front of the
    /// node are never touched.
    pub fn insert_line_comment_before(
        &mut self,
        node: &SyntaxNode,
        text: &str,
    ) -> Result<(), EditError> {
        if text.contains('\n') || text.contains('\r') || text.contains("?>") {
            return Err(EditError::CommentTextBreaksOut {
                text: text.to_owned(),
            });
        }
        let indentation = indentation_before(node);
        self.edits.push(TextEdit {
            file: self.file,
            range: TextRange::empty(node.text_range().start()),
            replacement: format!("// {text}\n{indentation}"),
        });
        Ok(())
    }
```

And the helper, next to `single_token_kind`:

```rust
/// The whitespace run between the last line break and `node`, used to
/// reproduce the node's indentation on an inserted line. A node with
/// no preceding whitespace token has no indentation to reproduce; a
/// mid-line node (preceding whitespace without a line break) reuses
/// that whitespace as-is.
fn indentation_before(node: &SyntaxNode) -> String {
    let Some(first_token) = node.first_token() else {
        return String::new();
    };
    let Some(previous) = first_token.prev_token() else {
        return String::new();
    };
    if previous.kind() != SyntaxKind::Whitespace {
        return String::new();
    }
    let text = previous.text();
    let after_break = text.rfind('\n').map_or(0, |index| index + 1);
    text.get(after_break..).unwrap_or("").to_owned()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_edit`
Expected: all tests PASS. If `a_comment_is_inserted_above_an_indented_statement` fails on the indentation, the likely cause is trivia attachment (the whitespace before the statement not being the token immediately preceding the node's first token) — print the token stream around the node before changing anything, and fix the helper, not the expected output.

- [ ] **Step 5: Run the workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_edit
git commit -m "✨ feat(edit): line-comment insertion reproduces the indentation"
```

---

### Task 5: The edit-application fuzz target

**Files:**
- Create: `fuzz/fuzz_targets/edit_apply.rs`
- Create: `fuzz/corpus/edit_apply/seed_basic`
- Modify: `fuzz/Cargo.toml`
- Modify: `.github/workflows/fuzz.yml`

**Interfaces:**
- Consumes: `celerrate_edit::apply` exactly as produced by task 2.
- Produces: the `edit_apply` fuzz binary; no library surface.

**Properties under fuzz (design section 11: "never panics, never silently resolves an overlap"):**
1. `apply` never panics on any (source, edit set) input.
2. If `apply` returns `Ok`, no two edits in the set intersect — an overlap that slipped through would be a silent resolution.
3. If `apply` returns `Ok`, the result agrees with a differential oracle: splicing the sorted edits one at a time, back to front, into the source. (Back to front so earlier offsets stay valid; correct exactly because the set is non-overlapping.)

The fuzz harness is a test harness: panics inside it on violated properties are the failure signal, which is what libFuzzer reports. Note that `TextRange::new` panics when `start > end`, so the harness normalizes each arbitrary offset pair before constructing a range.

- [ ] **Step 1: Add the dependency and the binary to the fuzz manifest**

In `fuzz/Cargo.toml`, add to `[dependencies]`:

```toml
arbitrary = { version = "1", features = ["derive"] }
celerrate_edit = { path = "../crates/celerrate_edit" }
```

And append the binary section (after the existing `[[bin]]` blocks, before the `[workspace]` line):

```toml
[[bin]]
name = "edit_apply"
path = "fuzz_targets/edit_apply.rs"
test = false
doc = false
bench = false
```

- [ ] **Step 2: Write the fuzz target**

Create `fuzz/fuzz_targets/edit_apply.rs`:

```rust
//! Arbitrary source text and arbitrary edit sets through
//! `celerrate_edit::apply`. Invariants: `apply` never panics, an `Ok`
//! result never contains a silently resolved overlap, and the patched
//! text agrees with one-at-a-time back-to-front splicing.

#![no_main]

use arbitrary::Arbitrary;
use celerrate_source::{FileId, TextEdit, TextRange, TextSize};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Input {
    source: String,
    edits: Vec<(u16, u16, String)>,
}

fuzz_target!(|input: Input| {
    let edits: Vec<TextEdit> = input
        .edits
        .iter()
        .map(|(first, second, replacement)| {
            // `TextRange::new` requires start <= end; arbitrary pairs
            // are normalized, everything else is apply's problem.
            let low = u32::from(*first.min(second));
            let high = u32::from(*first.max(second));
            TextEdit {
                file: FileId::new(0),
                range: TextRange::new(TextSize::from(low), TextSize::from(high)),
                replacement: replacement.clone(),
            }
        })
        .collect();
    let Ok(patched) = celerrate_edit::apply(&input.source, &edits) else {
        // Refusal is always a legal outcome; the properties constrain
        // what `apply` accepts, not what it rejects.
        return;
    };
    let mut sorted = edits;
    sorted.sort();
    for pair in sorted.windows(2) {
        if let [first, second] = pair {
            assert!(
                first.range.end() <= second.range.start(),
                "apply silently resolved an overlap: {first:?} / {second:?}",
            );
        }
    }
    let mut expected = input.source.clone();
    for edit in sorted.iter().rev() {
        let start = usize::from(edit.range.start());
        let end = usize::from(edit.range.end());
        let (Some(head), Some(tail)) = (expected.get(..start), expected.get(end..)) else {
            panic!("apply accepted an edit its oracle cannot splice: {edit:?}");
        };
        expected = format!("{head}{}{tail}", edit.replacement);
    }
    assert_eq!(
        patched, expected,
        "apply disagrees with one-at-a-time splicing",
    );
});
```

- [ ] **Step 3: Commit a seed input**

```bash
mkdir -p fuzz/corpus/edit_apply
printf '<?php echo 1;' > fuzz/corpus/edit_apply/seed_basic
```

(With an `Arbitrary`-typed input the seed bytes are decoded structurally; any short byte string is a valid starting point, and the fuzzer grows the corpus from it.)

- [ ] **Step 4: Verify the fuzz workspace compiles**

Run: `cargo check --manifest-path fuzz/Cargo.toml`
Expected: clean build of all four targets. (`fuzz/Cargo.lock` updates locally but is gitignored — do not force-add it.)

If `cargo-fuzz` and a nightly toolchain are available locally, also run a short smoke: `cargo +nightly fuzz run edit_apply -- -max_total_time=30 -timeout=25 -rss_limit_mb=4096`. Expected: no crash. If nightly is not installed, skip this — CI runs it.

- [ ] **Step 5: Wire the target into CI**

In `.github/workflows/fuzz.yml`, after the `docblock` run step, add:

```yaml
      - if: ${{ env.RUN_FUZZ == 'true' }}
        run: cargo +nightly fuzz run edit_apply -- -max_total_time=${{ steps.duration.outputs.seconds }} -timeout=25 -rss_limit_mb=4096
```

(Same shape as the three existing steps: the duration comes from the `duration` step, the timeout flags a hanging input, the RSS limit bounds runaway memory.)

- [ ] **Step 6: Commit (two commits: the target, then the CI wiring)**

```bash
git add fuzz/Cargo.toml fuzz/fuzz_targets/edit_apply.rs fuzz/corpus/edit_apply
git commit -m "✅ test(fuzz): the edit-application target joins the fuzz suite"
git add .github/workflows/fuzz.yml
git commit -m "💚 ci: fuzz the edit-application target"
```

---

### Task 6: Changelog and closure verification

**Files:**
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: everything above.
- Produces: the recorded entry and a fully verified branch.

- [ ] **Step 1: Add the changelog entry**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, append:

```markdown
- The structured-edit library `celerrate_edit`: structured operations on
  the syntax tree — kind-checked token replacement and line-comment
  insertion that reproduces the surrounding indentation — compile into
  the deterministic, sorted, conflict-free `TextEdit` sets suggestions
  transport, and an application primitive splices such a set into
  source text. Overlapping edits are reported errors, never silent
  resolutions, and an edit never touches trivia it was not aimed at. An
  edit-application fuzz target joins the fuzz suite. Library groundwork
  only: no rule emits edits yet and no output changes.
```

- [ ] **Step 2: Run the full local gates**

Run, in order:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git diff --exit-code
cargo deny check
```

Expected: everything green; `git diff --exit-code` proves formatting produced no churn.

- [ ] **Step 3: Run the corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: the diagnostic snapshot matches the committed snapshot byte for byte and the mixed-rate baseline is unchanged — this plan adds a library no analysis path consumes yet, so any delta is a bug in the plan's execution, not a re-blessing candidate.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the structured-edit library"
```

---

## Self-review notes (checked against design section 6)

- **Model** — `replace`, `insert_before` over tokens/nodes: built as `replace_token` and `insert_line_comment_before`; `insert_after`/`delete`/node replacement deliberately deferred per the design's "the API grows only at the pace of shipped fixes" (stated in the header, not silently dropped). Terminal form: sorted, non-overlapping `Vec<TextEdit>` — `finish()` and `apply` both enforce it.
- **Style preservation** — replacements cover exactly the token's range; insertions are pure insertions at the node's first byte with indentation computed from the preceding trivia; tests pin both.
- **Overlaps** — `Result` in `finish()` (`EditConflict`) and `apply` (`ApplyError::Conflict`); the application layer decides. The fuzz target asserts no silent resolution ever survives.
- **Fuzzing (design section 11)** — the edit-application target joins the existing three, with the differential-splice oracle and CI wiring.
- Type-consistency pass done: `EditConflict { first, second }`, `find_conflict(sorted)`, `apply(source, edits)`, `EditBuilder::{new, replace_token, insert_line_comment_before, finish}`, `EditError`/`ApplyError` variants are used with identical shapes across tasks 1-5.
