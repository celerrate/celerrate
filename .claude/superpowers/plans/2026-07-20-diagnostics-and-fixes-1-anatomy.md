# Diagnostics and Fixes, Part 1: The Enriched Diagnostic Anatomy

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enrich the shared diagnostic model with the anchor (span or
project-level), labeled spans, notes, and structured suggestions; add the
`TextEdit` shape to `celerrate_source` and the explain-page store to the
registry; extend the stored-verdict schema accordingly. Pure model
groundwork: no user-visible behavior changes in this part.

**Architecture:** Spec section 3 of
`.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md`.
`TextEdit` lives at the bottom (`celerrate_source`) so diagnostics, the
future `celerrate_edit`, the formatter, and migrations all take it from
below. `Diagnostic` keeps its two mechanical contracts (total deterministic
`Ord`, cheap deterministic `Eq`) while gaining the anchor and the anatomy
vectors. Secondary spans in other files are symbolic (a display path
string), never concrete ranges. The stored-verdict wire format mirrors the
new shape under a bumped `CACHE_SCHEMA_VERSION`, with hostile-input bounds
validation on every stored range. Project notices do NOT move into the
shared stream in this part (that is part 7, where the renderer can render
them); this part only makes the model able to carry them.

**Tech Stack:** Rust, Cargo workspace; `serde` (already a dependency of
`celerrate_cli`) for the stored mirrors; `insta` snapshots untouched.

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code`
  is forbidden. Production code returns `Result`/`Option`. Test modules may
  locally `#[allow]` these lints (existing house pattern).
- Strict layering: `celerrate_source` depends on no workspace crate;
  `celerrate_diagnostics` depends only on `celerrate_source`. No new
  workspace dependencies in this part.
- Determinism: all ordering total and deterministic; no wall-clock, no
  randomness, no environment reads inside queries.
- Everything in English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits, repository-configured identity,
  no Claude attribution of any kind.
