# CLI Product v0.1, Part 1: The `celerrate_config` Crate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the `celerrate_config` crate (span-preserving `celerrate.toml` parsing into a pure `Configuration` model, with configuration diagnostics CEL0043 to CEL0049), wire the CLI to load the file and report its diagnostics span-anchored and exit-affecting, and hold zero-config parity.

**Architecture:** `celerrate_config` sits above `celerrate_diagnostics` and below `celerrate_project` in the DAG. It exposes `parse` (structural walk over a `toml_edit` document, producing `(Configuration, Vec<Diagnostic>)`) and `validate` (semantic checks parameterized by `KnownSets`, so the crate never depends on the rule registry above it). The CLI loads `celerrate.toml` next to `composer.json`, interns it into the VFS for rendering, builds `KnownSets` from `celerrate_rules::core_rules()` and `celerrate_diagnostics::REGISTRY`, and merges the configuration diagnostics into the presented report and the exit code. The `Configuration` value itself is consumed by nobody yet: behavioral wiring (include/exclude, `php`, active set, severity remap, cache digest) is part 2.

**Tech Stack:** Rust (edition 2024), `toml_edit` (new workspace dependency, MIT OR Apache-2.0), existing crates `celerrate_source` (`FileId`, `TextRange`, `TextSize`), `celerrate_diagnostics` (`Diagnostic`, `DiagnosticId`, `Severity`, `ExplainPage`, `REGISTRY`).

Spec: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` (sections 2, 3, and 10 item 1). Read it before starting.

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result` or emits diagnostics. Test modules open with the local `#![allow(...)]` block (copy the pattern from `crates/celerrate_diagnostics/src/registry.rs` tests).
- TDD: every task writes the failing test first, sees it fail, implements minimally, sees it pass, commits.
- Everything in files is English, full words, no abbreviated names.
- Layering: `celerrate_config` depends on `celerrate_source`, `celerrate_diagnostics`, and `toml_edit` only. Never on salsa, never on any higher crate.
- No user input may crash the tool: a malformed `celerrate.toml` produces diagnostics, never a failure.
- Commits: gitmoji + Conventional Commits (`✨ feat(config): ...`). Never override the repository git identity. No Claude attribution anywhere.
- Mechanical suite that must stay green after every task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo deny check`.
- Part 1 boundary, stated: the parsed `Configuration` is loaded and reported but **not consumed**; `--watch` does not report configuration diagnostics yet (both are part 2, and Task 6 leaves a comment saying so).

---

### Task 1: Crate scaffold, the configuration model, and the identifier constants

**Files:**
- Modify: `Cargo.toml` (workspace root; add `toml_edit` to `[workspace.dependencies]`)
- Create: `crates/celerrate_config/Cargo.toml`
- Create: `crates/celerrate_config/src/lib.rs`
- Create: `crates/celerrate_config/src/identifiers.rs`
- Create: `crates/celerrate_config/src/model.rs`

**Interfaces:**
- Consumes: `celerrate_source::TextRange`, `celerrate_diagnostics::{DiagnosticId, Severity}`.
- Produces (later tasks rely on these exact names): `Spanned<T> { value: T, range: TextRange }`, `Configuration { php: Option<Spanned<(u8, u8)>>, include: Vec<Spanned<String>>, exclude: Vec<Spanned<String>>, rules: Vec<RuleEntry>, severity: Vec<SeverityEntry> }`, `RuleEntry { name: Spanned<String>, enabled: Option<Spanned<bool>> }`, `SeverityEntry { identifier: Spanned<String>, severity: Spanned<Severity> }`, and the seven `DiagnosticId` constants named below.

- [ ] **Step 1: Add the workspace dependency**

In the root `Cargo.toml`, `[workspace.dependencies]` section, add in alphabetical order:

```toml
toml_edit = "0.23"
```

If `cargo check` later reports that this version does not exist, run `cargo add --dry-run toml_edit` to learn the current one and pin that instead; `toml_edit` is MIT OR Apache-2.0, inside the `deny.toml` license allowlist.

- [ ] **Step 2: Create the crate manifest**

`crates/celerrate_config/Cargo.toml`:

```toml
[package]
name = "celerrate_config"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_diagnostics = { path = "../celerrate_diagnostics" }
celerrate_source = { path = "../celerrate_source" }
toml_edit = { workspace = true }

[lints]
workspace = true
```

The workspace `members = ["crates/*", "xtask"]` glob picks the crate up; no root edit needed for membership.

- [ ] **Step 3: Write the failing test (model construction)**

`crates/celerrate_config/src/model.rs`:

```rust
//! The pure configuration model: what a parsed `celerrate.toml` says,
//! with the span of every user-written value so semantic validation
//! (here and at the composition root) can anchor precise diagnostics.
//!
//! This model is data, not behavior: nothing here reads a file, knows
//! salsa, or sees the rule registry. Part 2 of the sub-project wires
//! its consumption.

use celerrate_diagnostics::Severity;
use celerrate_source::TextRange;

/// A value and the range of its source text in `celerrate.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub value: T,
    pub range: TextRange,
}

/// One `[rules.<name>]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleEntry {
    /// The rule name as written (the table key).
    pub name: Spanned<String>,
    /// The `enabled` key, absent when the table does not set it (a
    /// valid no-op table).
    pub enabled: Option<Spanned<bool>>,
}

/// One `[severity]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeverityEntry {
    /// The identifier as written (existence is checked by `validate`,
    /// not at parse time).
    pub identifier: Spanned<String>,
    pub severity: Spanned<Severity>,
}

/// A parsed `celerrate.toml`. Every field is optional or empty by
/// default: an empty file is a valid configuration (zero config is the
/// contract; a file only narrows it).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Configuration {
    /// `[project] php = "8.2"`: the version point that collapses the
    /// detected range (consumed in part 2).
    pub php: Option<Spanned<(u8, u8)>>,
    /// `[project] include = [...]`, relative paths.
    pub include: Vec<Spanned<String>>,
    /// `[project] exclude = [...]`, relative paths.
    pub exclude: Vec<Spanned<String>>,
    /// The `[rules.<name>]` tables, in file order.
    pub rules: Vec<RuleEntry>,
    /// The `[severity]` entries, in file order.
    pub severity: Vec<SeverityEntry>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::Configuration;

    #[test]
    fn the_default_configuration_is_empty() {
        let configuration = Configuration::default();
        assert!(configuration.php.is_none());
        assert!(configuration.include.is_empty());
        assert!(configuration.exclude.is_empty());
        assert!(configuration.rules.is_empty());
        assert!(configuration.severity.is_empty());
    }
}
```

- [ ] **Step 4: Write the identifier constants**

`crates/celerrate_config/src/identifiers.rs`:

```rust
//! The diagnostic identifiers this crate emits. Allocated CEL0043 to
//! CEL0049 in the canonical registry (`celerrate_diagnostics`); the
//! registry entries land with the explain pages, after the CLI wiring
//! makes their examples executable.

use celerrate_diagnostics::DiagnosticId;

