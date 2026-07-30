# CLI Product 4: Output Formats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--output=json|sarif|github` to `celerrate check`: three machine writers in `celerrate_cli` that serialize the same final diagnostic stream the human renderer consumes, with a committed versioned JSON Schema, SARIF 2.1.0 validation, and GitHub workflow commands.

**Architecture:** One shared machine-report model (`output/model.rs`) is built once from the post-suppression, post-baseline, post-configuration stream, with anchors and secondary labels resolved at the same presentation edge the human renderer uses; each format is a pure serialization of that model. Nothing enters the queries or the cache. The human path stays byte-identical.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), clap 4 derive, serde + serde_json (already workspace dependencies), insta snapshots, `jsonschema` (new dev-dependency) for the schema gates.

Spec: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`, section 6 (output formats), section 9 (testing), closure gate 5.

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Test modules and test files carry local `#![allow]`s. Serializers must be total: no user input may crash them.
- TDD: failing test first, minimal implementation, refactor.
- Everything in files is English, full words, no em-dashes anywhere (code, comments, docs, commits).
- Commits: gitmoji + Conventional Commits (`✨ feat(cli): ...`). Never reference the plan, a task, or a phase in commits or docs.
- Mechanical suite green after every task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo deny check` (mandatory when a manifest changed), `cargo xtask dependency-shape`, `cargo xtask emission-scan`.
- The corpus snapshot and the mixed-rate baseline must not move: no default behavior changes. The human output must stay byte-identical (the existing insta snapshots are the proof; if one changes, that is a bug in this plan's wiring, not a snapshot to re-bless).
- Machine stdout purity: with a machine format, stdout carries the serialized document and nothing else. Meta-reporting (excluded plugins, cache statistics) already goes to stderr and stays there.
- Machine writers ignore `ColorMode` entirely: no ANSI byte may ever appear in a machine format.
- The spec's transverse decisions, held throughout: the writers consume the same final stream as the human renderer (post-suppression, post-baseline, same deterministic order, same exit code); `--output=human` is the explicit default; one format per run; notices stay notices in every format; none of this enters queries or cache.

## Load-bearing codebase facts (verified 2026-07-28)

- `crates/celerrate_cli/src/arguments.rs`: clap 4 derive. `Command::Check { path, watch, fix, fix_suggestions, baseline, ignore_baseline }` with inline `#[arg]` attributes; boolean conflicts are declared via `conflicts_with` / `conflicts_with_all`. No `ValueEnum` exists in the crate yet.
- `crates/celerrate_cli/src/lib.rs:88-100`: clap errors go to the `output` writer and map to `Outcome::UsageError` (or `Clean` for `--help`). Hand-rolled post-parse usage errors (`unusable_root`, lines 110-113) are the template for value-dependent flag incompatibilities, which clap attributes cannot express.
- The check pipeline (`lib.rs:101-195`): `Session::start` → `analysis::analyze` → `session.absorb_outcome` → `presented = AnalysisOutcome { diagnostics: suggest::enrich(...), panicked }` → baseline record/apply → `configuration::merge_diagnostics(&session, &mut presented)` (extends then re-sorts) → `render::render_report(output, &session, &presented, color, &baseline_outcome)` → `cache::persist` → fix → `render_internal_errors` → `Outcome::of(outcome.diagnostics.len().saturating_sub(baseline_outcome.hidden) + configuration_diagnostics, session.internal_errors.len())`.
- `presented.diagnostics` at render time IS the spec's "final stream": post-suppression (in-engine), post-severity-remap, enriched, post-baseline, configuration merged, sorted by `Ord for Diagnostic` (project anchors first, then `(file, range)`).
- `Outcome` (`lib.rs:56-84`): `Clean`/`DiagnosticsReported`/`InternalError`/`UsageError`, `exit_code()` maps to 0/1/2/2.
- `celerrate_diagnostics`: `Diagnostic { id, severity, anchor, message, labels, notes, suggestions }`; `Anchor::{Project, Span { file, range }}`; `Severity::{Warning, Error}` (two variants only); `Label { target: LabelTarget::{Local { range }, Symbolic { symbol }}, message }`; `Suggestion { message, confidence: Confidence::{Safe, NeedsReview}, edits: Vec<TextEdit> }`; `TextEdit { file, range, replacement }` (in `celerrate_source`). `DiagnosticId(&'static str)`, `as_str()`.
- `celerrate_diagnostics::REGISTRY: &[RegisteredDiagnostic { id, family, owner, explain }]` (51 entries, sorted, gapless); `find_identifier(&str) -> Option<DiagnosticId>`; `find_page(id) -> Option<&'static ExplainPage>`; `ExplainPage { why, failing_example, fixed_example, configuration }`. `family` is the short human description (for SARIF `shortDescription`).
- Symbolic label resolution (presentation-time, outside queries): `celerrate_rules::render::{SymbolResolver, ResolvedLabel::{Concrete { file, range }, Degraded}, DatabaseResolver}`; the CLI wires `DatabaseResolver::new(&session.database, session.files)` in `render.rs:244-253` (`build_report`). Degraded labels become a note formatted `` `symbol`: message `` (`degraded_note` in `crates/celerrate_rules/src/render/adapter.rs`, currently private).
- `crates/celerrate_cli/src/render.rs`: `SessionSources { session }` (private) implements `celerrate_rules::render::SourceAccess` (`display_path` returns project-relative OS-native paths, `text` handles the `celerrate.toml` special case). `write_summary_line` prints `"{N} notice(s), {M} diagnostic(s)"` via the private `count` helper. The private `trait Notice { identifier(), message() }` unifies `ProjectNotice` and `BaselineNotice`.
- `celerrate_source::LineIndex { new(text), line_column(offset) -> LineColumn { line, column } }`: zero-based, `column` is a byte offset within the line. No byte-to-code-point converter exists anywhere; SARIF and GitHub need one.
- `crates/celerrate_cli/src/baseline/mod.rs`: `BaselineOutcome { hidden: usize, recorded: Option<usize>, notices: Vec<BaselineNotice> }`; `BaselineNotice::{InvalidFile, ObsoleteEntries}` with `identifier()` (CEL0051/CEL0050) and `message()`; exit-neutral by construction.
- No identifier-to-rule-name index exists; `configuration::severity_remap` walks `celerrate_rules::core_rules() -> Vec<(RuleMetadata, RuleImplementation)>` where `RuleMetadata { name, group, identifiers: Vec<RuleIdentifier { id, severity }>, tier }`.
- Dependencies: `serde` (derive) is already a `celerrate_cli` dependency; `serde_json = "1"` is in `[workspace.dependencies]` but NOT in `celerrate_cli`; `schemars`/`jsonschema` are absent from the workspace. `deny.toml` allows MIT, Apache-2.0, Zlib, Unicode-3.0, ISC, CC0-1.0, BSD-2-Clause; a new license needs an allowlist entry plus the explanatory comment convention.
- `xtask dependency-shape` derives the plugin-crate set from cargo metadata; `celerrate_cli` is a composition root and exempt. No xtask change is needed for this plan.
- Test conventions: in-process `celerrate_cli::run(vec![...], &mut Vec<u8>, ColorMode::Plain)`; each test file re-declares its own `project(&[(path, contents)]) -> TempDir` and `check_with(root, extra)` helpers (deliberate duplication, see `tests/baseline.rs:13-36`); insta snapshots land in `crates/celerrate_cli/tests/snapshots/<file>__<name>.snap`. Cargo makes `[dependencies]` visible to integration tests, so `serde_json` in `[dependencies]` is usable from `tests/`.
- `tests/documentation.rs` has a `workspace_page` helper reading `docs/*.md` from the workspace root; the precedent for docs drift gates.
- Stderr today: only `report_excluded_plugins` (`eprintln!`) and opt-in cache statistics. Everything else goes to the `output` writer bound to stdout in `main.rs`.

## Decisions (and why)

