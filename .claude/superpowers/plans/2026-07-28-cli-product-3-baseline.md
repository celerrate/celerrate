# Baseline (CLI Product Part 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `celerrate check --baseline` records `celerrate-baseline.toml`; a present file hides recorded findings from the report and the exit code; obsolete entries surface as an exit-neutral notice.

**Architecture:** All baseline mechanics live in `celerrate_cli` (a new `baseline` module), entirely outside salsa queries: the filter applies after analysis, suppression, and enrichment, before configuration-diagnostic merging, rendering, and the exit code. Persisted cache verdicts stay pre-baseline by construction. The entry key is structural — `(relative path, CEL identifier, enclosing symbol path, message, count)` — with no line number anywhere.

**Tech Stack:** Rust, `toml_edit` 0.23 (already a workspace dependency, used both to parse and to serialize), `clap` 4 derive, `insta` for snapshots, in-process CLI integration tests.

**Spec:** `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`, section 4 (and section 2 for identifier allocation). Read it before deviating from anything below.

**Branch:** work on `feat-cli-baseline` off `main`.

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` forbidden. Test modules may locally `#[allow]` these lints (existing convention: `#![allow(clippy::unwrap_used, clippy::indexing_slicing)]` at the top of test files/modules).
- TDD: failing test first, minimal implementation, refactor. No production code without a test that demanded it.
- The baseline never enters queries or the persistent cache: it is presentation, applied at the CLI layer. `cache::persist` keeps consuming the pre-baseline `outcome`.
- The baseline covers span-anchored diagnostics only; configuration diagnostics (in `celerrate.toml`) and project notices are never baselined.
- No line numbers (and no line hashes) in baseline entries — explicitly rejected by the spec.
- No automatic pruning of obsolete entries — a notice plus explicit re-record.
- No `baseline` key in `celerrate.toml`: the file path is fixed at `<root>/celerrate-baseline.toml`.
- Entry paths use `/` separators on every platform; entries are deterministically sorted.
- Everything written in English, full words. Commits: gitmoji + Conventional Commits.
- Full gate suite guards every task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`, `cargo deny check`.

## Load-bearing codebase facts (verified 2026-07-28)

- CLI args: `crates/celerrate_cli/src/arguments.rs` — clap derive, `Command::Check { path, watch, fix, fix_suggestions }` at lines 20-37. Usage errors exit 2 via `Outcome::UsageError`.
- Single-pass pipeline: `crates/celerrate_cli/src/lib.rs:118-157`. Order: `Session::start` → `analyze` → `absorb_outcome` → `presented = { suggest::enrich(...), ... }` → `configuration::merge_diagnostics(&session, &mut presented)` → `render::render_report(output, &session, &presented, color)` → `cache::persist(&mut session, &outcome)` → fix → `Outcome::of(outcome.diagnostics.len() + configuration_diagnostics, session.internal_errors.len())`.
- Watch pipeline: `crates/celerrate_cli/src/watch.rs`, per-cycle assembly in `completed_cycle` (lines 109-160): `presented = outcome.clone()` → `merge_diagnostics` → `render_cycle`; exit-code path at lines 331-335. No `suggest::enrich` in watch.
- Exit code: `Outcome::of(diagnostics, internal_errors)` counts blindly — any diagnostic (warning or error) exits 1. Notices are exit-neutral only because they ride a separate channel (`Session::notices()` → `write_notices`, `render.rs:147`), never `AnalysisOutcome::diagnostics`.
- Summary line: `write_summary_line(output, notice_count, diagnostic_count)` at `render.rs:181-192`, prints `"{N} notices, {M} diagnostics"`, called from `render_report` and `render_cycle`.
- `Diagnostic` (`crates/celerrate_diagnostics/src/diagnostic.rs:43`): `{ id: DiagnosticId, severity, anchor, message: String (pre-rendered), labels, notes, suggestions }`; `span() -> Option<(FileId, TextRange)>` is `None` for `Anchor::Project`. Enrichment mutates labels/notes/suggestions only — id, anchor, and message are stable across `suggest::enrich`, so a filter keyed on them may run on either list.
- `DiagnosticId` is a `&'static str` newtype (`identifier.rs:7`). Next free identifier: **CEL0050** (registry gapless test asserts `previous == 49` at `crates/celerrate_diagnostics/src/registry.rs:383`).
- No enclosing-symbol query exists anywhere. Building blocks: `celerrate_semantics::{analyzed_file_index, ast_id_map, member_tree}` queries; `MemberTree { classes: Vec<ClassMembers>, functions: Vec<FreeFunction> }` where `ClassMembers { name: Option<String>, namespace, ast_id, members: Vec<Member>, .. }`, `Member { name, ast_id, .. }`, `FreeFunction { name, namespace, ast_id, .. }`; `celerrate_semantics::fully_qualified_name(namespace, name) -> String`. Root-node acquisition + `pointer.try_to_node(&root)` pattern: copy from `DatabaseResolver::first_line_of`, `crates/celerrate_rules/src/render/resolve.rs:49-66`.
- Relative display paths: `render.rs:356` `relative_path` strips `session.discovery.root`, emits OS-native separators — the baseline must normalize to `/`.
- Atomic file write precedent: `write_atomically(path, bytes)` in `crates/celerrate_cli/src/cache/pack.rs:146-158`.
- `celerrate.toml` load precedent (the shape `baseline::load` mirrors): `crates/celerrate_cli/src/configuration.rs:48-84`; stored as `Session.loaded_configuration`; reloaded under watch via `Session::rediscover` (session.rs:517) because `is_project_manifest` (session.rs:504-509) matches it and `Watch::spawn` registers it (~watch.rs:556).
- Registry-test gotcha: `crates/celerrate_cli/tests/registry.rs` derives producers from the CLI's *dependencies* — `celerrate_cli` itself is not scanned. Task 8 extends the derivation.
- Integration test conventions: in-process `celerrate_cli::run(Vec<OsString>, &mut Vec<u8>, ColorMode::Plain)`; each test file re-declares its own `project(&[(path, contents)]) -> TempDir` and `check(root) -> (Outcome, String)` helpers (deliberate duplication); see `tests/check.rs:11-30` and the flag-carrying `check_with` in `tests/fix.rs:24-31`.

## File structure

- Create `crates/celerrate_cli/src/baseline/mod.rs` — public surface: file-name constant, identifier constants, `Mode`, `LoadedBaseline`, `load`, `fingerprint`, `apply`, `record`, `BaselineOutcome`, `BaselineNotice`.
- Create `crates/celerrate_cli/src/baseline/entry.rs` — `BaselineEntry`, `BaselineKey`, ordering.
- Create `crates/celerrate_cli/src/baseline/file.rs` — versioned TOML parse/serialize.
- Create `crates/celerrate_cli/src/baseline/symbol.rs` — enclosing symbol path computation.
- Create `crates/celerrate_cli/tests/baseline.rs` — integration tests (flags, record, apply, properties).
- Modify `crates/celerrate_cli/src/lib.rs` — module declaration, check-flow wiring, exit-code adjustment.
- Modify `crates/celerrate_cli/src/arguments.rs` — `--baseline`, `--ignore-baseline`.
- Modify `crates/celerrate_cli/src/session.rs` — `loaded_baseline` field, load at start, reload in `rediscover`, `is_project_manifest`.
- Modify `crates/celerrate_cli/src/render.rs` — baseline notices, hidden/recorded summary lines, signature threading.
- Modify `crates/celerrate_cli/src/watch.rs` — mode threading, per-cycle filter, manifest registration, exit count.
- Modify `crates/celerrate_diagnostics/src/registry.rs`, create `crates/celerrate_diagnostics/src/pages/baseline.rs` — CEL0050/CEL0051 (task 8 only).
- Modify `crates/celerrate_cli/tests/{registry.rs, explain_pages.rs, suppression_correspondence.rs}`, `docs/diagnostics.md`, `CHANGELOG.md`.

---

### Task 1: Baseline entry model and versioned TOML file format