- After every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all` (then `git diff --exit-code` to prove no drift).
- Behavior preservation is this part's contract: the committed insta
  snapshots (`crates/celerrate_cli/tests/snapshots/`) must not change, and
  the corpus and mixed-rate gates must match their committed baselines at
  closure (Task 6).

---

### Task 1: `TextEdit` in `celerrate_source`

**Files:**
- Create: `crates/celerrate_source/src/text_edit.rs`
- Modify: `crates/celerrate_source/src/lib.rs` (module + re-export)

**Interfaces:**
- Consumes: `FileId`, `TextRange` (existing, same crate).
- Produces: `celerrate_source::TextEdit { pub file: FileId, pub range:
  TextRange, pub replacement: String }` with total `Ord`. Later tasks and
  parts (`celerrate_diagnostics::Suggestion`, `celerrate_edit`, the autofix
  engine) rely on this exact shape.

- [ ] **Step 1: Write the failing test**

Create `crates/celerrate_source/src/text_edit.rs` with the test module
only (the type does not exist yet, so write the whole file in Step 3; for
the strict red step, add the module declaration first):

In `crates/celerrate_source/src/lib.rs`, after `mod source_text;`:

```rust
mod text_edit;
```

and after the last `pub use`:

```rust
pub use text_edit::TextEdit;
```

Create `crates/celerrate_source/src/text_edit.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crate::{FileId, TextEdit, TextRange, TextSize};

    fn edit(file: u32, start: u32, end: u32, replacement: &str) -> TextEdit {
        TextEdit {
            file: FileId::new(file),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            replacement: replacement.to_owned(),
        }
    }

    #[test]
    fn edits_order_by_file_then_range_then_replacement() {
        let mut edits = [
            edit(1, 0, 1, "b"),
            edit(0, 5, 9, "a"),
            edit(0, 0, 4, "b"),
            edit(0, 0, 4, "a"),
        ];
        edits.sort();
        assert_eq!(
            edits
                .iter()
                .map(|edit| (edit.file.as_u32(), edit.replacement.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "a"), (0, "b"), (0, "a"), (1, "b")],
        );
    }

    #[test]
    fn equal_edits_compare_equal() {
        assert_eq!(edit(0, 0, 1, "x"), edit(0, 0, 1, "x"));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p celerrate_source`
Expected: compile error, `cannot find type TextEdit in this scope` (the
re-export in `lib.rs` has nothing to point at).

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_source/src/text_edit.rs` (above the test
module):

```rust
use crate::{FileId, TextRange};

/// One finalized textual replacement: `replacement` takes the place of
/// `range` in `file`. The terminal, tree-free form every structured edit
/// compiles down to: suggestions transport it, and the application engine
/// consumes it. Defined at the bottom of the workspace so the diagnostics
/// model, the edit library, and later the formatter and migrations all
/// take it from below. Ordering is total and deterministic so edit sets
/// can be sorted, compared, and checked for overlap byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextEdit {
    pub file: FileId,
    pub range: TextRange,
    pub replacement: String,
}

impl Ord for TextEdit {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.file,
            self.range.start(),
            self.range.end(),
            &self.replacement,
        )
            .cmp(&(
                other.file,
                other.range.start(),
                other.range.end(),
                &other.replacement,
            ))
    }
}

impl PartialOrd for TextEdit {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
```

(`TextRange` has no `Ord`, hence the manual tuple implementation, exactly
like `Diagnostic`'s in `crates/celerrate_diagnostics/src/diagnostic.rs`.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_source`
Expected: PASS, including the two new tests.

- [ ] **Step 5: Workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green, no formatting drift.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_source/src/text_edit.rs crates/celerrate_source/src/lib.rs
git commit -m "✨ feat(source): add the finalized TextEdit shape"
```

---

### Task 2: `Label`, `Confidence`, `Suggestion` in `celerrate_diagnostics`

**Files:**
- Create: `crates/celerrate_diagnostics/src/label.rs`
- Create: `crates/celerrate_diagnostics/src/suggestion.rs`
- Modify: `crates/celerrate_diagnostics/src/lib.rs` (modules + re-exports)

**Interfaces:**
- Consumes: `celerrate_source::{TextRange, TextEdit}`.
- Produces (Task 3's `Diagnostic` embeds all of these, so the names and
  shapes are load-bearing):
  - `Label { pub target: LabelTarget, pub message: String }`
  - `LabelTarget::Local { range: TextRange }` (same file as the primary
    span) and `LabelTarget::Symbolic { symbol: String }` (another file or
    a stub declaration, resolved at render time; spec section 3)
  - `Confidence::{Safe, NeedsReview}` (derived `Ord`: `Safe <
    NeedsReview`)
  - `Suggestion { pub message: String, pub confidence: Confidence, pub
    edits: Vec<TextEdit> }`
  - All types implement total `Ord` (required by `Diagnostic`'s `Ord` in
    Task 3).

- [ ] **Step 1: Write the failing tests**

In `crates/celerrate_diagnostics/src/lib.rs`, extend the module list and
re-exports:

```rust
mod diagnostic;
mod identifier;
mod label;
mod registry;
mod severity;
mod suggestion;

pub use diagnostic::Diagnostic;
pub use identifier::DiagnosticId;
pub use label::{Label, LabelTarget};
pub use registry::{REGISTRY, RegisteredDiagnostic, find_identifier};
pub use severity::Severity;
pub use suggestion::{Confidence, Suggestion};
```

Create `crates/celerrate_diagnostics/src/label.rs` containing only the
test module:

```rust
#[cfg(test)]
mod tests {
    use celerrate_source::{TextRange, TextSize};

    use crate::{Label, LabelTarget};

    fn local(start: u32, end: u32, message: &str) -> Label {
        Label {
            target: LabelTarget::Local {
                range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            },
            message: message.to_owned(),
        }
    }

    fn symbolic(symbol: &str, message: &str) -> Label {
        Label {
            target: LabelTarget::Symbolic {
                symbol: symbol.to_owned(),
            },
            message: message.to_owned(),
        }
    }

    #[test]
    fn local_labels_order_before_symbolic_ones() {
        let mut labels = [symbolic("App\\User::save", "declared here"), local(0, 4, "here")];
        labels.sort();
        assert!(matches!(
            labels.first().map(|label| &label.target),
            Some(LabelTarget::Local { .. })
        ));
    }

    #[test]
    fn labels_order_by_target_then_message() {
        let mut labels = [local(0, 4, "beta"), local(0, 4, "alpha"), local(0, 2, "zeta")];
        labels.sort();
        assert_eq!(
            labels
                .iter()
                .map(|label| label.message.as_str())
                .collect::<Vec<_>>(),
            vec!["zeta", "alpha", "beta"],
        );
    }
}
```

Create `crates/celerrate_diagnostics/src/suggestion.rs` containing only
the test module:

```rust
#[cfg(test)]
mod tests {
    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use crate::{Confidence, Suggestion};

    fn suggestion(message: &str, confidence: Confidence) -> Suggestion {
        Suggestion {
            message: message.to_owned(),
            confidence,
            edits: vec![TextEdit {
                file: FileId::new(0),
                range: TextRange::new(TextSize::from(0), TextSize::from(4)),
                replacement: "save".to_owned(),
            }],
        }
    }

    #[test]
    fn safe_orders_below_needs_review() {
        assert!(Confidence::Safe < Confidence::NeedsReview);
    }

    #[test]
    fn suggestions_order_by_message_then_confidence() {
        let mut suggestions = [
            suggestion("beta", Confidence::Safe),
            suggestion("alpha", Confidence::NeedsReview),
            suggestion("alpha", Confidence::Safe),
        ];
        suggestions.sort();
        assert_eq!(
            suggestions
                .iter()
                .map(|suggestion| (suggestion.message.as_str(), suggestion.confidence))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", Confidence::Safe),
                ("alpha", Confidence::NeedsReview),
                ("beta", Confidence::Safe),
            ],
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_diagnostics`
Expected: compile error, `cannot find type Label` (the `lib.rs`
re-exports point at nothing).

- [ ] **Step 3: Write the minimal implementations**

Prepend to `crates/celerrate_diagnostics/src/label.rs`:

```rust
use celerrate_source::TextRange;

/// Where a secondary label points.
///
/// A label in the primary span's own file carries its concrete range. A
/// label in another file (or in a stub, which has no source at all) is
/// carried symbolically: the referenced declaration's display path,
/// resolved to a concrete location at render time, outside queries. The
/// symbolic form is deliberate and load-bearing: a concrete range of
/// another file embedded in a per-file artifact goes stale invisibly, and
/// resolving it inside a query would pierce the range-free invalidation
/// boundary (design section 3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LabelTarget {
    Local { range: TextRange },
    Symbolic { symbol: String },
}

impl Ord for LabelTarget {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        match (self, other) {
            (Self::Local { range: left }, Self::Local { range: right }) => {
                (left.start(), left.end()).cmp(&(right.start(), right.end()))
            }
            (Self::Local { .. }, Self::Symbolic { .. }) => core::cmp::Ordering::Less,
            (Self::Symbolic { .. }, Self::Local { .. }) => core::cmp::Ordering::Greater,
            (Self::Symbolic { symbol: left }, Self::Symbolic { symbol: right }) => {
                left.cmp(right)
            }
        }
    }
}

impl PartialOrd for LabelTarget {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// One secondary annotated span: "the parameter is declared `int` here".
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label {
    pub target: LabelTarget,
    pub message: String,
}
```

(`Label` can derive `Ord` because the field order, `target` then
`message`, is the intended lexicographic key and both fields implement
`Ord`.)

Prepend to `crates/celerrate_diagnostics/src/suggestion.rs`:

```rust
use celerrate_source::TextEdit;

/// How much a suggestion can be trusted. `Safe` is mass-applicable via
/// `celerrate check --fix` and guaranteed not to change semantics;
/// `NeedsReview` is applied only under `--fix-suggestions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Confidence {
    Safe,
    NeedsReview,
}

/// One structured suggestion: a message, a confidence, and the finalized
/// same-file text edits that realize it. Edits target the diagnostic's
/// own file in this sub-project (design section 3); the stored form
/// enforces that structurally by carrying no file identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Suggestion {
    pub message: String,
    pub confidence: Confidence,
    pub edits: Vec<TextEdit>,
}

impl Ord for Suggestion {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (&self.message, self.confidence, &self.edits).cmp(&(
            &other.message,
            other.confidence,
            &other.edits,
        ))
    }
}

