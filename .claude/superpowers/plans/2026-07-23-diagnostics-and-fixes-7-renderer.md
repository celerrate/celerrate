# Diagnostics and Fixes Part 7: The Renderer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the CLI's temporary one-line diagnostic format with a rustc-style renderer living in `celerrate_rules` behind a cargo feature, with render-time symbolic label resolution, a fault-injected fallback path, color decided in the CLI, an explain pointer trailer, and a watch-mode height cap.

**Architecture:** A new `render` module in `celerrate_rules` (feature `render`) is a pure function from enriched diagnostics plus sources to text: `adapter.rs` is the only module that references `annotate-snippets`, `resolve.rs` resolves symbolic labels through the semantic queries at render time (outside any salsa query), and `mod.rs` owns the public vocabulary (traits, fault seam, report assembly, minimal fallback). The CLI implements the source-access trait over its `Session`, decides color at the `main` boundary, absorbs per-diagnostic render failures as internal errors, and caps the watch frame at the terminal height.

**Tech Stack:** Rust 1.94 (edition 2024), `annotate-snippets` 0.12 (rust-lang, MIT OR Apache-2.0), `terminal_size` 0.4, `insta` snapshots, salsa 0.27.

**Spec:** `.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md`, section 9 (rendering), plus section 3 (symbolic labels resolved at render time), section 11 (rendering snapshot list), section 12 item 7 (this plan).

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result`. Test modules may locally `#[allow]` these lints (the repository's standard block, see any existing test module).
- `annotate-snippets` is NOT under the workspace zero-panic lints, so every rich block renders in isolation behind `catch_unwind`; a failure falls back to the minimal one-line format plus an internal-error report, never a crash.
- TDD: failing test → minimal implementation → refactor. No production code without a test that demanded it.
- Strict layering: the renderer lives in `celerrate_rules` (it may consult `celerrate_semantics`, `celerrate_db`, `celerrate_source` — all below it). No rule or diagnostic type may reference `annotate-snippets`; the mapping is one module (`adapter.rs`).
- Determinism: no wall-clock, no randomness, no environment reads inside queries. TTY detection, `NO_COLOR`, and terminal size are read in the CLI, outside queries. Snapshots pin the colorless mode.
- The summary line (`N notices, M diagnostics`) stays byte-identical to today. The pinned corpus snapshot is `0 notices, 0 diagnostics` and must not change (`cargo xtask corpus`); the mixed-rate baseline must not change (`cargo xtask mixed-rate`). Both need `cargo xtask fetch-corpus` first.
- Everything in English, full words, no abbreviated names (standard acronyms fine).
- Commits: gitmoji + Conventional Commits (e.g. `✨ feat(rules): render a span diagnostic as a rustc-style block`), authored with the repository-configured identity, no Claude attribution.
- The exit-code contract does not move: 0 clean, 1 any span-anchored diagnostic, 2 internal error. A render fallback produces an internal error, so it exits 2 like every other internal error.

## Design decisions fixed by this plan

1. **Feature name `render`** on `celerrate_rules`, gating the whole `src/render/` module and the optional `annotate-snippets` dependency (`render = ["dep:annotate-snippets"]`). `celerrate_cli` depends on the feature. Precedent: `celerrate_stubs`' `compiler` feature.
2. **`annotate-snippets` 0.12** (latest, the rustc-derived API: `Level::ERROR.primary_title(...)` → `Group`, `Snippet::source(...).path(...).annotation(...)`, `Patch::new(range, replacement)`, `Renderer::plain()` / `Renderer::styled()`).
3. **Project-anchored diagnostics never touch `annotate-snippets`**: they render as the existing notice line `notice CEL####: message` (the notice contract, spec section 3), so they cannot fail and need no fallback.
4. **Symbolic label resolution scope for part 7**: bare class-like and function symbols (`App\User`, `strlen`-shaped) resolve to their declaring file and the declaration's first line; member symbols (`App\User::save`, anything containing `::`), stub-backed, define-origin, and unknown symbols degrade to a note naming the declaration. No rule produces a `LabelTarget::Symbolic` yet (verified: the only non-test construction sites are the cache round-trip), so member-precision resolution is deferred to the first producer and recorded as a stated limitation in the module documentation.
5. **The fault-injection seam is a value, not a hook**: `FaultInjection::ForIdentifier(DiagnosticId)` makes the rich path of the matching diagnostics return an error before touching `annotate-snippets`, exercising exactly the fallback path a real panic takes. It is a normal parameter threaded from the CLI (always `None` in production), so no test-only compilation switches exist.
6. **Watch mode keeps un-enriched diagnostics** (it does not call `suggest::enrich` today, and the autofix design scoped enrichment to the reported one-shot path); this plan does not change that. The watch frame does gain the rich blocks and the height cap.
7. **The renderer's `SourceAccess`/`SymbolResolver` traits keep the core pure**: unit snapshot tests run against in-memory fixtures with no database; only `resolve.rs` (the `DatabaseResolver`) and the CLI wiring touch salsa.

## File Structure

```
Cargo.toml                                      modify: workspace dependencies (annotate-snippets, terminal_size)
.github/workflows/ci.yml                        modify: rules-render clippy/test steps
crates/celerrate_source/src/line_index.rs       modify: add line_range
crates/celerrate_rules/Cargo.toml               modify: [features] render, optional dep, dev-dependencies
crates/celerrate_rules/src/lib.rs               modify: declare the feature-gated module
crates/celerrate_rules/src/render/mod.rs        create: public vocabulary, render_report, render_minimal, explain_pointers
crates/celerrate_rules/src/render/adapter.rs    create: the ONLY module referencing annotate-snippets
crates/celerrate_rules/src/render/resolve.rs    create: DatabaseResolver (render-time symbolic resolution)
crates/celerrate_rules/tests/render.rs          create: colorless rendering snapshot suite
crates/celerrate_rules/tests/snapshots/         create: render__*.snap
crates/celerrate_cli/src/main.rs                modify: color decision (IsTerminal + NO_COLOR)
crates/celerrate_cli/src/lib.rs                 modify: run() gains ColorMode, absorbs render failures
crates/celerrate_cli/src/render.rs              modify: rich report assembly, watch height cap
crates/celerrate_cli/src/session.rs             modify: InternalError::DiagnosticRenderFailed + absorb helper
crates/celerrate_cli/src/watch.rs               modify: terminal height per cycle, threading
crates/celerrate_cli/tests/check.rs             modify: helper signature, normalization, re-blessed snapshots
CHANGELOG.md                                    modify: record the renderer
```

---

### Task 1: The `render` feature and the minimal fallback line

The feature-gated module skeleton, the `SourceAccess` trait, and `render_minimal` — the exact one-line format the preview renderer prints today, which becomes the fallback of every rich block. Includes the CI steps so the feature is linted and tested from its first commit.

**Files:**
- Modify: `crates/celerrate_rules/Cargo.toml`
- Modify: `crates/celerrate_rules/src/lib.rs`
- Create: `crates/celerrate_rules/src/render/mod.rs`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `pub trait SourceAccess { fn display_path(&self, file: FileId) -> Option<String>; fn text(&self, file: FileId) -> Option<&str>; }` and `pub fn render_minimal(diagnostic: &Diagnostic, sources: &dyn SourceAccess) -> String`, both under `celerrate_rules::render` behind `#[cfg(feature = "render")]`. Every later task builds on these.

- [ ] **Step 1: Declare the feature and the module**

In `crates/celerrate_rules/Cargo.toml`, add after `[dependencies]`'s closing entries (the crate has no `[features]` or `[dev-dependencies]` section today):

```toml
[features]
# The rustc-style renderer. Behind a feature so plugin crates and the
# future structured-diagnostics consumers (LSP) do not compile the
# rendering dependency (design section 9).
render = []

[dev-dependencies]
insta = { workspace = true }
```

In `crates/celerrate_rules/src/lib.rs`, add alongside the existing module declarations:

```rust
#[cfg(feature = "render")]
pub mod render;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_rules/src/render/mod.rs` with the module documentation, the trait, and a test module (implementation stubbed just enough to compile is NOT allowed — write the tests first in the same file, watch them fail to compile, then implement):