/// `celerrate.toml` is not valid TOML (or not valid UTF-8, or
/// unreadable): the file exists but cannot be read as a configuration.
pub const INVALID_CONFIGURATION: DiagnosticId = DiagnosticId::new("CEL0043");
/// A key the schema does not know, anywhere in the file.
pub const UNKNOWN_CONFIGURATION_KEY: DiagnosticId = DiagnosticId::new("CEL0044");
/// A known key with a value of the wrong type or shape.
pub const INVALID_CONFIGURATION_VALUE: DiagnosticId = DiagnosticId::new("CEL0045");
/// A `[rules.<name>]` table naming a rule the registry does not know.
pub const UNKNOWN_RULE: DiagnosticId = DiagnosticId::new("CEL0046");
/// A `[rules.<name>]` key other than `enabled`: no rule has options yet.
pub const UNSUPPORTED_RULE_OPTION: DiagnosticId = DiagnosticId::new("CEL0047");
/// A `[severity]` key naming an identifier the registry does not know.
pub const UNKNOWN_SEVERITY_IDENTIFIER: DiagnosticId = DiagnosticId::new("CEL0048");
/// A `[severity]` key naming a resilience identifier: those are neither
/// disableable nor remappable by design.
pub const RESILIENCE_SEVERITY_REMAP: DiagnosticId = DiagnosticId::new("CEL0049");
```

- [ ] **Step 5: Create the crate root**

`crates/celerrate_config/src/lib.rs`:

```rust
//! `celerrate.toml` parsing and validation: the pure configuration
//! model of the CLI product design (spec section 2).
//!
//! Two functions, one boundary: [`parse`] turns file text into a
//! [`Configuration`] plus structural diagnostics, and [`validate`]
//! checks the names the file uses against [`KnownSets`] the caller
//! provides, because the sets live above this crate in the DAG (the
//! rule registry) and the composition root is the only place that
//! sees both.

mod identifiers;
mod model;

pub use identifiers::{
    INVALID_CONFIGURATION, INVALID_CONFIGURATION_VALUE, RESILIENCE_SEVERITY_REMAP, UNKNOWN_RULE,
    UNKNOWN_CONFIGURATION_KEY, UNKNOWN_SEVERITY_IDENTIFIER, UNSUPPORTED_RULE_OPTION,
};
pub use model::{Configuration, RuleEntry, SeverityEntry, Spanned};
```

(`parse` and `validate` join these exports in Tasks 2 and 5.)

- [ ] **Step 6: Run the test and the suite**

Run: `cargo test --package celerrate_config`
Expected: PASS (`the_default_configuration_is_empty`).
Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: clean (deny now also vets `toml_edit`).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/celerrate_config
git commit -m "✨ feat(config): scaffold celerrate_config with the configuration model"
```

---

### Task 2: `parse`, first slice: syntax errors as CEL0043

**Files:**
- Create: `crates/celerrate_config/src/parse.rs`
- Modify: `crates/celerrate_config/src/lib.rs`

**Interfaces:**
- Produces: `pub fn parse(file: FileId, text: &str) -> (Configuration, Vec<Diagnostic>)`. On a syntax error: empty `Configuration` plus one CEL0043 diagnostic with the parser's span. Also the crate-internal helper `fn text_range(span: core::ops::Range<usize>) -> TextRange` reused by Tasks 3 and 4.

- [ ] **Step 1: Write the failing tests**