**Files:**
- Create: `crates/celerrate_cli/src/baseline/mod.rs`
- Create: `crates/celerrate_cli/src/baseline/entry.rs`
- Create: `crates/celerrate_cli/src/baseline/file.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (add `pub mod baseline;` next to the existing module declarations)

**Interfaces:**
- Consumes: `toml_edit` (already a dependency of the workspace; add `toml_edit.workspace = true` to `crates/celerrate_cli/Cargo.toml` dependencies).
- Produces (later tasks rely on these exact names):
  - `baseline::BASELINE_FILE_NAME: &str = "celerrate-baseline.toml"`
  - `baseline::entry::BaselineEntry { path: String, identifier: String, symbol: String, message: String, count: u32 }` with derived `Ord` (field order path → identifier → symbol → message → count) and `fn key(&self) -> BaselineKey`
  - `baseline::entry::BaselineKey { path, identifier, symbol, message }` (all `String`, derived `Ord`)
  - `baseline::file::FORMAT_VERSION: i64 = 1`
  - `baseline::file::ParsedBaseline { entries: Vec<BaselineEntry>, failures: Vec<String> }`
  - `baseline::file::parse(text: &str) -> ParsedBaseline`
  - `baseline::file::serialize(entries: &[BaselineEntry]) -> String`

- [ ] **Step 1: Write the failing round-trip and resilience tests**

In `crates/celerrate_cli/src/baseline/file.rs`, start with the test module (the implementation does not exist yet):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::baseline::entry::BaselineEntry;

    fn entry(path: &str, identifier: &str, symbol: &str, message: &str, count: u32) -> BaselineEntry {
        BaselineEntry {
            path: path.to_string(),
            identifier: identifier.to_string(),
            symbol: symbol.to_string(),
            message: message.to_string(),
            count,
        }
    }

    #[test]
    fn serialize_then_parse_round_trips() {
        let entries = vec![
            entry("src/B.php", "CEL0018", "App\\B::run", "unknown class `Missing`", 2),
            entry("src/A.php", "CEL0018", "(top level)", "unknown class `Missing`", 1),
        ];
        let text = serialize(&entries);
        let parsed = parse(&text);
        assert!(parsed.failures.is_empty(), "failures: {:?}", parsed.failures);
        // Serialization sorts: A.php before B.php.
        assert_eq!(parsed.entries[0].path, "src/A.php");
        assert_eq!(parsed.entries[1].count, 2);
        assert_eq!(parsed.entries.len(), 2);
    }

    #[test]
    fn serialization_is_deterministic_regardless_of_input_order() {
        let forward = vec![
            entry("src/A.php", "CEL0018", "(top level)", "m", 1),
            entry("src/B.php", "CEL0018", "(top level)", "m", 1),
        ];
        let backward: Vec<_> = forward.iter().rev().cloned().collect();
        assert_eq!(serialize(&forward), serialize(&backward));
    }

    #[test]
    fn messages_with_toml_special_characters_round_trip() {
        let entries = vec![entry(
            "src/A.php",
            "CEL0030",
            "App\\A::run",
            "unknown method `save` on `App\\User` with \"quotes\"\nand a newline",
            1,
        )];
        let parsed = parse(&serialize(&entries));
        assert!(parsed.failures.is_empty(), "failures: {:?}", parsed.failures);
        assert_eq!(parsed.entries, entries);
    }

    #[test]
    fn invalid_toml_reports_one_failure_and_no_entries() {
        let parsed = parse("version = 1\n[[entry]\n");
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.failures.len(), 1);
        assert!(parsed.failures[0].contains("invalid TOML"), "was: {}", parsed.failures[0]);
    }

    #[test]
    fn a_missing_or_unsupported_version_rejects_the_whole_file() {
        let missing = parse("[[entry]]\npath = \"a\"\n");
        assert!(missing.entries.is_empty());
        assert!(missing.failures[0].contains("version"), "was: {}", missing.failures[0]);

        let unsupported = parse("version = 2\n");
        assert!(unsupported.entries.is_empty());
        assert!(unsupported.failures[0].contains("version 2"), "was: {}", unsupported.failures[0]);
    }

    #[test]
    fn a_malformed_entry_is_reported_and_the_valid_ones_still_apply() {
        let text = "version = 1\n\n[[entry]]\npath = \"src/A.php\"\nidentifier = \"CEL0018\"\nsymbol = \"(top level)\"\nmessage = \"m\"\ncount = 1\n\n[[entry]]\npath = \"src/B.php\"\ncount = 0\n";
        let parsed = parse(text);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].path, "src/A.php");
        assert!(!parsed.failures.is_empty());
    }

    #[test]
    fn an_unknown_key_in_an_entry_is_reported_but_the_entry_still_applies() {
        let text = "version = 1\n\n[[entry]]\npath = \"src/A.php\"\nidentifier = \"CEL0018\"\nsymbol = \"(top level)\"\nmessage = \"m\"\ncount = 1\nline = 12\n";
        let parsed = parse(text);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.failures.len(), 1);
        assert!(parsed.failures[0].contains("`line`"), "was: {}", parsed.failures[0]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p celerrate_cli baseline`
Expected: compilation error — module `baseline` does not exist.

- [ ] **Step 3: Implement the model and the format**

`crates/celerrate_cli/src/baseline/mod.rs`:

```rust
//! The baseline: known findings hidden from the report and the exit code.
//!
//! Entirely CLI-layer machinery. Nothing here enters a salsa query or the
//! persistent cache: the baseline is presentation, applied after analysis
//! and suppression, before rendering and the exit code.

pub mod entry;
pub mod file;

/// The fixed file name at the project root. No configuration key moves it.
pub const BASELINE_FILE_NAME: &str = "celerrate-baseline.toml";
```

`crates/celerrate_cli/src/baseline/entry.rs`:

```rust
//! The baseline entry: the structural fingerprint of a known finding.
//!
//! The key is `(relative path, CEL identifier, enclosing symbol path,
//! message, count)`. No line number anywhere: the symbol path provides the
//! locality a line number used to provide, without its fragility.

/// One recorded finding. The derived ordering (path, then identifier, then
/// symbol, then message, then count) is the deterministic file order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineEntry {
    /// Project-relative path with `/` separators on every platform.
    pub path: String,
    /// The diagnostic identifier, e.g. `CEL0034`.
    pub identifier: String,
    /// The enclosing symbol path (`App\Service\Checkout::finalize`), or
    /// `(top level)` for code outside declarations.
    pub symbol: String,
    /// The full rendered message. Two diagnostics with the same identifier
    /// in the same scope are distinguished by their messages.
    pub message: String,
    /// True duplicates absorbed: matching consumes at most this many
    /// occurrences; occurrence `count + 1` is reported as new.
    pub count: u32,
}

impl BaselineEntry {
    pub fn key(&self) -> BaselineKey {
        BaselineKey {
            path: self.path.clone(),
            identifier: self.identifier.clone(),
            symbol: self.symbol.clone(),
            message: self.message.clone(),
        }
    }
}

/// The matching key: every field of the entry except the count.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineKey {
    pub path: String,
    pub identifier: String,
    pub symbol: String,
    pub message: String,
}
```

`crates/celerrate_cli/src/baseline/file.rs`:

```rust
//! The `celerrate-baseline.toml` format: a versioned header and
//! deterministically sorted `[[entry]]` tables — minimal diffs, reviewable
//! in a pull request. Resilient like everything else: what does not parse
//! produces a failure line, never a crash, and valid entries still apply.

use crate::baseline::entry::BaselineEntry;

pub const FORMAT_VERSION: i64 = 1;

const ENTRY_KEYS: [&str; 5] = ["path", "identifier", "symbol", "message", "count"];

const HEADER: &str = "\
# Celerrate baseline: known findings hidden from the report and the exit code.
# Recorded by `celerrate check --baseline`. Entries are structural (no line
# numbers): they survive moving code and die with their finding.
";

pub struct ParsedBaseline {
    pub entries: Vec<BaselineEntry>,
    pub failures: Vec<String>,
}

pub fn parse(text: &str) -> ParsedBaseline {
    // Mirror `celerrate_config`'s parse entry point (crates/celerrate_config/
    // src/parse.rs:38) for the toml_edit 0.23 API spelling.
    let document = match toml_edit::Document::parse(text) {
        Ok(document) => document,
        Err(error) => {
            return ParsedBaseline {
                entries: Vec::new(),
                failures: vec![format!("invalid TOML: {error}")],
            };
        }
    };
    match document.get("version").and_then(toml_edit::Item::as_integer) {
        Some(FORMAT_VERSION) => {}
        Some(other) => {
            return ParsedBaseline {
                entries: Vec::new(),
                failures: vec![format!(
                    "unsupported baseline version {other}; this binary reads version {FORMAT_VERSION}"
                )],
            };
        }
        None => {
            return ParsedBaseline {
                entries: Vec::new(),
                failures: vec!["the `version` key is missing".to_string()],
            };
        }
    }
    let mut entries = Vec::new();
    let mut failures = Vec::new();
    let tables = document
        .get("entry")
        .and_then(toml_edit::Item::as_array_of_tables);
    if let Some(tables) = tables {
        for (index, table) in tables.iter().enumerate() {
            match parse_entry(table) {
                Ok(entry) => entries.push(entry),
                Err(reason) => failures.push(format!("entry {index}: {reason}")),
            }
            for (key, _) in table.iter() {
                if !ENTRY_KEYS.contains(&key) {
                    failures.push(format!("entry {index}: unknown key `{key}`"));
                }
            }
        }
    }
    ParsedBaseline { entries, failures }
}