```rust
//! The rustc-style renderer: a pure function from enriched
//! diagnostics plus sources to text (design section 9).
//!
//! Everything here runs OUTSIDE salsa queries, at presentation time.
//! `adapter.rs` is the only module that references `annotate-snippets`;
//! `resolve.rs` resolves symbolic labels against the database at render
//! time. Color, TTY detection, and terminal size are the CLI's
//! business: this module receives a [`ColorMode`] and never reads the
//! environment.

use celerrate_diagnostics::{Anchor, Diagnostic};
use celerrate_source::{FileId, LineIndex};

/// Read access to the sources a rendered report excerpts. The CLI
/// implements this over its session; tests implement it over fixtures.
pub trait SourceAccess {
    /// The project-relative display path of a file.
    fn display_path(&self, file: FileId) -> Option<String>;
    /// The decoded source text of a file.
    fn text(&self, file: FileId) -> Option<&str>;
}

/// The minimal one-line format: the fallback of every rich block, and
/// the preview format it replaces, byte for byte.
/// `path:line:column identifier message` (one-based), or the notice
/// line for a project-anchored finding.
pub fn render_minimal(diagnostic: &Diagnostic, sources: &dyn SourceAccess) -> String {
    match diagnostic.anchor {
        Anchor::Project => {
            format!("notice {}: {}", diagnostic.id.as_str(), diagnostic.message)
        }
        Anchor::Span { file, range } => {
            let path = sources
                .display_path(file)
                .unwrap_or_else(|| "<unknown>".to_owned());
            let (line, column) = match sources.text(file) {
                Some(text) => {
                    let position = LineIndex::new(text).line_column(range.start());
                    (position.line + 1, position.column + 1)
                }
                None => (1, 1),
            };
            format!(
                "{path}:{line}:{column} {} {}",
                diagnostic.id.as_str(),
                diagnostic.message,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
    use celerrate_source::{FileId, TextRange, TextSize};

    use super::{SourceAccess, render_minimal};

    pub(crate) struct FixtureSources(pub(crate) Vec<(FileId, &'static str, &'static str)>);

    impl SourceAccess for FixtureSources {
        fn display_path(&self, file: FileId) -> Option<String> {
            self.0
                .iter()
                .find(|(id, _, _)| *id == file)
                .map(|(_, path, _)| (*path).to_owned())
        }

        fn text(&self, file: FileId) -> Option<&str> {
            self.0
                .iter()
                .find(|(id, _, _)| *id == file)
                .map(|(_, _, text)| *text)
        }
    }

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::from(start), TextSize::from(end))
    }

    #[test]
    fn a_span_diagnostic_renders_one_based_path_line_and_column() {
        let sources = FixtureSources(vec![(
            FileId::new(0),
            "src/Kernel.php",
            "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n",
        )]);
        let diagnostic = Diagnostic::spanned(
            find_identifier("CEL0018").unwrap(),
            Severity::Error,
            FileId::new(0),
            range(42, 49),
            "unknown class `Missing`".to_owned(),
        );
        assert_eq!(
            render_minimal(&diagnostic, &sources),
            "src/Kernel.php:4:22 CEL0018 unknown class `Missing`",
        );
    }

    #[test]
    fn a_project_diagnostic_renders_the_notice_line() {
        let sources = FixtureSources(vec![]);
        let diagnostic = Diagnostic::project(
            find_identifier("CEL0025").unwrap(),
            Severity::Warning,
            "no composer.json found; analyzing the whole project root".to_owned(),
        );
        assert_eq!(
            render_minimal(&diagnostic, &sources),
            "notice CEL0025: no composer.json found; analyzing the whole project root",
        );
    }

    #[test]
    fn a_missing_source_still_renders_a_line() {
        let sources = FixtureSources(vec![]);
        let diagnostic = Diagnostic::spanned(
            find_identifier("CEL0018").unwrap(),
            Severity::Error,
            FileId::new(7),
            range(0, 1),
            "unknown class `Missing`".to_owned(),
        );
        assert_eq!(
            render_minimal(&diagnostic, &sources),
            "<unknown>:1:1 CEL0018 unknown class `Missing`",
        );
    }
}
```

Note: the byte offsets in the first test target the `Missing` token of the fixture source; verify against the same fixture the committed snapshot `check__findings.snap` pins (`src/Kernel.php:4:22`). If the count is off by a byte, correct the test's range, not the implementation's arithmetic.

- [ ] **Step 3: Run the tests with the feature on**

Run: `cargo test --package celerrate_rules --features render render::`
Expected: the three tests PASS (write the tests before the body if you split the file creation in two edits; the deliverable of this step is three green tests).

Run: `cargo test --package celerrate_rules`
Expected: PASS (the module vanishes without the feature; nothing else changed).

- [ ] **Step 4: Add the CI steps**

In `.github/workflows/ci.yml`, add a job after the `stubs` job (same shape, lines 134-151):

```yaml
  rules-render:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          components: clippy
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo test --package celerrate_rules --features render
```

- [ ] **Step 5: Full check and commit**

Run: `cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings && cargo fmt --all && cargo test --package celerrate_rules --features render`
Expected: clean.

```bash
git add crates/celerrate_rules .github/workflows/ci.yml
git commit -m "✨ feat(rules): add the render feature and the minimal fallback line"
```

---

### Task 2: The rich block adapter — one span diagnostic

`adapter.rs`, the only module referencing `annotate-snippets`: a span-anchored diagnostic becomes a rustc-style block (`error[CEL0018]: message` header, source excerpt, primary underline). Plain and styled renderers selected by `ColorMode`.

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/celerrate_rules/Cargo.toml`
- Modify: `crates/celerrate_rules/src/render/mod.rs`
- Create: `crates/celerrate_rules/src/render/adapter.rs`
- Create: `crates/celerrate_rules/tests/render.rs`

**Interfaces:**
- Consumes: `SourceAccess` from Task 1.
- Produces: `pub enum ColorMode { Plain, Styled }` in `render/mod.rs`; `pub(crate) fn adapter::rich_block(diagnostic: &Diagnostic, path: &str, text: &str, file: FileId, sources: &dyn SourceAccess, resolver: &dyn SymbolResolver, color: ColorMode) -> String` — Tasks 3, 4, 6 extend this exact function. Also `pub trait SymbolResolver` and `pub struct DegradeEverything` (needed by the signature now; the database-backed implementation is Task 6).

- [ ] **Step 1: Add the dependency**

Workspace root `Cargo.toml`, in `[workspace.dependencies]` (alphabetical):

```toml
annotate-snippets = "0.12"
```

`crates/celerrate_rules/Cargo.toml`:

```toml
[dependencies]
annotate-snippets = { workspace = true, optional = true }
# ... existing entries unchanged
```

and change the feature to:

```toml
render = ["dep:annotate-snippets"]
```

Run: `cargo deny check`
Expected: PASS (`annotate-snippets` is MIT OR Apache-2.0; its `anstyle` dependency is already in the tree at 1.0.14 via clap).

- [ ] **Step 2: Add the public vocabulary to `render/mod.rs`**

```rust
/// Whether the rendered text carries ANSI styling. Decided by the CLI
/// (TTY detection, `NO_COLOR`) outside queries; snapshots pin `Plain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Plain,
    Styled,
}

/// A symbolic label resolved at render time (design section 3): a
/// concrete location when the declaration is VFS-backed and locatable,
/// or a degraded form rendered as a note naming the declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedLabel {
    Concrete { file: FileId, range: TextRange },
    Degraded,
}

/// Resolves a symbolic label's declaration path to a location. The
/// database-backed implementation lives in [`resolve`]; tests and
/// database-free callers use [`DegradeEverything`].
pub trait SymbolResolver {
    fn resolve(&self, symbol: &str) -> ResolvedLabel;
}

/// The resolver for contexts with no database at hand: every symbolic
/// label degrades to its note form.
pub struct DegradeEverything;

impl SymbolResolver for DegradeEverything {
    fn resolve(&self, _symbol: &str) -> ResolvedLabel {
        ResolvedLabel::Degraded
    }
}
```

(add `TextRange` to the `celerrate_source` imports), plus the module declaration:

```rust
mod adapter;
```

- [ ] **Step 3: Write the failing snapshot test**

Create `crates/celerrate_rules/tests/render.rs`:

```rust
//! Colorless rendering snapshots (design section 11): the rich block
//! shapes, pinned byte for byte in `ColorMode::Plain`.

#![cfg(feature = "render")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
use celerrate_rules::render::{
    ColorMode, DegradeEverything, FaultInjection, SourceAccess, render_report,
};
use celerrate_source::{FileId, TextRange, TextSize};

struct FixtureSources(Vec<(FileId, &'static str, &'static str)>);

impl SourceAccess for FixtureSources {
    fn display_path(&self, file: FileId) -> Option<String> {
        self.0
            .iter()
            .find(|(id, _, _)| *id == file)
            .map(|(_, path, _)| (*path).to_owned())
    }

    fn text(&self, file: FileId) -> Option<&str> {
        self.0
            .iter()
            .find(|(id, _, _)| *id == file)
            .map(|(_, _, text)| *text)
    }
}

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

const KERNEL: &str = "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n";

fn kernel_diagnostic() -> Diagnostic {
    Diagnostic::spanned(
        find_identifier("CEL0018").unwrap(),
        Severity::Error,
        FileId::new(0),
        range(42, 49),
        "unknown class `Missing`".to_owned(),
    )
}

fn sources() -> FixtureSources {
    FixtureSources(vec![(FileId::new(0), "src/Kernel.php", KERNEL)])
}

#[test]
fn a_span_diagnostic_renders_a_rustc_style_block() {
    let report = render_report(
        &[kernel_diagnostic()],
        &sources(),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("single_span", report.blocks.join("\n\n"));
}
```

NOTE for this step: `render_report` and `FaultInjection` do not exist yet — Task 4 introduces the full report entry point. To keep this task self-contained, add the minimal versions to `render/mod.rs` NOW (Task 4 extends them without changing their signatures):

```rust
/// Forces the rich path of matching diagnostics to fail, so the
/// fallback path is snapshot-tested rather than merely asserted
/// (design section 9). Always [`FaultInjection::None`] in production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultInjection {
    None,
    ForIdentifier(DiagnosticId),
}

/// One diagnostic whose rich rendering failed and fell back to the
/// minimal line. The CLI reports each as an internal error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFailure {
    pub id: DiagnosticId,
    pub location: String,
}

/// The rendered report: one text block per diagnostic, in input
/// order, plus the rich-rendering failures the caller must surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedReport {
    pub blocks: Vec<String>,
    pub failures: Vec<RenderFailure>,
}