impl PartialOrd for Suggestion {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_diagnostics`
Expected: PASS, including the four new tests.

- [ ] **Step 5: Workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_diagnostics/src/label.rs crates/celerrate_diagnostics/src/suggestion.rs crates/celerrate_diagnostics/src/lib.rs
git commit -m "✨ feat(diagnostics): add the label and suggestion vocabulary"
```

---

### Task 3: The anchor, the enriched `Diagnostic`, and the workspace migration

This is one task, not two: the model change and the consumer migration are
one deliverable (a reviewer cannot approve a model that nothing compiles
against). The wire format of the persistent cache does NOT change here
(Task 5 does that); this task adapts `stored.rs` behavior-preservingly.

**Files:**
- Modify: `crates/celerrate_diagnostics/src/diagnostic.rs` (the whole
  model)
- Modify: every `Diagnostic { .. }` construction and `.file`/`.range`
  field access in the workspace. Known sites (the compiler enumerates the
  rest; migrate every one it names):
  - `crates/celerrate_db/src/queries.rs` (decode and parse projection)
  - `crates/celerrate_semantics/src/reference_checks.rs`
  - `crates/celerrate_semantics/src/syntax_gating.rs`
  - `crates/celerrate_semantics/src/queries.rs`
  - `crates/celerrate_types/src/checks/mod.rs` (verdict reconciliation)
  - `crates/celerrate_cli/src/analysis.rs` (`retain_unsuppressed`)
  - `crates/celerrate_cli/src/render.rs` (`render_diagnostic`)
  - `crates/celerrate_cli/src/cache/stored.rs` (`StoredDiagnostic`)
  - test modules alongside each

**Interfaces:**
- Consumes: Task 2's `Label`, `Suggestion`.
- Produces (every later part builds on these exact signatures):
  - `Anchor::Project` and `Anchor::Span { file: FileId, range: TextRange }`
  - `Diagnostic { pub id: DiagnosticId, pub severity: Severity, pub
    anchor: Anchor, pub message: String, pub labels: Vec<Label>, pub
    notes: Vec<String>, pub suggestions: Vec<Suggestion> }`
  - `Diagnostic::spanned(id, severity, file, range, message) -> Diagnostic`
    (anatomy vectors empty; the constructor every existing producer
    migrates to)
  - `Diagnostic::project(id, severity, message) -> Diagnostic`
  - `Diagnostic::span(&self) -> Option<(FileId, TextRange)>` (`None` for
    project-anchored)
  - Total `Ord`: project-anchored findings order before span-anchored
    ones; among span-anchored, the existing key `(file, start, end, id,
    severity, message)` is preserved (then labels, notes, suggestions as
    final tie-breaks), so every committed snapshot and sorted list stays
    byte-identical.

- [ ] **Step 1: Write the failing tests**

Replace the test module of
`crates/celerrate_diagnostics/src/diagnostic.rs` with (the existing four
tests survive, rewritten against the constructor; three tests are new):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, TextRange, TextSize};

    use crate::{Anchor, Diagnostic, DiagnosticId, Severity};

    fn diagnostic(file: u32, start: u32, end: u32, id: &'static str) -> Diagnostic {
        Diagnostic::spanned(
            DiagnosticId::new(id),
            Severity::Error,
            FileId::new(file),
            TextRange::new(TextSize::from(start), TextSize::from(end)),
            String::new(),
        )
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
        let mut diagnostics = [
            diagnostic(1, 0, 1, "CEL0002"),
            diagnostic(0, 5, 9, "CEL0002"),
            diagnostic(0, 0, 4, "CEL0003"),
            diagnostic(0, 0, 4, "CEL0002"),
        ];
        diagnostics.sort();
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.span().map(|(file, _)| file.as_u32()),
                    diagnostic.id.as_str(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (Some(0), "CEL0002"),
                (Some(0), "CEL0003"),
                (Some(0), "CEL0002"),
                (Some(1), "CEL0002")
            ],
        );
    }

    #[test]
    fn equal_diagnostics_compare_equal() {
        assert_eq!(
            diagnostic(0, 0, 1, "CEL0002"),
            diagnostic(0, 0, 1, "CEL0002")
        );
    }

    #[test]
    fn a_project_finding_orders_before_every_span_finding() {
        let mut diagnostics = [
            diagnostic(0, 0, 1, "CEL0002"),
            Diagnostic::project(
                DiagnosticId::new("CEL0025"),
                Severity::Warning,
                "no composer.json found".to_owned(),
            ),
        ];
        diagnostics.sort();
        assert!(matches!(
            diagnostics.first().map(|diagnostic| &diagnostic.anchor),
            Some(Anchor::Project)
        ));
        assert!(diagnostics.first().unwrap().span().is_none());
    }

    #[test]
    fn the_message_is_the_ordering_tie_break_before_the_anatomy() {
        let first = Diagnostic {
            message: "alpha".to_owned(),
            ..diagnostic(0, 0, 1, "CEL9999")
        };
        let second = Diagnostic {
            message: "beta".to_owned(),
            ..first.clone()
        };
        assert!(first < second);
    }

    #[test]
    fn the_anatomy_is_the_final_ordering_tie_break() {
        let bare = diagnostic(0, 0, 1, "CEL9999");
        let annotated = Diagnostic {
            notes: vec!["inferred `string|null` because this path returns `null`".to_owned()],
            ..bare.clone()
        };
        assert!(bare < annotated);
        assert_ne!(bare, annotated);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_diagnostics`
Expected: compile error, `no function or associated item named spanned`.

- [ ] **Step 3: Rewrite the model**

Replace the non-test content of
`crates/celerrate_diagnostics/src/diagnostic.rs` with:

```rust
use celerrate_source::{FileId, TextRange};

use crate::identifier::DiagnosticId;
use crate::label::Label;
use crate::severity::Severity;
use crate::suggestion::Suggestion;

/// Where a diagnostic points.
///
/// Almost every finding has a primary span. A project-level finding (a
/// missing Composer manifest, a version fallback) has none, and anchoring
/// it to a fictional `composer.json:1:1` is forbidden by design; the
/// anchor carries that honestly instead of forcing a fake range.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Anchor {
    /// The whole project: a finding with no honest span. Exit-code
    /// neutral by the notice contract (design section 3).
    Project,
    /// A primary span in one file.
    Span { file: FileId, range: TextRange },
}