- **A shared model, then three serializers.** `output/model.rs` builds one `MachineReport` from `(&Session, &AnalysisOutcome, &BaselineOutcome, Outcome)`; `json.rs`, `sarif.rs`, `github.rs` serialize it. The model follows the `StoredDiagnostic` precedent (`src/cache/stored.rs`): a CLI-owned DTO layer, `celerrate_diagnostics` stays serde-free.
- **Machine formats are incompatible with `--watch`, `--fix`, `--fix-suggestions`, and `--baseline` (recording).** Watching loops, fixing mutates, recording is an interactive operation whose confirmation line is the point of the run; all three would corrupt or contradict a single machine document on stdout. Applying an existing baseline works with every format (the hidden count rides in the summary). These are value-dependent conflicts, so they are hand-rolled post-parse usage errors (the `unusable_root` pattern), not clap attributes.
- **The exit code is embedded, never recomputed.** The machine path hoists `Outcome::of(...)` (same expression as the human arm), after `cache::persist` so persist-time internal errors are counted, and the payload's `exit_code` comes from that one value via a new `Outcome::code() -> u8` that `exit_code()` wraps. For machine formats nothing after serialization can change the outcome (no rich rendering, no fix), so payload and process agree by construction.
- **Positions: 1-based lines, 1-based columns in Unicode code points, plus exact byte offsets.** The line index speaks zero-based byte columns; conversion happens once in the model. SARIF declares `"columnKind": "unicodeCodePoints"`. Byte offsets ride along for tools that edit.
- **Degraded symbolic labels become notes**, with the exact wording the human renderer uses (`degraded_note`, exported from `celerrate_rules::render` so the wording has one source). Resolved labels (local or symbolic-concrete) become structured locations.
- **JSON is pretty-printed** (stable, reviewable, snapshot-friendly; size is irrelevant at this scale) with a trailing newline.
- **The JSON Schema is strict (`additionalProperties: false`).** The schema ships in lockstep with the emitter, so strictness catches emitter drift in the gate; the written compatibility policy (adding a field is non-breaking, removal or meaning change increments `schema_version`) governs consumers, and every field addition updates the schema file in the same change.
- **SARIF: notices are `level: "note"` results with no location; only referenced identifiers populate `rules`** (deterministic, sorted, small); `shortDescription` is the registry `family`, `fullDescription` the explain page's `why`, `help.text` points at `celerrate explain CELxxxx`. No `helpUri` in v0.1 (no stable per-identifier URL exists; synthesizing one to a docs anchor would rot). `Safe` suggestions become `fixes` with byte-offset replacement regions; `NeedsReview` suggestions and notes ride in `properties` (the spec's rule: never twist a standard field).
- **Internal errors: machine formats carry the count** (in the JSON summary and the SARIF invocation); the detailed report stays on the human channel in v0.1. CI sees exit code 2 either way.
- **GitHub format**: `::error`/`::warning` workflow commands with `file`/`line`/`col`/`endLine`/`endColumn` properties, `::notice` for notices, standard `%25`/`%0D`/`%0A` data escaping, then the same end-of-run summary wording as the human report.

## File structure

- Modify: `crates/celerrate_cli/Cargo.toml` (add `serde_json` to `[dependencies]`; Task 2 adds `jsonschema` to `[dev-dependencies]`)
- Modify: `Cargo.toml` (workspace root; Task 2 adds `jsonschema` to `[workspace.dependencies]`)
- Create: `crates/celerrate_cli/src/output/mod.rs` (MachineFormat, dispatch)
- Create: `crates/celerrate_cli/src/output/model.rs` (the shared report model)
- Create: `crates/celerrate_cli/src/output/json.rs`
- Create: `crates/celerrate_cli/src/output/sarif.rs`
- Create: `crates/celerrate_cli/src/output/github.rs`
- Modify: `crates/celerrate_cli/src/arguments.rs` (`OutputFormat`, the `--output` flag)
- Modify: `crates/celerrate_cli/src/lib.rs` (`Outcome::code`, usage guards, the machine branch)
- Modify: `crates/celerrate_cli/src/render.rs` (visibility only: `SessionSources`, `count`)
- Modify: `crates/celerrate_rules/src/render/adapter.rs` + `mod.rs` (export `degraded_note`)
- Create: `schemas/celerrate-json-report.v1.schema.json`, `schemas/sarif-2.1.0.schema.json`, `schemas/README.md`
- Create: `crates/celerrate_cli/tests/output_json.rs`, `tests/output_sarif.rs`, `tests/output_github.rs`, `tests/output_equivalence.rs`
- Modify: `crates/celerrate_cli/tests/documentation.rs` (docs drift gate)
- Create: `docs/output-formats.md`
- Modify: `CHANGELOG.md`

---

### Task 1: The `--output` flag, the machine-report model, and the JSON writer

**Files:**
- Modify: `crates/celerrate_cli/Cargo.toml`
- Modify: `crates/celerrate_cli/src/arguments.rs`
- Modify: `crates/celerrate_cli/src/lib.rs`
- Modify: `crates/celerrate_cli/src/render.rs` (visibility only)
- Modify: `crates/celerrate_rules/src/render/adapter.rs`, `crates/celerrate_rules/src/render/mod.rs` (export `degraded_note`)
- Create: `crates/celerrate_cli/src/output/mod.rs`, `src/output/model.rs`, `src/output/json.rs`
- Test: `crates/celerrate_cli/tests/output_json.rs`

**Interfaces:**
- Consumes: everything listed in the load-bearing facts.
- Produces (later tasks rely on these exact names):
  - `arguments::OutputFormat { Human, Json }` (`clap::ValueEnum`; Tasks 3 and 4 add `Sarif` and `Github`), with `pub fn as_argument(self) -> &'static str`.
  - `output::MachineFormat { Json }` with `pub fn of(format: OutputFormat) -> Option<Self>`; `output::write(format: MachineFormat, output: &mut dyn Write, report: &model::MachineReport) -> io::Result<()>`.
  - `output::model::{MachineReport, Summary, Notice, ReportedDiagnostic, ReportedSeverity, ReportedAnchor, SpanLocation, ResolvedReportLabel, ReportedSuggestion, ReportedConfidence, ReportedEdit, SCHEMA_VERSION, build}` with `pub fn build(session: &Session, presented: &AnalysisOutcome, baseline: &BaselineOutcome, verdict: Outcome) -> MachineReport`.
  - `Outcome::code(self) -> u8` on `celerrate_cli::Outcome`.
  - `celerrate_rules::render::degraded_note(symbol: &str, message: &str) -> String` (public re-export).

- [ ] **Step 1: Write the failing integration tests**

Create `crates/celerrate_cli/tests/output_json.rs`:

```rust
//! `--output=json`: the versioned machine report over the same final
//! stream as the human renderer.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::ffi::OsString;
use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

const MANIFEST: &str = r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#;
const FAILING_EXAMPLE: &str = "<?php\n\nstrlenn(\"hello\");\n";
const CLEAN_EXAMPLE: &str = "<?php\n\n$greeting = \"hello\";\n";

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

fn check_with(root: &Path, extra: &[&str]) -> (Outcome, String) {
    let mut arguments: Vec<OsString> =
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()];
    arguments.extend(extra.iter().map(Into::into));
    let mut output = Vec::new();
    let outcome = run(arguments, &mut output, ColorMode::Plain);
    (outcome, String::from_utf8(output).unwrap())
}

fn findings_project() -> tempfile::TempDir {
    project(&[("composer.json", MANIFEST), ("src/Example.php", FAILING_EXAMPLE)])
}

#[test]
fn json_reports_schema_version_one_and_the_embedded_exit_code() {
    let root = findings_project();
    let (outcome, text) = check_with(root.path(), &["--output", "json"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["summary"]["exit_code"], 1);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert!(!diagnostics.is_empty());
    for diagnostic in diagnostics {
        assert!(diagnostic["id"].as_str().unwrap().starts_with("CEL"));
        let anchor = &diagnostic["anchor"];
        if anchor["kind"] == "span" {
            assert!(anchor["start_line"].as_u64().unwrap() >= 1);
            assert!(anchor["start_column"].as_u64().unwrap() >= 1);
            assert!(!anchor["path"].as_str().unwrap().contains('\\'));
        }
    }
}

#[test]
fn a_clean_project_reports_exit_code_zero_and_no_diagnostics() {
    let root = project(&[("composer.json", MANIFEST), ("src/Clean.php", CLEAN_EXAMPLE)]);
    let (outcome, text) = check_with(root.path(), &["--output", "json"]);
    assert_eq!(outcome, Outcome::Clean);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["summary"]["exit_code"], 0);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 0);
}

#[test]
fn json_is_the_entire_stdout() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "json"]);
    // One document, nothing before, one trailing newline after.
    assert!(text.starts_with('{'));
    assert!(text.ends_with("}\n"));
    serde_json::from_str::<serde_json::Value>(&text).unwrap();
}

#[test]
fn project_notices_are_carried_and_exit_neutral() {
    // No composer.json: the project notice channel fires.
    let root = project(&[("src/Clean.php", CLEAN_EXAMPLE)]);
    let (_, text) = check_with(root.path(), &["--output", "json"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let notices = value["notices"].as_array().unwrap();
    assert!(!notices.is_empty());
    for notice in notices {
        assert!(notice["id"].as_str().unwrap().starts_with("CEL"));
        assert!(!notice["message"].as_str().unwrap().is_empty());
    }
    assert_eq!(value["summary"]["notices"], notices.len());
}

#[test]
fn an_applied_baseline_hides_findings_from_the_payload() {
    let root = findings_project();
    let (_, _) = check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check_with(root.path(), &["--output", "json"]);
    assert_eq!(outcome, Outcome::Clean);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["summary"]["exit_code"], 0);
    assert!(value["summary"]["baselined_hidden"].as_u64().unwrap() >= 1);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 0);
}

#[test]
fn machine_output_refuses_the_mutating_and_looping_flags() {
    let root = findings_project();
    for flag in ["--watch", "--fix", "--fix-suggestions", "--baseline"] {
        let (outcome, text) = check_with(root.path(), &["--output", "json", flag]);
        assert_eq!(outcome, Outcome::UsageError, "{flag}");
        assert!(text.contains(flag), "{flag}: {text}");
        assert!(text.contains("--output=json"), "{flag}: {text}");
    }
}

#[test]
fn ignore_baseline_combines_with_machine_output() {
    let root = findings_project();
    let (_, _) = check_with(root.path(), &["--baseline"]);
    let (outcome, text) =
        check_with(root.path(), &["--output", "json", "--ignore-baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(!value["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn explicit_human_output_is_byte_identical_to_the_default() {
    let root = findings_project();
    let (default_outcome, default_text) = check_with(root.path(), &[]);
    let (human_outcome, human_text) = check_with(root.path(), &["--output", "human"]);
    assert_eq!(default_outcome, human_outcome);
    assert_eq!(default_text, human_text);
}

#[test]
fn json_findings_snapshot() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "json"]);
    insta::assert_snapshot!("json_findings", text);
}
```