/// Renders every diagnostic to its block: the notice line for a
/// project-anchored finding, the rustc-style block for a span-anchored
/// one, the minimal line when rich rendering fails.
pub fn render_report(
    diagnostics: &[Diagnostic],
    sources: &dyn SourceAccess,
    resolver: &dyn SymbolResolver,
    color: ColorMode,
    fault: &FaultInjection,
) -> RenderedReport {
    let mut blocks = Vec::new();
    let mut failures = Vec::new();
    for diagnostic in diagnostics {
        match diagnostic.anchor {
            Anchor::Project => blocks.push(render_minimal(diagnostic, sources)),
            Anchor::Span { file, .. } => {
                let rich = sources.display_path(file).and_then(|path| {
                    let text = sources.text(file)?;
                    adapter::rich_block(
                        diagnostic, &path, text, file, sources, resolver, color, fault,
                    )
                });
                match rich {
                    Some(block) => blocks.push(block),
                    None => {
                        let line = render_minimal(diagnostic, sources);
                        failures.push(RenderFailure {
                            id: diagnostic.id,
                            location: line
                                .split_whitespace()
                                .next()
                                .unwrap_or("<unknown>")
                                .to_owned(),
                        });
                        blocks.push(line);
                    }
                }
            }
        }
    }
    RenderedReport { blocks, failures }
}
```

(import `DiagnosticId`; in this task `rich_block` ignores `fault` and never returns `None` except on missing sources — Task 4 adds the `catch_unwind` and the injected fault.)

Run: `cargo test --package celerrate_rules --features render --test render`
Expected: FAIL — `adapter::rich_block` not found.

- [ ] **Step 4: Implement the adapter**

Create `crates/celerrate_rules/src/render/adapter.rs`. This is the ONLY module in the workspace allowed to `use annotate_snippets`:

```rust
//! The one module that maps the diagnostic anatomy onto
//! `annotate-snippets` input types. Keeping the mapping here keeps the
//! library replaceable: nothing else references it (design section 9).

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::{FileId, TextRange};

use super::{ColorMode, FaultInjection, SourceAccess, SymbolResolver};

/// Renders one span-anchored diagnostic as a rustc-style block.
/// `None` means the rich path failed; the caller falls back to the
/// minimal line and records the failure.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rich_block(
    diagnostic: &Diagnostic,
    path: &str,
    text: &str,
    file: FileId,
    sources: &dyn SourceAccess,
    resolver: &dyn SymbolResolver,
    color: ColorMode,
    fault: &FaultInjection,
) -> Option<String> {
    let _ = (sources, resolver, fault, file);
    Some(build(diagnostic, path, text, color))
}

fn level_of(severity: Severity) -> Level<'static> {
    match severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
    }
}

fn to_usize_range(range: TextRange) -> core::ops::Range<usize> {
    u32::from(range.start()) as usize..u32::from(range.end()) as usize
}

fn build(diagnostic: &Diagnostic, path: &str, text: &str, color: ColorMode) -> String {
    let range = match diagnostic.anchor {
        celerrate_diagnostics::Anchor::Span { range, .. } => range,
        celerrate_diagnostics::Anchor::Project => TextRange::empty(0.into()),
    };
    let snippet = Snippet::source(text)
        .path(path)
        .line_start(1)
        .fold(true)
        .annotation(AnnotationKind::Primary.span(to_usize_range(range)));
    let group = level_of(diagnostic.severity)
        .primary_title(diagnostic.message.as_str())
        .id(diagnostic.id.as_str())
        .element(snippet);
    let renderer = match color {
        ColorMode::Plain => Renderer::plain(),
        ColorMode::Styled => Renderer::styled(),
    };
    renderer.render(&[group]).to_string()
}
```

API note for the implementer: this targets `annotate-snippets` 0.12 (`Level::ERROR.primary_title(...)` returns a `Group`; `.id(...)` prints the bracketed code; `Renderer::plain()`/`Renderer::styled()`; `render(&[Group]) -> impl Display`). If a method name differs at the pinned minor version, consult `cargo doc --package annotate-snippets --open` and adjust the CALL, never the module boundary. The default decor is the ASCII rustc style; do not set `DecorStyle`.

- [ ] **Step 5: Run, inspect, accept the snapshot**

Run: `cargo test --package celerrate_rules --features render --test render`
Expected: FAIL with a new snapshot to review. Inspect `crates/celerrate_rules/tests/snapshots/render__single_span.snap.new`: it must contain the header `error[CEL0018]: unknown class \`Missing\``, an origin line pointing at `src/Kernel.php:4:22`, the excerpted line 4, and a caret underline under `Missing`. Accept by removing the `.new` suffix (verify-then-accept), then re-run.
Expected: PASS.

- [ ] **Step 6: Full check and commit**

Run: `cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace && cargo deny check`
Expected: clean.

```bash
git add Cargo.toml Cargo.lock crates/celerrate_rules
git commit -m "✨ feat(rules): render a span diagnostic as a rustc-style block"
```

---

### Task 3: Anatomy rendering — labels, notes, suggestions, unicode, project anchors

The rich block gains the full anatomy: local labels as secondary underlines, notes as `note:` lines, suggestions as `help:` lines with the rendered replacement as a patch. Snapshot each shape colorless, including unicode content and the project-anchored notice form.

**Files:**
- Modify: `crates/celerrate_rules/src/render/adapter.rs`
- Modify: `crates/celerrate_rules/tests/render.rs`

**Interfaces:**
- Consumes: `rich_block` from Task 2 (same signature).
- Produces: the complete anatomy mapping; Task 6 adds only the symbolic-label arm.

- [ ] **Step 1: Write the failing snapshot tests**

Append to `crates/celerrate_rules/tests/render.rs`:

```rust
const CALLER: &str =
    "<?php\nnamespace App;\n\nfunction render(User $user): void\n{\n    $user->svae();\n}\n";

#[test]
fn labels_notes_and_suggestions_render_in_the_block() {
    use celerrate_diagnostics::{Confidence, Label, LabelTarget, Suggestion};
    use celerrate_source::TextEdit;

    let file = FileId::new(0);
    // `svae` in `$user->svae();` — verify the offsets against CALLER.
    let member = range(58, 62);
    let mut diagnostic = Diagnostic::spanned(
        find_identifier("CEL0030").unwrap(),
        Severity::Error,
        file,
        member,
        "unknown method `svae` on `App\\User`".to_owned(),
    );
    diagnostic.labels.push(Label {
        // `User $user` in the signature — the receiver's declared type.
        target: LabelTarget::Local { range: range(37, 41) },
        message: "the receiver is typed `App\\User` here".to_owned(),
    });
    diagnostic
        .notes
        .push("`App\\User` declares no method or magic accessor named `svae`".to_owned());
    diagnostic.suggestions.push(Suggestion {
        message: "did you mean `save`?".to_owned(),
        confidence: Confidence::NeedsReview,
        edits: vec![TextEdit {
            file,
            range: member,
            replacement: "save".to_owned(),
        }],
    });

    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![(file, "src/Caller.php", CALLER)]),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("anatomy", report.blocks.join("\n\n"));
}

#[test]
fn unicode_content_renders_without_column_drift() {
    // Multi-byte content before and inside the underlined range: the
    // caret must sit under the token, not at its byte offset.
    let source = "<?php\n// café ☕\n$noël = strlen_typo(\"été\");\n";
    let file = FileId::new(0);
    let start = source.find("strlen_typo").unwrap() as u32;
    let diagnostic = Diagnostic::spanned(
        find_identifier("CEL0019").unwrap(),
        Severity::Error,
        file,
        range(start, start + 11),
        "unknown function `strlen_typo`".to_owned(),
    );
    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![(file, "src/unicode.php", source)]),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("unicode", report.blocks.join("\n\n"));
}

#[test]
fn a_project_diagnostic_keeps_the_notice_vocabulary_without_an_excerpt() {
    let diagnostic = Diagnostic::project(
        find_identifier("CEL0025").unwrap(),
        Severity::Warning,
        "no composer.json found; analyzing the whole project root".to_owned(),
    );
    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![]),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    assert_eq!(
        report.blocks,
        vec![
            "notice CEL0025: no composer.json found; analyzing the whole project root"
                .to_owned()
        ],
    );
}
```