At the bottom of `crates/celerrate_config/src/parse.rs` (create the file with just the test module and a stub-free compile error for now, or write test-first in one file; the module is wired in Step 3):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use celerrate_source::FileId;

    use crate::identifiers::INVALID_CONFIGURATION;
    use crate::parse::parse;

    fn file() -> FileId {
        FileId::new(0)
    }

    #[test]
    fn an_empty_file_is_an_empty_configuration() {
        let (configuration, diagnostics) = parse(file(), "");
        assert_eq!(configuration, crate::Configuration::default());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_syntax_error_reports_cel0043_with_a_span() {
        let (configuration, diagnostics) = parse(file(), "[project\n");
        assert_eq!(configuration, crate::Configuration::default());
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION);
        assert!(diagnostic.span().is_some(), "syntax errors are span-anchored");
        assert!(diagnostic.message.starts_with("invalid TOML:"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package celerrate_config`
Expected: FAIL to compile ("cannot find function `parse`").

- [ ] **Step 3: Implement the parse skeleton**

Top of `crates/celerrate_config/src/parse.rs`:

```rust
//! The structural walk: file text to `Configuration` plus structural
//! diagnostics (syntax, unknown keys, invalid values). Semantic checks
//! against the registries live in `validate`, not here.

use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::{FileId, TextRange, TextSize};

use crate::identifiers::INVALID_CONFIGURATION;
use crate::model::Configuration;

/// A byte span from `toml_edit` as a `TextRange`. Configuration files
/// are far below `u32` size; a hypothetical overflow saturates rather
/// than panics.
fn text_range(span: core::ops::Range<usize>) -> TextRange {
    let start = u32::try_from(span.start).unwrap_or(u32::MAX);
    let end = u32::try_from(span.end).unwrap_or(u32::MAX);
    TextRange::new(TextSize::from(start), TextSize::from(end.max(start)))
}

/// The whole-file fallback anchor for findings the parser gives no
/// span for: the first byte, or an empty range on an empty file.
fn fallback_range(text: &str) -> TextRange {
    let end = u32::from(!text.is_empty());
    TextRange::new(TextSize::from(0), TextSize::from(end))
}

/// Parses `celerrate.toml` text. Never fails: what does not parse is a
/// diagnostic, and the configuration degrades to its default.
pub fn parse(file: FileId, text: &str) -> (Configuration, Vec<Diagnostic>) {
    let document = match toml_edit::ImDocument::parse(text) {
        Ok(document) => document,
        Err(error) => {
            let range = error.span().map_or_else(|| fallback_range(text), text_range);
            let diagnostic = Diagnostic::spanned(
                INVALID_CONFIGURATION,
                Severity::Error,
                file,
                range,
                format!("invalid TOML: {}", error.message()),
            );
            return (Configuration::default(), vec![diagnostic]);
        }
    };
    let mut configuration = Configuration::default();
    let mut diagnostics = Vec::new();
    walk_root(file, document.as_table(), &mut configuration, &mut diagnostics);
    diagnostics.sort();
    (configuration, diagnostics)
}

/// The root walk grows in the next tasks; for now every top-level key
/// is accepted silently so the syntax slice lands alone.
fn walk_root(
    _file: FileId,
    _table: &toml_edit::Table,
    _configuration: &mut Configuration,
    _diagnostics: &mut Vec<Diagnostic>,
) {
}
```

In `crates/celerrate_config/src/lib.rs`, add `mod parse;` beside the other modules and `pub use parse::parse;`.

Note on `toml_edit`: `ImDocument::parse` keeps source spans (`TomlError::span()`, `Key::span()`, `Item::span()` return `Option<Range<usize>>`; on an `ImDocument` they are populated). If a signature differs under the pinned version, consult `docs.rs/toml_edit` for the release actually resolved and adapt the call sites, not the tests.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package celerrate_config`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_config
git commit -m "✨ feat(config): report celerrate.toml syntax errors as CEL0043"
```

---

### Task 3: `parse`, second slice: the `[project]` table

**Files:**
- Modify: `crates/celerrate_config/src/parse.rs`

**Interfaces:**
- Consumes: `text_range`, `Configuration`, `Spanned` from Tasks 1 and 2.
- Produces: `walk_root` handles `[project]` (keys `php`, `include`, `exclude`), unknown root keys, and unknown `[project]` keys. Message shapes later tasks and pages rely on: `` unknown configuration key `{key}` `` (CEL0044) and `` invalid value for `{key}`: {expectation} `` (CEL0045).

- [ ] **Step 1: Write the failing tests**

Append to the test module of `parse.rs`:

```rust
    use crate::identifiers::{INVALID_CONFIGURATION_VALUE, UNKNOWN_CONFIGURATION_KEY};

    fn single(diagnostics: &[celerrate_diagnostics::Diagnostic]) -> &celerrate_diagnostics::Diagnostic {
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        diagnostics.first().unwrap()
    }

    #[test]
    fn the_project_table_parses_php_include_and_exclude() {
        let text = "[project]\nphp = \"8.2\"\ninclude = [\"src\", \"tests\"]\nexclude = [\"src/Generated\"]\n";
        let (configuration, diagnostics) = parse(file(), text);
        assert!(diagnostics.is_empty());
        assert_eq!(configuration.php.as_ref().unwrap().value, (8, 2));
        let include: Vec<&str> = configuration.include.iter().map(|entry| entry.value.as_str()).collect();
        assert_eq!(include, ["src", "tests"]);
        let exclude: Vec<&str> = configuration.exclude.iter().map(|entry| entry.value.as_str()).collect();
        assert_eq!(exclude, ["src/Generated"]);
    }

    #[test]
    fn an_unknown_root_key_reports_cel0044() {
        let (_, diagnostics) = parse(file(), "reals = 1\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNKNOWN_CONFIGURATION_KEY);
        assert_eq!(diagnostic.message, "unknown configuration key `reals`");
    }

    #[test]
    fn an_unknown_project_key_reports_cel0044() {
        let (_, diagnostics) = parse(file(), "[project]\nincludes = [\"src\"]\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNKNOWN_CONFIGURATION_KEY);
        assert_eq!(diagnostic.message, "unknown configuration key `project.includes`");
    }

    #[test]
    fn a_php_constraint_that_is_not_a_version_point_reports_cel0045() {
        let (configuration, diagnostics) = parse(file(), "[project]\nphp = \"^8.1\"\n");
        assert!(configuration.php.is_none());
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `php`: expected a version point like \"8.2\"",
        );
    }

    #[test]
    fn a_non_array_include_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "[project]\ninclude = \"src\"\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `include`: expected an array of relative paths",
        );
    }

    #[test]
    fn an_absolute_or_empty_include_entry_reports_cel0045() {
        let (configuration, diagnostics) = parse(file(), "[project]\ninclude = [\"/etc\", \"\"]\n");
        assert!(configuration.include.is_empty());
        assert_eq!(diagnostics.len(), 2);
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
            assert_eq!(
                diagnostic.message,
                "invalid value for `include`: expected a non-empty relative path",
            );
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package celerrate_config`
Expected: the new tests FAIL (empty `walk_root` accepts everything and parses nothing).

- [ ] **Step 3: Implement the `[project]` walk**

Replace the `walk_root` stub in `parse.rs` (the `[rules]`, `[severity]`, and `[plugins]` arms are Task 4; until then those root keys fall through to "unknown", which Task 4's tests will move):

```rust
use crate::identifiers::{INVALID_CONFIGURATION_VALUE, UNKNOWN_CONFIGURATION_KEY};
use crate::model::Spanned;

/// The span of `key` in `table`, with the whole-table fallback that
/// keeps every diagnostic anchored even if `toml_edit` yields no span.
fn key_range(table: &toml_edit::Table, key: &str, text_fallback: TextRange) -> TextRange {
    table
        .key(key)
        .and_then(|key| key.span())
        .map_or(text_fallback, text_range)
}

/// The span of a value item, falling back to its key's span.
fn item_range(table: &toml_edit::Table, key: &str, item: &toml_edit::Item, fallback: TextRange) -> TextRange {
    item.span().map_or_else(|| key_range(table, key, fallback), text_range)
}

fn unknown_key(file: FileId, range: TextRange, path: &str) -> Diagnostic {
    Diagnostic::spanned(
        UNKNOWN_CONFIGURATION_KEY,
        Severity::Error,
        file,
        range,
        format!("unknown configuration key `{path}`"),
    )
}

fn invalid_value(file: FileId, range: TextRange, key: &str, expectation: &str) -> Diagnostic {
    Diagnostic::spanned(
        INVALID_CONFIGURATION_VALUE,
        Severity::Error,
        file,
        range,
        format!("invalid value for `{key}`: {expectation}"),
    )
}

/// `"8.2"` as a `(major, minor)` point; `None` for every other shape
/// (ranges, carets, prose), which CEL0045 reports.
fn version_point(text: &str) -> Option<(u8, u8)> {
    let (major, minor) = text.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn walk_root(
    file: FileId,
    table: &toml_edit::Table,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fallback = fallback_range("x");
    for (key, item) in table.iter() {
        let range = key_range(table, key, fallback);
        match key {
            "project" => match item.as_table() {
                Some(project) => {
                    walk_project(file, project, configuration, diagnostics, fallback);
                }
                None => diagnostics.push(invalid_value(
                    file,
                    item_range(table, key, item, fallback),
                    key,
                    "expected a table",
                )),
            },
            _ => diagnostics.push(unknown_key(file, range, key)),
        }
    }
}

fn walk_project(
    file: FileId,
    project: &toml_edit::Table,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (key, item) in project.iter() {
        let value_range = item_range(project, key, item, fallback);
        match key {
            "php" => match item.as_str().and_then(version_point) {
                Some(point) => {
                    configuration.php = Some(Spanned { value: point, range: value_range });
                }
                None => diagnostics.push(invalid_value(
                    file,
                    value_range,
                    key,
                    "expected a version point like \"8.2\"",
                )),
            },
            "include" | "exclude" => {
                let entries = path_array(file, key, item, value_range, diagnostics);
                if key == "include" {
                    configuration.include = entries;
                } else {
                    configuration.exclude = entries;
                }
            }
            _ => diagnostics.push(unknown_key(
                file,
                key_range(project, key, fallback),
                &format!("project.{key}"),
            )),
        }
    }
}

/// An `include`/`exclude` array: non-empty relative path strings. A
/// malformed entry is reported and skipped; the well-formed ones stay.
fn path_array(
    file: FileId,
    key: &str,
    item: &toml_edit::Item,
    value_range: TextRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Spanned<String>> {
    let Some(array) = item.as_array() else {
        diagnostics.push(invalid_value(
            file,
            value_range,
            key,
            "expected an array of relative paths",
        ));
        return Vec::new();
    };
    let mut entries = Vec::new();
    for value in array.iter() {
        let entry_range = value.span().map_or(value_range, text_range);
        match value.as_str() {
            Some(path) if !path.is_empty() && !std::path::Path::new(path).is_absolute() => {
                entries.push(Spanned { value: path.to_owned(), range: entry_range });
            }
            _ => diagnostics.push(invalid_value(
                file,
                entry_range,
                key,
                "expected a non-empty relative path",
            )),
        }
    }
    entries
}
```

Adjust the earlier `walk_root` call in `parse` if its signature moved; keep `diagnostics.sort()` at the end of `parse`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package celerrate_config`
Expected: PASS (all tests so far).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_config
git commit -m "✨ feat(config): walk the [project] table with structural diagnostics"
```

---

### Task 4: `parse`, third slice: `[rules]`, `[severity]`, and `[plugins]`

**Files:**
- Modify: `crates/celerrate_config/src/parse.rs`

**Interfaces:**
- Produces: `walk_root` gains the three remaining arms. Message shapes: `` rule `{name}` has no configurable options; `{key}` is not recognized `` (CEL0047), `` invalid value for `{path}`: expected "error" or "warning" `` (CEL0045 on severity values), `` invalid value for `enabled`: expected a boolean `` (CEL0045). `[plugins]` keys report CEL0044 as `` unknown configuration key `plugins.{key}` ``.

- [ ] **Step 1: Write the failing tests**

Append to the test module:

```rust
    use crate::identifiers::UNSUPPORTED_RULE_OPTION;

    #[test]
    fn a_rule_table_parses_its_enabled_flag() {
        let text = "[rules.null-dereference]\nenabled = false\n\n[rules.some-nursery-rule]\nenabled = true\n";
        let (configuration, diagnostics) = parse(file(), text);
        assert!(diagnostics.is_empty());
        let rules: Vec<(&str, Option<bool>)> = configuration
            .rules
            .iter()
            .map(|rule| (rule.name.value.as_str(), rule.enabled.as_ref().map(|flag| flag.value)))
            .collect();
        assert_eq!(
            rules,
            [("null-dereference", Some(false)), ("some-nursery-rule", Some(true))],
        );
    }

    #[test]
    fn an_empty_rule_table_is_a_valid_no_op() {
        let (configuration, diagnostics) = parse(file(), "[rules.null-dereference]\n");
        assert!(diagnostics.is_empty());
        assert_eq!(configuration.rules.first().unwrap().enabled, None);
    }

    #[test]
    fn a_rule_option_other_than_enabled_reports_cel0047() {
        let (_, diagnostics) = parse(file(), "[rules.null-dereference]\nmax = 3\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNSUPPORTED_RULE_OPTION);
        assert_eq!(
            diagnostic.message,
            "rule `null-dereference` has no configurable options; `max` is not recognized",
        );
    }

    #[test]
    fn a_non_boolean_enabled_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "[rules.null-dereference]\nenabled = \"yes\"\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(diagnostic.message, "invalid value for `enabled`: expected a boolean");
    }

    #[test]
    fn a_rules_entry_that_is_not_a_table_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "[rules]\ndisable = [\"null-dereference\"]\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `rules.disable`: expected a table like [rules.disable]",
        );
    }

    #[test]
    fn severity_entries_parse_and_reject_other_words() {
        let text = "[severity]\n\"CEL0034\" = \"warning\"\n\"CEL0035\" = \"info\"\n";
        let (configuration, diagnostics) = parse(file(), text);
        assert_eq!(configuration.severity.len(), 1);
        let entry = configuration.severity.first().unwrap();
        assert_eq!(entry.identifier.value, "CEL0034");
        assert_eq!(entry.severity.value, celerrate_diagnostics::Severity::Warning);
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `severity.CEL0035`: expected \"error\" or \"warning\"",
        );
    }

    #[test]
    fn a_plugins_key_reports_cel0044() {
        let (_, diagnostics) = parse(file(), "[plugins]\nphpdoc-bridge = true\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNKNOWN_CONFIGURATION_KEY);
        assert_eq!(diagnostic.message, "unknown configuration key `plugins.phpdoc-bridge`");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package celerrate_config`
Expected: the new tests FAIL (`rules`, `severity`, `plugins` currently report CEL0044 as unknown root keys).

- [ ] **Step 3: Implement the three arms**

In `walk_root`, replace the `_ =>` arm's neighborhood so the match reads `"project" => ...`, `"rules" => ...`, `"severity" => ...`, `"plugins" => ...`, `_ => unknown root key`. New arms and helpers:

```rust
use crate::identifiers::UNSUPPORTED_RULE_OPTION;
use crate::model::{RuleEntry, SeverityEntry};

// inside walk_root's match:
            "rules" => match item.as_table() {
                Some(rules) => walk_rules(file, rules, configuration, diagnostics, fallback),
                None => diagnostics.push(invalid_value(
                    file,
                    item_range(table, key, item, fallback),
                    key,
                    "expected a table of [rules.<name>] tables",
                )),
            },
            "severity" => match item.as_table() {
                Some(severity) => walk_severity(file, severity, configuration, diagnostics, fallback),
                None => diagnostics.push(invalid_value(
                    file,
                    item_range(table, key, item, fallback),
                    key,
                    "expected a table",
                )),
            },
            "plugins" => match item.as_table() {
                Some(plugins) => {
                    for (plugin_key, _) in plugins.iter() {
                        diagnostics.push(unknown_key(
                            file,
                            key_range(plugins, plugin_key, fallback),
                            &format!("plugins.{plugin_key}"),
                        ));
                    }
                }
                None => diagnostics.push(invalid_value(
                    file,
                    item_range(table, key, item, fallback),
                    key,
                    "expected a table",
                )),
            },

fn walk_rules(
    file: FileId,
    rules: &toml_edit::Table,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (name, item) in rules.iter() {
        let name_range = key_range(rules, name, fallback);
        let Some(rule) = item.as_table() else {
            diagnostics.push(invalid_value(
                file,
                item_range(rules, name, item, fallback),
                &format!("rules.{name}"),
                &format!("expected a table like [rules.{name}]"),
            ));
            continue;
        };
        let mut enabled = None;
        for (key, value) in rule.iter() {
            let value_range = item_range(rule, key, value, fallback);
            if key == "enabled" {
                match value.as_bool() {
                    Some(flag) => enabled = Some(Spanned { value: flag, range: value_range }),
                    None => diagnostics.push(invalid_value(
                        file,
                        value_range,
                        "enabled",
                        "expected a boolean",
                    )),
                }
            } else {
                diagnostics.push(Diagnostic::spanned(
                    UNSUPPORTED_RULE_OPTION,
                    Severity::Error,
                    file,
                    key_range(rule, key, fallback),
                    format!("rule `{name}` has no configurable options; `{key}` is not recognized"),
                ));
            }
        }
        configuration.rules.push(RuleEntry {
            name: Spanned { value: name.to_owned(), range: name_range },
            enabled,
        });
    }
}

fn walk_severity(
    file: FileId,
    severity: &toml_edit::Table,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (identifier, item) in severity.iter() {
        let identifier_range = key_range(severity, identifier, fallback);
        let value_range = item_range(severity, identifier, item, fallback);
        let parsed = match item.as_str() {
            Some("error") => Some(Severity::Error),
            Some("warning") => Some(Severity::Warning),
            _ => None,
        };
        match parsed {
            Some(value) => configuration.severity.push(SeverityEntry {
                identifier: Spanned { value: identifier.to_owned(), range: identifier_range },
                severity: Spanned { value, range: value_range },
            }),
            None => diagnostics.push(invalid_value(
                file,
                value_range,
                &format!("severity.{identifier}"),
                "expected \"error\" or \"warning\"",
            )),
        }
    }
}
```

Note: `invalid_value` currently takes `&str` for both `key` and `expectation`; the new call sites pass formatted strings, which coerce via `&format!(...)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package celerrate_config`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_config
git commit -m "✨ feat(config): walk the [rules], [severity], and [plugins] tables"
```

---

### Task 5: `validate` against `KnownSets`

**Files:**
- Create: `crates/celerrate_config/src/validate.rs`
- Modify: `crates/celerrate_config/src/lib.rs`

**Interfaces:**
- Consumes: `Configuration` from Task 1.
- Produces (Task 6 relies on these exact names):

```rust
pub struct KnownSets<'sets> {
    pub rule_names: BTreeSet<&'sets str>,
    pub remappable_identifiers: BTreeSet<&'sets str>,
    pub registered_identifiers: BTreeSet<&'sets str>,
}
pub fn validate(file: FileId, configuration: &Configuration, known: &KnownSets<'_>) -> Vec<Diagnostic>
```

- [ ] **Step 1: Write the failing tests**

`crates/celerrate_config/src/validate.rs`, test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use std::collections::BTreeSet;

    use celerrate_source::FileId;

    use crate::identifiers::{RESILIENCE_SEVERITY_REMAP, UNKNOWN_RULE, UNKNOWN_SEVERITY_IDENTIFIER};
    use crate::parse::parse;
    use crate::validate::{KnownSets, validate};

    fn known() -> KnownSets<'static> {
        KnownSets {
            rule_names: BTreeSet::from(["null-dereference", "unknown-members"]),
            remappable_identifiers: BTreeSet::from(["CEL0034", "CEL0030"]),
            registered_identifiers: BTreeSet::from(["CEL0034", "CEL0030", "CEL0026"]),
        }
    }

    fn diagnostics_for(text: &str) -> Vec<celerrate_diagnostics::Diagnostic> {
        let file = FileId::new(0);
        let (configuration, structural) = parse(file, text);
        assert!(structural.is_empty(), "fixture must be structurally clean");
        validate(file, &configuration, &known())
    }

    #[test]
    fn a_known_rule_and_a_remappable_identifier_are_silent() {
        let text = "[rules.null-dereference]\nenabled = false\n\n[severity]\n\"CEL0034\" = \"warning\"\n";
        assert!(diagnostics_for(text).is_empty());
    }

    #[test]
    fn an_unknown_rule_reports_cel0046() {
        let diagnostics = diagnostics_for("[rules.nul-dereference]\nenabled = false\n");
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, UNKNOWN_RULE);
        assert_eq!(diagnostic.message, "unknown rule `nul-dereference`");
        assert!(diagnostic.span().is_some());
    }

    #[test]
    fn an_unregistered_severity_identifier_reports_cel0048() {
        let diagnostics = diagnostics_for("[severity]\n\"CEL9999\" = \"warning\"\n");
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, UNKNOWN_SEVERITY_IDENTIFIER);
        assert_eq!(diagnostic.message, "unknown diagnostic identifier `CEL9999`");
    }

    #[test]
    fn a_resilience_identifier_remap_reports_cel0049() {
        let diagnostics = diagnostics_for("[severity]\n\"CEL0026\" = \"error\"\n");
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, RESILIENCE_SEVERITY_REMAP);
        assert_eq!(
            diagnostic.message,
            "`CEL0026` is a resilience diagnostic; its severity cannot be remapped",
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package celerrate_config`
Expected: FAIL to compile (`validate` not defined).

- [ ] **Step 3: Implement `validate`**

Top of `validate.rs`:

```rust
//! Semantic validation: the names the file uses, checked against the
//! sets the composition root provides. The sets are parameters because
//! they live above this crate in the DAG (the rule registry, the
//! diagnostic registry): the crate owns the diagnostics, the caller
//! owns the knowledge.

use std::collections::BTreeSet;

use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::FileId;

use crate::identifiers::{RESILIENCE_SEVERITY_REMAP, UNKNOWN_RULE, UNKNOWN_SEVERITY_IDENTIFIER};
use crate::model::Configuration;

/// What the composition root knows and this crate cannot: the
/// registered rule names, the identifiers rules may emit (the
/// remappable set), and every registered identifier.
pub struct KnownSets<'sets> {
    pub rule_names: BTreeSet<&'sets str>,
    pub remappable_identifiers: BTreeSet<&'sets str>,
    pub registered_identifiers: BTreeSet<&'sets str>,
}

/// Checks every name `configuration` uses. A typo must never silently
/// configure nothing: each unknown name is a span-anchored diagnostic.
pub fn validate(
    file: FileId,
    configuration: &Configuration,
    known: &KnownSets<'_>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for rule in &configuration.rules {
        if !known.rule_names.contains(rule.name.value.as_str()) {
            diagnostics.push(Diagnostic::spanned(
                UNKNOWN_RULE,
                Severity::Error,
                file,
                rule.name.range,
                format!("unknown rule `{}`", rule.name.value),
            ));
        }
    }
    for entry in &configuration.severity {
        let identifier = entry.identifier.value.as_str();
        if !known.registered_identifiers.contains(identifier) {
            diagnostics.push(Diagnostic::spanned(
                UNKNOWN_SEVERITY_IDENTIFIER,
                Severity::Error,
                file,
                entry.identifier.range,
                format!("unknown diagnostic identifier `{identifier}`"),
            ));
        } else if !known.remappable_identifiers.contains(identifier) {
            diagnostics.push(Diagnostic::spanned(
                RESILIENCE_SEVERITY_REMAP,
                Severity::Error,
                file,
                entry.identifier.range,
                format!("`{identifier}` is a resilience diagnostic; its severity cannot be remapped"),
            ));
        }
    }
    diagnostics.sort();
    diagnostics
}
```

In `lib.rs`: `mod validate;` and `pub use validate::{KnownSets, validate};`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package celerrate_config`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_config
git commit -m "✨ feat(config): validate rule names and identifiers against known sets"
```

---

### Task 6: CLI wiring: load, report, and count `celerrate.toml` diagnostics

**Files:**
- Create: `crates/celerrate_cli/src/configuration.rs`
- Modify: `crates/celerrate_cli/Cargo.toml` (add the `celerrate_config` dependency)
- Modify: `crates/celerrate_cli/src/session.rs` (the `Session` struct and `Session::start`)
- Modify: `crates/celerrate_cli/src/render.rs` (the `SessionSources::text` fallback)
- Modify: `crates/celerrate_cli/src/lib.rs` (the `Command::Check` arm)
- Create: `crates/celerrate_cli/tests/configuration.rs`

**Interfaces:**
- Consumes: `celerrate_config::{parse, validate, Configuration, KnownSets}`; `celerrate_rules::core_rules()` (returns `Vec<(RuleMetadata, RuleImplementation)>`; `RuleMetadata { name: String, identifiers: Vec<RuleIdentifier { id, severity }>, .. }`); `celerrate_diagnostics::REGISTRY`; `Vfs::file_id(&mut self, path: &Path) -> FileId`.
- Produces: `configuration::LoadedConfiguration { file: FileId, text: String, configuration: celerrate_config::Configuration, diagnostics: Vec<Diagnostic> }`, `configuration::load(root: &Path, vfs: &mut Vfs) -> Option<LoadedConfiguration>`, and the new `Session` field `pub loaded_configuration: Option<LoadedConfiguration>` (part 2 consumes `configuration` from it).

- [ ] **Step 1: Write the failing integration tests**

`crates/celerrate_cli/tests/configuration.rs` (mirror the harness style of `tests/explain_pages.rs`: tempdir, write files, call `celerrate_cli::run`):

```rust
//! `celerrate.toml` loading: configuration diagnostics are reported
//! span-anchored and affect the exit code; a missing file changes
//! nothing (zero-config parity); the configuration itself is not yet
//! consumed (part 2).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::ffi::OsString;
use std::fs;