Note: `FAILING_EXAMPLE` calls an unknown function (`strlenn`), which fires the `unknown-symbols` rule and exercises the did-you-mean enrichment path. If the fixture turns out not to fire (check against `tests/check.rs`'s own findings fixture at execution time), reuse the exact fixture from that file instead; per repository convention every test file re-declares its fixtures.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --test output_json`
Expected: FAIL at parse time inside `run` ("unexpected argument '--output'"), surfacing as `Outcome::UsageError` where `DiagnosticsReported` was expected.

- [ ] **Step 3: Export `degraded_note` from `celerrate_rules::render`**

In `crates/celerrate_rules/src/render/adapter.rs`, make the existing private helper public and move its doc comment to state the contract:

```rust
/// The note wording a degraded symbolic label takes, shared by the human
/// renderer and the machine formats so the channels never drift.
pub fn degraded_note(symbol: &str, message: &str) -> String {
    format!("`{symbol}`: {message}")
}
```

(Keep the body exactly as it is today; only the visibility and doc change. If today's body differs from the above, keep today's body: the wording is the contract.)

In `crates/celerrate_rules/src/render/mod.rs`, add `degraded_note` to the adapter re-exports (match the file's existing `pub use` style).

- [ ] **Step 4: Add `serde_json` to the CLI and widen two visibilities**

In `crates/celerrate_cli/Cargo.toml`, under `[dependencies]`, in alphabetical order:

```toml
serde_json = { workspace = true }
```

In `crates/celerrate_cli/src/render.rs`:
- `struct SessionSources<'a> { session: &'a Session }` becomes `pub(crate) struct SessionSources<'a> { pub(crate) session: &'a Session }`.
- `fn count(...)` becomes `pub(crate) fn count(...)` (Task 4's GitHub summary reuses it).

- [ ] **Step 5: Add `OutputFormat` and the `--output` flag**

In `crates/celerrate_cli/src/arguments.rs`, above `Command`:

```rust
/// The report's serialization. The machine formats consume the same
/// final stream as the human renderer: post-suppression, post-baseline,
/// same order, same exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// The rich human report.
    Human,
    /// The versioned JSON report for tooling.
    Json,
}

impl OutputFormat {
    /// The value as typed on the command line, for usage errors.
    pub fn as_argument(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Json => "json",
        }
    }
}
```

In the `Check` variant, after `ignore_baseline`:

```rust
/// Serialize the report: human (default) or a machine format.
#[arg(long, value_enum, default_value_t = OutputFormat::Human)]
output: OutputFormat,
```

Add unit tests to the existing `#[cfg(test)]` module in `arguments.rs` (match its style):

```rust
#[test]
fn output_defaults_to_human() {
    let arguments = Arguments::try_parse_from(["celerrate", "check"]).unwrap();
    let Command::Check { output, .. } = arguments.command else {
        panic!("expected check");
    };
    assert_eq!(output, OutputFormat::Human);
}

#[test]
fn output_accepts_json() {
    let arguments =
        Arguments::try_parse_from(["celerrate", "check", "--output", "json"]).unwrap();
    let Command::Check { output, .. } = arguments.command else {
        panic!("expected check");
    };
    assert_eq!(output, OutputFormat::Json);
}
```

(If the existing tests in that module destructure `Command::Check` without `..`, update them for the new field; the by-name destructuring in `lib.rs` will not compile until Step 8 either, which is the intended guard.)

- [ ] **Step 6: Write the machine-report model**

Create `crates/celerrate_cli/src/output/model.rs`:

```rust
//! The machine-report model: one serializable projection of the final
//! stream (post-suppression, post-baseline, post-configuration, sorted),
//! built once and consumed by every machine writer. Anchors and secondary
//! labels resolve here, at the same presentation edge the human renderer
//! uses. Pure presentation: nothing enters the queries or the cache.

use std::collections::BTreeMap;

use celerrate_diagnostics::{Anchor, Confidence, Diagnostic, LabelTarget, Severity};
use celerrate_rules::render::{
    DatabaseResolver, ResolvedLabel, SourceAccess, SymbolResolver, degraded_note,
};
use celerrate_source::{FileId, LineIndex, TextRange, TextSize};
use serde::Serialize;

use crate::Outcome;
use crate::analysis::AnalysisOutcome;
use crate::baseline::BaselineOutcome;
use crate::render::SessionSources;
use crate::session::Session;

/// The JSON contract version. Adding a field is non-breaking; removing
/// one or changing its meaning increments this constant and forks the
/// committed schema file.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct MachineReport {
    pub schema_version: u32,
    pub summary: Summary,
    pub notices: Vec<Notice>,
    pub diagnostics: Vec<ReportedDiagnostic>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub errors: usize,
    pub warnings: usize,
    pub notices: usize,
    pub baselined_hidden: usize,
    pub internal_errors: usize,
    pub exit_code: u8,
}

/// An exit-neutral notice: a project notice or a baseline notice, in the
/// order the human report prints them.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub id: String,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReportedDiagnostic {
    pub id: String,
    pub severity: ReportedSeverity,
    /// The owning rule's kebab-case name; absent for identifiers no rule
    /// owns (syntax, project, configuration resilience).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub anchor: ReportedAnchor,
    pub message: String,
    pub labels: Vec<ResolvedReportLabel>,
    pub notes: Vec<String>,
    pub suggestions: Vec<ReportedSuggestion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportedSeverity {
    Warning,
    Error,
}

impl From<Severity> for ReportedSeverity {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Warning => Self::Warning,
            Severity::Error => Self::Error,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ReportedAnchor {
    Project,
    Span(SpanLocation),
}

/// A concrete location: project-relative path with forward slashes,
/// 1-based lines, 1-based columns counted in Unicode code points, plus
/// the exact byte offsets for tools that edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpanLocation {
    pub path: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub byte_start: u32,
    pub byte_end: u32,
}

/// A secondary label that resolved to a concrete location, local or
/// symbolic alike. Degraded symbolic labels become notes instead, with
/// the same wording as the human renderer.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedReportLabel {
    pub location: SpanLocation,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReportedSuggestion {
    pub message: String,
    pub confidence: ReportedConfidence,
    pub edits: Vec<ReportedEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportedConfidence {
    Safe,
    NeedsReview,
}

impl From<Confidence> for ReportedConfidence {
    fn from(confidence: Confidence) -> Self {
        match confidence {
            Confidence::Safe => Self::Safe,
            Confidence::NeedsReview => Self::NeedsReview,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReportedEdit {
    pub location: SpanLocation,
    pub replacement: String,
}

/// Build the report from the final stream. `presented` is the exact
/// vector the human renderer receives; `verdict` is the run's one
/// `Outcome`, computed by the caller from the same inputs as the human
/// arm, never recomputed here.
pub fn build(
    session: &Session,
    presented: &AnalysisOutcome,
    baseline: &BaselineOutcome,
    verdict: Outcome,
) -> MachineReport {
    let sources = SessionSources { session };
    let resolver = DatabaseResolver::new(&session.database, session.files);
    let rules = rule_name_index();
    let diagnostics: Vec<ReportedDiagnostic> = presented
        .diagnostics
        .iter()
        .map(|diagnostic| reported(diagnostic, &sources, &resolver, &rules))
        .collect();
    let errors = presented
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count();
    let warnings = presented
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    let mut notices: Vec<Notice> = session
        .notices()
        .iter()
        .map(|notice| Notice {
            id: notice.identifier().as_str().to_owned(),
            message: notice.message(),
        })
        .collect();
    notices.extend(baseline.notices.iter().map(|notice| Notice {
        id: notice.identifier().as_str().to_owned(),
        message: notice.message(),
    }));
    MachineReport {
        schema_version: SCHEMA_VERSION,
        summary: Summary {
            errors,
            warnings,
            notices: notices.len(),
            baselined_hidden: baseline.hidden,
            internal_errors: session.internal_errors.len(),
            exit_code: verdict.code(),
        },
        notices,
        diagnostics,
    }
}

fn reported(
    diagnostic: &Diagnostic,
    sources: &SessionSources<'_>,
    resolver: &DatabaseResolver<'_>,
    rules: &BTreeMap<&'static str, String>,
) -> ReportedDiagnostic {
    let anchored_file = match diagnostic.anchor {
        Anchor::Span { file, .. } => Some(file),
        Anchor::Project => None,
    };
    let anchor = match diagnostic.anchor {
        Anchor::Project => ReportedAnchor::Project,
        Anchor::Span { file, range } => {
            ReportedAnchor::Span(location(sources, file, range))
        }
    };
    let mut labels = Vec::new();
    let mut notes = diagnostic.notes.clone();
    for label in &diagnostic.labels {
        match &label.target {
            LabelTarget::Local { range } => match anchored_file {
                Some(file) => labels.push(ResolvedReportLabel {
                    location: location(sources, file, *range),
                    message: label.message.clone(),
                }),
                // A local label on a project anchor has no file to
                // resolve against; the message survives as a note.
                None => notes.push(label.message.clone()),
            },
            LabelTarget::Symbolic { symbol } => match resolver.resolve(symbol) {
                ResolvedLabel::Concrete { file, range } => {
                    labels.push(ResolvedReportLabel {
                        location: location(sources, file, range),
                        message: label.message.clone(),
                    });
                }
                ResolvedLabel::Degraded => {
                    notes.push(degraded_note(symbol, &label.message));
                }
            },
        }
    }
    let suggestions = diagnostic
        .suggestions
        .iter()
        .map(|suggestion| ReportedSuggestion {
            message: suggestion.message.clone(),
            confidence: suggestion.confidence.into(),
            edits: suggestion
                .edits
                .iter()
                .map(|edit| ReportedEdit {
                    location: location(sources, edit.file, edit.range),
                    replacement: edit.replacement.clone(),
                })
                .collect(),
        })
        .collect();
    ReportedDiagnostic {
        id: diagnostic.id.as_str().to_owned(),
        severity: diagnostic.severity.into(),
        rule: rules.get(diagnostic.id.as_str()).cloned(),
        anchor,
        message: diagnostic.message.clone(),
        labels,
        notes,
        suggestions,
    }
}

/// Total conversion: an unreadable file degrades to line 1, column 1,
/// mirroring `render_minimal`, and the byte offsets stay exact.
fn location(sources: &SessionSources<'_>, file: FileId, range: TextRange) -> SpanLocation {
    let path = sources
        .display_path(file)
        .unwrap_or_else(|| String::from("<unknown>"))
        .replace('\\', "/");
    let (start_line, start_column, end_line, end_column) = match sources.text(file) {
        Some(text) => {
            let index = LineIndex::new(text);
            let (start_line, start_column) = position(&index, text, range.start());
            let (end_line, end_column) = position(&index, text, range.end());
            (start_line, start_column, end_line, end_column)
        }
        None => (1, 1, 1, 1),
    };
    SpanLocation {
        path,
        start_line,
        start_column,
        end_line,
        end_column,
        byte_start: range.start().into(),
        byte_end: range.end().into(),
    }
}

/// 1-based line, 1-based column in Unicode code points. The line index
/// speaks zero-based byte columns; the conversion happens here, once.
fn position(index: &LineIndex, text: &str, offset: TextSize) -> (u32, u32) {
    let line_column = index.line_column(offset);
    let offset = usize::from(offset);
    let line_start = offset.saturating_sub(line_column.column as usize);
    let code_points = text
        .get(line_start..offset)
        .map(|prefix| prefix.chars().count() as u32)
        .unwrap_or(line_column.column);
    (line_column.line + 1, code_points + 1)
}

/// Identifier to owning rule name, derived from the core rule metadata
/// the same way `configuration::severity_remap` walks it. Identifiers no
/// rule owns (syntax, project, configuration) are simply absent.
fn rule_name_index() -> BTreeMap<&'static str, String> {
    let mut index = BTreeMap::new();
    for (metadata, _) in celerrate_rules::core_rules() {
        for identifier in &metadata.identifiers {
            index.insert(identifier.id.as_str(), metadata.name.clone());
        }
    }
    index
}
```

Adjust the small unknowns against the real code at execution time (they are API facts, not design choices): the exact import paths (`LineIndex` may live behind `celerrate_source::LineIndex` directly), whether `session.files` is `Copy` (clone if not), and `DatabaseResolver`'s exact constructor signature (`render.rs:244-253` is the reference usage). If `TextSize` does not convert to `u32` via `Into`, use `u32::from(range.start())`.

- [ ] **Step 7: Write the dispatch and the JSON writer**

Create `crates/celerrate_cli/src/output/mod.rs`:

```rust
//! Machine output formats: pure serializations of the final stream, at
//! the edge, after suppression and the baseline. One pipeline, four
//! serializations (design spec 2026-07-24, section 6).

pub mod json;
pub mod model;

use std::io::{self, Write};

use crate::arguments::OutputFormat;

/// The non-human formats. Converting up front keeps every writer match
/// exhaustive: adding a format extends this enum and the compiler walks
/// the plan to every dispatch site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineFormat {
    Json,
}

impl MachineFormat {
    pub fn of(format: OutputFormat) -> Option<Self> {
        match format {
            OutputFormat::Human => None,
            OutputFormat::Json => Some(Self::Json),
        }
    }
}

pub fn write(
    format: MachineFormat,
    output: &mut dyn Write,
    report: &model::MachineReport,
) -> io::Result<()> {
    match format {
        MachineFormat::Json => json::write(output, report),
    }
}
```

Create `crates/celerrate_cli/src/output/json.rs`:

```rust
//! `--output=json`: the versioned JSON report. Pretty-printed for stable
//! diffs, one document, one trailing newline, nothing else on stdout.

use std::io::{self, Write};

use super::model::MachineReport;

pub fn write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report).map_err(io::Error::from)?;
    writeln!(output)
}
```

- [ ] **Step 8: Wire the machine branch into `lib.rs`**

In `crates/celerrate_cli/src/lib.rs`:

1. Declare the module alongside its siblings: `mod output;` (the modules list around `mod render;`).

2. Add `Outcome::code` and make `exit_code` wrap it:

```rust
/// The numeric exit code, the single source `exit_code` wraps. The JSON
/// summary embeds this same number, so payload and process cannot drift.
pub fn code(self) -> u8 {
    match self {
        Self::Clean => 0,
        Self::DiagnosticsReported => 1,
        Self::InternalError | Self::UsageError => 2,
    }
}

pub fn exit_code(self) -> ExitCode {
    ExitCode::from(self.code())
}
```

3. In the `Command::Check` arm, extend the destructuring with `output: format` (the local `output` name is taken by the writer):

```rust
Command::Check { path, watch, fix, fix_suggestions, baseline, ignore_baseline, output: format } => {
```

4. Directly after the existing `unusable_root` / `absolute_root` guards, the value-dependent usage guard:

```rust
if let Some(_machine) = crate::output::MachineFormat::of(format) {
    let incompatible = [
        (watch, "--watch"),
        (fix, "--fix"),
        (fix_suggestions, "--fix-suggestions"),
        (baseline, "--baseline"),
    ]
    .into_iter()
    .find_map(|(set, flag)| set.then_some(flag));
    if let Some(flag) = incompatible {
        let _ = writeln!(
            output,
            "error: --output={} cannot be combined with {flag}",
            format.as_argument(),
        );
        return Outcome::UsageError;
    }
}
```

5. After `configuration::merge_diagnostics` and before `render::render_report`, the machine branch:

```rust
if let Some(machine) = crate::output::MachineFormat::of(format) {
    // Persist first: a persist-time internal error must be counted in
    // the verdict the payload embeds. Nothing after serialization can
    // change the outcome (no rich rendering, no fix on this path).
    cache::persist(&mut session, &outcome);
    let verdict = Outcome::of(
        outcome.diagnostics.len().saturating_sub(baseline_outcome.hidden)
            + configuration_diagnostics,
        session.internal_errors.len(),
    );
    let report = crate::output::model::build(&session, &presented, &baseline_outcome, verdict);
    if crate::output::write(machine, output, &report).is_err() {
        return Outcome::InternalError;
    }
    session.statistics.report();
    return verdict;
}
```

The human path below stays character-for-character untouched. Adapt the exact variable names (`baseline_outcome`, `configuration_diagnostics`) to the file; the verdict expression must be copied from the human arm's `Outcome::of` call at the bottom of the function, not paraphrased.

- [ ] **Step 9: Run the new tests**

Run: `cargo test --package celerrate_cli --test output_json`
Expected: all pass except `json_findings_snapshot`, which fails once with the new snapshot pending. Review the generated snapshot (`cargo insta review`, or inspect `tests/snapshots/output_json__json_findings.snap.new` and rename): the JSON must show `schema_version: 1`, a `span` anchor with `path: "src/Example.php"`, 1-based positions, and the enrichment (a note or suggestion) if the fixture fires one. Accept only after reading it.

- [ ] **Step 10: Prove the human path did not move**

Run: `cargo test --workspace`
Expected: PASS with zero snapshot changes outside `tests/snapshots/output_json__*`. Any changed human snapshot means the wiring touched the human path: stop and fix (systematic-debugging), do not re-bless.

- [ ] **Step 11: Lint, format, deny**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: clean (the manifest changed: `serde_json` joined `celerrate_cli`; it is already in the workspace tree, so `cargo deny` has nothing new to judge, but run it anyway).

- [ ] **Step 12: Commit**

```bash
git add crates/celerrate_cli crates/celerrate_rules/src/render
git commit -m "✨ feat(cli): emit a versioned JSON report behind --output=json"
```

---

### Task 2: The committed JSON Schema and its validation gate

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/celerrate_cli/Cargo.toml`
- Create: `schemas/celerrate-json-report.v1.schema.json`
- Create: `schemas/README.md`
- Test: `crates/celerrate_cli/tests/output_json.rs` (extend)

**Interfaces:**
- Consumes: the JSON emitted by `output::json::write` (Task 1), the `project`/`check_with` helpers already in `tests/output_json.rs`.
- Produces: the schema file path other tooling and the docs reference: `schemas/celerrate-json-report.v1.schema.json`; the `jsonschema` dev-dependency Task 3 reuses.

- [ ] **Step 1: Write the failing validation test**

Append to `crates/celerrate_cli/tests/output_json.rs`:

```rust
#[test]
fn json_output_validates_against_the_committed_schema() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../schemas/celerrate-json-report.v1.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    // Three shapes: findings, clean, and notices-plus-baseline.
    let findings = findings_project();
    let clean = project(&[("composer.json", MANIFEST), ("src/Clean.php", CLEAN_EXAMPLE)]);
    let noticed = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    let (_, _) = check_with(findings.path(), &["--baseline"]);
    let runs = [
        check_with(findings.path(), &["--output", "json"]).1,
        check_with(findings.path(), &["--output", "json", "--ignore-baseline"]).1,
        check_with(clean.path(), &["--output", "json"]).1,
        check_with(noticed.path(), &["--output", "json"]).1,
    ];
    for text in runs {
        let instance: serde_json::Value = serde_json::from_str(&text).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}
```

If the installed `jsonschema` version exposes a different API (`validator_for` and `iter_errors` are the current names; older versions use `JSONSchema::compile` and `validate`), adapt the test to the version that resolves, not the other way round.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_cli --test output_json json_output_validates`
Expected: FAIL to compile (`jsonschema` unresolved) or FAIL on the missing schema file.

- [ ] **Step 3: Add the `jsonschema` dev-dependency**

In the workspace root `Cargo.toml`, `[workspace.dependencies]`, alphabetical order:

```toml
jsonschema = { version = "0.29", default-features = false }
```

If `cargo check` reports this version does not exist, run `cargo add --dry-run jsonschema` to learn the current one and pin that instead. If disabling default features breaks draft-04 detection or the resolver (Task 3 needs draft-04 for the SARIF schema), drop `default-features = false`. `jsonschema` is MIT; run `cargo deny check` after adding it, and if a transitive crate introduces a license outside the allowlist, extend `deny.toml` following its existing comment convention (one comment block per license naming the introducing dependency).

In `crates/celerrate_cli/Cargo.toml`, `[dev-dependencies]`, alphabetical order:

```toml
jsonschema = { workspace = true }
```

(`serde_json` is already a regular dependency and therefore visible to integration tests; do not add it to dev-dependencies.)

- [ ] **Step 4: Author the schema**

Create `schemas/celerrate-json-report.v1.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://raw.githubusercontent.com/celerrate/celerrate/main/schemas/celerrate-json-report.v1.schema.json",
  "title": "Celerrate JSON report, schema version 1",
  "description": "The report emitted by `celerrate check --output=json`. Compatibility policy: adding a field is non-breaking and updates this file in the same release; removing a field or changing its meaning increments schema_version and forks a new schema file.",
  "type": "object",
  "required": ["schema_version", "summary", "notices", "diagnostics"],
  "additionalProperties": false,
  "properties": {
    "schema_version": { "const": 1 },
    "summary": { "$ref": "#/$defs/summary" },
    "notices": { "type": "array", "items": { "$ref": "#/$defs/notice" } },
    "diagnostics": { "type": "array", "items": { "$ref": "#/$defs/diagnostic" } }
  },
  "$defs": {
    "identifier": { "type": "string", "pattern": "^CEL[0-9]{4}$" },
    "summary": {
      "type": "object",
      "required": [
        "errors",
        "warnings",
        "notices",
        "baselined_hidden",
        "internal_errors",
        "exit_code"
      ],
      "additionalProperties": false,
      "properties": {
        "errors": { "type": "integer", "minimum": 0 },
        "warnings": { "type": "integer", "minimum": 0 },
        "notices": { "type": "integer", "minimum": 0 },
        "baselined_hidden": { "type": "integer", "minimum": 0 },
        "internal_errors": { "type": "integer", "minimum": 0 },
        "exit_code": { "enum": [0, 1, 2] }
      }
    },
    "notice": {
      "type": "object",
      "required": ["id", "message"],
      "additionalProperties": false,
      "properties": {
        "id": { "$ref": "#/$defs/identifier" },
        "message": { "type": "string" }
      }
    },
    "location": {
      "type": "object",
      "required": [
        "path",
        "start_line",
        "start_column",
        "end_line",
        "end_column",
        "byte_start",
        "byte_end"
      ],
      "additionalProperties": false,
      "properties": {
        "path": { "type": "string" },
        "start_line": { "type": "integer", "minimum": 1 },
        "start_column": { "type": "integer", "minimum": 1 },
        "end_line": { "type": "integer", "minimum": 1 },
        "end_column": { "type": "integer", "minimum": 1 },
        "byte_start": { "type": "integer", "minimum": 0 },
        "byte_end": { "type": "integer", "minimum": 0 }
      }
    },
    "anchor": {
      "oneOf": [
        {
          "type": "object",
          "required": ["kind"],
          "additionalProperties": false,
          "properties": { "kind": { "const": "project" } }
        },
        {
          "type": "object",
          "required": [
            "kind",
            "path",
            "start_line",
            "start_column",
            "end_line",
            "end_column",
            "byte_start",
            "byte_end"
          ],
          "additionalProperties": false,
          "properties": {
            "kind": { "const": "span" },
            "path": { "type": "string" },
            "start_line": { "type": "integer", "minimum": 1 },
            "start_column": { "type": "integer", "minimum": 1 },
            "end_line": { "type": "integer", "minimum": 1 },
            "end_column": { "type": "integer", "minimum": 1 },
            "byte_start": { "type": "integer", "minimum": 0 },
            "byte_end": { "type": "integer", "minimum": 0 }
          }
        }
      ]
    },
    "label": {
      "type": "object",
      "required": ["location", "message"],
      "additionalProperties": false,
      "properties": {
        "location": { "$ref": "#/$defs/location" },
        "message": { "type": "string" }
      }
    },
    "edit": {
      "type": "object",
      "required": ["location", "replacement"],
      "additionalProperties": false,
      "properties": {
        "location": { "$ref": "#/$defs/location" },
        "replacement": { "type": "string" }
      }
    },
    "suggestion": {
      "type": "object",
      "required": ["message", "confidence", "edits"],
      "additionalProperties": false,
      "properties": {
        "message": { "type": "string" },
        "confidence": { "enum": ["safe", "needs-review"] },
        "edits": { "type": "array", "items": { "$ref": "#/$defs/edit" } }
      }
    },
    "diagnostic": {
      "type": "object",
      "required": [
        "id",
        "severity",
        "anchor",
        "message",
        "labels",
        "notes",
        "suggestions"
      ],
      "additionalProperties": false,
      "properties": {
        "id": { "$ref": "#/$defs/identifier" },
        "severity": { "enum": ["error", "warning"] },
        "rule": { "type": "string" },
        "anchor": { "$ref": "#/$defs/anchor" },
        "message": { "type": "string" },
        "labels": { "type": "array", "items": { "$ref": "#/$defs/label" } },
        "notes": { "type": "array", "items": { "type": "string" } },
        "suggestions": {
          "type": "array",
          "items": { "$ref": "#/$defs/suggestion" }
        }
      }
    }
  }
}
```

Create `schemas/README.md`:

```markdown
# Schemas

- `celerrate-json-report.v1.schema.json`: the contract for
  `celerrate check --output=json`, authored here. Compatibility policy:
  adding a field is non-breaking and updates this file in the same
  release; removing a field or changing its meaning increments
  `schema_version` and forks a new file. The test suite validates real
  output against this file.
- `sarif-2.1.0.schema.json`: the official SARIF 2.1.0 schema, committed
  verbatim so the validation gate runs without network access.
  Provenance:
  https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json
  (mirror: https://json.schemastore.org/sarif-2.1.0.json).
```

(The SARIF file itself lands in Task 3; naming it here now keeps the README a single write.)

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --package celerrate_cli --test output_json`
Expected: PASS. If validation fails, the emitter and the schema disagree: fix whichever is wrong against the model in `output/model.rs` (the model is the design; the schema documents it).

- [ ] **Step 6: Full suite, lint, deny**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green. `cargo deny check` matters here: `jsonschema` brought a dependency tree.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/celerrate_cli schemas deny.toml
git commit -m "✨ feat(cli): commit the JSON report schema and validate output against it"
```

(Include `deny.toml` only if it actually changed.)

---

### Task 3: The SARIF writer and its 2.1.0 validation gate

**Files:**
- Modify: `crates/celerrate_cli/src/arguments.rs` (add `Sarif` to `OutputFormat`)
- Modify: `crates/celerrate_cli/src/output/mod.rs` (add `MachineFormat::Sarif`)
- Create: `crates/celerrate_cli/src/output/sarif.rs`
- Create: `schemas/sarif-2.1.0.schema.json` (downloaded, committed verbatim)
- Test: `crates/celerrate_cli/tests/output_sarif.rs`

**Interfaces:**
- Consumes: `output::model::MachineReport` and every type listed in Task 1's Produces block; `celerrate_diagnostics::{REGISTRY, find_identifier, find_page}`; the `jsonschema` dev-dependency from Task 2.
- Produces: `output::sarif::write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()>`; `OutputFormat::Sarif`; `MachineFormat::Sarif`.

- [ ] **Step 1: Commit the official SARIF schema**

```bash
mkdir -p schemas
curl -fsSL https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json -o schemas/sarif-2.1.0.schema.json
```

If the OASIS URL is unreachable, use the mirror `https://json.schemastore.org/sarif-2.1.0.json`. Sanity-check the download: the file starts with `{`, declares `"$schema": "http://json-schema.org/draft-04/schema#"`, and is a few hundred kilobytes. Commit it verbatim, no reformatting.

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_cli/tests/output_sarif.rs`. Re-declare the `project` / `check_with` / `MANIFEST` / `FAILING_EXAMPLE` / `CLEAN_EXAMPLE` / `findings_project` helpers exactly as in `tests/output_json.rs` (per-file duplication is the convention), then:

```rust
#[test]
fn sarif_validates_against_the_official_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/sarif-2.1.0.schema.json"))
            .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let findings = findings_project();
    let clean = project(&[("composer.json", MANIFEST), ("src/Clean.php", CLEAN_EXAMPLE)]);
    let noticed = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    for root in [findings.path(), clean.path(), noticed.path()] {
        let (_, text) = check_with(root, &["--output", "sarif"]);
        let instance: serde_json::Value = serde_json::from_str(&text).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}

#[test]
fn results_carry_locations_and_referenced_rules_are_described() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "sarif"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let run = &value["runs"][0];
    assert_eq!(run["columnKind"], "unicodeCodePoints");
    let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
    let described: Vec<&str> = rules
        .iter()
        .map(|rule| rule["id"].as_str().unwrap())
        .collect();
    let results = run["results"].as_array().unwrap();
    assert!(!results.is_empty());
    for result in results {
        let rule_id = result["ruleId"].as_str().unwrap();
        assert!(described.contains(&rule_id), "{rule_id} lacks a descriptor");
        if result["level"] != "note" {
            let region = &result["locations"][0]["physicalLocation"]["region"];
            assert!(region["startLine"].as_u64().unwrap() >= 1);
        }
    }
}

#[test]
fn notices_become_note_level_results_without_location() {
    let root = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    let (_, text) = check_with(root.path(), &["--output", "sarif"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let results = value["runs"][0]["results"].as_array().unwrap();
    let notes: Vec<_> = results.iter().filter(|r| r["level"] == "note").collect();
    assert!(!notes.is_empty());
    for note in notes {
        assert!(note.get("locations").is_none());
    }
}

#[test]
fn the_invocation_embeds_the_exit_code() {
    let root = findings_project();
    let (outcome, text) = check_with(root.path(), &["--output", "sarif"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let invocation = &value["runs"][0]["invocations"][0];
    assert_eq!(invocation["exitCode"].as_u64().unwrap(), u64::from(outcome.code()));
    assert_eq!(invocation["executionSuccessful"], true);
}

#[test]
fn sarif_findings_snapshot() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "sarif"]);
    insta::assert_snapshot!("sarif_findings", text);
}
```

(`outcome.code()` requires `Outcome::code` to be public, which Task 1 made it.)

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --test output_sarif`
Expected: FAIL inside `run` ("invalid value 'sarif' for '--output'"), before any assertion.

- [ ] **Step 4: Extend the enums**

In `arguments.rs`, add to `OutputFormat`:

```rust
/// SARIF 2.1.0 for code-scanning integrations.
Sarif,
```

and `Self::Sarif => "sarif"` in `as_argument`. In `output/mod.rs`, add `Sarif` to `MachineFormat`, `OutputFormat::Sarif => Some(Self::Sarif)` to `of`, `MachineFormat::Sarif => sarif::write(output, report)` to `write`, and `pub mod sarif;`.

- [ ] **Step 5: Write the SARIF writer**

Create `crates/celerrate_cli/src/output/sarif.rs`:

```rust
//! `--output=sarif`: SARIF 2.1.0, the honest subset. What SARIF cannot
//! carry (the needs-review confidence, the engine notes) rides in
//! `properties`, never twisted into a standard field.

use std::io::{self, Write};

use serde_json::{Value, json};

use super::model::{
    MachineReport, ReportedAnchor, ReportedDiagnostic, ReportedSeverity,
    ReportedSuggestion, SpanLocation,
};

pub fn write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()> {
    let document = document(report);
    serde_json::to_writer_pretty(&mut *output, &document).map_err(io::Error::from)?;
    writeln!(output)
}

fn document(report: &MachineReport) -> Value {
    json!({
        "$schema": "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "celerrate",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/celerrate/celerrate",
                    "rules": rules(report),
                }
            },
            "columnKind": "unicodeCodePoints",
            "invocations": [{
                "executionSuccessful": report.summary.internal_errors == 0,
                "exitCode": report.summary.exit_code,
            }],
            "results": results(report),
        }]
    })
}

/// Reporting descriptors for exactly the identifiers this run referenced,
/// sorted and unique: deterministic output, no dead catalogue.
fn rules(report: &MachineReport) -> Vec<Value> {
    let mut identifiers: Vec<&str> = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.id.as_str())
        .chain(report.notices.iter().map(|notice| notice.id.as_str()))
        .collect();
    identifiers.sort_unstable();
    identifiers.dedup();
    identifiers
        .into_iter()
        .map(|text| match described(text) {
            Some((family, why)) => json!({
                "id": text,
                "shortDescription": { "text": family },
                "fullDescription": { "text": why },
                "help": {
                    "text": format!("Run `celerrate explain {text}` for the full page."),
                },
            }),
            // Resilience: an identifier outside the registry still gets
            // a descriptor, never a crash.
            None => json!({ "id": text }),
        })
        .collect()
}

fn described(text: &str) -> Option<(&'static str, &'static str)> {
    let id = celerrate_diagnostics::find_identifier(text)?;
    let entry = celerrate_diagnostics::REGISTRY
        .iter()
        .find(|entry| entry.id == id)?;
    Some((entry.family, entry.explain.why))
}

fn results(report: &MachineReport) -> Vec<Value> {
    let mut results = Vec::new();
    for notice in &report.notices {
        results.push(json!({
            "ruleId": notice.id,
            "level": "note",
            "message": { "text": notice.message },
        }));
    }
    for diagnostic in &report.diagnostics {
        results.push(result(diagnostic));
    }
    results
}

fn result(diagnostic: &ReportedDiagnostic) -> Value {
    let level = match diagnostic.severity {
        ReportedSeverity::Error => "error",
        ReportedSeverity::Warning => "warning",
    };
    let mut value = json!({
        "ruleId": diagnostic.id,
        "level": level,
        "message": { "text": diagnostic.message },
    });
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    if let ReportedAnchor::Span(location) = &diagnostic.anchor {
        object.insert(
            "locations".to_owned(),
            json!([physical_location(location)]),
        );
    }
    if !diagnostic.labels.is_empty() {
        let related: Vec<Value> = diagnostic
            .labels
            .iter()
            .map(|label| {
                let mut related = physical_location(&label.location);
                if let Some(object) = related.as_object_mut() {
                    object.insert(
                        "message".to_owned(),
                        json!({ "text": label.message }),
                    );
                }
                related
            })
            .collect();
        object.insert("relatedLocations".to_owned(), json!(related));
    }
    let (safe, needs_review): (Vec<&ReportedSuggestion>, Vec<&ReportedSuggestion>) =
        diagnostic
            .suggestions
            .iter()
            .partition(|suggestion| {
                suggestion.confidence == super::model::ReportedConfidence::Safe
            });
    if !safe.is_empty() {
        let fixes: Vec<Value> = safe.iter().map(|suggestion| fix(suggestion)).collect();
        object.insert("fixes".to_owned(), json!(fixes));
    }
    let mut properties = serde_json::Map::new();
    if !needs_review.is_empty() {
        properties.insert(
            "needsReviewSuggestions".to_owned(),
            serde_json::to_value(&needs_review).unwrap_or(Value::Null),
        );
    }
    if !diagnostic.notes.is_empty() {
        properties.insert(
            "notes".to_owned(),
            serde_json::to_value(&diagnostic.notes).unwrap_or(Value::Null),
        );
    }
    if !properties.is_empty() {
        object.insert("properties".to_owned(), Value::Object(properties));
    }
    value
}

fn physical_location(location: &SpanLocation) -> Value {
    json!({
        "physicalLocation": {
            "artifactLocation": { "uri": location.path },
            "region": {
                "startLine": location.start_line,
                "startColumn": location.start_column,
                "endLine": location.end_line,
                "endColumn": location.end_column,
                "byteOffset": location.byte_start,
                "byteLength": location.byte_end.saturating_sub(location.byte_start),
            }
        }
    })
}

fn fix(suggestion: &ReportedSuggestion) -> Value {
    let changes: Vec<Value> = suggestion
        .edits
        .iter()
        .map(|edit| {
            json!({
                "artifactLocation": { "uri": edit.location.path },
                "replacements": [{
                    "deletedRegion": {
                        "byteOffset": edit.location.byte_start,
                        "byteLength": edit.location.byte_end
                            .saturating_sub(edit.location.byte_start),
                    },
                    "insertedContent": { "text": edit.replacement },
                }]
            })
        })
        .collect();
    json!({
        "description": { "text": suggestion.message },
        "artifactChanges": changes,
    })
}
```

`serde_json::to_value(...).unwrap_or(Value::Null)` is not an `unwrap`: it is a total fallback (serialization of these plain structs cannot fail, and if it ever did, `null` in a properties bag is the honest degradation). If the SARIF schema rejects mixing text and byte region properties (it should not; the spec allows both), drop `byteOffset`/`byteLength` from `physical_location` (keep them in fixes, where they are the payload).

- [ ] **Step 6: Run the tests**

Run: `cargo test --package celerrate_cli --test output_sarif`
Expected: PASS except the snapshot pending review. Read the snapshot: `rules` must describe every referenced identifier, notices must be `note` results, positions 1-based. Accept after reading.

- [ ] **Step 7: Full suite, lint**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green; `--help` snapshots may legitimately change if one pins the `--output` value list (accept only that).

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_cli schemas
git commit -m "✨ feat(cli): emit SARIF 2.1.0 behind --output=sarif"
```

---

### Task 4: The GitHub Actions writer

**Files:**
- Modify: `crates/celerrate_cli/src/arguments.rs` (add `Github`)
- Modify: `crates/celerrate_cli/src/output/mod.rs` (add `MachineFormat::Github`)
- Create: `crates/celerrate_cli/src/output/github.rs`
- Test: `crates/celerrate_cli/tests/output_github.rs`

**Interfaces:**
- Consumes: `output::model::MachineReport` and its types; `crate::render::count` (made `pub(crate)` in Task 1).
- Produces: `output::github::write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()>`; `OutputFormat::Github`; `MachineFormat::Github`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_cli/tests/output_github.rs` with the same duplicated helpers, then:

```rust
#[test]
fn findings_become_error_annotations_with_positions() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    let annotations: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("::error") || line.starts_with("::warning"))
        .collect();
    assert!(!annotations.is_empty());
    for annotation in &annotations {
        assert!(annotation.contains("file=src/Example.php"), "{annotation}");
        assert!(annotation.contains("line="), "{annotation}");
        assert!(annotation.contains("col="), "{annotation}");
        assert!(annotation.contains("::CEL"), "{annotation}");
    }
}

#[test]
fn notices_become_notice_commands() {
    let root = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    assert!(text.lines().any(|line| line.starts_with("::notice::CEL")));
}

#[test]
fn the_summary_closes_the_output() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    let last = text.lines().last().unwrap();
    assert!(last.contains("diagnostic"), "{last}");
    assert!(!last.starts_with("::"), "{last}");
}

#[test]
fn github_findings_snapshot() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    insta::assert_snapshot!("github_findings", text);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --test output_github`
Expected: FAIL ("invalid value 'github' for '--output'").

- [ ] **Step 3: Extend the enums and write the writer**

`arguments.rs`: add `/// GitHub Actions workflow commands for pull-request annotations.` `Github,` and `Self::Github => "github"`. `output/mod.rs`: add the variant, the `of` arm, the dispatch arm, `pub mod github;`.

Create `crates/celerrate_cli/src/output/github.rs`:

```rust
//! `--output=github`: workflow commands for native pull-request
//! annotations, one line per finding, then the human summary wording.

use std::io::{self, Write};

use super::model::{MachineReport, ReportedAnchor, ReportedSeverity};
use crate::render::count;

pub fn write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()> {
    for notice in &report.notices {
        writeln!(
            output,
            "::notice::{}",
            escape_data(&format!("{}: {}", notice.id, notice.message)),
        )?;
    }
    for diagnostic in &report.diagnostics {
        let command = match diagnostic.severity {
            ReportedSeverity::Error => "error",
            ReportedSeverity::Warning => "warning",
        };
        let text = escape_data(&format!("{}: {}", diagnostic.id, diagnostic.message));
        match &diagnostic.anchor {
            ReportedAnchor::Span(location) => writeln!(
                output,
                "::{command} file={},line={},col={},endLine={},endColumn={}::{text}",
                escape_property(&location.path),
                location.start_line,
                location.start_column,
                location.end_line,
                location.end_column,
            )?,
            ReportedAnchor::Project => writeln!(output, "::{command}::{text}")?,
        }
    }
    writeln!(
        output,
        "{}, {}",
        count(report.summary.notices, "notice", "notices"),
        count(
            report.summary.errors + report.summary.warnings,
            "diagnostic",
            "diagnostics",
        ),
    )?;
    if report.summary.baselined_hidden > 0 {
        writeln!(
            output,
            "{} hidden",
            count(
                report.summary.baselined_hidden,
                "baselined diagnostic",
                "baselined diagnostics",
            ),
        )?;
    }
    Ok(())
}

/// The workflow-command data escaping GitHub documents: percent, then
/// carriage return, then newline.
fn escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Property values additionally escape the separators.
fn escape_property(value: &str) -> String {
    escape_data(value).replace(',', "%2C").replace(':', "%3A")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

    use super::*;

    #[test]
    fn data_escaping_covers_percent_and_line_breaks() {
        assert_eq!(escape_data("50% done\r\nnext"), "50%25 done%0D%0Anext");
    }

    #[test]
    fn property_escaping_also_covers_separators() {
        assert_eq!(escape_property("a,b:c"), "a%2Cb%3Ac");
    }
}
```

Match the exact `count` signature from `render.rs` when calling it (it may take `usize` plus two `&str`; adapt the calls, not the helper).

- [ ] **Step 4: Run the tests**

Run: `cargo test --package celerrate_cli --test output_github`
Expected: PASS with the snapshot pending; read and accept it (every annotation line well-formed, summary last).

- [ ] **Step 5: Full suite, lint, commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): emit GitHub workflow commands behind --output=github"
```

---

### Task 5: Cross-format guarantees

**Files:**
- Test: `crates/celerrate_cli/tests/output_equivalence.rs`

**Interfaces:**
- Consumes: the three machine formats end to end via `run`; `celerrate_cli::color_mode(stdout_is_terminal: bool, no_color: Option<&OsStr>) -> ColorMode` (check the exact signature in `lib.rs:40-47` and adapt).
- Produces: nothing new; this task is the spec's cross-format equivalence and determinism evidence (spec section 9).

- [ ] **Step 1: Write the tests**

Create `crates/celerrate_cli/tests/output_equivalence.rs` with the duplicated helpers plus:

```rust
fn identifier_regex_matches(text: &str) -> Vec<String> {
    // CEL followed by four digits, in order of appearance.
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        if &bytes[i..i + 3] == b"CEL"
            && bytes[i + 3..i + 7].iter().all(u8::is_ascii_digit)
        {
            found.push(text[i..i + 7].to_owned());
            i += 7;
        } else {
            i += 1;
        }
    }
    found
}

fn json_diagnostic_ids(text: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_owned())
        .collect()
}