fn parse_entry(table: &toml_edit::Table) -> Result<BaselineEntry, String> {
    let count = table
        .get("count")
        .and_then(toml_edit::Item::as_integer)
        .ok_or_else(|| "the `count` key is missing or not an integer".to_string())?;
    let count = u32::try_from(count)
        .ok()
        .filter(|count| *count >= 1)
        .ok_or_else(|| format!("the `count` key must be a positive integer, got {count}"))?;
    Ok(BaselineEntry {
        path: required_text(table, "path")?,
        identifier: required_text(table, "identifier")?,
        symbol: required_text(table, "symbol")?,
        message: required_text(table, "message")?,
        count,
    })
}

fn required_text(table: &toml_edit::Table, key: &str) -> Result<String, String> {
    table
        .get(key)
        .and_then(toml_edit::Item::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("the `{key}` key is missing or not a string"))
}

pub fn serialize(entries: &[BaselineEntry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort();
    let mut document = toml_edit::DocumentMut::new();
    document.insert("version", toml_edit::value(FORMAT_VERSION));
    let mut tables = toml_edit::ArrayOfTables::new();
    for entry in &sorted {
        let mut table = toml_edit::Table::new();
        table.insert("path", toml_edit::value(&entry.path));
        table.insert("identifier", toml_edit::value(&entry.identifier));
        table.insert("symbol", toml_edit::value(&entry.symbol));
        table.insert("message", toml_edit::value(&entry.message));
        table.insert("count", toml_edit::value(i64::from(entry.count)));
        tables.push(table);
    }
    document.insert("entry", toml_edit::Item::ArrayOfTables(tables));
    format!("{HEADER}\n{document}")
}
```

Add to `crates/celerrate_cli/Cargo.toml` under `[dependencies]`: `toml_edit.workspace = true`. Add `pub mod baseline;` to `crates/celerrate_cli/src/lib.rs`. If the toml_edit 0.23 API spells any call differently (`Document::parse` versus `str::parse::<DocumentMut>()`, `Item::as_str`), follow `crates/celerrate_config/src/parse.rs` — it compiles against the exact vendored version.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli baseline`
Expected: all file-format tests PASS.

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy -p celerrate_cli --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli/src/baseline crates/celerrate_cli/src/lib.rs crates/celerrate_cli/Cargo.toml
git commit -m "✨ feat(baseline): model entries and the versioned baseline file format"
```

---

### Task 2: Enclosing symbol path

**Files:**
- Create: `crates/celerrate_cli/src/baseline/symbol.rs`
- Modify: `crates/celerrate_cli/src/baseline/mod.rs` (add `pub mod symbol;`)

**Interfaces:**
- Consumes: `celerrate_semantics::{analyzed_file_index, ast_id_map, member_tree, fully_qualified_name}`; the root-node acquisition pattern of `DatabaseResolver::first_line_of` (`crates/celerrate_rules/src/render/resolve.rs:49-66`) — read that function first and reuse its exact call chain for SourceFile lookup, root node, and pointer resolution.
- Produces:
  - `baseline::symbol::TOP_LEVEL_SYMBOL: &str = "(top level)"`
  - `baseline::symbol::ANONYMOUS_CLASS: &str = "(anonymous class)"`
  - `baseline::symbol::enclosing_symbol_path(database: &dyn salsa::Database, files: AnalyzedFileSet, file: FileId, range: TextRange) -> String`

**Semantics (decided):** the innermost declaration whose syntax range contains `range.start()` wins. A method/property/constant/case inside a class renders as `<class display>::<member name>`; a class-like alone as its fully qualified name; a free function as its fully qualified name; anonymous class-likes render their class part as `(anonymous class)`; anything else is `(top level)`. Closures are not declarations: a finding inside a closure keys on the enclosing method or function, which is the intended locality.

- [ ] **Step 1: Write the failing tests**

In `crates/celerrate_cli/src/baseline/symbol.rs`, a `#[cfg(test)]` module that builds a real `Session` over a tempdir fixture (in-crate tests can access `Session` internals; `tempfile` is already a dev-dependency — see `src/watch.rs`'s test module for the precedent):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::session::Session;

    const MANIFEST: &str =
        r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

    /// Builds a session over one PHP file and returns the symbol path at the
    /// first occurrence of `needle` in that file.
    fn symbol_at(source: &str, needle: &str) -> String {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("composer.json"), MANIFEST).unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let file_path = root.path().join("src").join("Example.php");
        std::fs::write(&file_path, source).unwrap();
        let mut session = Session::start(root.path());
        let file = session.vfs.file_id(&file_path);
        let offset = u32::try_from(source.find(needle).unwrap()).unwrap();
        let range = celerrate_source::TextRange::new(
            offset.into(),
            (offset + u32::try_from(needle.len()).unwrap()).into(),
        );
        enclosing_symbol_path(&session.database, session.files, file, range)
    }

    #[test]
    fn a_finding_in_a_method_keys_on_class_and_method() {
        let source = "<?php\nnamespace App\\Service;\n\nclass Checkout\n{\n    public function finalize(): void\n    {\n        new Missing();\n    }\n}\n";
        assert_eq!(symbol_at(source, "new Missing"), "App\\Service\\Checkout::finalize");
    }

    #[test]
    fn a_finding_in_a_free_function_keys_on_the_function() {
        let source = "<?php\nnamespace App;\n\nfunction helper(): void\n{\n    new Missing();\n}\n";
        assert_eq!(symbol_at(source, "new Missing"), "App\\helper");
    }

    #[test]
    fn a_finding_on_a_class_header_keys_on_the_class() {
        let source = "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n";
        assert_eq!(symbol_at(source, "Missing"), "App\\Kernel");
    }

    #[test]
    fn a_finding_outside_declarations_is_top_level() {
        let source = "<?php\n\nnew Missing();\n";
        assert_eq!(symbol_at(source, "new Missing"), TOP_LEVEL_SYMBOL);
    }

    #[test]
    fn a_finding_in_a_closure_keys_on_the_enclosing_method() {
        let source = "<?php\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        $callback = function (): void {\n            new Missing();\n        };\n    }\n}\n";
        assert_eq!(symbol_at(source, "new Missing"), "App\\Runner::run");
    }

    #[test]
    fn an_anonymous_class_method_uses_the_anonymous_marker() {
        let source = "<?php\n\n$instance = new class {\n    public function run(): void\n    {\n        new Missing();\n    }\n};\n";
        assert_eq!(symbol_at(source, "new Missing"), format!("{ANONYMOUS_CLASS}::run"));
    }
}
```

Adjust the `Session` field access (`session.vfs`, `session.database`, `session.files`) to the actual field names and visibility in `crates/celerrate_cli/src/session.rs:86-137`; render.rs line 174 (`DatabaseResolver::new(&session.database, session.files)`) shows the database and file-set access that works in-crate. If a field is private to `session.rs`, add a `pub(crate)` accessor rather than widening the field.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli baseline::symbol`
Expected: compilation error — `enclosing_symbol_path` does not exist.

- [ ] **Step 3: Implement**

```rust
//! The enclosing symbol path of a finding: the locality a line number used
//! to provide, without its fragility. Read-only over existing semantic
//! queries; nothing here is a query itself.

use celerrate_semantics::fully_qualified_name;
use celerrate_source::{FileId, TextRange};

pub const TOP_LEVEL_SYMBOL: &str = "(top level)";
pub const ANONYMOUS_CLASS: &str = "(anonymous class)";

pub fn enclosing_symbol_path(
    database: &dyn salsa::Database,
    files: AnalyzedFileSet,
    file: FileId,
    range: TextRange,
) -> String {
    // FileId -> SourceFile, root node, and pointer resolution: same call
    // chain as DatabaseResolver::first_line_of (celerrate_rules resolve.rs).
    let Some(source_file) = source_file_of(database, files, file) else {
        return TOP_LEVEL_SYMBOL.to_string();
    };
    let root = /* same root acquisition as resolve.rs first_line_of */;
    let ast_ids = celerrate_semantics::ast_id_map(database, source_file);
    let members = celerrate_semantics::member_tree(database, source_file);

    let mut best: Option<(TextRange, String)> = None;
    let mut consider = |index: u32, display: String| {
        let Some(pointer) = ast_ids.pointer(index) else { return };
        let Some(node) = pointer.try_to_node(&root) else { return };
        let node_range = node.text_range();
        if !node_range.contains(range.start()) {
            return;
        }
        let smaller = best
            .as_ref()
            .is_none_or(|(current, _)| node_range.len() < current.len());
        if smaller {
            best = Some((node_range, display));
        }
    };

    for class in &members.classes {
        let class_display = match &class.name {
            Some(name) => fully_qualified_name(&class.namespace, name),
            None => ANONYMOUS_CLASS.to_string(),
        };
        consider(class.ast_id.index, class_display.clone());
        for member in &class.members {
            consider(member.ast_id.index, format!("{class_display}::{}", member.name));
        }
    }
    for function in &members.functions {
        consider(
            function.ast_id.index,
            fully_qualified_name(&function.namespace, &function.name),
        );
    }
    best.map_or_else(|| TOP_LEVEL_SYMBOL.to_string(), |(_, display)| display)
}
```