impl Anchor {
    /// The deterministic ordering key: project findings first, then
    /// span findings in `(file, start, end)` order, exactly the key the
    /// pre-anatomy model sorted by.
    fn key(&self) -> (u8, u32, u32, u32) {
        match self {
            Self::Project => (0, 0, 0, 0),
            Self::Span { file, range } => (
                1,
                file.as_u32(),
                range.start().into(),
                range.end().into(),
            ),
        }
    }
}

/// One reported finding: a stable identifier, a severity, an anchor, the
/// rendered message, and the rich anatomy (labeled spans, notes,
/// structured suggestions). Ordering is total and deterministic so
/// diagnostic lists can be sorted and compared byte for byte; equality is
/// cheap and deterministic because salsa early cutoff depends on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: DiagnosticId,
    pub severity: Severity,
    pub anchor: Anchor,
    /// The rendered one-sentence message, parameterized by the producer
    /// (the written name, the required version).
    pub message: String,
    /// Secondary annotated spans, local or symbolic (design section 3).
    pub labels: Vec<Label>,
    /// The engine's reasoning, one line each.
    pub notes: Vec<String>,
    /// Structured suggestions with their confidence and same-file edits.
    pub suggestions: Vec<Suggestion>,
}

impl Diagnostic {
    /// The common case: a finding with a primary span and no anatomy
    /// yet. Every pre-anatomy producer constructs through this.
    pub fn spanned(
        id: DiagnosticId,
        severity: Severity,
        file: FileId,
        range: TextRange,
        message: String,
    ) -> Self {
        Self {
            id,
            severity,
            anchor: Anchor::Span { file, range },
            message,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// A project-level finding with no honest span.
    pub fn project(id: DiagnosticId, severity: Severity, message: String) -> Self {
        Self {
            id,
            severity,
            anchor: Anchor::Project,
            message,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// The primary span, if the finding has one. `None` for
    /// project-anchored findings, which no span-keyed machinery
    /// (suppression, per-file persistence) ever touches.
    pub fn span(&self) -> Option<(FileId, TextRange)> {
        match self.anchor {
            Anchor::Project => None,
            Anchor::Span { file, range } => Some((file, range)),
        }
    }
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.anchor.key(),
            self.id,
            self.severity,
            &self.message,
            &self.labels,
            &self.notes,
            &self.suggestions,
        )
            .cmp(&(
                other.anchor.key(),
                other.id,
                other.severity,
                &other.message,
                &other.labels,
                &other.notes,
                &other.suggestions,
            ))
    }
}

impl PartialOrd for Diagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
```

Note the ordering key change relative to the old model: the old key was
`(file, start, end, id, severity, message)`; the new key is
`(anchor.key(), id, severity, message, ...)` where `anchor.key()` for a
span is `(1, file, start, end)`. For any list of span-anchored
diagnostics (which is every list the tool produces today), the relative
order is IDENTICAL. Do not "improve" this.

In `crates/celerrate_diagnostics/src/lib.rs`, extend the diagnostic
re-export:

```rust
pub use diagnostic::{Anchor, Diagnostic};
```

- [ ] **Step 4: Verify the crate is green, then let the compiler drive
  the migration**

Run: `cargo test -p celerrate_diagnostics`
Expected: PASS (all seven tests).

Run: `cargo check --workspace 2>&1 | grep -c "error\["`
Expected: a nonzero count. Every error is one of three shapes; migrate
each site the compiler names, mechanically, changing NO behavior:

1. **Construction** `Diagnostic { id, severity, file, range, message }`
   becomes `Diagnostic::spanned(id, severity, file, range, message)`.
   Example, from `crates/celerrate_semantics/src/syntax_gating.rs`:

   ```rust
   // before
   Diagnostic {
       id: SYNTAX_NOT_AVAILABLE,
       severity: Severity::Error,
       file,
       range,
       message,
   }
   // after
   Diagnostic::spanned(SYNTAX_NOT_AVAILABLE, Severity::Error, file, range, message)
   ```

2. **Span reads** `diagnostic.file` / `diagnostic.range` become a
   destructured `span()`. In span-only code paths, handle the `None` arm
   explicitly and behavior-preservingly, never with `unwrap`:
   - `crates/celerrate_cli/src/analysis.rs` (`retain_unsuppressed`): a
     project-anchored diagnostic is never suppressed:

     ```rust
     let Some((_, range)) = diagnostic.span() else {
         return true;
     };
     ```

     then match on `range.start()` exactly as the existing code does.
   - `crates/celerrate_cli/src/render.rs` (`render_diagnostic`): match
     the anchor; the span arm keeps the existing
     `path:line:column identifier message` format verbatim; the project
     arm reuses the existing notice line shape:

     ```rust
     match diagnostic.anchor {
         Anchor::Span { file, range } => {
             let (line, column) = position(session, file, range.start());
             format!(
                 "{}:{line}:{column} {} {}",
                 display_path(session, file),
                 diagnostic.id.as_str(),
                 diagnostic.message,
             )
         }
         Anchor::Project => format!(
             "notice {}: {}",
             diagnostic.id.as_str(),
             diagnostic.message
         ),
     }
     ```

     (No project-anchored diagnostic is produced anywhere yet; the arm is
     the defensive shape part 7 will replace with the real notice
     rendering.)
   - `crates/celerrate_cli/src/cache/stored.rs`: `StoredDiagnostic::of`
     takes span-anchored diagnostics only in this task (the wire format
     does not change until Task 5). Change its signature to return
     `Option<Self>`:

     ```rust
     pub fn of(diagnostic: &Diagnostic) -> Option<Self> {
         let (_, range) = diagnostic.span()?;
         Some(Self {
             id: diagnostic.id.as_str().to_owned(),
             severity: match diagnostic.severity {
                 Severity::Warning => StoredSeverity::Warning,
                 Severity::Error => StoredSeverity::Error,
             },
             start: range.start().into(),
             end: range.end().into(),
             message: diagnostic.message.clone(),
         })
     }
     ```

     and its callers `filter_map` over it (every persisted diagnostic is
     span-anchored today, so the persisted set is unchanged).
     `to_diagnostic` constructs through `Diagnostic::spanned(...)`; the
     bounds checks stay verbatim.

3. **Struct-update tests** (`..first.clone()` patterns) keep working
   unchanged; test helpers constructing literals move to the
   constructor, as the diagnostics crate's own tests did in Step 1.

Run `cargo check --workspace` repeatedly until zero errors. Do not
suppress a single warning; fix each site.

- [ ] **Step 5: Run the full suite to verify behavior preservation**

Run: `cargo test --workspace`
Expected: PASS. In particular
`crates/celerrate_cli/tests/check.rs` snapshot tests pass UNCHANGED (run
`git status crates/celerrate_cli/tests/snapshots/` and expect no
modification; if a `.pending-snap` appears, the migration changed
behavior: fix the migration, never the snapshot).

- [ ] **Step 6: Workspace gates**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: clippy green; formatting stable.

- [ ] **Step 7: Commit**

```bash
git add -A crates/
git commit -m "✨ feat(diagnostics): the anchor and the rich anatomy join the model"
```

---

### Task 4: The explain-page store

**Files:**
- Create: `crates/celerrate_diagnostics/src/explain.rs`
- Modify: `crates/celerrate_diagnostics/src/registry.rs`
- Modify: `crates/celerrate_diagnostics/src/lib.rs`

**Interfaces:**
- Produces:
  - `ExplainPage { pub why: &'static str, pub failing_example: &'static
    str, pub fixed_example: &'static str, pub configuration: &'static
    str }`
  - `RegisteredDiagnostic` gains `pub explain: Option<&'static
    ExplainPage>` (`None` for every entry in this part; part 8 writes the
    pages, flips the field to mandatory, and adds the executable-page
    harness with its exemption list, per spec section 10)
  - `find_page(id: DiagnosticId) -> Option<&'static ExplainPage>`

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_diagnostics/src/explain.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use crate::{DiagnosticId, ExplainPage, find_page};

    #[test]
    fn no_page_is_registered_yet() {
        assert!(find_page(DiagnosticId::new("CEL0018")).is_none());
        assert!(find_page(DiagnosticId::new("CEL9999")).is_none());
    }

    #[test]
    fn a_page_carries_its_four_sections() {
        static PAGE: ExplainPage = ExplainPage {
            why: "calling an unknown method fails at runtime",
            failing_example: "<?php (new \\DateTime())->fromat('Y');",
            fixed_example: "<?php (new \\DateTime())->format('Y');",
            configuration: "reported by the unknown-members rule",
        };
        assert!(!PAGE.why.is_empty());
        assert!(!PAGE.failing_example.is_empty());
        assert!(!PAGE.fixed_example.is_empty());
        assert!(!PAGE.configuration.is_empty());
    }
}
```

In `crates/celerrate_diagnostics/src/lib.rs`, add `mod explain;` (sorted
into the module list) and:

```rust
pub use explain::ExplainPage;
pub use registry::{REGISTRY, RegisteredDiagnostic, find_identifier, find_page};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_diagnostics`
Expected: compile error, `cannot find type ExplainPage`.

- [ ] **Step 3: Write the minimal implementation**

Prepend to `crates/celerrate_diagnostics/src/explain.rs`:

```rust
/// One identifier's long-form explanation, embedded in the binary and
/// served by `celerrate explain`. Every section is mandatory content-wise
/// at sub-project closure (the composition-root test enforces presence);
/// in this part the store exists and no page is written yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplainPage {
    /// Why the reported pattern is a problem.
    pub why: &'static str,
    /// A minimal example that fires the identifier (executable by the
    /// part 8 harness, unless the identifier is on the declared
    /// environment-condition exemption list; spec section 10).
    pub failing_example: &'static str,
    /// The same example, corrected; must not fire the identifier.
    pub fixed_example: &'static str,
    /// Configuration notes and the owning rule.
    pub configuration: &'static str,
}
```

In `crates/celerrate_diagnostics/src/registry.rs`:

1. Import the page type: `use crate::explain::ExplainPage;`
2. Extend the struct:

   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub struct RegisteredDiagnostic {
       pub id: DiagnosticId,
       pub family: &'static str,
       pub owner: &'static str,
       /// The long-form explanation. `Option` only until part 8 writes
       /// the pages and makes the field mandatory.
       pub explain: Option<&'static ExplainPage>,
   }
   ```