use celerrate_cli::{ColorMode, Outcome};

const MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
const CLEAN_SOURCE: &str = "<?php\nnamespace App;\n\nfunction example(): void {}\n";

fn check(files: &[(&str, &str)]) -> (Outcome, String) {
    let directory = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = directory.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full, contents).unwrap();
    }
    let mut output = Vec::new();
    let outcome = celerrate_cli::run(
        vec![
            OsString::from("celerrate"),
            OsString::from("check"),
            directory.path().as_os_str().to_owned(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn without_a_configuration_file_a_clean_project_stays_clean() {
    let (outcome, report) = check(&[("composer.json", MANIFEST), ("src/Example.php", CLEAN_SOURCE)]);
    assert!(matches!(outcome, Outcome::Clean), "report was:\n{report}");
    assert!(!report.contains("CEL004"), "report was:\n{report}");
}

#[test]
fn a_valid_configuration_file_reports_nothing() {
    let configuration = "[project]\nphp = \"8.2\"\n\n[rules.null-dereference]\nenabled = false\n";
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate.toml", configuration),
    ]);
    assert!(matches!(outcome, Outcome::Clean), "report was:\n{report}");
}

#[test]
fn a_syntax_error_in_the_configuration_exits_one_with_a_rich_block() {
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate.toml", "[project\n"),
    ]);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "report was:\n{report}");
    assert!(report.contains("CEL0043"), "report was:\n{report}");
    assert!(report.contains("celerrate.toml"), "report was:\n{report}");
}