fn sarif_finding_ids(text: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    value["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["level"] != "note")
        .map(|r| r["ruleId"].as_str().unwrap().to_owned())
        .collect()
}

fn github_finding_ids(text: &str) -> Vec<String> {
    // One identifier per annotation line: the leading `CELxxxx: ` token.
    // A message could itself mention an identifier, so only the first
    // match on each line counts.
    text.lines()
        .filter(|line| line.starts_with("::error") || line.starts_with("::warning"))
        .filter_map(|line| identifier_regex_matches(line).into_iter().next())
        .collect()
}

#[test]
fn every_format_reports_the_same_findings_in_the_same_order() {
    let root = findings_project();
    let (_, json) = check_with(root.path(), &["--output", "json"]);
    let (_, sarif) = check_with(root.path(), &["--output", "sarif"]);
    let (_, github) = check_with(root.path(), &["--output", "github"]);
    let (_, human) = check_with(root.path(), &[]);
    let ids = json_diagnostic_ids(&json);
    assert!(!ids.is_empty());
    assert_eq!(ids, sarif_finding_ids(&sarif));
    assert_eq!(ids, github_finding_ids(&github));
    for id in &ids {
        assert!(human.contains(id), "{id} missing from the human report");
    }
}

#[test]
fn every_format_agrees_on_the_outcome() {
    let root = findings_project();
    let mut outcomes = Vec::new();
    for format in ["human", "json", "sarif", "github"] {
        outcomes.push(check_with(root.path(), &["--output", format]).0);
    }
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]), "{outcomes:?}");
}