3. Extend the `registered` const constructor with `explain: None` (the
   40 entries stay textually unchanged).
4. Add the lookup beside `find_identifier`:

   ```rust
   /// The explain page registered for `id`, if any is written yet.
   pub fn find_page(id: DiagnosticId) -> Option<&'static ExplainPage> {
       REGISTRY
           .iter()
           .find(|entry| entry.id == id)
           .and_then(|entry| entry.explain)
   }
   ```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_diagnostics`
Expected: PASS.

- [ ] **Step 5: Workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green (the registry composition test at
`crates/celerrate_cli/tests/registry.rs` compiles unchanged: it reads
`id`, `family`, `owner` only).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_diagnostics/
git commit -m "✨ feat(diagnostics): the explain-page store behind the registry"
```

---

### Task 5: The stored-verdict anatomy mirrors and the schema bump

**Files:**
- Modify: `crates/celerrate_cli/src/cache/stored.rs` (`StoredDiagnostic`
  and new mirrors)
- Modify: `crates/celerrate_cli/src/cache/pack.rs:39`
  (`CACHE_SCHEMA_VERSION`)

**Interfaces:**
- Consumes: Task 3's `Anchor`/`Diagnostic`, Task 2's
  `Label`/`LabelTarget`/`Confidence`/`Suggestion`,
  `celerrate_source::TextEdit`.