Fill the two open points from the actual code, not from memory: (a) `source_file_of` iterates `celerrate_semantics::analyzed_file_index(database, files)` to find the `SourceFile` whose `FileId` matches; (b) the root node comes from the same query `resolve.rs:first_line_of` uses. If `MemberTree`/`ClassMembers` expose accessors instead of public fields, use those. Innermost-wins works because a member's range is strictly inside its class's range; the closure borrow of `best` may need a small helper function instead of a closure if the borrow checker objects — a plain `fn consider(best: &mut Option<...>, ...)` is fine.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli baseline::symbol`
Expected: all six PASS. If the anonymous-class or closure test reveals a different `MemberTree` shape (e.g. anonymous classes absent from `classes`), adapt the display fallback but keep the assertion meaningful — the marker must be stable and documented.

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy -p celerrate_cli --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli/src/baseline
git commit -m "✨ feat(baseline): compute the enclosing symbol path of a finding"
```

---

### Task 3: The `--baseline` and `--ignore-baseline` flags

**Files:**
- Modify: `crates/celerrate_cli/src/arguments.rs:20-37` (the `Check` variant)
- Modify: `crates/celerrate_cli/src/lib.rs` (destructure and thread the two booleans; ignore them beyond parsing for now)
- Create: `crates/celerrate_cli/tests/baseline.rs`

**Interfaces:**
- Produces: `Command::Check { path, watch, fix, fix_suggestions, baseline, ignore_baseline }`; `baseline::Mode { Apply, Record, Ignore }` with `Mode::of(record: bool, ignore: bool) -> Mode` in `baseline/mod.rs`.
- Usage-error contract (spec section 4): `--baseline` combined with `--fix`, `--fix-suggestions`, or `--watch` is a usage error. Decision recorded here: `--baseline --ignore-baseline` is also a conflict (two contradictory instructions); applying a baseline under `--watch` stays legal.

- [ ] **Step 1: Write the failing integration tests**

`crates/celerrate_cli/tests/baseline.rs` (re-declare the local helpers per convention):

```rust
//! Baseline integration tests: flags, recording, applying, and the
//! spec's three invariants.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::ffi::OsString;
use std::path::Path;

use celerrate_cli::{ColorMode, Outcome};

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

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
    let outcome = celerrate_cli::run(arguments, &mut output, ColorMode::Plain);
    (outcome, String::from_utf8(output).unwrap())
}

fn check(root: &Path) -> (Outcome, String) {
    check_with(root, &[])
}

const CLEAN_SOURCE: &str = "<?php\n\nnamespace App;\n\nclass Example\n{\n}\n";

#[test]
fn baseline_with_watch_is_a_usage_error() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", CLEAN_SOURCE)]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--watch"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn baseline_with_fix_is_a_usage_error() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", CLEAN_SOURCE)]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--fix"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn baseline_with_fix_suggestions_is_a_usage_error() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", CLEAN_SOURCE)]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--fix-suggestions"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn baseline_with_ignore_baseline_is_a_usage_error() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", CLEAN_SOURCE)]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--ignore-baseline"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn the_flags_are_accepted_alone_on_a_clean_project() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", CLEAN_SOURCE)]);
    let (recorded, _) = check_with(root.path(), &["--baseline"]);
    assert_eq!(recorded, Outcome::Clean);
    let (strict, _) = check_with(root.path(), &["--ignore-baseline"]);
    assert_eq!(strict, Outcome::Clean);
    let (watched_strict, _) = check_with(root.path(), &["--ignore-baseline", "--watch"]);
    // --ignore-baseline composes with --watch; the watch loop exits cleanly
    // in tests only via Ctrl+C machinery, so this line is NOT part of this
    // task — delete it if it cannot terminate; the two above must pass.
    let _ = watched_strict;
}
```

Delete the `--watch` composition lines before committing if the watch loop cannot terminate in-process (it blocks); the conflict tests and the two solo-flag tests are the deliverable.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli --test baseline`
Expected: FAIL — clap rejects the unknown `--baseline` flag with a usage error for ALL tests, so `the_flags_are_accepted_alone_on_a_clean_project` fails (the conflict tests pass vacuously; that is expected at this stage).

- [ ] **Step 3: Implement the flags and the mode**

In `arguments.rs`, extend `Check`:

```rust
/// Record (or rewrite) `celerrate-baseline.toml` from the current findings.
#[arg(long, conflicts_with_all = ["watch", "fix", "fix_suggestions", "ignore_baseline"])]
baseline: bool,

/// Ignore an existing `celerrate-baseline.toml` and report every finding.
#[arg(long)]
ignore_baseline: bool,
```

In `baseline/mod.rs`:

```rust
/// How this run treats the baseline file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A present file applies automatically (the default).
    Apply,
    /// `--baseline`: record or rewrite the file from the current findings.
    Record,
    /// `--ignore-baseline`: strict run, the file is not consulted.
    Ignore,
}

impl Mode {
    pub fn of(record: bool, ignore: bool) -> Self {
        // clap guarantees record and ignore are mutually exclusive.
        if record {
            Self::Record
        } else if ignore {
            Self::Ignore
        } else {
            Self::Apply
        }
    }
}
```

In `lib.rs`, destructure the two new fields in the `Command::Check` arm and compute `let mode = baseline::Mode::of(baseline, ignore_baseline);` — unused beyond that for now (prefix with `_mode` if clippy complains, removed in task 4).

- [ ] **Step 4: Run to verify green**

Run: `cargo test -p celerrate_cli --test baseline`
Expected: PASS (all conflict tests now fail with clap's real conflict error, still `UsageError`; solo flags accepted).

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy -p celerrate_cli --all-targets -- -D warnings && cargo fmt --all && cargo test -p celerrate_cli`

```bash
git add crates/celerrate_cli/src/arguments.rs crates/celerrate_cli/src/lib.rs crates/celerrate_cli/src/baseline/mod.rs crates/celerrate_cli/tests/baseline.rs
git commit -m "✨ feat(cli): add the --baseline and --ignore-baseline flags"
```

---

### Task 4: Recording — `celerrate check --baseline` writes the file

**Files:**
- Modify: `crates/celerrate_cli/src/baseline/mod.rs` (`fingerprint`, `record`, `BaselineOutcome`)
- Modify: `crates/celerrate_cli/src/lib.rs:118-157` (the check flow)
- Modify: `crates/celerrate_cli/src/render.rs` (thread `BaselineOutcome`; recorded line)
- Modify: `crates/celerrate_cli/tests/baseline.rs`

**Interfaces:**
- Consumes: `entry::{BaselineEntry, BaselineKey}` (task 1), `symbol::enclosing_symbol_path` (task 2), `Mode` (task 3), `write_atomically` (`crates/celerrate_cli/src/cache/pack.rs:146`, make it reachable: it is `pub` in its module; re-export or call via `crate::cache::pack::write_atomically`).
- Produces:
  - `baseline::fingerprint(session: &Session, diagnostic: &Diagnostic) -> Option<BaselineKey>` — `None` for project-anchored diagnostics.
  - `baseline::record(session: &Session, diagnostics: &[Diagnostic]) -> std::io::Result<usize>` — aggregates duplicate keys into counts, writes atomically, returns the entry count; writes **no** file when there are zero entries and no file exists; rewrites a header-only file when a file exists (never deletes a user file).
  - `baseline::BaselineOutcome { hidden: usize, recorded: Option<usize> }` with `Default`. Task 5 adds a third field, `notices: Vec<BaselineNotice>`; in this task the struct has exactly these two.
  - `render::render_report(output, session, outcome, color, baseline: &BaselineOutcome)` — signature gains the parameter; prints `recorded N baseline entries to celerrate-baseline.toml` (singular: `1 baseline entry`) after the summary line when `recorded` is `Some`.
- Behavior decided (spec-consistent): the recording run runs strict (an existing file is not applied), records every span-anchored diagnostic, hides them from the report and the exit code (the clean-slate contract: recording exits 0 unless configuration diagnostics or internal errors remain), and configuration diagnostics are never recorded (they merge after the baseline step).

- [ ] **Step 1: Write the failing tests**

Append to `tests/baseline.rs`:

```rust
const FAILING_SOURCE: &str =
    "<?php\n\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n";

fn baseline_text(root: &Path) -> String {
    std::fs::read_to_string(root.join("celerrate-baseline.toml")).unwrap()
}

#[test]
fn recording_writes_a_sorted_versioned_file_and_exits_clean() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    let (outcome, text) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("recorded 1 baseline entry"), "report was:\n{text}");
    let written = baseline_text(root.path());
    assert!(written.contains("version = 1"), "file was:\n{written}");
    assert!(written.contains("path = \"src/Kernel.php\""), "file was:\n{written}");
    assert!(written.contains("symbol = \"App\\\\Kernel\""), "file was:\n{written}");
    assert!(written.contains("count = 1"), "file was:\n{written}");
}

#[test]
fn recording_twice_is_byte_identical() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let first = baseline_text(root.path());
    check_with(root.path(), &["--baseline"]);
    assert_eq!(first, baseline_text(root.path()));
}

#[test]
fn recording_a_clean_project_writes_no_file() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", CLEAN_SOURCE)]);
    let (outcome, _) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean);
    assert!(!root.path().join("celerrate-baseline.toml").exists());
}

#[test]
fn recording_a_now_clean_project_rewrites_the_existing_file_header_only() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate-baseline.toml", "version = 1\n\n[[entry]]\npath = \"src/Old.php\"\nidentifier = \"CEL0018\"\nsymbol = \"(top level)\"\nmessage = \"m\"\ncount = 1\n"),
    ]);
    let (outcome, text) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    let written = baseline_text(root.path());
    assert!(written.contains("version = 1"));
    assert!(!written.contains("src/Old.php"), "file was:\n{written}");
}

#[test]
fn duplicate_occurrences_aggregate_into_one_entry_with_a_count() {
    let source = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n        new Missing();\n    }\n}\n";
    let root = project(&[("composer.json", MANIFEST), ("src/Runner.php", source)]);
    let (outcome, _) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean);
    let written = baseline_text(root.path());
    assert!(written.contains("count = 2"), "file was:\n{written}");
    assert_eq!(written.matches("[[entry]]").count(), 1, "file was:\n{written}");
}

#[test]
fn configuration_diagnostics_are_never_recorded_and_still_fail_the_run() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
        ("celerrate.toml", "[rules.no-such-rule]\nenabled = true\n"),
    ]);
    let (outcome, text) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("CEL0046"), "report was:\n{text}");
    let written = baseline_text(root.path());
    assert!(!written.contains("CEL0046"), "file was:\n{written}");
}
```

Note on the symbol assertion: TOML escapes backslashes, so `App\Kernel` serializes as `"App\\Kernel"`, which in Rust source is `"App\\\\Kernel"`. If the recorded diagnostic for `FAILING_SOURCE` anchors differently than expected (check the actual `check__findings.snap` snapshot for what CEL identifier and span `extends Missing` produces), adapt the fixture, not the invariant.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli --test baseline`
Expected: the new tests FAIL (no file written; `recorded` line absent; exit is `DiagnosticsReported` instead of `Clean`).

- [ ] **Step 3: Implement fingerprint, record, and the wiring**

`baseline/mod.rs` additions:

```rust
use std::collections::BTreeMap;
use std::io;

use celerrate_diagnostics::Diagnostic;

use crate::session::Session;
use entry::{BaselineEntry, BaselineKey};

/// What the baseline step did this run; consumed by rendering and the
/// exit-code computation.
#[derive(Debug, Default)]
pub struct BaselineOutcome {
    /// Diagnostics removed from the report and the exit code.
    pub hidden: usize,
    /// Entry count written by `--baseline`, when recording.
    pub recorded: Option<usize>,
}

/// The structural key of one span-anchored diagnostic; `None` for
/// project-anchored findings (the baseline covers spans only).
pub fn fingerprint(session: &Session, diagnostic: &Diagnostic) -> Option<BaselineKey> {
    let (file, range) = diagnostic.span()?;
    let absolute = session.vfs.path(file)?;
    let relative = absolute
        .strip_prefix(&session.discovery.root)
        .unwrap_or(&absolute);
    let path = relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    let symbol =
        symbol::enclosing_symbol_path(&session.database, session.files, file, range);
    Some(BaselineKey {
        path,
        identifier: diagnostic.id.as_str().to_string(),
        symbol,
        message: diagnostic.message.clone(),
    })
}