#[test]
fn an_unknown_rule_in_the_configuration_exits_one() {
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate.toml", "[rules.nul-dereference]\nenabled = false\n"),
    ]);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "report was:\n{report}");
    assert!(report.contains("CEL0046"), "report was:\n{report}");
    assert!(report.contains("nul-dereference"), "report was:\n{report}");
}

#[test]
fn a_resilience_remap_in_the_configuration_exits_one() {
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate.toml", "[severity]\n\"CEL0026\" = \"error\"\n"),
    ]);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "report was:\n{report}");
    assert!(report.contains("CEL0049"), "report was:\n{report}");
}
```

If `Outcome` is not exported from `celerrate_cli` or lacks the shape these `matches!` calls assume, check `crates/celerrate_cli/src/lib.rs` (the enum is `pub enum Outcome { Clean, DiagnosticsReported, InternalError, UsageError }`) and export it if needed.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package celerrate_cli --test configuration`
Expected: the three failing-configuration tests FAIL (`celerrate.toml` is currently ignored, so every project is `Clean`); the two parity tests PASS.

- [ ] **Step 3: Implement the loader**

Add to `crates/celerrate_cli/Cargo.toml` `[dependencies]`: `celerrate_config = { path = "../celerrate_config" }`.

`crates/celerrate_cli/src/configuration.rs`:

```rust
//! `celerrate.toml` loading at the composition root: reads the file
//! next to `composer.json`, interns it into the VFS so the renderer
//! can excerpt it, and runs `celerrate_config`'s parse and validate
//! with the known sets only this crate can see (the rule registry and
//! the diagnostic registry).
//!
//! Part 1 boundary: the parsed configuration is carried but not yet
//! consumed; include/exclude, `php`, the active set, and the severity
//! remap are part 2 of the sub-project.

use std::collections::BTreeSet;
use std::path::Path;

use celerrate_config::KnownSets;
use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::FileId;
use celerrate_vfs::Vfs;

/// The loaded `celerrate.toml`: its identity, its text (for the
/// renderer), the parsed model (for part 2), and its diagnostics.
pub struct LoadedConfiguration {
    pub file: FileId,
    pub text: String,
    pub configuration: celerrate_config::Configuration,
    pub diagnostics: Vec<Diagnostic>,
}

/// Loads `<root>/celerrate.toml`. `None` when the file does not exist:
/// zero config is the contract, and absence is not an event. Every
/// other failure (unreadable, not UTF-8, invalid TOML) is a diagnostic
/// on the file, never a crash.
pub fn load(root: &Path, vfs: &mut Vfs) -> Option<LoadedConfiguration> {
    let path = root.join("celerrate.toml");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            let file = vfs.file_id(&path);
            return Some(unreadable(file, format!("celerrate.toml could not be read: {error}")));
        }
    };
    let file = vfs.file_id(&path);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return Some(unreadable(file, "celerrate.toml is not valid UTF-8".to_owned())),
    };
    let (configuration, mut diagnostics) = celerrate_config::parse(file, &text);
    diagnostics.extend(celerrate_config::validate(file, &configuration, &known_sets()));
    diagnostics.sort();
    Some(LoadedConfiguration { file, text, configuration, diagnostics })
}

fn unreadable(file: FileId, message: String) -> LoadedConfiguration {
    let range = celerrate_source::TextRange::new(
        celerrate_source::TextSize::from(0),
        celerrate_source::TextSize::from(0),
    );
    LoadedConfiguration {
        file,
        text: String::new(),
        configuration: celerrate_config::Configuration::default(),
        diagnostics: vec![Diagnostic::spanned(
            celerrate_config::INVALID_CONFIGURATION,
            Severity::Error,
            file,
            range,
            message,
        )],
    }
}

/// The known sets, from the registries only the composition root sees.
/// Remappable means "an identifier some core rule may emit"; everything
/// registered but not remappable is resilience by construction.
fn known_sets() -> KnownSets<'static> {
    let mut rule_names = BTreeSet::new();
    let mut remappable_identifiers = BTreeSet::new();
    for (metadata, _) in celerrate_rules::core_rules() {
        // `RuleMetadata.name` is owned; leak-free borrowing needs the
        // metadata to outlive the sets, so intern through the registry:
        // every emitted identifier is registered, and rule names are
        // 'static in the rule definitions. Collect via the registry
        // instead when this assumption breaks.
        rule_names.insert(leak_name(metadata.name));
        for identifier in metadata.identifiers {
            remappable_identifiers.insert(identifier.id.as_str());
        }
    }
    let registered_identifiers = celerrate_diagnostics::REGISTRY
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    KnownSets { rule_names, remappable_identifiers, registered_identifiers }
}

/// Rule names are `String` in `RuleMetadata` (owned registration data,
/// spec 2026-07-20 section 8). The set of core rules is small and
/// computed once per process in practice; leaking the handful of names
/// is bounded and keeps `KnownSets` borrow-free. Part 2 revisits this
/// when the composition root computes the active set and can own the
/// metadata for the session's lifetime.
fn leak_name(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}
```