- Produces: the schema-6 wire shapes. Part 5 (suppression) will extend
  this same file with per-directive match records; part 7 decides
  persistence of project-anchored findings. Exact shapes:
  - `StoredAnchor::{Project, Span { start: u32, end: u32 }}`
  - `StoredLabelTarget::{Local { start: u32, end: u32 }, Symbolic {
    symbol: String }}`, `StoredLabel { target, message }`
  - `StoredConfidence::{Safe, NeedsReview}`
  - `StoredTextEdit { start: u32, end: u32, replacement: String }` (no
    file identity: same-file is structural)
  - `StoredSuggestion { message, confidence, edits: Vec<StoredTextEdit> }`
  - `StoredDiagnostic { id, severity, anchor: StoredAnchor, message,
    labels: Vec<StoredLabel>, notes: Vec<String>, suggestions:
    Vec<StoredSuggestion> }`, `of(&Diagnostic) -> Self` (total again),
    `to_diagnostic(FileId, content_length: u32) -> Option<Diagnostic>`
    with every stored range bounds-checked

- [ ] **Step 1: Write the failing tests**

Add to the test module of `crates/celerrate_cli/src/cache/stored.rs`
(follow the module's existing helper style; the three tests are
self-contained):

```rust
#[test]
fn an_enriched_diagnostic_round_trips_with_its_anatomy() {
    let diagnostic = Diagnostic {
        labels: vec![
            Label {
                target: LabelTarget::Local {
                    range: TextRange::new(TextSize::from(2), TextSize::from(5)),
                },
                message: "declared `int` here".to_owned(),
            },
            Label {
                target: LabelTarget::Symbolic {
                    symbol: "App\\User::save".to_owned(),
                },
                message: "declared here".to_owned(),
            },
        ],
        notes: vec!["inferred `string|null` on this path".to_owned()],
        suggestions: vec![Suggestion {
            message: "did you mean `format`".to_owned(),
            confidence: Confidence::NeedsReview,
            edits: vec![TextEdit {
                file: FileId::new(7),
                range: TextRange::new(TextSize::from(4), TextSize::from(10)),
                replacement: "format".to_owned(),
            }],
        }],
        ..Diagnostic::spanned(
            DiagnosticId::new("CEL0030"),
            Severity::Error,
            FileId::new(7),
            TextRange::new(TextSize::from(4), TextSize::from(10)),
            "unknown method `fromat`".to_owned(),
        )
    };
    let stored = StoredDiagnostic::of(&diagnostic);
    let restored = stored.to_diagnostic(FileId::new(7), 100).unwrap();
    assert_eq!(restored, diagnostic);
}

#[test]
fn hostile_stored_ranges_discard_the_entry() {
    let sound = StoredDiagnostic::of(&Diagnostic::spanned(
        DiagnosticId::new("CEL0030"),
        Severity::Error,
        FileId::new(7),
        TextRange::new(TextSize::from(4), TextSize::from(10)),
        "unknown method".to_owned(),
    ));
    // A label range past the content length.
    let mut hostile_label = sound.clone();
    hostile_label.labels = vec![StoredLabel {
        target: StoredLabelTarget::Local { start: 0, end: 999 },
        message: "here".to_owned(),
    }];
    assert!(hostile_label.to_diagnostic(FileId::new(7), 100).is_none());
    // An inverted edit range.
    let mut hostile_edit = sound.clone();
    hostile_edit.suggestions = vec![StoredSuggestion {
        message: "did you mean `format`".to_owned(),
        confidence: StoredConfidence::NeedsReview,
        edits: vec![StoredTextEdit {
            start: 10,
            end: 4,
            replacement: "format".to_owned(),
        }],
    }];
    assert!(hostile_edit.to_diagnostic(FileId::new(7), 100).is_none());
}

#[test]
fn a_project_anchored_diagnostic_round_trips_without_bounds() {
    let diagnostic = Diagnostic::project(
        DiagnosticId::new("CEL0025"),
        Severity::Warning,
        "no composer.json found".to_owned(),
    );
    let stored = StoredDiagnostic::of(&diagnostic);
    let restored = stored.to_diagnostic(FileId::new(0), 0).unwrap();
    assert_eq!(restored, diagnostic);
}
```