#[test]
fn machine_output_is_deterministic_across_runs() {
    let root = findings_project();
    for format in ["json", "sarif", "github"] {
        let (_, first) = check_with(root.path(), &["--output", format]);
        let (_, second) = check_with(root.path(), &["--output", format]);
        assert_eq!(first, second, "{format}");
    }
}

#[test]
fn machine_output_ignores_the_color_mode() {
    let root = findings_project();
    let colored = celerrate_cli::color_mode(true, None);
    for format in ["json", "sarif", "github"] {
        let mut plain_buffer = Vec::new();
        let mut colored_buffer = Vec::new();
        let arguments = |root: &Path| -> Vec<OsString> {
            vec![
                "celerrate".into(),
                "check".into(),
                root.as_os_str().into(),
                "--output".into(),
                format.into(),
            ]
        };
        run(arguments(root.path()), &mut plain_buffer, ColorMode::Plain);
        run(arguments(root.path()), &mut colored_buffer, colored);
        assert_eq!(plain_buffer, colored_buffer, "{format}");
        assert!(!plain_buffer.contains(&0x1b), "{format} emitted an ANSI escape");
    }
}
```

The determinism test runs the same project twice, so the second run is warm: byte-equality here also proves the warm and cold paths serialize identically, which is the spec's warm/cold equivalence extended to machine formats for free. Note it in the test's doc comment.

- [ ] **Step 2: Run, expect green**

Run: `cargo test --package celerrate_cli --test output_equivalence`
Expected: PASS on the first run (these are properties Tasks 1 to 4 already established; a failure here is a real bug in a writer, not a test to adjust). If ordering differs between JSON and SARIF, remember SARIF prepends notices as `note` results; the filter in `sarif_finding_ids` accounts for that.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_cli/tests/output_equivalence.rs
git commit -m "✅ test(cli): pin cross-format equivalence, determinism, and color immunity"
```