/// Records the given diagnostics into `celerrate-baseline.toml` at the
/// project root. Returns the number of entries written. Never deletes the
/// file: a now-clean project rewrites it header-only when it exists.
pub fn record(session: &Session, diagnostics: &[Diagnostic]) -> io::Result<usize> {
    let mut counts: BTreeMap<BaselineKey, u32> = BTreeMap::new();
    for diagnostic in diagnostics {
        if let Some(key) = fingerprint(session, diagnostic) {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let entries: Vec<BaselineEntry> = counts
        .into_iter()
        .map(|(key, count)| BaselineEntry {
            path: key.path,
            identifier: key.identifier,
            symbol: key.symbol,
            message: key.message,
            count,
        })
        .collect();
    let path = session.discovery.root.join(BASELINE_FILE_NAME);
    if entries.is_empty() && !path.exists() {
        return Ok(0);
    }
    crate::cache::pack::write_atomically(&path, file::serialize(&entries).as_bytes())?;
    Ok(entries.len())
}
```

Adjust `session.vfs.path(file)` to the actual VFS accessor `render.rs:display_path` uses (line 333). If `cache::pack` is not visible from `baseline`, widen it to `pub(crate)` — do not duplicate the atomic-write logic.

`lib.rs` check-flow rewiring (the exact insertion point is after `suggest::enrich` builds `presented` and **before** `configuration::merge_diagnostics`):

```rust
let baseline_outcome = match mode {
    baseline::Mode::Record => match baseline::record(&session, &presented.diagnostics) {
        Ok(recorded) => {
            let before = presented.diagnostics.len();
            presented.diagnostics.retain(|diagnostic| diagnostic.span().is_none());
            baseline::BaselineOutcome {
                hidden: before - presented.diagnostics.len(),
                recorded: Some(recorded),
            }
        }
        Err(error) => {
            let _ = writeln!(
                output,
                "error: could not write {}: {error}",
                baseline::BASELINE_FILE_NAME
            );
            return Outcome::InternalError;
        }
    },
    baseline::Mode::Apply | baseline::Mode::Ignore => baseline::BaselineOutcome::default(),
};
```

and the exit-code line becomes:

```rust
Outcome::of(
    outcome
        .diagnostics
        .len()
        .saturating_sub(baseline_outcome.hidden)
        + configuration_diagnostics,
    session.internal_errors.len(),
)
```

`render.rs`: add the parameter `baseline: &crate::baseline::BaselineOutcome` to `render_report` and `render_report_with` (and the `render_check` test seam); after `write_summary_line`, print:

```rust
if let Some(recorded) = baseline.recorded {
    writeln!(
        output,
        "recorded {} to {}",
        count(recorded, "baseline entry", "baseline entries"),
        crate::baseline::BASELINE_FILE_NAME
    )?;
}
```

(`count` is the existing private pluralization helper next to `write_summary_line`.) Pass `&baseline::BaselineOutcome::default()` from the one remaining caller that has no baseline context yet (watch's `render_cycle` is untouched until task 7 — only extend `render_report`'s signature now).

- [ ] **Step 4: Run to verify green**

Run: `cargo test -p celerrate_cli --test baseline && cargo test -p celerrate_cli`
Expected: new tests PASS; the existing snapshot tests unchanged (no baseline file in their fixtures — zero-config parity).

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy -p celerrate_cli --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli/src crates/celerrate_cli/tests/baseline.rs
git commit -m "✨ feat(baseline): record celerrate-baseline.toml from the current findings"
```

---

### Task 5: Applying — the filter, the hidden line, the exit code, the invalid-file notice

**Files:**
- Modify: `crates/celerrate_cli/src/baseline/mod.rs` (`load`, `LoadedBaseline`, `apply`, `BaselineNotice`, identifier constants)
- Modify: `crates/celerrate_cli/src/session.rs` (field + load at start + reload in `rediscover` + `is_project_manifest`)
- Modify: `crates/celerrate_cli/src/lib.rs` (Apply arm)
- Modify: `crates/celerrate_cli/src/render.rs` (baseline notices, hidden line, notice count, explain pointers)
- Modify: `crates/celerrate_cli/tests/baseline.rs`

**Interfaces:**
- Produces:
  - `baseline::OBSOLETE_BASELINE_ENTRIES: DiagnosticId = DiagnosticId::new("CEL0050")` and `baseline::INVALID_BASELINE_FILE: DiagnosticId = DiagnosticId::new("CEL0051")`; `baseline::ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[OBSOLETE_BASELINE_ENTRIES, INVALID_BASELINE_FILE]` (registered in the registry in task 8 — until then nothing looks these up, which is safe: the registry test does not scan the composition root yet).
  - `baseline::LoadedBaseline { entries: Vec<BaselineEntry>, failures: Vec<String> }`; `baseline::load(root: &Path) -> Option<LoadedBaseline>` (`None` when the file is absent; present-but-unreadable yields empty entries plus a failure line — never a crash).
  - `baseline::BaselineNotice { InvalidFile { detail: String }, ObsoleteEntries { count: usize } }` with `identifier() -> DiagnosticId`, `severity() -> Severity` (always `Severity::Warning`, mirroring `ProjectNotice::severity`, `crates/celerrate_project/src/notice.rs:78`), `message() -> String`.
  - `BaselineOutcome` gains `pub notices: Vec<BaselineNotice>`.
  - `baseline::apply(session: &Session, diagnostics: &mut Vec<Diagnostic>) -> BaselineOutcome` — pure over its inputs, callable repeatedly (watch reuses it in task 7).
  - `Session.loaded_baseline: Option<baseline::LoadedBaseline>` (`pub(crate)`), loaded in `Session::start` and reloaded in `rediscover`; `is_project_manifest` additionally matches `<root>/celerrate-baseline.toml`.
- Exit-neutrality contract: baseline notices ride the notice channel (`write_notices`-style rendering, counted in the summary's notice count, included in the explain trailer), never `AnalysisOutcome::diagnostics`. `ObsoleteEntries` is emitted in task 6; declare the variant now so the enum is complete, but only `InvalidFile` fires in this task.

- [ ] **Step 1: Write the failing tests**

Append to `tests/baseline.rs`:

```rust
#[test]
fn a_present_baseline_hides_its_findings_and_the_exit_code() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("1 baselined diagnostic hidden"), "report was:\n{text}");
    assert!(!text.contains("CEL0018"), "report was:\n{text}");
}

#[test]
fn ignore_baseline_runs_strict() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check_with(root.path(), &["--ignore-baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(!text.contains("hidden"), "report was:\n{text}");
}

#[test]
fn an_unreadable_baseline_reports_cel0051_and_runs_strict() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
        ("celerrate-baseline.toml", "version = 1\n[[entry]\n"),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("notice CEL0051"), "report was:\n{text}");
}

#[test]
fn configuration_diagnostics_are_never_hidden_by_a_baseline() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(
        root.path().join("celerrate.toml"),
        "[rules.no-such-rule]\nenabled = true\n",
    )
    .unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("CEL0046"), "report was:\n{text}");
}

#[test]
fn the_persisted_cache_stays_pre_baseline() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    // Cold run with the baseline applied persists packs; the warm strict
    // run must still report the finding from the cache.
    check(root.path());
    let (outcome, text) = check_with(root.path(), &["--ignore-baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("CEL0018"), "report was:\n{text}");
}

#[test]
fn a_baselined_diagnostic_is_not_fixed_by_fix() {
    // --fix with an applied baseline is legal (only recording conflicts).
    // A hidden finding must not be mutated.
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let before = std::fs::read_to_string(root.path().join("src/Kernel.php")).unwrap();
    let (outcome, _) = check_with(root.path(), &["--fix"]);
    assert_eq!(outcome, Outcome::Clean);
    let after = std::fs::read_to_string(root.path().join("src/Kernel.php")).unwrap();
    assert_eq!(before, after);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli --test baseline`
Expected: the new tests FAIL (nothing is hidden; CEL0051 unknown).

- [ ] **Step 3: Implement load, apply, notices, and the wiring**

`baseline/mod.rs` additions:

```rust
use celerrate_diagnostics::{DiagnosticId, Severity};

/// Obsolete baseline entries: recorded findings that no longer match.
pub const OBSOLETE_BASELINE_ENTRIES: DiagnosticId = DiagnosticId::new("CEL0050");
/// The baseline file exists but could not be (fully) read.
pub const INVALID_BASELINE_FILE: DiagnosticId = DiagnosticId::new("CEL0051");
/// Checked against the registry by the composition-root guard (task 8).
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] =
    &[OBSOLETE_BASELINE_ENTRIES, INVALID_BASELINE_FILE];

/// The baseline file as loaded at session start: parsed entries plus the
/// failure lines of whatever did not parse.
#[derive(Debug)]
pub struct LoadedBaseline {
    pub entries: Vec<BaselineEntry>,
    pub failures: Vec<String>,
}

/// Reads `<root>/celerrate-baseline.toml`. `None` when absent; a present
/// but unreadable file yields no entries and a failure line — resilience:
/// never crash, and never hide silently.
pub fn load(root: &std::path::Path) -> Option<LoadedBaseline> {
    let path = root.join(BASELINE_FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(LoadedBaseline {
                entries: Vec::new(),
                failures: vec![format!("the file could not be read: {error}")],
            });
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            return Some(LoadedBaseline {
                entries: Vec::new(),
                failures: vec!["the file is not valid UTF-8".to_string()],
            });
        }
    };
    let parsed = file::parse(&text);
    Some(LoadedBaseline {
        entries: parsed.entries,
        failures: parsed.failures,
    })
}

/// An exit-neutral, project-anchored baseline notice. Rides the notice
/// channel (like `ProjectNotice`), never `AnalysisOutcome::diagnostics`,
/// so it cannot reach the exit code by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineNotice {
    InvalidFile { detail: String },
    ObsoleteEntries { count: usize },
}

impl BaselineNotice {
    pub fn identifier(&self) -> DiagnosticId {
        match self {
            Self::InvalidFile { .. } => INVALID_BASELINE_FILE,
            Self::ObsoleteEntries { .. } => OBSOLETE_BASELINE_ENTRIES,
        }
    }

    pub fn severity(&self) -> Severity {
        Severity::Warning
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidFile { detail } => format!(
                "{BASELINE_FILE_NAME} could not be fully read ({detail}); unreadable entries are ignored and their findings reported"
            ),
            Self::ObsoleteEntries { count: 1 } => "1 baseline entry no longer matches the current findings; re-record with `celerrate check --baseline`".to_string(),
            Self::ObsoleteEntries { count } => format!(
                "{count} baseline entries no longer match the current findings; re-record with `celerrate check --baseline`"
            ),
        }
    }
}

/// Applies the session's loaded baseline to the diagnostic list, in place.
/// Matching consumes at most `count` occurrences per key; occurrence
/// `count + 1` stays reported. Obsolescence (leftover capacity) becomes a
/// notice in task 6.
pub fn apply(session: &Session, diagnostics: &mut Vec<Diagnostic>) -> BaselineOutcome {
    let Some(loaded) = session.loaded_baseline.as_ref() else {
        return BaselineOutcome::default();
    };
    let mut notices = Vec::new();
    if !loaded.failures.is_empty() {
        notices.push(BaselineNotice::InvalidFile {
            detail: loaded.failures.join("; "),
        });
    }
    let mut remaining: BTreeMap<BaselineKey, u32> = BTreeMap::new();
    for entry in &loaded.entries {
        *remaining.entry(entry.key()).or_insert(0) += entry.count;
    }
    let before = diagnostics.len();
    diagnostics.retain(|diagnostic| {
        let Some(key) = fingerprint(session, diagnostic) else {
            return true;
        };
        match remaining.get_mut(&key) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        }
    });
    BaselineOutcome {
        hidden: before - diagnostics.len(),
        recorded: None,
        notices,
    }
}
```

Also add the module-level allocation guard mirroring `crates/celerrate_project/src/notice.rs:180-200`:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn the_allocation_list_is_exactly_what_the_notices_use() {
        let used = [
            BaselineNotice::ObsoleteEntries { count: 1 }.identifier(),
            BaselineNotice::InvalidFile { detail: String::new() }.identifier(),
        ];
        assert_eq!(ALLOCATED_IDENTIFIERS, used.as_slice());
    }
}
```

`session.rs`: add `pub(crate) loaded_baseline: Option<crate::baseline::LoadedBaseline>`, set in `Session::start` via `crate::baseline::load(&root)` (after root normalization, alongside `loaded_configuration`), reload it in `rediscover` (session.rs:517-532), and extend `is_project_manifest` (session.rs:504-509) with `|| path == root.join(crate::baseline::BASELINE_FILE_NAME)`.

`lib.rs`: the `Mode::Apply` arm becomes `baseline::apply(&session, &mut presented.diagnostics)` (still before `configuration::merge_diagnostics`, so configuration diagnostics are exempt by construction). Verify what stream the fix pipeline consumes at `lib.rs:150` — if `fix` reads `outcome` rather than `presented`, switch it to the filtered `presented` so hidden findings are not mutated (the `a_baselined_diagnostic_is_not_fixed_by_fix` test pins this).

`render.rs`:
- next to `write_notices`, add:

```rust
fn write_baseline_notices(
    output: &mut dyn Write,
    notices: &[crate::baseline::BaselineNotice],
) -> io::Result<()> {
    for notice in notices {
        writeln!(output, "notice {}: {}", notice.identifier(), notice.message())?;
    }
    if !notices.is_empty() {
        writeln!(output)?;
    }
    Ok(())
}
```

(match `write_notices`'s exact formatting at render.rs:147-159, including how the identifier displays and the blank-line placement);
- call it right after `write_notices` in `render_report_with`;
- the summary call becomes `write_summary_line(output, session.notices().len() + baseline.notices.len(), outcome.diagnostics.len())`;
- after the summary line, before the recorded line:

```rust
if baseline.hidden > 0 {
    writeln!(
        output,
        "{} hidden",
        count(baseline.hidden, "baselined diagnostic", "baselined diagnostics")
    )?;
}
```

- extend `explain_pointers` (render.rs:132-139) to include `baseline.notices.iter().map(BaselineNotice::identifier)`.

- [ ] **Step 4: Run to verify green**

Run: `cargo test -p celerrate_cli --test baseline && cargo test -p celerrate_cli`
Expected: PASS, including every pre-existing snapshot (fixtures without a baseline file see an empty `BaselineOutcome`, so output is byte-identical — zero-config parity).

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy -p celerrate_cli --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli/src crates/celerrate_cli/tests/baseline.rs
git commit -m "✨ feat(baseline): hide recorded findings from the report and the exit code"
```

---

### Task 6: Obsolete entries and the three invariants

**Files:**
- Modify: `crates/celerrate_cli/src/baseline/mod.rs` (obsolescence in `apply`)
- Modify: `crates/celerrate_cli/tests/baseline.rs` (the property suite — spec closure gate 3)

**Interfaces:**
- Consumes: everything from task 5.
- Produces: `apply` now pushes `BaselineNotice::ObsoleteEntries { count }` when any entry retains unconsumed capacity after matching. Decision recorded: an entry is obsolete when it consumed **fewer occurrences than its count** (a partially dead entry is surplus capacity that could silently absorb future regressions — report it).

- [ ] **Step 1: Write the failing property tests**

Append to `tests/baseline.rs`:

```rust
#[test]
fn an_entry_survives_line_movement() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let moved = "<?php\n\nnamespace App;\n\n// pushed down\n// by two comment lines\nclass Kernel extends Missing\n{\n}\n";
    std::fs::write(root.path().join("src/Kernel.php"), moved).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("1 baselined diagnostic hidden"), "report was:\n{text}");
    assert!(!text.contains("CEL0050"), "report was:\n{text}");
}

#[test]
fn an_entry_dies_with_its_diagnostic_and_is_reported_obsolete() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(root.path().join("src/Kernel.php"), CLEAN_SOURCE).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("notice CEL0050"), "report was:\n{text}");
    assert!(text.contains("re-record"), "report was:\n{text}");
}

#[test]
fn the_count_never_masks_occurrence_n_plus_one() {
    let two = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n        new Missing();\n    }\n}\n";
    let three = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n        new Missing();\n        new Missing();\n    }\n}\n";
    let root = project(&[("composer.json", MANIFEST), ("src/Runner.php", two)]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(root.path().join("src/Runner.php"), three).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("2 baselined diagnostics hidden"), "report was:\n{text}");
    assert!(text.contains("1 diagnostic"), "report was:\n{text}");
}

#[test]
fn a_renamed_method_resurfaces_its_findings_and_reports_obsolescence() {
    let before = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n    }\n}\n";
    let renamed = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function launch(): void\n    {\n        new Missing();\n    }\n}\n";
    let root = project(&[("composer.json", MANIFEST), ("src/Runner.php", before)]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(root.path().join("src/Runner.php"), renamed).unwrap();
    let (outcome, text) = check(root.path());
    // Noisy but honest, never silent: the finding is back AND the stale
    // entry is announced.
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("notice CEL0050"), "report was:\n{text}");
}

#[test]
fn a_new_suppression_makes_the_baseline_entry_obsolete() {
    // Filter order: suppression (in-engine), then baseline (CLI). Adding a
    // suppression starves the entry — the intended behavior.
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let suppressed = "<?php\n\nnamespace App;\n\n/** @celerrate-suppress CEL0018 */\nclass Kernel extends Missing\n{\n}\n";
    std::fs::write(root.path().join("src/Kernel.php"), suppressed).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("notice CEL0050"), "report was:\n{text}");
}
```

For the suppression test, use the project's real directive syntax — read `crates/celerrate_cli/tests/suppressions.rs` for the exact form (`@celerrate-suppress` is an assumption; copy a working fixture from that file). If the directive must sit on the diagnostic's line rather than the declaration docblock, adapt the fixture.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli --test baseline`
Expected: the obsolescence tests FAIL (no CEL0050 notice exists yet); the survival and count tests may already pass — that is fine, they pin gate 3.

- [ ] **Step 3: Implement obsolescence**

In `apply`, after the `retain`:

```rust
let obsolete = remaining.values().filter(|capacity| **capacity > 0).count();
if obsolete > 0 {
    notices.push(BaselineNotice::ObsoleteEntries { count: obsolete });
}
```

(One aggregated notice with the entry count — visible, never blocking, advising a re-record; per-entry noise was rejected.)

- [ ] **Step 4: Run to verify green**

Run: `cargo test -p celerrate_cli --test baseline`
Expected: PASS.

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy -p celerrate_cli --all-targets -- -D warnings && cargo fmt --all && cargo test -p celerrate_cli`

```bash
git add crates/celerrate_cli/src/baseline/mod.rs crates/celerrate_cli/tests/baseline.rs
git commit -m "✨ feat(baseline): report obsolete entries with an exit-neutral notice"
```

---

### Task 7: Watch integration

**Files:**
- Modify: `crates/celerrate_cli/src/watch.rs` (mode threading, per-cycle filter, manifest registration, exit count, in-module tests)
- Modify: `crates/celerrate_cli/src/render.rs` (`render_cycle` gains the `BaselineOutcome` parameter)
- Modify: `crates/celerrate_cli/src/lib.rs` (pass the mode into `watch::watch`)

**Interfaces:**
- Consumes: `baseline::{Mode, apply, BaselineOutcome}`; `Session.loaded_baseline` reload already works via `rediscover` (task 5 extended `is_project_manifest`).
- Produces: `watch::watch(session: &mut Session, output: &mut dyn Write, color: ColorMode, mode: baseline::Mode) -> Outcome`. `Mode::Record` cannot reach it (clap conflict); apply/ignore both legal. `Watch::spawn`'s manifest list (~watch.rs:556) gains `"celerrate-baseline.toml"` so edits — and a file created mid-session — trigger a cycle and a reload.

- [ ] **Step 1: Write the failing in-module watch test**

Follow the existing pattern in `watch.rs`'s `mod tests` (line 978+): build a session over a fixture with a recorded baseline, drive one `iteration` via `watch_with_held_sender`, and assert the rendered cycle hides the finding:

```rust
#[test]
fn a_cycle_applies_the_baseline_and_reports_hidden_findings() {
    // Fixture: composer.json + a failing source + a baseline recorded for
    // it (write the file by calling crate::baseline::record on a first
    // session, or check --baseline via celerrate_cli::run).
    // Then: one watch iteration with Mode::Apply.
    // Assert: the frame contains "1 baselined diagnostic hidden" and the
    // finding's identifier is absent; the returned exit outcome on
    // shutdown is Clean.
}
```

Write it as a real test by copying the setup of the nearest existing cycle test in that module (one that asserts on rendered frame content), not from scratch — the `Watch` scaffolding (held sender, shutdown event) is already there. Add a second test: after one cycle, delete `celerrate-baseline.toml` on disk, send its path as a change event, run another iteration, and assert the finding is reported again (the reload path).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli watch`
Expected: compilation error (`watch` has no mode parameter) or assertion failure.

- [ ] **Step 3: Implement**

- `watch::watch` and `iteration`/`completed_cycle` take `mode: crate::baseline::Mode` (thread it down).
- In `completed_cycle` (watch.rs:109-160), after `let mut presented = outcome.clone();` and before `merge_diagnostics`:

```rust
let baseline_outcome = match mode {
    crate::baseline::Mode::Apply => {
        crate::baseline::apply(session, &mut presented.diagnostics)
    }
    _ => crate::baseline::BaselineOutcome::default(),
};
```

- `render_cycle` gains `baseline: &crate::baseline::BaselineOutcome` and renders the baseline notices, adds them to the notice count, and prints the hidden line, mirroring task 5's `render_report` changes.
- The graceful-exit path (watch.rs:331-335) recomputes on the final outcome:

```rust
let mut final_diagnostics = outcome.diagnostics.clone();
let hidden = match mode {
    crate::baseline::Mode::Apply => {
        crate::baseline::apply(session, &mut final_diagnostics).hidden
    }
    _ => 0,
};
crate::cache::persist(session, &outcome);
Outcome::of(
    final_diagnostics.len() + crate::configuration::diagnostic_count(session),
    session.internal_errors.len(),
)
```

(`apply` already removed the hidden ones from `final_diagnostics`, so its length is the post-baseline count; the `hidden` binding is only needed if you prefer subtracting — pick one, do not do both.)
- `Watch::spawn`: add `"celerrate-baseline.toml"` to the manifest registration list (~watch.rs:556) so the reload test passes — `Session::absorb` already routes it to `rediscover` since task 5.
- `lib.rs:121`: `return watch::watch(&mut session, output, color, mode);`.

- [ ] **Step 4: Run to verify green**

Run: `cargo test -p celerrate_cli`
Expected: PASS, including the two new watch tests.

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy -p celerrate_cli --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli/src
git commit -m "✨ feat(watch): apply and reload the baseline each cycle"
```

---

### Task 8: Register CEL0050 and CEL0051 — explain pages, guards, documentation

**Files:**
- Create: `crates/celerrate_diagnostics/src/pages/baseline.rs`
- Modify: `crates/celerrate_diagnostics/src/pages/mod.rs`, `crates/celerrate_diagnostics/src/registry.rs`
- Modify: `crates/celerrate_cli/tests/registry.rs`, `crates/celerrate_cli/tests/explain_pages.rs`, `crates/celerrate_cli/tests/suppression_correspondence.rs`
- Modify: `docs/diagnostics.md`

**Interfaces:**
- Consumes: `baseline::ALLOCATED_IDENTIFIERS` (task 5), the feature end to end (tasks 4-6 — the explain-page honesty harness runs real `celerrate check` invocations, which is why this task comes after the mechanics).
- Produces: registry entries for CEL0050/CEL0051; the registry gapless count moves from 49 to 51; `producers()` in `tests/registry.rs` gains `("celerrate_cli", celerrate_cli::baseline::ALLOCATED_IDENTIFIERS)` and the graph derivation is extended to scan the composition root itself.

- [ ] **Step 1: Registry entries and pages (the registry tests are the failing tests)**

Append to `REGISTRY` in `crates/celerrate_diagnostics/src/registry.rs` (before line 338, keeping the gapless order):

```rust
registered(
    "CEL0050",
    "obsolete baseline entries",
    "celerrate_cli",
    &pages::baseline::CEL0050,
),
registered(
    "CEL0051",
    "invalid baseline file",
    "celerrate_cli",
    &pages::baseline::CEL0051,
),
```

Bump the gapless assertion at registry.rs:383 from `49` to `51` (and its comment). Add `pub(crate) mod baseline;` to `pages/mod.rs`.

`crates/celerrate_diagnostics/src/pages/baseline.rs` — both pages use the `//// ` multi-file fixture form (see `pages/configuration.rs:119-156` for the shape). The failing examples fire for real: the explain-page harness materializes the declared files and runs `celerrate check`:

```rust
//! Explain pages for the baseline notices, owned by `celerrate_cli` where
//! the baseline mechanics live.

use crate::explain::ExplainPage;

pub(crate) const CEL0050: ExplainPage = ExplainPage {
    why: "\
A baseline entry records a known finding so it stops failing the run. When \
no current finding matches an entry any longer — the code was fixed, the \
enclosing method was renamed, or an engine upgrade reworded the message — \
the entry is obsolete. Celerrate reports it and never prunes silently: \
re-record with `celerrate check --baseline` to refresh the file.",
    failing_example: "\
//// celerrate-baseline.toml
version = 1

[[entry]]
path = \"src/Example.php\"
identifier = \"CEL0018\"
symbol = \"App\\\\Example\"
message = \"a finding that no longer exists\"
count = 1
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    fixed_example: "\
//// celerrate-baseline.toml
version = 1
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    configuration: "\
This notice is exit-neutral and can be neither disabled nor remapped. \
Re-record with `celerrate check --baseline`, or delete \
`celerrate-baseline.toml` to drop the baseline entirely.",
};

pub(crate) const CEL0051: ExplainPage = ExplainPage {
    why: "\
`celerrate-baseline.toml` exists but could not be fully read: invalid TOML, \
a missing or unsupported version, or a malformed entry. Unreadable entries \
are ignored and their findings reported — noisy but honest, never silent. \
Valid entries in the same file still apply.",
    failing_example: "\
//// celerrate-baseline.toml
version = 1
[[entry]
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    fixed_example: "\
//// celerrate-baseline.toml
version = 1
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    configuration: "\
This notice is exit-neutral and can be neither disabled nor remapped. Fix \
or re-record the file with `celerrate check --baseline`.",
};
```

The exact escaping inside the string literals matters: check `pages/configuration.rs` for how backslashes in PSR-4 keys are written there and copy that spelling. The header-only `celerrate-baseline.toml` in both fixed examples parses cleanly (version present, zero entries) and fires nothing.

- [ ] **Step 2: Extend the composition-root guards**

- `crates/celerrate_cli/tests/registry.rs`: add `("celerrate_cli", celerrate_cli::baseline::ALLOCATED_IDENTIFIERS)` to `producers()` (lines 20-31), and in the graph derivation (`the_named_producers_are_exactly_the_producers_in_the_dependency_graph`, lines 59-91) include the composition root itself — after collecting `celerrate_dependencies(...)`, push `"celerrate_cli"` into the derived set with a one-line comment: the composition root allocates the baseline notices because the baseline mechanics live there.
- `crates/celerrate_cli/tests/explain_pages.rs`: extend the clean-run literal list (lines 140-142) from `["CEL0043", ..., "CEL0049"]` to include `"CEL0050", "CEL0051"` (a bare run has no baseline file, so neither can fire).
- `crates/celerrate_cli/tests/suppression_correspondence.rs`: add CEL0050 and CEL0051 to `UNMAPPED_BY_DESIGN` (lines 60-95) with the reason `project-anchored baseline notices; there is no span to suppress`.

- [ ] **Step 3: Run the full guard suite to verify green**

Run: `cargo test -p celerrate_cli -p celerrate_diagnostics`
Expected: PASS — registry gapless at 51, both pages honest (failing example fires, fixed example silent), producers balanced, correspondence complete. If `every_written_page_example_is_honest` fails because the harness's forced-activation configuration interacts with the fixture, read the failure text: it prints the full report, which tells you exactly what fired.

- [ ] **Step 4: Documentation**

`docs/diagnostics.md` (guarded by `tests/documentation.rs:24-34` — every registry identifier must appear in the page):
- Update the exit-code paragraph (lines 25-29) to include CEL0050/CEL0051 among the exit-neutral notices.
- Rewrite the "there is no baseline yet" sentence (lines 34-37) to describe the shipped behavior: a present `celerrate-baseline.toml` hides recorded findings from the report and the exit code; `--baseline` records; `--ignore-baseline` runs strict.
- Add a section "Baseline notices (CEL0050, CEL0051)" as a sibling of "Project discovery notices", with a two-row table (identifier, when it fires, what to do) and a short description of the structural entry key — no line numbers, survives moving code, dies with its finding, never masks occurrence count + 1.

Run: `cargo test -p celerrate_cli --test documentation`
Expected: PASS.

- [ ] **Step 5: Gates and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo test --workspace`

```bash
git add crates/celerrate_diagnostics crates/celerrate_cli/tests docs/diagnostics.md
git commit -m "✨ feat(diagnostics): allocate CEL0050 and CEL0051 for the baseline notices"
```

---

### Task 9: Full verification, changelog, closure

**Files:**
- Modify: `CHANGELOG.md`

**Interfaces:** none — this task proves the sub-project gates that this plan touches and records the feature.

- [ ] **Step 1: Full mechanical suite**

Run, in order, each must pass:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo xtask dependency-shape
cargo xtask emission-scan
```

`dependency-shape` and `emission-scan` must pass untouched: `celerrate_cli` is a composition root (exempt), and no governed crate gained a diagnostic constructor.

- [ ] **Step 2: Corpus gates (zero-config parity)**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: the diagnostic snapshot does not move (the corpus has no `celerrate-baseline.toml`, and without a file the feature is inert — spec closure gate 1), and the mixed-rate baseline is unchanged (no type work happened — gate 9). Any delta is a bug in this plan's wiring: stop and investigate before proceeding (systematic-debugging), do not re-bless.

- [ ] **Step 3: Changelog**

Under `## [Unreleased]` in `CHANGELOG.md` (create the section if absent, matching the file's existing style):

```markdown
### Added

- The baseline: a present `celerrate-baseline.toml` hides recorded findings
  from the report and the exit code, so adoption on an existing codebase
  starts clean while new problems still fail. `celerrate check --baseline`
  records or rewrites the file; `--ignore-baseline` runs strict. Entries are
  structural (path, identifier, enclosing symbol, message, count) with no
  line numbers: they survive moving code and die with their finding.
  Obsolete entries are announced by the exit-neutral notice CEL0050, an
  unreadable file by CEL0051; nothing is ever pruned silently.
```

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the baseline under Unreleased"
```

- [ ] **Step 5: Hand off**

The branch is ready for review and merge (pull request per repository convention). Spec closure gate 3 (the baseline property suite) is delivered by task 6; gates 1 and 9 re-proven in step 2 of this task. The remaining sub-project 5 work (output formats, migrate, verbose channel, distribution, benchmark, release) continues in later plans.