Extend the test module's imports accordingly (`Anchor` is not needed;
`Label`, `LabelTarget`, `Suggestion`, `Confidence` from
`celerrate_diagnostics`, `TextEdit` from `celerrate_source`, and the new
stored types from `super`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli stored`
Expected: compile error (`StoredLabel` does not exist; `of` returns
`Option` from Task 3, so the round-trip test does not type-check).

- [ ] **Step 3: Write the implementation**

In `crates/celerrate_cli/src/cache/stored.rs`, next to `StoredSeverity`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredAnchor {
    Project,
    Span { start: u32, end: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredLabelTarget {
    Local { start: u32, end: u32 },
    Symbolic { symbol: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredLabel {
    pub target: StoredLabelTarget,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredConfidence {
    Safe,
    NeedsReview,
}

/// A stored edit carries no file identity: a suggestion's edits target
/// the diagnostic's own file (design section 3), and the stored form
/// enforces that structurally by having nowhere to write another file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTextEdit {
    pub start: u32,
    pub end: u32,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSuggestion {
    pub message: String,
    pub confidence: StoredConfidence,
    pub edits: Vec<StoredTextEdit>,
}
```

Replace `StoredDiagnostic` (fields, `of`, `to_diagnostic`) with:

```rust
/// One diagnostic with its process-local file identity removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDiagnostic {
    pub id: String,
    pub severity: StoredSeverity,
    pub anchor: StoredAnchor,
    pub message: String,
    pub labels: Vec<StoredLabel>,
    pub notes: Vec<String>,
    pub suggestions: Vec<StoredSuggestion>,
}

impl StoredDiagnostic {
    pub fn of(diagnostic: &Diagnostic) -> Self {
        Self {
            id: diagnostic.id.as_str().to_owned(),
            severity: match diagnostic.severity {
                Severity::Warning => StoredSeverity::Warning,
                Severity::Error => StoredSeverity::Error,
            },
            anchor: match diagnostic.anchor {
                Anchor::Project => StoredAnchor::Project,
                Anchor::Span { range, .. } => StoredAnchor::Span {
                    start: range.start().into(),
                    end: range.end().into(),
                },
            },
            message: diagnostic.message.clone(),
            labels: diagnostic
                .labels
                .iter()
                .map(|label| StoredLabel {
                    target: match &label.target {
                        LabelTarget::Local { range } => StoredLabelTarget::Local {
                            start: range.start().into(),
                            end: range.end().into(),
                        },
                        LabelTarget::Symbolic { symbol } => StoredLabelTarget::Symbolic {
                            symbol: symbol.clone(),
                        },
                    },
                    message: label.message.clone(),
                })
                .collect(),
            notes: diagnostic.notes.clone(),
            suggestions: diagnostic
                .suggestions
                .iter()
                .map(|suggestion| StoredSuggestion {
                    message: suggestion.message.clone(),
                    confidence: match suggestion.confidence {
                        Confidence::Safe => StoredConfidence::Safe,
                        Confidence::NeedsReview => StoredConfidence::NeedsReview,
                    },
                    edits: suggestion
                        .edits
                        .iter()
                        .map(|edit| StoredTextEdit {
                            start: edit.range.start().into(),
                            end: edit.range.end().into(),
                            replacement: edit.replacement.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// `None` when the stored identifier is unknown to the registry, or
    /// when ANY stored range (the anchor's, a local label's, an edit's)
    /// is inverted or reaches past `content_length`. The blake3 checksum
    /// a pack carries proves only that its bytes were not corrupted in
    /// transit, never that whoever wrote them was honest, so every range
    /// is checked here rather than trusted (design section 3).
    pub fn to_diagnostic(&self, file: FileId, content_length: u32) -> Option<Diagnostic> {
        let in_bounds =
            |start: u32, end: u32| start <= end && end <= content_length;
        let anchor = match self.anchor {
            StoredAnchor::Project => Anchor::Project,
            StoredAnchor::Span { start, end } => {
                if !in_bounds(start, end) {
                    return None;
                }
                Anchor::Span {
                    file,
                    range: TextRange::new(TextSize::from(start), TextSize::from(end)),
                }
            }
        };
        let mut labels = Vec::with_capacity(self.labels.len());
        for label in &self.labels {
            let target = match &label.target {
                StoredLabelTarget::Local { start, end } => {
                    if !in_bounds(*start, *end) {
                        return None;
                    }
                    LabelTarget::Local {
                        range: TextRange::new(
                            TextSize::from(*start),
                            TextSize::from(*end),
                        ),
                    }
                }
                StoredLabelTarget::Symbolic { symbol } => LabelTarget::Symbolic {
                    symbol: symbol.clone(),
                },
            };
            labels.push(Label {
                target,
                message: label.message.clone(),
            });
        }
        let mut suggestions = Vec::with_capacity(self.suggestions.len());
        for suggestion in &self.suggestions {
            let mut edits = Vec::with_capacity(suggestion.edits.len());
            for edit in &suggestion.edits {
                if !in_bounds(edit.start, edit.end) {
                    return None;
                }
                edits.push(TextEdit {
                    file,
                    range: TextRange::new(
                        TextSize::from(edit.start),
                        TextSize::from(edit.end),
                    ),
                    replacement: edit.replacement.clone(),
                });
            }
            suggestions.push(Suggestion {
                message: suggestion.message.clone(),
                confidence: match suggestion.confidence {
                    StoredConfidence::Safe => Confidence::Safe,
                    StoredConfidence::NeedsReview => Confidence::NeedsReview,
                },
                edits,
            });
        }
        Some(Diagnostic {
            id: find_identifier(&self.id)?,
            severity: match self.severity {
                StoredSeverity::Warning => Severity::Warning,
                StoredSeverity::Error => Severity::Error,
            },
            anchor,
            message: self.message.clone(),
            labels,
            notes: self.notes.clone(),
            suggestions,
        })
    }
}
```