Note for the implementer: if leaking reads wrong, the alternative is `KnownSets<'sets>` borrowing from a `Vec<RuleMetadata>` held by the caller; that inverts this function into `known_sets(metadata: &[RuleMetadata]) -> KnownSets<'_>` with `load` taking it as a parameter. Either is acceptable; keep whichever compiles cleanly and document it. `load` is called once per `Session::start`, so the leak is at most a few hundred bytes per process for the eight rule names, and repeated `Session::start` calls in tests leak the same handful again, which is harmless.

Register the module in `crates/celerrate_cli/src/lib.rs` beside the others: `mod configuration;`.

- [ ] **Step 4: Carry it on the session**

In `crates/celerrate_cli/src/session.rs`:

- Add the field to `Session` (after `plugin_set_digest`):

```rust
    /// The loaded `celerrate.toml`, `None` when the project has none.
    /// Part 1 reports its diagnostics; part 2 consumes its content.
    /// `--watch` does not reload or report it yet (part 2, with the
    /// behavioral wiring).
    pub loaded_configuration: Option<crate::configuration::LoadedConfiguration>,
```

- In `Session::start`, initialize `loaded_configuration: None` in the struct literal, then load it right after `session.load(&walk);` (the VFS then already holds the walked files; interning `celerrate.toml` afterwards keeps PHP `FileId`s unchanged relative to today):

```rust
        let walk = enumerate_php_files(&session.discovery.walk_roots());
        session.load(&walk);
        session.loaded_configuration = crate::configuration::load(root, &mut session.vfs);
        session
```

- [ ] **Step 5: Teach the renderer the configuration text**

In `crates/celerrate_cli/src/render.rs`, `impl SourceAccess for SessionSources<'_>`, replace the `text` method:

```rust
    fn text(&self, file: FileId) -> Option<&str> {
        if let Some(loaded) = &self.session.loaded_configuration
            && loaded.file == file
        {
            return Some(&loaded.text);
        }
        let source = self.session.sources.get(&file)?;
        celerrate_db::source_text(&self.session.database, *source)
            .as_ref()
            .ok()
            .map(|text| text.text())
    }
```

(`display_path` already resolves any interned `FileId` through `session.vfs.path`, so the configuration file's path renders without changes.)

- [ ] **Step 6: Merge into the report and the exit code**

In `crates/celerrate_cli/src/lib.rs`, the `Command::Check` arm, replace the `presented` construction and the final `Outcome::of` call:

```rust
            let mut presented = analysis::AnalysisOutcome {
                diagnostics: suggest::enrich(&session, &outcome.diagnostics),
                panicked: outcome.panicked.clone(),
            };
            // Configuration diagnostics are presentation and exit-code
            // input, never cache input: `outcome` stays untouched, so
            // the persisted verdicts cannot absorb them.
            let configuration_diagnostics = session
                .loaded_configuration
                .as_ref()
                .map_or(0, |loaded| loaded.diagnostics.len());
            if let Some(loaded) = &session.loaded_configuration {
                presented.diagnostics.extend(loaded.diagnostics.iter().cloned());
                presented.diagnostics.sort();
            }
```

and at the end of the arm:

```rust
            Outcome::of(
                outcome.diagnostics.len() + configuration_diagnostics,
                session.internal_errors.len(),
            )
```

`render_report` receives `&presented`, so the blocks, the summary count, and the `celerrate explain` pointer trailer include the configuration diagnostics with no renderer change. `cache::persist(&mut session, &outcome)` still reads `outcome`. `fix::plan` reads `presented.diagnostics`; configuration diagnostics carry no suggestions, so nothing plans against them.

- [ ] **Step 7: Run the tests**

Run: `cargo test --package celerrate_cli --test configuration`
Expected: PASS (all five).
Run: `cargo test --workspace`
Expected: PASS (nothing else observed `celerrate.toml`; the corpus has none).

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_cli crates/celerrate_config Cargo.lock
git commit -m "✨ feat(cli): load celerrate.toml and report configuration diagnostics"
```

---

### Task 7: Register CEL0043 to CEL0049 with their explain pages

**Files:**
- Create: `crates/celerrate_diagnostics/src/pages/configuration.rs`
- Modify: `crates/celerrate_diagnostics/src/pages/mod.rs`
- Modify: `crates/celerrate_diagnostics/src/registry.rs`
- Modify: `xtask/src/emission_scan.rs` (module doc only)

**Interfaces:**
- Consumes: the `ExplainPage` shape (`why`, `failing_example`, `fixed_example`, `configuration`, all `&'static str`), the `//// <path>` fixture-marker convention of `crates/celerrate_cli/tests/explain_pages.rs`, and the CLI wiring from Task 6 (which makes these examples fire).
- Produces: seven registry entries, owner `"celerrate_config"`.

- [ ] **Step 1: Write the failing registry update**

In `crates/celerrate_diagnostics/src/registry.rs`, append after the CEL0042 entry:

```rust
    registered(
        "CEL0043",
        "invalid configuration",
        "celerrate_config",
        &pages::configuration::CEL0043,
    ),
    registered(
        "CEL0044",
        "unknown configuration key",
        "celerrate_config",
        &pages::configuration::CEL0044,
    ),
    registered(
        "CEL0045",
        "invalid configuration value",
        "celerrate_config",
        &pages::configuration::CEL0045,
    ),
    registered(
        "CEL0046",
        "unknown rule",
        "celerrate_config",
        &pages::configuration::CEL0046,
    ),
    registered(
        "CEL0047",
        "unsupported rule option",
        "celerrate_config",
        &pages::configuration::CEL0047,
    ),
    registered(
        "CEL0048",
        "unknown severity identifier",
        "celerrate_config",
        &pages::configuration::CEL0048,
    ),
    registered(
        "CEL0049",
        "resilience severity remap",
        "celerrate_config",
        &pages::configuration::CEL0049,
    ),
```

Update the ledger test: in `the_registry_is_sorted_unique_and_gapless`, change the closing assertion to `assert_eq!(previous, 49, "forty-nine identifiers allocated so far");`.

In `crates/celerrate_diagnostics/src/pages/mod.rs`, add `pub(crate) mod configuration;` (alphabetical order).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_diagnostics`
Expected: FAIL to compile (`pages::configuration` does not exist).

- [ ] **Step 3: Write the seven pages**

`crates/celerrate_diagnostics/src/pages/configuration.rs`. Every fixture uses the explicit `//// <path>` marker form so it controls its `celerrate.toml`; each failing example fires exactly its own identifier and each fixed example is diagnostic-free. The shared fixture tail (a manifest and one clean source file) keeps the analysis itself silent:

```rust
//! The explain pages for the configuration diagnostics (CEL0043 to
//! CEL0049), owned by `celerrate_config`. Configuration errors are
//! span-anchored in `celerrate.toml` and affect the exit code: a
//! typoed configuration silently half-applying would be a #58-class
//! hole, so CI fails loudly instead (CLI product design, section 2).

use crate::explain::ExplainPage;

pub(crate) const CEL0043: ExplainPage = ExplainPage {
    why: "\
`celerrate.toml` exists but cannot be read as TOML (a syntax error,
an encoding problem, or an unreadable file). Analysis continues with
the default configuration, but the file's intent is not applied, so
the mismatch is reported as an error rather than silently ignored.",
    failing_example: "\
//// celerrate.toml
[project
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[project]
php = \"8.2\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`, not a rule: it is
neither disableable nor remappable, and it affects the exit code so CI
never analyzes with a configuration it could not read.",
};