---

### Task 6: Documentation and changelog

**Files:**
- Create: `docs/output-formats.md`
- Modify: `crates/celerrate_cli/tests/documentation.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: the `workspace_page` helper in `tests/documentation.rs`; the shipped behavior of Tasks 1 to 5.
- Produces: the docs page the README and the release task will link to.

- [ ] **Step 1: Write the failing drift gate**

Append to `crates/celerrate_cli/tests/documentation.rs` (reusing its `workspace_page` helper):

```rust
#[test]
fn every_output_format_is_documented() {
    let page = workspace_page("docs/output-formats.md");
    for format in ["human", "json", "sarif", "github"] {
        assert!(
            page.contains(&format!("`{format}`")),
            "docs/output-formats.md does not document `{format}`",
        );
    }
    assert!(page.contains("schema_version"));
    assert!(page.contains("schemas/celerrate-json-report.v1.schema.json"));
}
```

Run: `cargo test --package celerrate_cli --test documentation every_output_format`
Expected: FAIL (missing page).

- [ ] **Step 2: Write the page**

Create `docs/output-formats.md`:

```markdown
# Output formats

`celerrate check` serializes its report with `--output`:

- `human` (the default): the rich terminal report.
- `json`: a stable, versioned document for tooling.
- `sarif`: SARIF 2.1.0 for code-scanning integrations.
- `github`: GitHub Actions workflow commands for pull-request
  annotations.