`of` is total again: revert Task 3's `Option` return and its callers'
`filter_map` back to a plain `map`. Update the file's imports
(`Anchor`, `Label`, `LabelTarget`, `Confidence`, `Suggestion` from
`celerrate_diagnostics`; `TextEdit` from `celerrate_source`). Update the
module documentation comment (`stored.rs:10` mentions "schema 4's
convention"; reword to name the anatomy fields and the every-range
validation).

In `crates/celerrate_cli/src/cache/pack.rs:39`:

```rust
pub const CACHE_SCHEMA_VERSION: u32 = 6;
```

and extend the constant's documentation comment with one line: schema 6
carries the enriched diagnostic anatomy (anchor, labels, notes,
suggestions).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli`
Expected: PASS, including the three new tests and the existing
`a_diagnostic_round_trips_and_an_unknown_identifier_is_rejected` and
`an_empty_range_round_trips` (update their construction sites to the
new field set if the compiler asks; their assertions stay).

- [ ] **Step 5: Workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_cli/src/cache/
git commit -m "✨ feat(cli): the stored verdict carries the enriched anatomy (schema 6)"
```

---

### Task 6: Closure: corpus gates, changelog, ledger

**Files:**
- Modify: `CHANGELOG.md` (Unreleased)
- No source changes: this task is verification.

**Interfaces:**
- Consumes: everything above.
- Produces: the part-1 completion evidence the next plan builds on.

- [ ] **Step 1: Run the corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: `corpus` matches the committed `xtask/corpus-snapshot.txt`
(`0 notices, 0 diagnostics`) and `mixed-rate` matches the committed
baseline. Any delta is a behavior change this part forbids: find it and
fix it; never re-bless in this part.

- [ ] **Step 2: Changelog entry**

In `CHANGELOG.md` under `## [Unreleased]` / `### Changed`, add:

```markdown
- The shared diagnostic model carries the rich anatomy the
  diagnostics-and-fixes design specifies: an anchor that admits
  project-level findings, labeled secondary spans (concrete in the same
  file, symbolic across files), notes, and structured suggestions with
  confidence and finalized text edits. The persistent-cache verdict
  schema moves to 6 and bounds-checks every stored range on load. Model
  groundwork only: no producer emits anatomy yet and no output changes.
```

- [ ] **Step 3: Full verification**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
```

Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the diagnostic anatomy groundwork"
```

---

## Out of scope for this plan (later parts)

- `celerrate_edit` (part 2). The rule framework, contexts, registry
  (part 3). The migration of the six check families (parts 3 and 4).
  Identifier-aware suppression, the native directive, the per-directive
  match records (part 5). The autofix engine and flags (part 6). The
  rich renderer, notices joining the shared stream, watch height cap
  (part 7). Explain pages, the mandatory `explain` field, the executable
  harness and its exemption list (part 8).
- Each part gets its own plan, written when the previous part closes
  (the repository's established cadence).