pub(crate) const CEL0044: ExplainPage = ExplainPage {
    why: "\
A key `celerrate.toml` uses is not part of the configuration schema.
An unknown key is an error rather than a warning because a typoed key
would otherwise silently configure nothing: the file would look
authoritative while doing nothing at all.",
    failing_example: "\
//// celerrate.toml
[project]
includes = [\"src\"]
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[project]
include = [\"src\"]
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. The v0.1 schema
knows `[project]` (`php`, `include`, `exclude`), `[rules.<name>]`
(`enabled`), `[severity]`, and the reserved `[plugins]` table.",
};

pub(crate) const CEL0045: ExplainPage = ExplainPage {
    why: "\
A known configuration key carries a value of the wrong type or shape:
a `php` value that is not a version point, an `include` entry that is
absolute or empty, a non-boolean `enabled`, or a severity that is not
`\"error\"` or `\"warning\"`. The malformed value is skipped and
reported; the well-formed rest of the file still applies.",
    failing_example: "\
//// celerrate.toml
[project]
php = \"^8.1\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[project]
php = \"8.1\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. In `celerrate.toml`
the `php` key is a version point (`\"8.2\"`), not a Composer-style
constraint: the constraint belongs to `composer.json`, and the
configuration key collapses the detected range to one version.",
};

pub(crate) const CEL0046: ExplainPage = ExplainPage {
    why: "\
A `[rules.<name>]` table names a rule that does not exist. A typoed
rule name must not silently enable or disable nothing: the analysis
would run with a different rule set than the file claims.",
    failing_example: "\
//// celerrate.toml
[rules.nul-dereference]
enabled = false
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[rules.null-dereference]
enabled = false
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. Rule names are the
stable kebab-case names of `celerrate explain`'s owning-rule lines;
activation is per rule, while per-identifier severity lives under
`[severity]`.",
};

pub(crate) const CEL0047: ExplainPage = ExplainPage {
    why: "\
A `[rules.<name>]` table sets a key other than `enabled`. No shipped
rule has configurable options yet, so any other key would be silently
dead configuration; when parameterized rules arrive, their options
become sibling keys of `enabled` in this same table.",
    failing_example: "\
//// celerrate.toml
[rules.null-dereference]
max = 3
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[rules.null-dereference]
enabled = false
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. `enabled` is the
only recognized rule key in v0.1.",
};

pub(crate) const CEL0048: ExplainPage = ExplainPage {
    why: "\
A `[severity]` entry names a diagnostic identifier the registry does
not know. A typoed identifier must not silently remap nothing: the
severity the file claims and the severity the run uses would diverge
invisibly.",
    failing_example: "\
//// celerrate.toml
[severity]
\"CEL9999\" = \"warning\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[severity]
\"CEL0034\" = \"warning\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. Valid identifiers
are the `CEL####` codes of `celerrate explain`; only identifiers a
rule may emit can be remapped.",
};

pub(crate) const CEL0049: ExplainPage = ExplainPage {
    why: "\
A `[severity]` entry names a resilience diagnostic: a parse error, a
decode failure, or a project notice. Those are neither disableable nor
remappable by design, because they report the tool's own degraded
sight, not a property of the code under analysis.",
    failing_example: "\
//// celerrate.toml
[severity]
\"CEL0026\" = \"error\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[severity]
\"CEL0034\" = \"warning\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. Remappable
identifiers are exactly the ones the core rules may emit; everything
else in the registry is resilience.",
};
```

- [ ] **Step 4: Update the emission-scan module documentation**

In `xtask/src/emission_scan.rs`, extend the module doc's resilience-producer list to name `celerrate_config` beside `celerrate_db`, `celerrate_syntax`, and `celerrate_project` (configuration diagnostics are neither disableable nor configurable by nature; the crate stays ungoverned). Doc only; `GOVERNED_CRATES` does not change.

- [ ] **Step 5: Run the full workspace suite**

Run: `cargo test --workspace`
Expected: PASS. Pay attention to `crates/celerrate_cli/tests/explain_pages.rs`: the content gate now covers 49 pages, and `every_written_page_example_is_honest` executes the seven new failing/fixed fixtures through the real CLI (the Task 6 wiring is what makes them fire). If a failing example fires an unexpected second identifier or a fixed example is not clean, fix the fixture, not the harness.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_diagnostics xtask
git commit -m "✨ feat(diagnostics): register CEL0043 to CEL0049 with explain pages"
```

---

### Task 8: Verification: zero-config parity and every gate

**Files:**
- None to create; this task runs the gates and fixes only what they surface.

**Interfaces:**
- Consumes: everything above.
- Produces: the part 1 closure evidence.

- [ ] **Step 1: The mechanical suite**

Run, in order, each expected clean:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

- [ ] **Step 2: The xtask gates**

```bash
cargo xtask dependency-shape
cargo xtask emission-scan
```

Expected: both PASS (`celerrate_config` does not depend on `celerrate_plugin` or salsa, so `dependency-shape` does not govern it; `emission-scan` governs `celerrate_semantics`/`celerrate_types` only).

- [ ] **Step 3: Zero-config parity on the corpus (closure gate 1)**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: `corpus` byte-identical to the committed snapshot (`0 notices, 0 diagnostics`; the corpus has no `celerrate.toml`, so this run **is** the parity proof), `mixed-rate` unchanged. Any delta is a stop-and-investigate, not a re-bless.

- [ ] **Step 4: Commit (only if something was fixed)**

If steps 1 to 3 surfaced fixes, commit them with a message naming what the gate caught, e.g.:

```bash
git commit -m "🐛 fix(config): satisfy the workspace gates for the configuration crate"
```

---

## Self-review notes (already applied)

- Spec coverage: section 10 item 1 maps to Tasks 1 to 8; the section 2 decisions (span-preserving parsing, config errors span-anchored and exit-affecting, ownership of CEL0043+, zero-config contract) each have a task and a test. Deliberately out of part 1, per the spec's sequencing: config consumption, the active set and force-activation, the cache-header digest, `--watch` reporting parity (all part 2; Task 6 leaves the comment saying so).
- The registry ledger stays honest at every commit: identifiers are crate-local constants until Task 7, and nothing renders `celerrate explain CEL0043` as existing before Task 7 lands.
- Type consistency: `Spanned`, `RuleEntry.enabled: Option<Spanned<bool>>`, `KnownSets` field names, and `LoadedConfiguration` field names are used identically in Tasks 1, 5, and 6.
- `toml_edit` API risk is called out where it is used (Tasks 2 to 4): if the pinned release's span API differs, adapt call sites, never tests.