One format per run. Every format serializes the same final stream: the
same diagnostics, in the same order, after suppression and after the
baseline, with the same exit code. Machine formats write exactly one
document to stdout; meta-reporting goes to stderr.

A machine format cannot be combined with `--watch`, `--fix`,
`--fix-suggestions`, or `--baseline` (recording): those runs loop or
mutate, and their interactive reporting is the human channel's job.
Applying an existing baseline works with every format, and
`--ignore-baseline` does too.

## JSON

```sh
celerrate check --output=json
```

The root object carries `schema_version` (currently 1), a `summary`
(`errors`, `warnings`, `notices`, `baselined_hidden`, `internal_errors`,
`exit_code`), the exit-neutral `notices`, and the `diagnostics` in the
total deterministic order. Each diagnostic exposes its identifier,
severity, owning rule name (when a rule owns the identifier), anchor
(`project`, or a `span` with a project-relative path, 1-based lines and
columns, and exact byte offsets), message, resolved secondary labels,
notes, and suggestions with their edits and confidence (`safe` or
`needs-review`). Columns count Unicode code points; byte offsets index
the file's UTF-8 bytes.

The schema is committed at `schemas/celerrate-json-report.v1.schema.json`
and the test suite validates real output against it.

Compatibility policy: adding a field is non-breaking (and updates the
schema file in the same release); removing a field or changing its
meaning increments `schema_version`.