Run: `cargo test --package celerrate_rules --features render --test render`
Expected: the project-anchor test PASSES already (Task 2's `render_report` routes it to `render_minimal`); `anatomy` produces a block WITHOUT the label/note/help content (snapshot review shows the gap); treat any snapshot missing them as the failing state.

- [ ] **Step 2: Implement the anatomy mapping**

Replace `build` in `adapter.rs` (the `rich_block` signature is unchanged; it now forwards `resolver` and gathers owned strings before borrowing — the two-phase shape matters because `annotate-snippets` borrows every `&str` until `render` runs):

```rust
use annotate_snippets::Patch;
use celerrate_diagnostics::{Anchor, LabelTarget};

use super::ResolvedLabel;

/// Everything the block borrows must outlive the `render` call, so the
/// owned strings (foreign paths, degraded notes) are gathered first and
/// the `annotate-snippets` structures borrow from this plan.
struct BlockPlan<'s> {
    foreign: Vec<(String, &'s str, TextRange, String)>,
    degraded: Vec<String>,
}

fn plan_labels<'s>(
    diagnostic: &Diagnostic,
    file: FileId,
    sources: &'s dyn SourceAccess,
    resolver: &dyn SymbolResolver,
) -> (Vec<(TextRange, String)>, BlockPlan<'s>) {
    let mut local = Vec::new();
    let mut plan = BlockPlan { foreign: Vec::new(), degraded: Vec::new() };
    for label in &diagnostic.labels {
        match &label.target {
            LabelTarget::Local { range } => local.push((*range, label.message.clone())),
            LabelTarget::Symbolic { symbol } => match resolver.resolve(symbol) {
                ResolvedLabel::Concrete { file: other, range } if other == file => {
                    local.push((range, label.message.clone()));
                }
                ResolvedLabel::Concrete { file: other, range } => {
                    match (sources.display_path(other), sources.text(other)) {
                        (Some(path), Some(text)) => {
                            plan.foreign.push((path, text, range, label.message.clone()));
                        }
                        _ => plan.degraded.push(degraded_note(symbol, &label.message)),
                    }
                }
                ResolvedLabel::Degraded => {
                    plan.degraded.push(degraded_note(symbol, &label.message));
                }
            },
        }
    }
    (local, plan)
}

/// A symbolic label whose declaration has no excerptable source
/// degrades to a note naming the declaration (design section 3).
fn degraded_note(symbol: &str, message: &str) -> String {
    format!("`{symbol}`: {message}")
}

fn build(
    diagnostic: &Diagnostic,
    path: &str,
    text: &str,
    file: FileId,
    sources: &dyn SourceAccess,
    resolver: &dyn SymbolResolver,
    color: ColorMode,
) -> String {
    let range = match diagnostic.anchor {
        Anchor::Span { range, .. } => range,
        Anchor::Project => TextRange::empty(0.into()),
    };
    let (local_labels, plan) = plan_labels(diagnostic, file, sources, resolver);

    let mut snippet = Snippet::source(text)
        .path(path)
        .line_start(1)
        .fold(true)
        .annotation(AnnotationKind::Primary.span(to_usize_range(range)));
    for (label_range, message) in &local_labels {
        snippet = snippet.annotation(
            AnnotationKind::Context
                .span(to_usize_range(*label_range))
                .label(message.as_str()),
        );
    }

    let mut group = level_of(diagnostic.severity)
        .primary_title(diagnostic.message.as_str())
        .id(diagnostic.id.as_str())
        .element(snippet);

    for (foreign_path, foreign_text, foreign_range, message) in &plan.foreign {
        group = group.element(
            Snippet::source(*foreign_text)
                .path(foreign_path.as_str())
                .line_start(1)
                .fold(true)
                .annotation(
                    AnnotationKind::Context
                        .span(to_usize_range(*foreign_range))
                        .label(message.as_str()),
                ),
        );
    }
    for note in &plan.degraded {
        group = group.element(Level::NOTE.message(note.as_str()));
    }
    for note in &diagnostic.notes {
        group = group.element(Level::NOTE.message(note.as_str()));
    }
    for suggestion in &diagnostic.suggestions {
        group = group.element(Level::HELP.message(suggestion.message.as_str()));
        let same_file_edits: Vec<_> = suggestion
            .edits
            .iter()
            .filter(|edit| edit.file == file)
            .collect();
        if !same_file_edits.is_empty() {
            let mut patched = Snippet::source(text).path(path).line_start(1).fold(true);
            for edit in same_file_edits {
                patched = patched.patch(Patch::new(
                    to_usize_range(edit.range),
                    edit.replacement.as_str(),
                ));
            }
            group = group.element(patched);
        }
    }

    let renderer = match color {
        ColorMode::Plain => Renderer::plain(),
        ColorMode::Styled => Renderer::styled(),
    };
    renderer.render(&[group]).to_string()
}
```

and update `rich_block` to call `build(diagnostic, path, text, file, sources, resolver, color)`. If the 0.12 type system rejects mixing `.annotation` and `.patch` builders in the way written here (a `Snippet<Annotation>` versus `Snippet<Patch>` split), keep patches on their own snippet exactly as written — that is already the shape above — and adjust only the builder generics.

- [ ] **Step 3: Run, inspect, accept**

Run: `cargo test --package celerrate_rules --features render --test render`
Expected: `anatomy` and `unicode` produce new snapshots. Inspect: the anatomy block must show the secondary underline labeled `the receiver is typed \`App\\User\` here`, a `note:` line, a `help: did you mean \`save\`?` line, and the patch rendering of `save`; the unicode block's caret must align under `strlen_typo` despite the multi-byte line above. Accept, re-run, PASS.

- [ ] **Step 4: Full check and commit**

Run: `cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_rules
git commit -m "✨ feat(rules): render labels, notes, and suggestions in rich blocks"
```

---

### Task 4: Panic isolation and the fault-injection seam

`annotate-snippets` is outside the zero-panic lints, so every rich block is wrapped in `catch_unwind`; a panic or an injected fault falls back to the minimal line and records a `RenderFailure`. The mixed rich-and-minimal output is snapshot-tested through the seam.

**Files:**
- Modify: `crates/celerrate_rules/src/render/adapter.rs`
- Modify: `crates/celerrate_rules/tests/render.rs`

**Interfaces:**
- Consumes: `rich_block`, `render_report`, `FaultInjection`, `RenderFailure` from Tasks 2-3 (signatures unchanged).
- Produces: `rich_block` returning `None` on injected fault or caught panic; `render_report` therefore emitting the minimal line plus a failure record — the exact contract Task 9's CLI wiring consumes.

- [ ] **Step 1: Write the failing test**

Append to `crates/celerrate_rules/tests/render.rs`:

```rust
#[test]
fn an_injected_fault_falls_back_to_the_minimal_line_for_that_diagnostic_only() {
    use celerrate_diagnostics::DiagnosticId;

    let faulted: DiagnosticId = find_identifier("CEL0018").unwrap();
    let second = Diagnostic::spanned(
        find_identifier("CEL0019").unwrap(),
        Severity::Error,
        FileId::new(0),
        range(42, 49),
        "unknown function `Missing`".to_owned(),
    );
    let report = render_report(
        &[kernel_diagnostic(), second],
        &sources(),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::ForIdentifier(faulted),
    );
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].id, faulted);
    assert_eq!(report.failures[0].location, "src/Kernel.php:4:22");
    // Mixed output: the faulted diagnostic is one minimal line, the
    // other keeps its rich block.
    insta::assert_snapshot!("mixed_fallback", report.blocks.join("\n\n"));
}
```

Run: `cargo test --package celerrate_rules --features render --test render an_injected_fault`
Expected: FAIL — both blocks render rich, `failures` is empty (the seam is not honored yet).

- [ ] **Step 2: Implement the seam and the catch_unwind belt**

In `adapter.rs`, replace `rich_block`'s body:

```rust
use std::panic::{AssertUnwindSafe, catch_unwind};

#[allow(clippy::too_many_arguments)]
pub(crate) fn rich_block(
    diagnostic: &Diagnostic,
    path: &str,
    text: &str,
    file: FileId,
    sources: &dyn SourceAccess,
    resolver: &dyn SymbolResolver,
    color: ColorMode,
    fault: &FaultInjection,
) -> Option<String> {
    if let FaultInjection::ForIdentifier(id) = fault {
        if *id == diagnostic.id {
            return None;
        }
    }
    // `annotate-snippets` is not under the workspace zero-panic lints,
    // so one diagnostic's rendering panic must not take the report
    // down: it falls back to the minimal line (design section 9).
    catch_unwind(AssertUnwindSafe(|| {
        build(diagnostic, path, text, file, sources, resolver, color)
    }))
    .ok()
}
```

- [ ] **Step 3: Run, inspect, accept**

Run: `cargo test --package celerrate_rules --features render --test render`
Expected: the new snapshot shows `src/Kernel.php:4:22 CEL0018 unknown class \`Missing\`` as a bare line next to the rich CEL0019 block. Accept, re-run, PASS. All earlier snapshots unchanged.

- [ ] **Step 4: Full check and commit**

Run: `cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_rules
git commit -m "✨ feat(rules): isolate rich rendering behind a fault-injection seam"
```

---

### Task 5: `LineIndex::line_range`

`celerrate_source::LineIndex` exposes no way to get a line's range; the symbolic resolver (Task 6) needs it to clip a declaration to its first line.

**Files:**
- Modify: `crates/celerrate_source/src/line_index.rs`

**Interfaces:**
- Produces: `pub fn line_range(&self, line: u32) -> Option<TextRange>` — the range of one zero-based line INCLUDING its terminator; the last line runs to the end of the text; `None` when the line does not exist. Task 6 consumes it.

- [ ] **Step 1: Write the failing tests**

In the existing test module of `crates/celerrate_source/src/line_index.rs`:

```rust
#[test]
fn line_range_covers_a_middle_line_including_its_terminator() {
    let index = LineIndex::new("<?php\nclass A\n{\n}\n");
    let range = index.line_range(1).unwrap();
    assert_eq!(u32::from(range.start()), 6);
    assert_eq!(u32::from(range.end()), 14); // "class A\n"
}

#[test]
fn line_range_of_the_last_line_runs_to_the_end_of_the_text() {
    let index = LineIndex::new("a\nb");
    let range = index.line_range(1).unwrap();
    assert_eq!(u32::from(range.start()), 2);
    assert_eq!(u32::from(range.end()), 3);
}

#[test]
fn line_range_of_a_missing_line_is_none() {
    let index = LineIndex::new("a\nb");
    assert_eq!(index.line_range(2), None);
}
```

Run: `cargo test --package celerrate_source line_range`
Expected: FAIL — method not found.

- [ ] **Step 2: Implement**

```rust
/// The range of one zero-based line, including its terminator; the
/// last line runs to the end of the text. `None` when the line does
/// not exist.
pub fn line_range(&self, line: u32) -> Option<TextRange> {
    let start = *self.line_starts.get(line as usize)?;
    let end = match self.line_starts.get(line as usize + 1) {
        Some(next) => *next,
        None => self.len,
    };
    Some(TextRange::new(start, end))
}
```

- [ ] **Step 3: Run and commit**

Run: `cargo test --package celerrate_source && cargo clippy --package celerrate_source --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_source
git commit -m "✨ feat(source): expose a line's range from the line index"
```

---

### Task 6: Render-time symbolic label resolution

`resolve.rs`: the `DatabaseResolver` resolves a symbolic label's declaration path to its declaring file and the declaration's first line, through the semantic lookups, OUTSIDE any salsa query. Stub-backed, define-origin, member, and unknown symbols degrade.

**Files:**
- Create: `crates/celerrate_rules/src/render/resolve.rs`
- Modify: `crates/celerrate_rules/src/render/mod.rs` (declare `pub mod resolve;` inside the render module)
- Modify: `crates/celerrate_rules/tests/render.rs` (a report-level snapshot with a symbolic label)

**Interfaces:**
- Consumes: `SymbolResolver`/`ResolvedLabel` (Task 2), `LineIndex::line_range` (Task 5), and the semantic surface verified in the codebase: `celerrate_semantics::{SymbolQuery, SymbolSpace, folded_symbol_key, lookup_class_declaration, lookup_function_declaration, analyzed_file_index, ast_id_map}` (see `crates/celerrate_semantics/src/lookup.rs:88-119`, `lookup.rs:70-82`, `symbols.rs:49-68`; the `AstId → range` chain is the same one `phases.rs:280-313` uses).
- Produces: `pub struct DatabaseResolver<'db>` with `pub fn new(db: &'db dyn salsa::Database, files: AnalyzedFileSet) -> Self`, implementing `SymbolResolver`. Task 9's CLI wiring constructs it from `session.database` and `session.files`.

- [ ] **Step 1: Write the failing unit tests**

Create `crates/celerrate_rules/src/render/resolve.rs` starting with the tests (the fixture pattern is the one `rules/test_support.rs:80-96` uses, inlined because that helper is scoped to the rules modules):

```rust
#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_source::FileId;

    use super::DatabaseResolver;
    use crate::render::{ResolvedLabel, SymbolResolver};

    const DECLARING: &str =
        "<?php\nnamespace App;\n\nclass User\n{\n    public function save(): void {}\n}\n";

    fn fixture() -> (TestDatabase, AnalyzedFileSet) {
        let db = TestDatabase::default();
        let declaring =
            SourceFile::new(&db, FileId::new(0), DECLARING.as_bytes().to_vec());
        let files = AnalyzedFileSet::new(&db, vec![declaring]);
        (db, files)
    }

    #[test]
    fn a_source_class_resolves_to_the_first_line_of_its_declaration() {
        let (db, files) = fixture();
        let resolver = DatabaseResolver::new(&db, files);
        match resolver.resolve("App\\User") {
            ResolvedLabel::Concrete { file, range } => {
                assert_eq!(file, FileId::new(0));
                let start = u32::from(range.start()) as usize;
                let end = u32::from(range.end()) as usize;
                assert_eq!(&DECLARING[start..end], "class User");
            }
            ResolvedLabel::Degraded => panic!("a source class must resolve"),
        }
    }

    #[test]
    fn a_member_symbol_degrades_in_this_sub_project() {
        let (db, files) = fixture();
        let resolver = DatabaseResolver::new(&db, files);
        assert!(matches!(
            resolver.resolve("App\\User::save"),
            ResolvedLabel::Degraded
        ));
    }

    #[test]
    fn an_unknown_symbol_degrades() {
        let (db, files) = fixture();
        let resolver = DatabaseResolver::new(&db, files);
        assert!(matches!(
            resolver.resolve("App\\Missing"),
            ResolvedLabel::Degraded
        ));
    }
}
```

Run: `cargo test --package celerrate_rules --features render resolve::`
Expected: FAIL — `DatabaseResolver` not defined.

- [ ] **Step 2: Implement the resolver**

Above the test module in `resolve.rs`:

```rust
//! Render-time resolution of symbolic labels (design section 3): a
//! concrete range of another file must never enter a memoized per-file
//! artifact, so the stored form is the declaration's display path and
//! THIS module turns it into a location, at render time, outside
//! queries.
//!
//! Part 7 scope: bare class-like and function symbols resolve; member
//! symbols (`Class::member`), stub-backed, define-origin, and unknown
//! symbols degrade to the note form. Member precision arrives with the
//! first rule that emits a member label.

use celerrate_db::AnalyzedFileSet;
use celerrate_semantics::{
    AstId, SymbolQuery, SymbolSpace, analyzed_file_index, ast_id_map, folded_symbol_key,
    lookup_class_declaration, lookup_function_declaration,
};
use celerrate_source::{FileId, TextRange, TextSize};

use super::{ResolvedLabel, SymbolResolver};

/// The database-backed resolver the CLI wires at the composition root.
pub struct DatabaseResolver<'db> {
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
}

impl<'db> DatabaseResolver<'db> {
    pub fn new(db: &'db dyn salsa::Database, files: AnalyzedFileSet) -> Self {
        Self { db, files }
    }

    fn declaration_of(&self, symbol: &str) -> Option<AstId> {
        let class_query = SymbolQuery::new(
            self.db,
            SymbolSpace::ClassLike,
            folded_symbol_key(SymbolSpace::ClassLike, symbol),
        );
        if let Some((_, ast_id)) = lookup_class_declaration(self.db, self.files, class_query)
        {
            return Some(ast_id);
        }
        let function_query = SymbolQuery::new(
            self.db,
            SymbolSpace::Function,
            folded_symbol_key(SymbolSpace::Function, symbol),
        );
        lookup_function_declaration(self.db, self.files, function_query)
    }

    fn first_line_of(&self, ast_id: AstId) -> Option<(FileId, TextRange)> {
        let index = analyzed_file_index(self.db, self.files);
        let (_, source_file) = index.iter().find(|(file, _)| *file == ast_id.file)?;
        let map = ast_id_map(self.db, *source_file);
        let pointer = map.pointer(ast_id.index)?;
        let root = celerrate_db::parse(self.db, *source_file).tree();
        let node_range = pointer.try_to_node(&root)?.text_range();
        let line_index = celerrate_db::line_index(self.db, *source_file);
        let line = line_index.line_column(node_range.start()).line;
        let line_range = line_index.line_range(line)?;
        let text = celerrate_db::source_text(self.db, *source_file)
            .as_ref()
            .ok()?
            .text();
        Some((ast_id.file, clip_to_line(node_range, line_range, text)))
    }
}

impl SymbolResolver for DatabaseResolver<'_> {
    fn resolve(&self, symbol: &str) -> ResolvedLabel {
        if symbol.contains("::") {
            return ResolvedLabel::Degraded;
        }
        match self
            .declaration_of(symbol)
            .and_then(|ast_id| self.first_line_of(ast_id))
        {
            Some((file, range)) => ResolvedLabel::Concrete { file, range },
            None => ResolvedLabel::Degraded,
        }
    }
}

/// A whole-declaration underline would span the class body; the label
/// points at the declaration, so its first line carries the meaning.
fn clip_to_line(node: TextRange, line: TextRange, text: &str) -> TextRange {
    let end = node.end().min(line.end());
    if end <= node.start() {
        return node;
    }
    let start_usize = u32::from(node.start()) as usize;
    let end_usize = u32::from(end) as usize;
    let Some(slice) = text.get(start_usize..end_usize) else {
        return node;
    };
    let trimmed = slice.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return node;
    }
    TextRange::new(node.start(), node.start() + TextSize::of(trimmed))
}
```

In `render/mod.rs`, add `pub mod resolve;`. If `AstId`, `analyzed_file_index`, or `ast_id_map` are not re-exported at the `celerrate_semantics` root, import them from their modules exactly as `crates/celerrate_rules/src/phases.rs` does (it already calls `celerrate_semantics::ast_id_map`).

Run: `cargo test --package celerrate_rules --features render resolve::`
Expected: PASS. If the first test's slice is not `"class User"`, print the resolved slice, check whether the declaration node starts at a modifier or attribute, and fix the TEST's expectation to the first line of the actual node — the clipping contract is "first line of the declaration node", not a specific keyword.

- [ ] **Step 3: A report-level snapshot with symbolic labels (degraded form)**

Append to `crates/celerrate_rules/tests/render.rs` (this pins the degraded note shape end to end; the concrete cross-file shape is covered by the resolver unit tests plus the adapter's foreign-snippet arm, which shares its code path with the same-file arm already snapshotted):

```rust
#[test]
fn a_symbolic_label_with_no_source_degrades_to_a_note_naming_the_declaration() {
    use celerrate_diagnostics::{Label, LabelTarget};

    let mut diagnostic = kernel_diagnostic();
    diagnostic.labels.push(Label {
        target: LabelTarget::Symbolic { symbol: "App\\User::save".to_owned() },
        message: "the method is declared here".to_owned(),
    });
    let report = render_report(
        &[diagnostic],
        &sources(),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("degraded_symbolic", report.blocks.join("\n\n"));
}
```

Run, inspect (the block must contain `note: \`App\\User::save\`: the method is declared here`), accept, re-run.
Expected: PASS.

- [ ] **Step 4: Full check and commit**

Run: `cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`
Expected: clean.

```bash
git add crates/celerrate_rules
git commit -m "✨ feat(rules): resolve symbolic labels at render time"
```

---

### Task 7: The explain pointer trailer

`explain_pointers`: one rustc-style pointer per distinct identifier reported, sorted and deduplicated, appended at the end of the report by the CLI (Task 9). Pure string work, lives in `render/mod.rs`.

**Files:**
- Modify: `crates/celerrate_rules/src/render/mod.rs`

**Interfaces:**
- Produces: `pub fn explain_pointers(identifiers: impl IntoIterator<Item = DiagnosticId>) -> String` — empty string for no identifiers; otherwise newline-terminated lines `for more information, run \`celerrate explain CEL####\``. Task 9 feeds it the union of notice and diagnostic identifiers.

- [ ] **Step 1: Write the failing tests**

In `render/mod.rs`'s test module:

```rust
#[test]
fn explain_pointers_are_sorted_deduplicated_and_newline_terminated() {
    use super::explain_pointers;
    let identifiers = [
        find_identifier("CEL0030").unwrap(),
        find_identifier("CEL0018").unwrap(),
        find_identifier("CEL0030").unwrap(),
    ];
    assert_eq!(
        explain_pointers(identifiers),
        "for more information, run `celerrate explain CEL0018`\n\
         for more information, run `celerrate explain CEL0030`\n",
    );
}

#[test]
fn no_identifiers_produce_no_pointer_text() {
    use super::explain_pointers;
    assert_eq!(explain_pointers([]), "");
}
```

Run: `cargo test --package celerrate_rules --features render explain_pointers`
Expected: FAIL — function not found.

- [ ] **Step 2: Implement**

```rust
/// The report trailer that makes `celerrate explain` discoverable from
/// the primary output (design section 9): one pointer per distinct
/// identifier reported, in identifier order.
pub fn explain_pointers(identifiers: impl IntoIterator<Item = DiagnosticId>) -> String {
    let mut seen: Vec<DiagnosticId> = identifiers.into_iter().collect();
    seen.sort();
    seen.dedup();
    seen.iter()
        .map(|id| format!("for more information, run `celerrate explain {}`\n", id.as_str()))
        .collect()
}
```

- [ ] **Step 3: Run and commit**

Run: `cargo test --package celerrate_rules --features render && cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_rules
git commit -m "✨ feat(rules): point each reported identifier at celerrate explain"
```

---

### Task 8: CLI color mode — TTY detection and NO_COLOR

Color is decided once, in `main`, outside queries: styled only when stdout is a terminal AND `NO_COLOR` is absent or empty. `run` gains the `ColorMode` parameter; every test passes `Plain`, so all snapshots stay colorless.

**Files:**
- Modify: `crates/celerrate_cli/Cargo.toml` (the `celerrate_rules` dependency gains the feature)
- Modify: `crates/celerrate_cli/src/lib.rs`
- Modify: `crates/celerrate_cli/src/main.rs`
- Modify: every `run(` call site in `crates/celerrate_cli/tests/` (mechanical)

**Interfaces:**
- Consumes: `celerrate_rules::render::ColorMode` (Task 2).
- Produces: `pub fn run(arguments: Vec<OsString>, output: &mut dyn Write, color: ColorMode) -> Outcome` (was `run(arguments, output)`, `lib.rs:70`); `pub use celerrate_rules::render::ColorMode;` re-exported from `celerrate_cli`; `pub fn color_mode(stdout_is_terminal: bool, no_color: Option<&std::ffi::OsStr>) -> ColorMode` (pure, unit-tested). Tasks 9-10 thread `color` further.

- [ ] **Step 1: Enable the feature**

In `crates/celerrate_cli/Cargo.toml`:

```toml
celerrate_rules = { path = "../celerrate_rules", features = ["render"] }
```

- [ ] **Step 2: Write the failing tests**

In `crates/celerrate_cli/src/lib.rs`'s test module (or create the block in the pattern of the existing unit tests):

```rust
#[test]
fn color_is_styled_only_on_a_terminal_without_no_color() {
    use std::ffi::OsStr;
    assert_eq!(color_mode(true, None), ColorMode::Styled);
    assert_eq!(color_mode(false, None), ColorMode::Plain);
    assert_eq!(color_mode(true, Some(OsStr::new("1"))), ColorMode::Plain);
    // The NO_COLOR convention: an empty value does not disable color.
    assert_eq!(color_mode(true, Some(OsStr::new(""))), ColorMode::Styled);
}
```

Run: `cargo test --package celerrate_cli color_is_styled`
Expected: FAIL — `color_mode` not found.

- [ ] **Step 3: Implement and thread the parameter**

In `lib.rs`:

```rust
pub use celerrate_rules::render::ColorMode;

/// The color decision, pure so it is testable: styled only on a
/// terminal with `NO_COLOR` unset or empty (the no-color.org
/// convention). Read once in `main`, outside queries.
pub fn color_mode(
    stdout_is_terminal: bool,
    no_color: Option<&std::ffi::OsStr>,
) -> ColorMode {
    let disabled = no_color.is_some_and(|value| !value.is_empty());
    if stdout_is_terminal && !disabled {
        ColorMode::Styled
    } else {
        ColorMode::Plain
    }
}

pub fn run(arguments: Vec<OsString>, output: &mut dyn Write, color: ColorMode) -> Outcome {
```

`color` is unused inside `run` until Task 9: pass it through to the check arm now as `let _ = color;` is FORBIDDEN (dead parameter smell) — instead store it in scope and hand it to `render::render_report`/`watch::watch` in Task 9; for THIS task, thread it to nothing yet but change the signature and every caller, and add `#[allow(unused_variables)]` is also forbidden. The clean cut: do Steps 3 and 4 of this task, then Task 9 immediately consumes the parameter — if the compiler flags the unused parameter in the interim, name it `_color` in this task and rename in Task 9 (one word of churn, no lint suppression).

In `main.rs`:

```rust
use std::io::IsTerminal;

fn main() -> std::process::ExitCode {
    let color = celerrate_cli::color_mode(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").as_deref(),
    );
    let mut output = std::io::stdout().lock();
    celerrate_cli::run(std::env::args_os().collect(), &mut output, color).exit_code()
}
```

(match the existing `main.rs` shape at `main.rs:10-13`; only the two new lines differ.)

- [ ] **Step 4: Update every test call site**

Find them: `grep -rn "run(" crates/celerrate_cli/tests/ | grep -v "//"`. Each `celerrate_cli::run(arguments, &mut output)` becomes `celerrate_cli::run(arguments, &mut output, celerrate_cli::ColorMode::Plain)` (most sit inside one `check(...)`/`run_check(...)` helper per test file — update the helper, not each test).

- [ ] **Step 5: Run and commit**

Run: `cargo test --package celerrate_cli && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS — behavior unchanged (the parameter is threaded, not yet consumed).

```bash
git add Cargo.lock crates/celerrate_cli
git commit -m "✨ feat(cli): decide color from the terminal and NO_COLOR"
```

---

### Task 9: The CLI serves the rustc-style report

`render_report` in the CLI is rebuilt on the rules renderer: notices unchanged, rich blocks separated by blank lines, the summary line byte-identical, the explain trailer, render failures absorbed as internal errors. Existing CLI snapshots are re-blessed under verify-then-accept; the corpus gate proves the clean-run output did not move.

**Files:**
- Modify: `crates/celerrate_cli/src/render.rs`
- Modify: `crates/celerrate_cli/src/session.rs`
- Modify: `crates/celerrate_cli/src/lib.rs`
- Modify: `crates/celerrate_cli/src/watch.rs` (mechanical threading only; the cap is Task 10)
- Modify: `crates/celerrate_cli/tests/check.rs` (normalization helper + re-blessed snapshots)
- Modify: `crates/celerrate_cli/tests/snapshots/*.snap` (re-blessed)

**Interfaces:**
- Consumes: `celerrate_rules::render::{render_report, explain_pointers, RenderFailure, FaultInjection, ColorMode, SourceAccess, resolve::DatabaseResolver}` (Tasks 2-7); `Session` fields `database`, `files`, `sources`, `vfs` (`session.rs:79-113`).
- Produces: `pub fn render_report(output: &mut dyn Write, session: &Session, outcome: &AnalysisOutcome, color: ColorMode) -> io::Result<Vec<RenderFailure>>` (was `io::Result<()>`, `render.rs:48`); `pub(crate) fn render_report_with(..., fault: &FaultInjection) -> io::Result<Vec<RenderFailure>>` (the seam the fault tests use); `Session::absorb_render_failures(&mut self, failures: Vec<RenderFailure>)`; `InternalError::DiagnosticRenderFailed { identifier: String, location: String }`. Task 10 reuses the same assembly for the watch frame.

- [ ] **Step 1: Write the failing end-to-end test**

In `crates/celerrate_cli/tests/check.rs`, the existing snapshot test `a_project_with_findings_renders_notices_diagnostics_and_a_summary` (check.rs:67-78) is the driver: it will fail with the new rich output the moment the implementation lands, and the re-bless IS the review. Before touching the implementation, extend the normalization helper (check.rs:41-51) so rich origin lines survive Windows:

```rust
/// Path separators normalize in the two places a path can appear: the
/// leading `path:line:column` token of a minimal line, and the
/// ` --> path:line:column` origin line of a rich block. PHP source
/// excerpts keep their backslashes (namespaces are not paths).
fn normalize_location_separators(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("--> ") {
            let indent = &line[..line.len() - trimmed.len()];
            lines.push(format!("{indent}--> {}", rest.replace('\\', "/")));
        } else {
            // The existing leading-token rule, unchanged.
            lines.push(normalize_leading_token(line));
        }
    }
    let mut result = lines.join("\n");
    if text.ends_with('\n') {
        result.push('\n');
    }
    result
}
```

(`normalize_leading_token` is the current body of the helper, extracted verbatim; keep its logic byte-identical. The `line[..len]` slice sits in a test file whose module already allows `indexing_slicing`.)

Add the explain-trailer expectation as a substring test:

```rust
#[test]
fn the_report_points_at_celerrate_explain_for_each_reported_identifier() {
    let root = project(&[(
        "src/Kernel.php",
        "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    assert!(
        text.contains("for more information, run `celerrate explain CEL0018`"),
        "the trailer names the reported identifier: {text}",
    );
    assert!(
        text.contains("for more information, run `celerrate explain CEL0025`"),
        "the trailer also names the notice identifier: {text}",
    );
}

#[test]
fn a_clean_run_prints_exactly_the_summary_line() {
    // The corpus gate depends on this byte for byte. The fixture must
    // be TRULY clean — a composer.json included, so the CEL0025
    // zero-configuration notice does not fire. Build the project with
    // the exact fixture the existing `check__clean.snap` test uses
    // (see its test in this file) and add the two assertions below.
    let root = clean_project(); // the existing clean-fixture helper or its inline shape
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean);
    assert!(
        text.ends_with("0 notices, 0 diagnostics\n"),
        "clean output ends with the bare summary: {text}",
    );
    assert!(
        !text.contains("for more information"),
        "no identifiers reported, no trailer: {text}",
    );
}
```

Run: `cargo test --package celerrate_cli --test check`
Expected: the two new tests FAIL (no trailer yet); existing snapshots still pass (nothing changed yet).

- [ ] **Step 2: The internal-error variant and the absorb helper**

In `crates/celerrate_cli/src/session.rs`, add to `enum InternalError` (session.rs:36-75):

```rust
/// Rich rendering of one diagnostic failed; it was shown in the
/// minimal one-line format instead. Always a Celerrate bug: the
/// fallback keeps the report intact, the report invites the issue.
DiagnosticRenderFailed { identifier: String, location: String },
```

and on `Session`:

```rust
/// Absorbs the rich-rendering failures of one report, after the
/// report is written and before the internal errors render.
pub fn absorb_render_failures(
    &mut self,
    failures: Vec<celerrate_rules::render::RenderFailure>,
) {
    for failure in failures {
        self.internal_errors.push(InternalError::DiagnosticRenderFailed {
            identifier: failure.id.as_str().to_owned(),
            location: failure.location,
        });
    }
}
```

In `render_internal_errors` (render.rs:224-290), add the match arm among the Celerrate-bug variants (sets `has_celerrate_bug = true`):

```rust
InternalError::DiagnosticRenderFailed { identifier, location } => {
    writeln!(
        output,
        "internal error: rendering {identifier} at {location} failed; \
         the diagnostic was shown in the minimal format",
    )?;
    has_celerrate_bug = true;
}
```

(match the exact accumulation pattern the existing arms use.)

- [ ] **Step 3: Rebuild the CLI report assembly**

In `crates/celerrate_cli/src/render.rs`, replace `render_report` (render.rs:48-85) — the notice loop and the summary line move over verbatim; the diagnostic loop is replaced:

```rust
use celerrate_rules::render::{
    ColorMode, FaultInjection, RenderFailure, SourceAccess, explain_pointers,
    render_report as render_blocks, resolve::DatabaseResolver,
};

/// The CLI's view of its sources, for the renderer: display paths from
/// the VFS, text from the decode query. Both borrow the session.
struct SessionSources<'a> {
    session: &'a Session,
}

impl SourceAccess for SessionSources<'_> {
    fn display_path(&self, file: FileId) -> Option<String> {
        Some(display_path(self.session, file))
    }

    fn text(&self, file: FileId) -> Option<&str> {
        let source = self.session.sources.get(&file)?;
        celerrate_db::source_text(&self.session.database, *source)
            .as_ref()
            .ok()
            .map(|text| text.text())
    }
}

pub fn render_report(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
    color: ColorMode,
) -> io::Result<Vec<RenderFailure>> {
    render_report_with(output, session, outcome, color, &FaultInjection::None)
}

/// The seam the fault-injection tests use; production always passes
/// [`FaultInjection::None`] through [`render_report`].
pub(crate) fn render_report_with(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
    color: ColorMode,
    fault: &FaultInjection,
) -> io::Result<Vec<RenderFailure>> {
    let notices = session.notices();
    if !notices.is_empty() {
        for notice in notices {
            writeln!(
                output,
                "notice {}: {}",
                notice.identifier().as_str(),
                notice.message(),
            )?;
        }
        writeln!(output)?;
    }

    let sources = SessionSources { session };
    let resolver = DatabaseResolver::new(&session.database, session.files);
    let report = render_blocks(&outcome.diagnostics, &sources, &resolver, color, fault);
    for block in &report.blocks {
        writeln!(output, "{block}")?;
        writeln!(output)?;
    }

    writeln!(
        output,
        "{}, {}",
        count(notices.len(), "notice", "notices"),
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
    )?;

    let mut identifiers: Vec<DiagnosticId> =
        notices.iter().map(|notice| notice.identifier()).collect();
    identifiers.extend(outcome.diagnostics.iter().map(|diagnostic| diagnostic.id));
    let pointers = explain_pointers(identifiers);
    if !pointers.is_empty() {
        writeln!(output)?;
        write!(output, "{pointers}")?;
    }

    Ok(report.failures)
}
```

(`notice.identifier()` returns the notice's `DiagnosticId` — this is the accessor `render_report` already calls at render.rs:58; adjust the map accordingly if it returns a reference.)

In `lib.rs`'s check arm (lib.rs:107-131), the call becomes:

```rust
let failures = match render::render_report(output, &session, &presented, color) {
    Ok(failures) => failures,
    Err(_) => return Outcome::InternalError,
};
session.absorb_render_failures(failures);
```

(before `cache::persist`; `render_internal_errors` later in the arm then reports them, and `Outcome::of` counts them, so a fallback exits 2.)

`render_check` (render.rs:25) and the watch path: thread `color` through `render_check` and `render_cycle`, and restructure `render_check` so failures are absorbed before internal errors render. `render_cycle` takes `session: &mut Session`:

```rust
pub fn render_check(
    output: &mut dyn Write,
    session: &mut Session,
    outcome: &AnalysisOutcome,
    color: ColorMode,
) -> io::Result<()> {
    let failures = {
        let mut body: Vec<u8> = Vec::new();
        let failures = render_report(&mut body, session, outcome, color)?;
        output.write_all(&body)?;
        failures
    };
    session.absorb_render_failures(failures);
    render_internal_errors(output, session)
}
```

(the buffered body keeps the borrow checker happy: the immutable session borrow ends before the mutable absorb; the watch path already buffers frames conceptually and Task 10 formalizes the frame assembly). Update `watch.rs`'s `completed_cycle`/`cycle` call chain and `render_cycle`'s signature mechanically (`session: &mut Session`, plus `color: ColorMode` threaded from `watch(session, output, color)`, itself threaded from `run`'s check arm). `render.rs`'s own test module calls get the same mechanical updates with `ColorMode::Plain`.

- [ ] **Step 4: Re-bless the CLI snapshots under verify-then-accept**

Run: `cargo test --package celerrate_cli`
Expected: snapshot FAILURES for `check__findings`, `check__help_line`, `fix__*` output snapshots, plus assorted substring tests. For each `.snap.new`: verify the rich block is correct (header `error[CEL####]`, origin `--> path:line:column` matching the OLD snapshot's location byte for byte, correct underline, `note:`/`help:` lines carrying the same text the old format carried, summary line unchanged, trailer present), then accept. Substring tests that assert the old one-line format (e.g. anything matching `src/Caller.php:3:38 CEL0030`) are updated to assert the same facts against the rich shape (the origin line and the header line). The notice tests (`a_notice_announces_itself_as_a_notice`) must pass UNCHANGED — if one fails, the notice contract broke: stop and fix the code, not the test.

Run: `cargo test --package celerrate_cli`
Expected: PASS, including the two tests from Step 1.

- [ ] **Step 5: The fault-injected fallback, end to end**

In `render.rs`'s test module (which already builds sessions for `render_check`), add:

```rust
#[test]
fn a_render_failure_falls_back_and_reports_an_internal_error() {
    // Build the session + outcome the existing render_check tests
    // build (reuse the module's fixture helper), with one CEL0018
    // diagnostic, then inject a fault for it.
    let (mut session, outcome) = fixture_with_one_unknown_class();
    let mut body: Vec<u8> = Vec::new();
    let failures = render_report_with(
        &mut body,
        &session,
        &outcome,
        ColorMode::Plain,
        &FaultInjection::ForIdentifier(
            celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
        ),
    )
    .unwrap();
    session.absorb_render_failures(failures);
    render_internal_errors(&mut body, &session).unwrap();
    let text = String::from_utf8(body).unwrap();
    // Mixed output: the minimal line, no rich block for it, and the
    // internal-error trailer with the issue invitation.
    assert!(text.contains(" CEL0018 "), "the fallback line renders: {text}");
    assert!(!text.contains("error[CEL0018]"), "no rich block for the faulted one: {text}");
    assert!(
        text.contains("internal error: rendering CEL0018 at "),
        "the failure is reported: {text}",
    );
    assert!(text.contains("This is a bug in Celerrate"), "the invitation follows: {text}");
    insta::assert_snapshot!("fault_fallback", text);
}
```

(`fixture_with_one_unknown_class` is whatever the module's existing helper is named — reuse it; if none builds a diagnostic-bearing session, build one exactly as the neighboring tests do.) Run, inspect, accept.

- [ ] **Step 6: The corpus gates**

Run: `cargo xtask fetch-corpus && cargo xtask corpus && cargo xtask mixed-rate`
Expected: both byte-identical to the committed snapshot and baseline (the corpus is clean: its output is the bare summary line, which did not move). Any delta is a stop-and-inspect, not a re-bless.

- [ ] **Step 7: Full check and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo xtask dependency-shape && cargo xtask emission-scan`
Expected: clean.

```bash
git add crates/celerrate_cli crates/celerrate_rules
git commit -m "✨ feat(cli): serve the rustc-style renderer from check"
```

---

### Task 10: The watch-mode height cap

Rich blocks are 8 to 15 lines each and the watch cycle clears and reprints; the frame caps the diagnostic blocks at the terminal height and ends the capped list with an "and N more diagnostics" line. Terminal size is read in the CLI, per cycle, outside queries; tests pass a fixed height.

**Files:**
- Modify: `Cargo.toml` (workspace root: `terminal_size`)
- Modify: `crates/celerrate_cli/Cargo.toml`
- Modify: `crates/celerrate_cli/src/render.rs`
- Modify: `crates/celerrate_cli/src/watch.rs`

**Interfaces:**
- Consumes: `render_report` returning failures (Task 9), the `render_cycle` seam (`render.rs:90-111`).
- Produces: `pub fn render_cycle(output: &mut dyn Write, session: &mut Session, outcome: &AnalysisOutcome, reanalyzed: usize, elapsed: Duration, color: ColorMode, height: Option<usize>) -> io::Result<()>`; `fn capped_blocks(blocks: &[String], budget: usize) -> (usize, usize)` (blocks shown, diagnostics hidden). Watch passes `terminal_size::terminal_size().map(|(_, height)| height.0 as usize)` each cycle; one-shot mode never caps.

- [ ] **Step 1: Add the dependency**

Workspace root `Cargo.toml`, `[workspace.dependencies]`:

```toml
terminal_size = "0.4"
```

`crates/celerrate_cli/Cargo.toml`, `[dependencies]`:

```toml
terminal_size = { workspace = true }
```

Run: `cargo deny check`
Expected: PASS (MIT OR Apache-2.0).

- [ ] **Step 2: Write the failing tests**

In `render.rs`'s test module:

```rust
#[test]
fn capped_blocks_stops_before_the_budget_and_counts_the_hidden() {
    let blocks: Vec<String> = (0..5)
        .map(|index| format!("block {index}\nline\nline\nline"))
        .collect();
    // Each block is 4 lines + 1 separator = 5; a budget of 12 fits 2.
    assert_eq!(capped_blocks(&blocks, 12), (2, 3));
    // A huge budget fits everything.
    assert_eq!(capped_blocks(&blocks, 1000), (5, 0));
    // A tiny budget still shows the first block: a frame that hides
    // every diagnostic while reporting a nonzero count reads broken.
    assert_eq!(capped_blocks(&blocks, 1), (1, 4));
}

#[test]
fn a_capped_cycle_ends_with_the_more_diagnostics_line() {
    // Reuse the module's session fixture with several diagnostics (or
    // build one file per diagnostic exactly as the neighbors do).
    let (mut session, outcome) = fixture_with_many_diagnostics();
    let mut body: Vec<u8> = Vec::new();
    render_cycle(
        &mut body,
        &mut session,
        &outcome,
        outcome.diagnostics.len(),
        std::time::Duration::from_millis(4),
        ColorMode::Plain,
        Some(20),
    )
    .unwrap();
    let text = String::from_utf8(body).unwrap();
    assert!(
        text.contains("more diagnostic"),
        "the cap announces what it hid: {text}",
    );
    assert!(
        text.contains("watching for changes..."),
        "the status trailer survives the cap: {text}",
    );
}

#[test]
fn an_uncapped_cycle_renders_everything() {
    let (mut session, outcome) = fixture_with_many_diagnostics();
    let mut body: Vec<u8> = Vec::new();
    render_cycle(
        &mut body,
        &mut session,
        &outcome,
        0,
        std::time::Duration::from_millis(4),
        ColorMode::Plain,
        None,
    )
    .unwrap();
    let text = String::from_utf8(body).unwrap();
    assert!(!text.contains("more diagnostic"), "no cap without a height: {text}");
}
```

Run: `cargo test --package celerrate_cli capped`
Expected: FAIL — `capped_blocks` not found, `render_cycle` arity.

- [ ] **Step 3: Implement the cap**

In `render.rs`:

```rust
/// How many leading blocks fit a line budget, and how many diagnostics
/// that hides. Each block costs its lines plus one separator line. At
/// least one block always shows: a frame that hides everything while
/// counting nonzero diagnostics would read as broken.
fn capped_blocks(blocks: &[String], budget: usize) -> (usize, usize) {
    let mut used = 0usize;
    let mut shown = 0usize;
    for block in blocks {
        let cost = block.lines().count() + 1;
        if shown > 0 && used + cost > budget {
            break;
        }
        used += cost;
        shown += 1;
    }
    (shown, blocks.len().saturating_sub(shown))
}
```

`render_cycle` is rebuilt on the same assembly `render_report_with` uses, but with the blocks capped before writing (the clear-and-home codes and the status trailer keep their exact current bytes, render.rs:97-111):

```rust
pub fn render_cycle(
    output: &mut dyn Write,
    session: &mut Session,
    outcome: &AnalysisOutcome,
    reanalyzed: usize,
    elapsed: std::time::Duration,
    color: ColorMode,
    height: Option<usize>,
) -> io::Result<()> {
    // The frame is assembled off-screen, capped, then written after
    // the clear so a slow render never shows a half frame.
    let sources = SessionSources { session };
    let resolver = DatabaseResolver::new(&session.database, session.files);
    let report = celerrate_rules::render::render_report(
        &outcome.diagnostics,
        &sources,
        &resolver,
        color,
        &FaultInjection::None,
    );

    let notices = session.notices();
    // Overhead: notice lines + their blank, summary, cap line, blank,
    // status line, watching line, and one spare row for the cursor.
    let overhead = if notices.is_empty() { 0 } else { notices.len() + 1 } + 6;
    let (shown, hidden) = match height {
        Some(rows) => capped_blocks(&report.blocks, rows.saturating_sub(overhead)),
        None => (report.blocks.len(), 0),
    };

    write!(output, "\x1b[2J\x1b[H")?;
    if !notices.is_empty() {
        for notice in notices {
            writeln!(
                output,
                "notice {}: {}",
                notice.identifier().as_str(),
                notice.message(),
            )?;
        }
        writeln!(output)?;
    }
    for block in report.blocks.iter().take(shown) {
        writeln!(output, "{block}")?;
        writeln!(output)?;
    }
    if hidden > 0 {
        writeln!(output, "and {}", count(hidden, "more diagnostic", "more diagnostics"))?;
        writeln!(output)?;
    }
    writeln!(
        output,
        "{}, {}",
        count(notices.len(), "notice", "notices"),
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
    )?;
    session.absorb_render_failures(report.failures);
    render_internal_errors(output, session)?;
    writeln!(output)?;
    writeln!(
        output,
        "{}  |  {}  |  {}ms",
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
        count(reanalyzed, "file re-analyzed", "files re-analyzed"),
        elapsed.as_millis(),
    )?;
    writeln!(output, "watching for changes...")?;
    output.flush()
}
```

(borrow note: `notices` borrows the session immutably while `report.failures` waits to be absorbed — end the notice borrow before `absorb_render_failures` by collecting the notice lines into owned strings first if the borrow checker objects; the shape above assumes `session.notices()` returns data that can be re-fetched or cloned. Resolve mechanically, keep the output bytes as written.)

In `watch.rs`, where `completed_cycle` calls `render::render_cycle`, read the height per cycle:

```rust
let height = terminal_size::terminal_size()
    .map(|(_, terminal_size::Height(rows))| rows as usize);
```

and pass it through. The explain trailer is deliberately absent from the watch frame (the frame is transient and the cap needs its rows); the one-shot report keeps it. Note: watch tests drive `render_cycle` with `Some(height)` or `None` — never the real terminal — so they stay deterministic on CI.

- [ ] **Step 4: Run everything and commit**

Run: `cargo test --package celerrate_cli && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS, including the existing watch tests (update their `render_cycle` call sites mechanically with `ColorMode::Plain, None`; the frame bytes for an uncapped colorless cycle changed only by the rich blocks — re-verify the watch assertions that pinned the old one-line forms, updating facts, not intent).

```bash
git add Cargo.toml Cargo.lock crates/celerrate_cli
git commit -m "✨ feat(cli): cap the watch frame at the terminal height"
```

---

### Task 11: Closure — full gates and the CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Run every gate**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --package celerrate_rules --features render --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo xtask dependency-shape
cargo xtask emission-scan
cargo xtask fetch-corpus && cargo xtask corpus && cargo xtask mixed-rate
```

Expected: all clean; corpus snapshot and mixed-rate baseline byte-identical. Also verify the module boundary the spec fixes: `grep -rn "annotate_snippets" crates/ --include="*.rs" | grep -v "render/adapter.rs"` returns nothing.

- [ ] **Step 2: CHANGELOG**

Add under the Unreleased section, following the existing entry style (Keep a Changelog, see the autofix entry from part 6):

```markdown
- Rustc-style diagnostic rendering: `celerrate check` now reports each
  diagnostic as an annotated source block (header with the identifier,
  excerpt, labeled underlines, `note:` and `help:` lines with rendered
  replacements), with color on terminals (`NO_COLOR` honored), a
  `celerrate explain` pointer per reported identifier, a per-diagnostic
  fallback to the minimal line if rich rendering ever fails, and a
  watch-mode frame capped at the terminal height.
```

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the rustc-style renderer"
```

---

## Spec coverage self-check (section 9 → tasks)

| Spec requirement | Task |
| --- | --- |
| Renderer in `celerrate_rules` behind a cargo feature | 1, 2 |
| Pure function from enriched diagnostics plus sources to text | 2 (traits), 9 (CLI implements them) |
| Symbolic labels resolved to ranges at render time, outside queries | 6 |
| rustc-style form over `annotate-snippets`, one adapter module | 2, 3, 11 (boundary grep) |
| `error[CEL0030]` header, excerpt, labeled underlines, notes, help with rendered replacement | 2, 3 |
| Project-anchored findings render under the notice vocabulary, no excerpt | 1, 3 |
| Summary line stays | 9 |
| Explain pointer per distinct reported identifier | 7, 9 |
| Color in the CLI: TTY + `NO_COLOR`, snapshots pin colorless | 8 |
| Watch height cap + "and N more diagnostics"; one-shot renders everything | 10 |
| Per-diagnostic isolation, minimal fallback + internal-error report, never a crash | 4, 9 |
| Fault-injection seam, fallback snapshot-tested | 4, 9 (step 5) |
| Adapter keeps the library replaceable | 2, 11 |
| Section 11 snapshot list: multi-span (anatomy), multi-file (foreign snippet arm + degraded), notes, suggestions, project-anchored, unicode, fault fallback, explain trailer | 3, 4, 6, 9 |
| Stub-backed label degrades to a note naming the declaration | 6 |

Out of scope, restated: the alternate screen buffer (spec: a later refinement), enrichment in watch mode (unchanged, part 6 decision), member-precision symbolic resolution (no producer exists; degraded form pinned by test), `celerrate explain` itself (part 8), JSON/SARIF/GitHub formats (sub-project 5).