## SARIF

```sh
celerrate check --output=sarif
```

SARIF 2.1.0, validated against the official schema in CI. Referenced
identifiers are described under `tool.driver.rules` (short description,
full description, an `explain` pointer). Findings become `results` with
physical locations (`columnKind` is `unicodeCodePoints`); exit-neutral
notices become `level: note` results without location. Safe suggestions
become `fixes` with byte-precise replacements; what SARIF cannot carry
honestly (needs-review suggestions, engine notes) rides in `properties`.

Upload to GitHub code scanning:

```yaml
- run: celerrate check --output=sarif > celerrate.sarif || true
- uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: celerrate.sarif
```

## GitHub Actions

```sh
celerrate check --output=github
```

One workflow command per finding (`::error` or `::warning` with `file`,
`line`, `col`, `endLine`, `endColumn`), `::notice` for exit-neutral
notices, then the end-of-run summary. GitHub renders these as native
pull-request annotations with no further setup:

```yaml
- run: celerrate check --output=github
```

## Exit codes

Identical in every format: 0 clean, 1 diagnostics reported, 2 internal
or usage error. The JSON summary and the SARIF invocation embed the same
number the process exits with.
```

- [ ] **Step 3: Run the gate, then the changelog**

Run: `cargo test --package celerrate_cli --test documentation`
Expected: PASS.

Under `## [Unreleased]` / `### Added` in `CHANGELOG.md`, matching the file's prose style (wrapped near 72 columns):

```markdown
- Machine output formats: `celerrate check --output=json` emits a stable
  versioned document (schema committed at
  `schemas/celerrate-json-report.v1.schema.json`), `--output=sarif` emits
  SARIF 2.1.0 validated against the official schema, and
  `--output=github` emits workflow commands for native pull-request
  annotations. Every format serializes the same final stream as the
  human report: post-suppression, post-baseline, same order, same exit
  code. See `docs/output-formats.md`.
```

- [ ] **Step 4: Commit**

```bash
git add docs/output-formats.md crates/celerrate_cli/tests/documentation.rs
git commit -m "📝 docs(output): document the machine output formats"
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the output formats under Unreleased"
```

---

### Task 7: Verification and hand off

**Files:** none (verification only).

- [ ] **Step 1: The full mechanical suite**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo xtask dependency-shape
cargo xtask emission-scan
```

Expected: all green. `dependency-shape` and `emission-scan` must pass untouched: `celerrate_cli` is a composition root (exempt), and no domain crate gained an emission site.

- [ ] **Step 2: The corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: the diagnostic snapshot does not move (default behavior is unchanged: without `--output`, nothing new runs — spec closure gate 1 logic), and the mixed-rate baseline is unchanged (no type work happened — gate 9). Any delta is a bug in this plan's wiring: stop and investigate (systematic-debugging), do not re-bless.

- [ ] **Step 3: Hand off**

The branch is ready for review and merge (pull request per repository convention). Spec closure gate 5 (formats) is delivered: the JSON schema is versioned, committed, and validated against; SARIF validates against the 2.1.0 schema; the GitHub format is snapshot-pinned; cross-format equivalence is test-pinned. The remaining sub-project 5 work (migrate `--from-phpstan`, the verbose channel, distribution, benchmark, release) continues in later plans.

---

## Explicitly rejected

- **A renderer trait unifying human and machine writers**: the human
  renderer consumes blocks and terminal geometry, the machine writers
  consume the structured model; forcing one interface would flatten the
  richer of the two. The shared contract is the input stream, not a trait.
- **serde on `celerrate_diagnostics`**: the `StoredDiagnostic` precedent
  already establishes CLI-owned DTOs; the diagnostics crate stays
  dependency-minimal.
- **Machine formats under `--watch`**: a stream of concatenated JSON
  documents is a new contract the spec did not open; watch stays human.
- **Recording (`--baseline`) or fixing under a machine format**: mutating
  runs report interactively; their confirmations are human surface.
- **Emitting all 51 registry identifiers into SARIF `rules`**: dead
  catalogue weight in every report; referenced-only is deterministic and
  small.
- **A `helpUri` per SARIF rule**: no stable per-identifier URL exists in
  v0.1; a synthesized docs anchor would rot silently.
- **Compact JSON**: pretty-printing costs nothing at this scale and makes
  snapshots and CI diffs reviewable.
- **A permissive JSON Schema (`additionalProperties: true`)**: the schema
  ships in lockstep with the emitter; strictness turns emitter drift into
  a red gate instead of silent contract creep.
- **Recomputing the exit code inside a writer**: the payload embeds the
  one `Outcome` value the process returns; two derivations of the same
  number is how the watch arm already drifted once.
