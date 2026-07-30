# CLI Product 5: `celerrate migrate --from-phpstan` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `celerrate migrate --from-phpstan`: parse `phpstan.neon` (a minimal NEON subset with recursive, cycle-guarded includes), generate `celerrate.toml`, report every setting that does not carry over, and record `celerrate-baseline.toml` so the first `celerrate check` is clean and only new problems fail.

**Architecture:** A new `migrate` module inside `celerrate_cli` (mirroring `src/baseline/`): `neon.rs` is a pure resilient reader for the NEON subset, `settings.rs` resolves includes and merges parameters with NEON semantics, `convert.rs` turns the merged settings into `celerrate.toml` text (including the committed level-to-severity table), and `mod.rs` owns the command flow, the migration report, and the clean-slate recording that reuses the existing check pipeline (`Session::start`, `single_pass`, `suggest::enrich`, `baseline::record`). Nothing enters queries or the cache; the command is pure CLI surface.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), clap 4 derive, `toml_edit` (already a `celerrate_cli` dependency), insta snapshots. **No new dependency**: the NEON reader is hand-written (no mature Rust NEON crate exists; the spec mandates an in-module parser).

Spec: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`, section 5 (the command), section 9 (testing), closure gate 4.

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Test modules and test files carry local `#![allow]`s. The NEON reader must be total: no `phpstan.neon` content may crash it; what does not parse produces a report line, never a failure.
- TDD: failing test first, minimal implementation, refactor.
- Everything in files is English, full words, no em-dashes anywhere (code, comments, docs, commits).
- Commits: gitmoji + Conventional Commits (`✨ feat(cli): ...`). Never reference the plan, a task, or a phase in commits or docs.
- Mechanical suite green after every task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo deny check` (only if a manifest changed; none should), `cargo xtask dependency-shape`, `cargo xtask emission-scan`.
- The corpus snapshot and the mixed-rate baseline must not move: `migrate` adds no default-path behavior change and no type work. The final task proves it with `cargo xtask fetch-corpus` then `cargo xtask corpus` and `cargo xtask mixed-rate`.
- The spec's hard rules, held throughout: the command **never touches** `phpstan.neon` or any PHPStan baseline file (rollback stays free); **no entry-by-entry PHPStan baseline conversion** (continuity is delivered by re-recording); it refuses to overwrite an existing `celerrate.toml` without `--force`; the report is generated and never silent; an empty baseline is never written.
- No new CEL identifiers: the migration report is command output, not diagnostics. The identifier gates (registry, producers, docs, explain pages) are untouched by construction.

## Load-bearing codebase facts (verified 2026-07-30)

- `crates/celerrate_cli/src/arguments.rs`: clap 4 derive; `Command::{Check{...}, Explain{identifier}, GroundTruth{path} (hidden), MixedRate{path} (hidden)}`. The module doc (lines 1-4) already names `migrate --from-phpstan` as expected growth. Unit tests in `mod tests` (lines 98-235) use `Arguments::try_parse_from([...]).unwrap()` + let-else + `panic!`, with `#![allow(clippy::unwrap_used, clippy::panic)]`.
- `crates/celerrate_cli/src/lib.rs`: `pub fn run(arguments: Vec<OsString>, output: &mut dyn Write, color: ColorMode) -> Outcome` (line 94); the `match arguments.command` dispatch starts at line 108. `Outcome::{Clean, DiagnosticsReported, InternalError, UsageError}` with `code()` 0/1/2/2. Usage-error convention: `let _ = writeln!(output, "error: ...")` then `return Outcome::UsageError` (the `--output` machine-format guard at lines 129-146 is the template). Root validation helpers `unusable_root(&Path) -> Option<String>` (line 361) and `absolute_root(&Path) -> Result<PathBuf, String>` (line 327) are private items of the crate root, therefore visible from any child module via `crate::...`. Same for `fn single_pass(session: &mut Session, pass: impl FnOnce() -> Result<AnalysisOutcome, Cancelled>) -> AnalysisOutcome` (line 408).
- The check pipeline sequence (`lib.rs:154-193`), which the clean-slate recording mirrors: `Session::start(&root)` → `session.inputs()` → `single_pass(&mut session, || analysis::analyze(&inputs))` → `session.absorb_outcome(&outcome)` → `suggest::enrich(&session, &outcome.diagnostics) -> Vec<Diagnostic>` → `baseline::record(&session, &diagnostics)` → `configuration::merge_diagnostics` (AFTER recording, so configuration diagnostics are never baselined) → `cache::persist(&mut session, &outcome)`.
- `crates/celerrate_cli/src/baseline/mod.rs`: `pub const BASELINE_FILE_NAME: &str = "celerrate-baseline.toml"` (line 20); `pub fn record(session: &Session, diagnostics: &[Diagnostic]) -> io::Result<Option<usize>>` (line 211) returns `Ok(None)` only when there are no entries AND no existing file; the fingerprint is `(path, identifier, symbol, message, count)`, project-anchored diagnostics are skipped, suppressed diagnostics never reach it (suppression is in-engine).
- `crates/celerrate_cli/src/cache/pack.rs:147`: `pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()>` (tempfile + rename). Used by `baseline::record`; reuse it for `celerrate.toml`.
- `crates/celerrate_cli/src/configuration.rs`: `pub fn load(root: &Path, vfs: &mut Vfs) -> Option<LoadedConfiguration>` reads `<root>/celerrate.toml` inside `Session::start`, so a file written before `Session::start` is picked up; `pub fn diagnostic_count(session) -> usize` (line 266). `crates/celerrate_cli/src/render.rs`: `pub(crate) fn count(n, singular, plural) -> String`, `pub fn render_internal_errors(output, &Session) -> io::Result<()>` (line 588). `session.internal_errors` is readable crate-wide (`lib.rs` reads `.len()`).
- `celerrate_config` (`crates/celerrate_config/src/`): `parse(file, text) -> (Configuration, Vec<Diagnostic>)` and `validate(file, &Configuration, &KnownSets) -> Vec<Diagnostic>`; `KnownSets { rule_names, remappable_identifiers, registered_identifiers }` has public fields (constructible in tests). Accepted schema: `[project] php/include/exclude` (relative, non-empty paths only), `[rules.<name>] enabled`, `[severity] "<ID>" = "error"|"warning"`, `[plugins]` reserved. Bare keys like `CEL0030` are valid TOML keys.
- The nine identifiers of the three typed families (all `Tier::Default`, default severity `Error`): `unknown-members` CEL0030 CEL0031 CEL0032 CEL0033, `null-dereference` CEL0034, `argument-checks` CEL0035 CEL0036 CEL0037 CEL0038. All nine are remappable (rule-emitted, not resilience).
- TOML writing precedent: `crates/celerrate_cli/src/baseline/file.rs:103` `serialize` builds a `toml_edit::DocumentMut`, inserts values with `toml_edit::value(...)`, returns `format!("{HEADER}\n{document}")` with a `const HEADER: &str` comment block.
- `celerrate_vfs::normalize_path` exists (used by `absolute_root`); use it for the include cycle guard.
- The `@phpstan-ignore` suppression bridge already works at analysis time (`celerrate_phpdoc_bridge`, honored end to end; pinned by `crates/celerrate_cli/tests/directive_rules.rs:121`): inline suppressions have nothing to migrate.
- Test conventions: in-process `celerrate_cli::run(...)` (never a spawned binary); each test file re-declares its own `project(&[(path, contents)]) -> tempfile::TempDir` and per-subcommand helper (deliberate duplication); insta named snapshots land in `crates/celerrate_cli/tests/snapshots/<file>__<name>.snap`; every `assert!` carries `"report was:\n{text}"`-style context; the canonical failing PHP fixture is `"<?php\n\nstrlenn(\"hello\");\n"` and the manifest is `r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#`.
- `tests/documentation.rs` has a `workspace_page` helper reading `docs/*.md`; the docs drift gate precedent.
- `xtask dependency-shape`: `celerrate_cli` is a composition root and exempt; a migrate module inside it needs zero xtask changes.
- No `--force` flag exists anywhere yet; this is the first, mandated by the spec.
- No NEON, YAML, or `phpstan.neon` parsing exists anywhere in the workspace.

## Decisions (and why)

- **A module inside `celerrate_cli`, not a crate.** The command is product surface exactly like the baseline; `src/migrate/` mirrors `src/baseline/` and stays out of `dependency-shape` by construction.
- **`--from-phpstan` is a hand-checked flag, not `required = true`.** clap 4's `SetTrue` action gives bool flags an implicit default, and combining that with `required` is a known footgun. `migrate` without `--from-phpstan` takes the existing hand-rolled usage-error path: `error: migrate needs a source; pass --from-phpstan`. This also keeps room for future sources (`--from-psalm`).
- **Discovery order is PHPStan's own**: `phpstan.neon`, then `phpstan.neon.dist`, then `phpstan.dist.neon`, at the project root. None found is a usage error.
- **The NEON reader is a generic, total, line-based subset parser** producing `Scalar | List | Map` values plus a list of skipped lines with reasons. Indentation is counted in raw leading characters (tabs and spaces alike; `phpstan.neon` conventionally uses tabs). Comments (`#` outside quotes) are stripped. Inline `[a, b]` lists parse; inline `{...}` mappings and any construct outside the subset become skipped lines, reported, never fatal.
- **Baseline includes are ignored by name, never parsed** (the spec's amendment): an include whose lowercased target does not end in `.neon` or whose target contains `baseline` is listed in the report and not read. All other includes resolve relative to the including file, recursively, with a cycle guard keyed on `celerrate_vfs::normalize_path`.
- **Merge semantics are NEON's**: each file's `includes` are absorbed before its own `parameters`, so lists concatenate in include order and the including file's scalars win last (`level` in the entry file overrides an included one).
- **Paths are rebased onto the project root.** PHPStan resolves `paths`/`excludePaths` relative to the file that declares them; a path declared in `build/strict.neon` is carried as `build/<path>`. `excludePaths` accepts both the plain-list form and the `analyse`/`analyseAndScan` mapping form (both sections mean "exclude" for us).
- **Carry rules are honest and narrow**: a path is carried only if it is relative, placeholder-free, and glob-free. Absolute paths, `%parameter%` placeholders, `*`/`?` patterns, and paths escaping the root are dropped, each with a one-line reason in the report (celerrate.toml only accepts plain relative paths).
- **The level table, committed**: levels 0 to 5 (and a missing `level`, since PHPStan defaults to 0) generate `[severity]` entries mapping the nine typed-family identifiers CEL0030 to CEL0038 to `"warning"`; level 6 and above, and `max`, keep default severities; an unparseable level keeps defaults and says so. The table is documented in `docs/migration.md` and drift-gated.
- **The generated file must round-trip clean**: `celerrate_config::parse` + `validate` over the generated text yields zero diagnostics, pinned by test. `celerrate.toml` is written with `write_atomically`, always (even when nothing carries over: the header documents that the migration ran).
- **The clean-slate recording reuses the check pipeline verbatim** (analyze → enrich → `baseline::record`), then `cache::persist` (the user's first real `check` starts warm, for free). Configuration diagnostics merge after recording in `check`; here they are simply never merged into the recorded slice, so they cannot be baselined, same guarantee.
- **Exit codes**: `Clean` (0) on success even when findings were recorded (they are baselined; that is the point), `UsageError` (2) for no source file / existing target without `--force` / bad root / missing `--from-phpstan`, `InternalError` (2) for read or write failures and analysis internal errors. Precedent: the hidden channels return `Clean` when the run completed.
- **The report goes to the `output` writer** (stdout in production), like every subcommand report. Stderr stays what it is today.
- **Untransposed keys are deduplicated by key** (first origin wins) and rendered with a small committed explanation table; unknown keys get the honest generic line `no Celerrate equivalent in v0.1`.
- **Tasks 1 to 3 land under a temporary `#![allow(dead_code)]`** on `migrate/mod.rs` (the command wires in at Task 4, which removes it): every task stays green under `-D warnings` without wiring half a command.

## The committed level table (source of truth for Task 3 and `docs/migration.md`)

| PHPStan `level` | Generated `[severity]` |
| --- | --- |
| absent (PHPStan defaults to 0) | CEL0030 to CEL0038 = `"warning"` |
| 0, 1, 2, 3, 4, 5 | CEL0030 to CEL0038 = `"warning"` |
| 6 and above | none (default severities) |
| `max` | none (default severities) |
| anything else | none (default severities), noted in the report |

The nine identifiers: CEL0030, CEL0031, CEL0032, CEL0033 (`unknown-members`), CEL0034 (`null-dereference`), CEL0035, CEL0036, CEL0037, CEL0038 (`argument-checks`).

## File structure

- Create: `crates/celerrate_cli/src/migrate/mod.rs` (command flow, report, clean-slate recording, explanation table)
- Create: `crates/celerrate_cli/src/migrate/neon.rs` (the NEON subset reader)
- Create: `crates/celerrate_cli/src/migrate/settings.rs` (include resolution, NEON merge, rebasing)
- Create: `crates/celerrate_cli/src/migrate/convert.rs` (carry rules, level table, `celerrate.toml` generation)
- Modify: `crates/celerrate_cli/src/arguments.rs` (the `Migrate` variant + unit tests)
- Modify: `crates/celerrate_cli/src/lib.rs` (`mod migrate;`, the dispatch arm)
- Test: `crates/celerrate_cli/tests/migrate.rs` (integration, end to end, continuity, snapshot)
- Modify: `crates/celerrate_cli/tests/documentation.rs` (migration docs drift gate)
- Create: `docs/migration.md`
- Modify: `CHANGELOG.md`

---

### Task 1: The NEON subset reader (`migrate/neon.rs`)

**Files:**
- Create: `crates/celerrate_cli/src/migrate/mod.rs` (module shell only)
- Create: `crates/celerrate_cli/src/migrate/neon.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (register the module)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (Task 2 relies on these exact names):
  - `migrate::neon::Value { Scalar(String), List(Vec<Value>), Map(Vec<(String, Value)>) }`
  - `migrate::neon::Skipped { line: usize, reason: &'static str }` (line is 1-based)
  - `migrate::neon::Parsed { root: Vec<(String, Value)>, skipped: Vec<Skipped> }`
  - `migrate::neon::parse(text: &str) -> Parsed` (total; never fails)

- [ ] **Step 1: Register the module and write the failing unit tests**

In `crates/celerrate_cli/src/lib.rs`, next to `mod baseline;` (alphabetical order among the existing `mod` items):

```rust
mod migrate;
```

Create `crates/celerrate_cli/src/migrate/mod.rs`:

```rust
//! `celerrate migrate --from-phpstan`: convert a PHPStan project to
//! Celerrate in one command. Parse `phpstan.neon` (a minimal NEON
//! subset), generate `celerrate.toml`, report what does not carry
//! over, and record the baseline so only new problems fail.
// The command wires into the CLI in a later change; until then the
// module is library-only.
#![allow(dead_code)]

pub(crate) mod neon;
```

Create `crates/celerrate_cli/src/migrate/neon.rs` with the module doc and the test module only (implementation comes in Step 3):

```rust
//! A minimal NEON reader. NEON is the YAML-like dialect `phpstan.neon`
//! is written in; no mature Rust crate exists, and the migration
//! consumes only a small subset: `includes`, `parameters.paths`,
//! `parameters.excludePaths`, `parameters.level`. This reader parses
//! indentation-structured mappings, `- ` sequences, and inline `[...]`
//! lists into a generic value tree. It is total: every line it does
//! not understand becomes a `Skipped` entry, never a failure.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use super::*;

    fn root_value<'parsed>(parsed: &'parsed Parsed, key: &str) -> &'parsed Value {
        parsed
            .root
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
            .unwrap()
    }

    #[test]
    fn a_tab_indented_phpstan_file_parses() {
        let parsed = parse(
            "includes:\n\t- phpstan-baseline.neon\n\nparameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n\t\t- tests\n",
        );
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::List(includes) = root_value(&parsed, "includes") else {
            panic!("includes should be a list");
        };
        assert_eq!(includes, &[Value::Scalar("phpstan-baseline.neon".to_owned())]);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(parameters[0], ("level".to_owned(), Value::Scalar("5".to_owned())));
        let Value::List(paths) = &parameters[1].1 else {
            panic!("paths should be a list");
        };
        assert_eq!(
            paths,
            &[Value::Scalar("src".to_owned()), Value::Scalar("tests".to_owned())]
        );
    }

    #[test]
    fn space_indentation_and_comments_parse_alike() {
        let parsed = parse("parameters:\n    # tuning\n    level: 8 # strict\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(parameters[0], ("level".to_owned(), Value::Scalar("8".to_owned())));
    }

    #[test]
    fn inline_lists_and_quoted_scalars_parse() {
        let parsed = parse("parameters:\n\tpaths: [src, \"app dir\", 'lib']\n\tlevel: \"max\"\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(paths) = &parameters[0].1 else {
            panic!("paths should be a list");
        };
        assert_eq!(
            paths,
            &[
                Value::Scalar("src".to_owned()),
                Value::Scalar("app dir".to_owned()),
                Value::Scalar("lib".to_owned()),
            ]
        );
        assert_eq!(parameters[1].1, Value::Scalar("max".to_owned()));
    }

    #[test]
    fn a_hash_inside_quotes_is_not_a_comment() {
        let parsed = parse("parameters:\n\tignoreErrors:\n\t\t- '#^Call to undefined#'\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(ignores) = &parameters[0].1 else {
            panic!("ignoreErrors should be a list");
        };
        assert_eq!(ignores, &[Value::Scalar("#^Call to undefined#".to_owned())]);
    }

    #[test]
    fn a_dash_item_with_a_nested_block_parses_as_a_map() {
        let parsed = parse(
            "parameters:\n\tignoreErrors:\n\t\t-\n\t\t\tmessage: '#unused#'\n\t\t\tpath: src/Legacy.php\n",
        );
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(ignores) = &parameters[0].1 else {
            panic!("ignoreErrors should be a list");
        };
        let Value::Map(entry) = &ignores[0] else {
            panic!("the entry should be a map");
        };
        assert_eq!(entry[0], ("message".to_owned(), Value::Scalar("#unused#".to_owned())));
        assert_eq!(entry[1], ("path".to_owned(), Value::Scalar("src/Legacy.php".to_owned())));
    }

    #[test]
    fn a_dash_item_with_an_inline_key_parses_as_a_one_entry_map() {
        let parsed = parse("parameters:\n\texcludePaths:\n\t\t- analyse: src/Generated\n");
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(excludes) = &parameters[0].1 else {
            panic!("excludePaths should be a list");
        };
        assert_eq!(
            excludes[0],
            Value::Map(vec![("analyse".to_owned(), Value::Scalar("src/Generated".to_owned()))])
        );
    }

    #[test]
    fn the_exclude_paths_mapping_form_parses() {
        let parsed = parse("parameters:\n\texcludePaths:\n\t\tanalyse:\n\t\t\t- src/Generated\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::Map(sections) = &parameters[0].1 else {
            panic!("excludePaths should be a map");
        };
        let Value::List(analyse) = &sections[0].1 else {
            panic!("analyse should be a list");
        };
        assert_eq!(analyse, &[Value::Scalar("src/Generated".to_owned())]);
    }

    #[test]
    fn a_colon_inside_a_value_is_not_a_key_separator() {
        // A separator needs trailing whitespace or the end of the line.
        let parsed = parse("parameters:\n\ttmpDir: C:/tmp/phpstan\n");
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(
            parameters[0],
            ("tmpDir".to_owned(), Value::Scalar("C:/tmp/phpstan".to_owned()))
        );
    }

    #[test]
    fn an_empty_value_with_no_child_block_is_an_empty_scalar() {
        let parsed = parse("parameters:\n\tlevel:\n");
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(parameters[0], ("level".to_owned(), Value::Scalar(String::new())));
    }

    #[test]
    fn constructs_outside_the_subset_are_skipped_with_line_numbers() {
        let parsed = parse("services:\n\t- {factory: App\\Rule}\nparameters:\n\tlevel: 5\n");
        // The inline mapping on line 2 is outside the subset; the rest
        // of the document still parses.
        assert_eq!(parsed.skipped.len(), 1, "{:?}", parsed.skipped);
        assert_eq!(parsed.skipped[0].line, 2);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(parameters[0], ("level".to_owned(), Value::Scalar("5".to_owned())));
    }

    #[test]
    fn garbage_never_panics() {
        for garbage in [
            "",
            "\n\n\n",
            ":",
            "- - -",
            "\t\tdeep: orphan\n",
            "key with no colon\n",
            "a: [unclosed\n",
            "a: {inline: map}\n",
            "\u{0}\u{1}\u{2}",
            "key:\n\t- \"unterminated\n",
        ] {
            let _ = parse(garbage);
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli migrate::neon`
Expected: COMPILE FAILURE (`Value`, `Parsed`, `parse` not found), which is the red state for a not-yet-written unit.

- [ ] **Step 3: Write the reader**

Add above the test module in `crates/celerrate_cli/src/migrate/neon.rs`:

```rust
/// A NEON value, restricted to what the migration consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Scalar(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

/// A line the subset reader did not understand: skipped and reported,
/// never fatal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Skipped {
    /// One-based line number in the source file.
    pub(crate) line: usize,
    pub(crate) reason: &'static str,
}

/// A parsed document: the root mapping plus every skipped line.
#[derive(Debug, Default)]
pub(crate) struct Parsed {
    pub(crate) root: Vec<(String, Value)>,
    pub(crate) skipped: Vec<Skipped>,
}

/// A significant line: comment-stripped, indentation measured in raw
/// leading characters (tabs and spaces alike).
struct Line {
    number: usize,
    indent: usize,
    content: String,
}

/// Parse a NEON document. Total: unknown constructs become `skipped`
/// entries and the rest of the document still parses.
pub(crate) fn parse(text: &str) -> Parsed {
    let lines = significant_lines(text);
    let mut skipped = Vec::new();
    let mut cursor = 0;
    let indent = lines.first().map_or(0, |line| line.indent);
    let root = parse_map(&lines, &mut cursor, indent, &mut skipped);
    while let Some(line) = lines.get(cursor) {
        skipped.push(Skipped { line: line.number, reason: "unrecognized structure" });
        cursor += 1;
    }
    Parsed { root, skipped }
}

fn significant_lines(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let stripped = strip_comment(raw).trim_end();
        let content = stripped.trim_start();
        if content.is_empty() {
            continue;
        }
        lines.push(Line {
            number: index + 1,
            indent: stripped.len() - content.len(),
            content: content.to_owned(),
        });
    }
    lines
}

/// Cut the line at the first `#` that sits outside quotes.
fn strip_comment(raw: &str) -> &str {
    let mut single = false;
    let mut double = false;
    for (index, character) in raw.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '#' if !single && !double => return raw.get(..index).unwrap_or(raw),
            _ => {}
        }
    }
    raw
}

fn parse_map(
    lines: &[Line],
    cursor: &mut usize,
    indent: usize,
    skipped: &mut Vec<Skipped>,
) -> Vec<(String, Value)> {
    let mut entries = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            skipped.push(Skipped { line: line.number, reason: "unexpected indentation" });
            *cursor += 1;
            continue;
        }
        let Some((key, rest)) = split_key(&line.content) else {
            skipped.push(Skipped { line: line.number, reason: "expected `key: value`" });
            *cursor += 1;
            continue;
        };
        let number = line.number;
        *cursor += 1;
        let value = if rest.is_empty() {
            parse_block(lines, cursor, indent, skipped)
        } else {
            inline_value(&rest, number, skipped)
        };
        entries.push((key, value));
    }
    entries
}

/// Parse the child block that follows a `key:` (or bare `-`) line: a
/// sequence or a mapping, decided by the first deeper line. No deeper
/// line means the value was an empty scalar.
fn parse_block(
    lines: &[Line],
    cursor: &mut usize,
    parent_indent: usize,
    skipped: &mut Vec<Skipped>,
) -> Value {
    match lines.get(*cursor) {
        Some(child) if child.indent > parent_indent => {
            let indent = child.indent;
            if child.content.starts_with('-') {
                Value::List(parse_list(lines, cursor, indent, skipped))
            } else {
                Value::Map(parse_map(lines, cursor, indent, skipped))
            }
        }
        _ => Value::Scalar(String::new()),
    }
}

fn parse_list(
    lines: &[Line],
    cursor: &mut usize,
    indent: usize,
    skipped: &mut Vec<Skipped>,
) -> Vec<Value> {
    let mut items = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        if line.indent < indent {
            break;
        }
        let entry = (line.indent == indent)
            .then(|| line.content.strip_prefix('-'))
            .flatten();
        let Some(rest) = entry else {
            skipped.push(Skipped { line: line.number, reason: "expected a `- item` entry" });
            *cursor += 1;
            continue;
        };
        let rest = rest.trim_start().to_owned();
        let number = line.number;
        *cursor += 1;
        if rest.is_empty() {
            items.push(parse_block(lines, cursor, indent, skipped));
        } else if let Some((key, value_text)) = split_key(&rest) {
            let value = if value_text.is_empty() {
                parse_block(lines, cursor, indent, skipped)
            } else {
                inline_value(&value_text, number, skipped)
            };
            items.push(Value::Map(vec![(key, value)]));
        } else {
            items.push(Value::Scalar(unquote(&rest)));
        }
    }
    items
}

/// Split `key: rest` at the first `:` that sits outside quotes and is
/// followed by whitespace or the end of the line (so `C:/tmp` stays a
/// scalar). The returned key is unquoted, the rest is trimmed.
fn split_key(content: &str) -> Option<(String, String)> {
    let mut single = false;
    let mut double = false;
    for (index, character) in content.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            ':' if !single && !double => {
                let after = content.get(index + 1..)?;
                if after.is_empty() || after.starts_with(char::is_whitespace) {
                    let key = unquote(content.get(..index)?.trim());
                    if key.is_empty() {
                        return None;
                    }
                    return Some((key, after.trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    None
}

/// An inline value after `key: `: an `[...]` list, a scalar, or (for
/// `{...}` mappings, outside the subset) a reported skip.
fn inline_value(text: &str, line: usize, skipped: &mut Vec<Skipped>) -> Value {
    if let Some(inner) = text.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return Value::List(
            split_inline_items(inner)
                .into_iter()
                .map(|item| Value::Scalar(unquote(item)))
                .collect(),
        );
    }
    if text.starts_with('{') || text.starts_with('[') {
        skipped.push(Skipped { line, reason: "inline structures beyond `[a, b]` are outside the subset" });
        return Value::Scalar(String::new());
    }
    Value::Scalar(unquote(text))
}

/// Split the inside of an inline list on commas that sit outside quotes.
fn split_inline_items(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut single = false;
    let mut double = false;
    for (index, character) in inner.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            ',' if !single && !double => {
                if let Some(item) = inner.get(start..index) {
                    items.push(item);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if let Some(item) = inner.get(start..) {
        items.push(item);
    }
    items.into_iter().map(str::trim).filter(|item| !item.is_empty()).collect()
}

/// Strip one matching pair of quotes. Double-quoted strings unescape
/// `\"` and `\\` (the two escapes the subset needs); single quotes are
/// literal, NEON-style.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(inner) = trimmed.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
        return inner.to_owned();
    }
    if let Some(inner) = trimmed.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return inner.replace("\\\"", "\"").replace("\\\\", "\\");
    }
    trimmed.to_owned()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli migrate::neon`
Expected: all PASS. If `constructs_outside_the_subset_are_skipped_with_line_numbers` disagrees on the skip count, inspect which construct produced which skip and adjust the assertion to the actual honest behavior (the invariant that matters: at least the inline mapping is reported, `parameters.level` still parses, nothing panics).

- [ ] **Step 5: Full mechanical suite and commit**

Run: `cargo test --package celerrate_cli && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green, no formatting drift.

```bash
git add crates/celerrate_cli/src/lib.rs crates/celerrate_cli/src/migrate/
git commit -m "✨ feat(cli): read the NEON subset the PHPStan migration consumes"
```

---

### Task 2: Include resolution and merged settings (`migrate/settings.rs`)

**Files:**
- Create: `crates/celerrate_cli/src/migrate/settings.rs`
- Modify: `crates/celerrate_cli/src/migrate/mod.rs` (register the module)

**Interfaces:**
- Consumes: `migrate::neon::{parse, Value, Parsed}` (Task 1).
- Produces (Tasks 3 and 4 rely on these exact names):
  - `migrate::settings::Settings { paths: Vec<String>, exclude_paths: Vec<String>, level: Option<String>, untransposed: Vec<Untransposed>, ignored_includes: Vec<String>, problems: Vec<String> }` (`Default`)
  - `migrate::settings::Untransposed { key: String, origin: String }`
  - `migrate::settings::load(source: &Path, root: &Path) -> Result<Settings, String>` (Err carries the human message for `error: {message}`)
  - Paths in `paths`/`exclude_paths` are already rebased root-relative with `/` separators; placeholder-carrying and absolute paths pass through raw (Task 3 drops them with reasons).

- [ ] **Step 1: Write the failing unit tests**

Create `crates/celerrate_cli/src/migrate/settings.rs` with the module doc and tests (implementation in Step 3). The tests build real temporary directories because include resolution is filesystem work:

```rust
//! From a `phpstan.neon` entry file to one merged settings value:
//! includes resolved recursively (cycle-guarded, relative to the
//! including file), parameters merged with NEON semantics (lists
//! concatenate in include order, the including file's scalars win),
//! paths rebased onto the project root, and everything that does not
//! carry recorded for the report. Resilient throughout: a missing or
//! unparseable include is a report line, never a failure.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use std::path::Path;

    use super::*;

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

    fn load_from(root: &Path) -> Settings {
        load(&root.join("phpstan.neon"), root).unwrap()
    }

    #[test]
    fn paths_exclude_paths_and_level_are_read() {
        let root = project(&[(
            "phpstan.neon",
            "parameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n\texcludePaths:\n\t\t- src/Generated\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(settings.paths, ["src"]);
        assert_eq!(settings.exclude_paths, ["src/Generated"]);
        assert_eq!(settings.level.as_deref(), Some("5"));
        assert!(settings.problems.is_empty(), "{:?}", settings.problems);
    }

    #[test]
    fn includes_merge_with_neon_semantics() {
        // Lists concatenate in include order; the including file's
        // scalar wins last.
        let root = project(&[
            (
                "phpstan.neon",
                "includes:\n\t- build/strict.neon\nparameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n",
            ),
            (
                "build/strict.neon",
                "parameters:\n\tlevel: 3\n\texcludePaths:\n\t\t- fixtures\n",
            ),
        ]);
        let settings = load_from(root.path());
        assert_eq!(settings.level.as_deref(), Some("5"));
        assert_eq!(settings.paths, ["src"]);
        // Declared in build/strict.neon, so rebased onto the root.
        assert_eq!(settings.exclude_paths, ["build/fixtures"]);
    }

    #[test]
    fn baseline_and_non_neon_includes_are_listed_never_read() {
        let root = project(&[(
            "phpstan.neon",
            "includes:\n\t- phpstan-baseline.neon\n\t- rules.php\nparameters:\n\tlevel: 6\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(settings.ignored_includes, ["phpstan-baseline.neon", "rules.php"]);
        // Never read: no problem line even though neither file exists.
        assert!(settings.problems.is_empty(), "{:?}", settings.problems);
    }

    #[test]
    fn circular_includes_are_guarded_and_reported() {
        let root = project(&[
            ("phpstan.neon", "includes:\n\t- other.neon\n"),
            ("other.neon", "includes:\n\t- phpstan.neon\nparameters:\n\tlevel: 2\n"),
        ]);
        let settings = load_from(root.path());
        assert_eq!(settings.level.as_deref(), Some("2"));
        assert_eq!(settings.problems.len(), 1, "{:?}", settings.problems);
        assert!(settings.problems[0].contains("circular"), "{:?}", settings.problems);
    }

    #[test]
    fn a_missing_include_is_a_problem_line_not_a_failure() {
        let root = project(&[("phpstan.neon", "includes:\n\t- vanished.neon\n")]);
        let settings = load_from(root.path());
        assert_eq!(settings.problems.len(), 1, "{:?}", settings.problems);
        assert!(settings.problems[0].contains("vanished.neon"), "{:?}", settings.problems);
    }

    #[test]
    fn the_exclude_paths_mapping_form_feeds_both_sections() {
        let root = project(&[(
            "phpstan.neon",
            "parameters:\n\texcludePaths:\n\t\tanalyse:\n\t\t\t- one\n\t\tanalyseAndScan:\n\t\t\t- two\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(settings.exclude_paths, ["one", "two"]);
    }

    #[test]
    fn unknown_keys_are_untransposed_and_deduplicated() {
        let root = project(&[
            (
                "phpstan.neon",
                "includes:\n\t- extra.neon\nparameters:\n\tlevel: 5\n\tbootstrapFiles:\n\t\t- tests/bootstrap.php\nservices:\n\t-\n\t\tclass: App\\Extension\n",
            ),
            ("extra.neon", "parameters:\n\tbootstrapFiles:\n\t\t- other.php\n"),
        ]);
        let settings = load_from(root.path());
        let keys: Vec<&str> = settings.untransposed.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys, ["bootstrapFiles", "services"]);
        // First origin wins: the include is absorbed before the
        // including file's own parameters.
        assert_eq!(settings.untransposed[0].origin, "extra.neon");
    }

    #[test]
    fn absolute_and_placeholder_paths_pass_through_raw() {
        // Task 3 drops them with reasons; this layer must not mangle
        // them by rebasing.
        let root = project(&[(
            "phpstan.neon",
            "parameters:\n\tpaths:\n\t\t- /somewhere/absolute\n\t\t- '%rootDir%/../src'\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(settings.paths, ["/somewhere/absolute", "%rootDir%/../src"]);
    }

    #[test]
    fn a_missing_entry_file_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let error = load(&root.path().join("phpstan.neon"), root.path()).unwrap_err();
        assert!(error.contains("phpstan.neon"), "{error}");
    }
}
```

In `crates/celerrate_cli/src/migrate/mod.rs`, add below `pub(crate) mod neon;`:

```rust
pub(crate) mod settings;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli migrate::settings`
Expected: COMPILE FAILURE (`Settings`, `load` not found).

- [ ] **Step 3: Write the implementation**

Above the test module in `settings.rs`:

```rust
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::neon;

/// The merged, root-relative view of a PHPStan configuration tree.
#[derive(Debug, Default)]
pub(crate) struct Settings {
    pub(crate) paths: Vec<String>,
    pub(crate) exclude_paths: Vec<String>,
    pub(crate) level: Option<String>,
    pub(crate) untransposed: Vec<Untransposed>,
    pub(crate) ignored_includes: Vec<String>,
    pub(crate) problems: Vec<String>,
}

/// A key the migration does not carry over, with the file it came from.
#[derive(Debug)]
pub(crate) struct Untransposed {
    pub(crate) key: String,
    pub(crate) origin: String,
}

/// Load and merge a PHPStan configuration tree from its entry file.
/// The only hard failure is an unreadable entry file; everything else
/// degrades into report lines.
pub(crate) fn load(source: &Path, root: &Path) -> Result<Settings, String> {
    let origin = source
        .file_name()
        .map_or_else(|| "phpstan.neon".to_owned(), |name| name.to_string_lossy().into_owned());
    let text = std::fs::read_to_string(source)
        .map_err(|error| format!("could not read {origin}: {error}"))?;
    let mut settings = Settings::default();
    let mut visited = BTreeSet::new();
    visited.insert(celerrate_vfs::normalize_path(source));
    absorb(&text, source, &origin, &mut visited, &mut settings);
    Ok(settings)
}

/// Absorb one file: its includes first (NEON merge semantics make the
/// including file win), then its own parameters.
fn absorb(
    text: &str,
    file: &Path,
    origin: &str,
    visited: &mut BTreeSet<PathBuf>,
    settings: &mut Settings,
) {
    let parsed = neon::parse(text);
    for skipped in &parsed.skipped {
        settings.problems.push(format!("{origin}:{}: {}", skipped.line, skipped.reason));
    }
    for (key, value) in &parsed.root {
        if key == "includes" {
            absorb_includes(value, file, origin, visited, settings);
        }
    }
    for (key, value) in &parsed.root {
        match key.as_str() {
            "includes" => {}
            "parameters" => absorb_parameters(value, origin, settings),
            _ => note_untransposed(settings, key, origin),
        }
    }
}

fn absorb_includes(
    value: &neon::Value,
    file: &Path,
    origin: &str,
    visited: &mut BTreeSet<PathBuf>,
    settings: &mut Settings,
) {
    let neon::Value::List(items) = value else {
        settings.problems.push(format!("{origin}: `includes` is not a list, skipped"));
        return;
    };
    for item in items {
        let neon::Value::Scalar(target) = item else {
            settings.problems.push(format!("{origin}: structured include entry skipped"));
            continue;
        };
        if ignored_include(target) {
            settings.ignored_includes.push(target.clone());
            continue;
        }
        let directory = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let resolved = directory.join(target.replace('\\', "/"));
        let normalized = celerrate_vfs::normalize_path(&resolved);
        if !visited.insert(normalized) {
            settings.problems.push(format!("{origin}: circular include of {target}, skipped"));
            continue;
        }
        let child_origin = join_relative(parent_of(origin), target);
        match std::fs::read_to_string(&resolved) {
            Ok(text) => absorb(&text, &resolved, &child_origin, visited, settings),
            Err(error) => settings
                .problems
                .push(format!("{origin}: include {target} could not be read: {error}")),
        }
    }
}

/// A PHPStan baseline include or a non-NEON include: listed by name in
/// the report, never parsed (the spec's amendment; the recorded
/// Celerrate baseline carries the continuity instead).
fn ignored_include(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    !lower.ends_with(".neon") || lower.contains("baseline")
}

fn absorb_parameters(value: &neon::Value, origin: &str, settings: &mut Settings) {
    let neon::Value::Map(entries) = value else {
        settings.problems.push(format!("{origin}: `parameters` is not a mapping, skipped"));
        return;
    };
    let directory = parent_of(origin).to_owned();
    for (key, value) in entries {
        match key.as_str() {
            "paths" => {
                let mut paths = std::mem::take(&mut settings.paths);
                absorb_path_list(value, &directory, origin, key, &mut paths, settings);
                settings.paths = paths;
            }
            "excludePaths" => {
                let mut excludes = std::mem::take(&mut settings.exclude_paths);
                match value {
                    neon::Value::Map(sections) => {
                        for (section, list) in sections {
                            if section == "analyse" || section == "analyseAndScan" {
                                absorb_path_list(list, &directory, origin, key, &mut excludes, settings);
                            } else {
                                settings.problems.push(format!(
                                    "{origin}: excludePaths.{section} is not understood, skipped"
                                ));
                            }
                        }
                    }
                    other => absorb_path_list(other, &directory, origin, key, &mut excludes, settings),
                }
                settings.exclude_paths = excludes;
            }
            "level" => match value {
                neon::Value::Scalar(level) => settings.level = Some(level.clone()),
                _ => settings.problems.push(format!("{origin}: `level` is not a scalar, skipped")),
            },
            _ => note_untransposed(settings, key, origin),
        }
    }
}

fn absorb_path_list(
    value: &neon::Value,
    directory: &str,
    origin: &str,
    key: &str,
    into: &mut Vec<String>,
    settings: &mut Settings,
) {
    let items: Vec<&neon::Value> = match value {
        neon::Value::List(items) => items.iter().collect(),
        single @ neon::Value::Scalar(_) => vec![single],
        neon::Value::Map(_) => {
            settings.problems.push(format!("{origin}: `{key}` is not a list, skipped"));
            return;
        }
    };
    for item in items {
        match item {
            neon::Value::Scalar(path) => into.push(rebase(directory, path)),
            _ => settings.problems.push(format!("{origin}: structured `{key}` entry skipped")),
        }
    }
}

/// PHPStan resolves paths relative to the file that declares them:
/// rebase onto the project root. Absolute and placeholder paths pass
/// through raw so the conversion can drop them with a reason.
fn rebase(directory: &str, path: &str) -> String {
    if path.contains('%') || looks_absolute(path) {
        return path.to_owned();
    }
    join_relative(directory, path)
}

fn looks_absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with('\\')
        || path.chars().nth(1).is_some_and(|second| second == ':')
}

/// The directory part of a root-relative origin, empty at the root.
fn parent_of(origin: &str) -> &str {
    origin.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// Join a root-relative directory and a relative path, collapsing `.`
/// and resolving `..` textually. Leading `..` segments survive; the
/// conversion rules drop such paths later.
fn join_relative(directory: &str, relative: &str) -> String {
    let mut segments: Vec<String> = directory
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .map(str::to_owned)
        .collect();
    for segment in relative.replace('\\', "/").split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| last != "..") {
                    segments.pop();
                } else {
                    segments.push("..".to_owned());
                }
            }
            other => segments.push(other.to_owned()),
        }
    }
    segments.join("/")
}

fn note_untransposed(settings: &mut Settings, key: &str, origin: &str) {
    if settings.untransposed.iter().any(|entry| entry.key == key) {
        return;
    }
    settings.untransposed.push(Untransposed { key: key.to_owned(), origin: origin.to_owned() });
}
```

Note the `std::mem::take` dance on `paths`/`exclude_paths`: it exists only to satisfy the borrow checker (`absorb_path_list` pushes into one field while also pushing problems into `settings`). If a cleaner split reads better at implementation time (for example `absorb_path_list` returning `(Vec<String>, Vec<String>)`), take it; the tests are the contract, not this exact body.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli migrate::settings`
Expected: all PASS.

- [ ] **Step 5: Full mechanical suite and commit**

Run: `cargo test --package celerrate_cli && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli/src/migrate/
git commit -m "✨ feat(cli): resolve phpstan.neon includes into merged migration settings"
```

---

### Task 3: Conversion to `celerrate.toml` (`migrate/convert.rs`)

**Files:**
- Create: `crates/celerrate_cli/src/migrate/convert.rs`
- Modify: `crates/celerrate_cli/src/migrate/mod.rs` (register the module)

**Interfaces:**
- Consumes: `migrate::settings::Settings` (Task 2).
- Produces (Task 4 relies on these exact names):
  - `migrate::convert::REMAPPED_IDENTIFIERS: [&str; 9]` (CEL0030 to CEL0038, in order)
  - `migrate::convert::Conversion { toml: String, include: Vec<String>, exclude: Vec<String>, level_note: String, dropped: Vec<(String, &'static str)> }`
  - `migrate::convert::convert(settings: &Settings) -> Conversion`
  - The generated `toml` parses and validates with zero configuration diagnostics (pinned here by unit test, and again end to end in Task 4).

- [ ] **Step 1: Write the failing unit tests**

Create `crates/celerrate_cli/src/migrate/convert.rs`:

```rust
//! Merged PHPStan settings to `celerrate.toml` text: the carry rules
//! for paths (relative, placeholder-free, glob-free, inside the root),
//! the committed level table (levels 0 to 5 remap the typed families
//! to "warning"; 6 and above, and `max`, keep defaults), and the TOML
//! generation. The generated file must round-trip through
//! `celerrate_config` with zero diagnostics: that is the contract.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use super::*;
    use crate::migrate::settings::Settings;

    fn settings(paths: &[&str], excludes: &[&str], level: Option<&str>) -> Settings {
        Settings {
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
            exclude_paths: excludes.iter().map(|path| (*path).to_owned()).collect(),
            level: level.map(str::to_owned),
            ..Settings::default()
        }
    }

    #[test]
    fn paths_and_level_five_generate_project_and_severity_sections() {
        let conversion = convert(&settings(&["src", "tests"], &["src/Generated"], Some("5")));
        assert_eq!(conversion.include, ["src", "tests"]);
        assert_eq!(conversion.exclude, ["src/Generated"]);
        assert!(conversion.dropped.is_empty(), "{:?}", conversion.dropped);
        assert!(conversion.toml.contains("include = [\"src\", \"tests\"]"), "{}", conversion.toml);
        assert!(conversion.toml.contains("exclude = [\"src/Generated\"]"), "{}", conversion.toml);
        for identifier in REMAPPED_IDENTIFIERS {
            assert!(
                conversion.toml.contains(&format!("{identifier} = \"warning\"")),
                "missing {identifier} in:\n{}",
                conversion.toml
            );
        }
        assert!(conversion.level_note.contains("level 5"), "{}", conversion.level_note);
    }

    #[test]
    fn level_boundaries_follow_the_committed_table() {
        for (level, remapped) in [
            (None, true),
            (Some("0"), true),
            (Some("5"), true),
            (Some("6"), false),
            (Some("9"), false),
            (Some("max"), false),
            (Some("strict"), false),
        ] {
            let conversion = convert(&settings(&[], &[], level));
            assert_eq!(
                conversion.toml.contains("[severity]"),
                remapped,
                "level {level:?}:\n{}",
                conversion.toml
            );
        }
    }

    #[test]
    fn uncarriable_paths_are_dropped_with_reasons() {
        let conversion = convert(&settings(
            &["/absolute", "%rootDir%/src", "src/*", "../outside", "src", "src"],
            &[],
            Some("7"),
        ));
        // Deduplicated, only the honest survivor carries.
        assert_eq!(conversion.include, ["src"]);
        assert_eq!(conversion.dropped.len(), 4, "{:?}", conversion.dropped);
    }

    #[test]
    fn an_empty_conversion_still_generates_a_documented_header() {
        let conversion = convert(&settings(&[], &[], Some("max")));
        assert!(conversion.toml.starts_with('#'), "{}", conversion.toml);
        assert!(!conversion.toml.contains("[project]"), "{}", conversion.toml);
        assert!(!conversion.toml.contains("[severity]"), "{}", conversion.toml);
    }

    #[test]
    fn the_generated_file_round_trips_with_zero_configuration_diagnostics() {
        use std::collections::BTreeSet;

        let conversion = convert(&settings(&["src"], &["src/Generated"], Some("3")));
        let mut source_files = celerrate_source::SourceFiles::default();
        let file = source_files.intern("celerrate.toml");
        let (configuration, parse_diagnostics) = celerrate_config::parse(file, &conversion.toml);
        assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:?}\n{}", conversion.toml);
        let metadata: Vec<_> = celerrate_rules::core_rules()
            .into_iter()
            .map(|(metadata, _)| metadata)
            .collect();
        let known = celerrate_config::KnownSets {
            rule_names: metadata.iter().map(|rule| rule.name.as_str()).collect(),
            remappable_identifiers: metadata
                .iter()
                .flat_map(|rule| rule.identifiers.iter().map(|identifier| identifier.id.as_str()))
                .collect(),
            registered_identifiers: celerrate_diagnostics::REGISTRY
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<BTreeSet<_>>(),
        };
        let validation = celerrate_config::validate(file, &configuration, &known);
        assert!(validation.is_empty(), "{validation:?}\n{}", conversion.toml);
    }
}
```

Note on the round-trip test: the `SourceFiles::intern` and `KnownSets` construction shapes must be checked against the real APIs at implementation time (the CLI's own `configuration.rs` builds `KnownSets` from `celerrate_rules::core_rules()`; mirror that construction exactly, wherever the field types differ from this sketch). The assertion is the contract: parse plus validate over the generated text, zero diagnostics.

In `crates/celerrate_cli/src/migrate/mod.rs`, add:

```rust
pub(crate) mod convert;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli migrate::convert`
Expected: COMPILE FAILURE.

- [ ] **Step 3: Write the implementation**

Above the test module in `convert.rs`:

```rust
use super::settings::Settings;

/// The nine typed-family identifiers the level table remaps: the
/// `unknown-members`, `null-dereference`, and `argument-checks`
/// families, whole. Documented in `docs/migration.md` (drift-gated).
pub(crate) const REMAPPED_IDENTIFIERS: [&str; 9] = [
    "CEL0030", "CEL0031", "CEL0032", "CEL0033", "CEL0034", "CEL0035", "CEL0036", "CEL0037",
    "CEL0038",
];

/// The outcome of a conversion: the file text plus everything the
/// report needs to say about it.
#[derive(Debug)]
pub(crate) struct Conversion {
    pub(crate) toml: String,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) level_note: String,
    pub(crate) dropped: Vec<(String, &'static str)>,
}

pub(crate) fn convert(settings: &Settings) -> Conversion {
    let mut dropped = Vec::new();
    let include = carry(&settings.paths, &mut dropped);
    let exclude = carry(&settings.exclude_paths, &mut dropped);
    let (remapped, level_note) = severity_profile(settings.level.as_deref());
    let toml = generate(&include, &exclude, remapped);
    Conversion { toml, include, exclude, level_note, dropped }
}

fn carry(paths: &[String], dropped: &mut Vec<(String, &'static str)>) -> Vec<String> {
    let mut carried: Vec<String> = Vec::new();
    for path in paths {
        match carriable(path) {
            Ok(clean) => {
                if !carried.contains(&clean) {
                    carried.push(clean);
                }
            }
            Err(reason) => dropped.push((path.clone(), reason)),
        }
    }
    carried
}

/// `celerrate.toml` accepts plain relative paths inside the root and
/// nothing else; everything narrower is dropped with its reason.
fn carriable(path: &str) -> Result<String, &'static str> {
    if path.contains('%') {
        return Err("NEON parameter placeholders are not resolved");
    }
    if path.starts_with('/')
        || path.starts_with('\\')
        || path.chars().nth(1).is_some_and(|second| second == ':')
    {
        return Err("absolute paths cannot be carried into celerrate.toml");
    }
    if path.contains('*') || path.contains('?') {
        return Err("glob patterns are not supported");
    }
    let clean = path.trim_end_matches('/');
    if clean.is_empty() {
        return Err("empty path");
    }
    if clean.split('/').any(|segment| segment == "..") {
        return Err("the path escapes the project root");
    }
    Ok(clean.to_owned())
}

/// The committed level table. Levels 0 to 5 (and a missing level,
/// PHPStan's default being 0) remap the typed families to "warning";
/// 6 and above, and `max`, keep default severities.
fn severity_profile(level: Option<&str>) -> (bool, String) {
    match level {
        None => (
            true,
            "no level found: PHPStan defaults to level 0; typed-family identifiers remapped to \"warning\""
                .to_owned(),
        ),
        Some("max") => (false, "level max: default severities kept".to_owned()),
        Some(text) => match text.parse::<u32>() {
            Ok(level) if level <= 5 => {
                (true, format!("level {level}: typed-family identifiers remapped to \"warning\""))
            }
            Ok(level) => (false, format!("level {level}: default severities kept")),
            Err(_) => (false, format!("level \"{text}\" not understood: default severities kept")),
        },
    }
}

const HEADER: &str = "\
# Generated by `celerrate migrate --from-phpstan`. Review and commit.
# Everything not set here keeps its default; `celerrate explain CELxxxx`
# documents any identifier.
";

fn generate(include: &[String], exclude: &[String], remapped: bool) -> String {
    let mut document = toml_edit::DocumentMut::new();
    if !include.is_empty() || !exclude.is_empty() {
        let mut project = toml_edit::Table::new();
        if !include.is_empty() {
            project.insert("include", toml_edit::value(string_array(include)));
        }
        if !exclude.is_empty() {
            project.insert("exclude", toml_edit::value(string_array(exclude)));
        }
        document.insert("project", toml_edit::Item::Table(project));
    }
    if remapped {
        let mut severity = toml_edit::Table::new();
        for identifier in REMAPPED_IDENTIFIERS {
            severity.insert(identifier, toml_edit::value("warning"));
        }
        document.insert("severity", toml_edit::Item::Table(severity));
    }
    format!("{HEADER}\n{document}")
}

fn string_array(values: &[String]) -> toml_edit::Array {
    values.iter().map(String::as_str).collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli migrate::convert`
Expected: all PASS. If the round-trip test fails on API shapes (`SourceFiles::intern`, `KnownSets` field types), fix the test's construction against the real APIs (mirror `crates/celerrate_cli/src/configuration.rs`); if it fails on a real diagnostic, the generator is wrong: fix the generator, never relax the assertion.

- [ ] **Step 5: Full mechanical suite and commit**

Run: `cargo test --package celerrate_cli && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_cli/src/migrate/
git commit -m "✨ feat(cli): convert merged PHPStan settings to a celerrate.toml"
```

---

### Task 4: The `migrate` subcommand end to end

**Files:**
- Modify: `crates/celerrate_cli/src/arguments.rs` (the `Migrate` variant + unit tests)
- Modify: `crates/celerrate_cli/src/lib.rs` (the dispatch arm)
- Modify: `crates/celerrate_cli/src/migrate/mod.rs` (command flow, report, clean-slate recording; remove the `dead_code` allow)
- Test: `crates/celerrate_cli/tests/migrate.rs`

**Interfaces:**
- Consumes: `migrate::settings::load`, `migrate::convert::{convert, Conversion}` (Tasks 2 and 3); `crate::{unusable_root, absolute_root, single_pass}` (private crate-root items, visible from child modules); `crate::analysis::analyze`; `crate::suggest::enrich`; `crate::baseline::{record, BASELINE_FILE_NAME}`; `crate::cache::{persist, pack::write_atomically}`; `crate::render::{count, render_internal_errors}`; `crate::configuration::diagnostic_count`; `crate::session::Session`.
- Produces: `migrate::execute(root: &Path, force: bool, output: &mut dyn Write) -> Outcome`; the `Command::Migrate { path, from_phpstan, force }` variant; `migrate::SOURCE_FILE_NAMES`.

- [ ] **Step 1: Write the failing integration tests**

Create `crates/celerrate_cli/tests/migrate.rs`:

```rust
//! `celerrate migrate --from-phpstan` end to end: conversion, the
//! report, the clean-slate baseline, and the continuity contract.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::ffi::OsString;
use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
const FAILING_EXAMPLE: &str = "<?php\n\nstrlenn(\"hello\");\n";
const SUPPRESSED_EXAMPLE: &str = "<?php\n\nstrlenn(\"hello\"); // @phpstan-ignore-line\n";
const CLEAN_EXAMPLE: &str = "<?php\n\n$greeting = \"hello\";\n";

// Tab-indented, as phpstan.neon conventionally is: a recursive
// include, a scattered baseline include, level 5, and keys that do
// not carry over.
const PHPSTAN_NEON: &str = "includes:\n\t- phpstan-baseline.neon\n\t- build/strict.neon\n\nparameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n\texcludePaths:\n\t\t- src/Generated\n\tignoreErrors:\n\t\t-\n\t\t\tmessage: '#unused#'\n\t\t\tpath: src/Legacy.php\n\tbootstrapFiles:\n\t\t- tests/bootstrap.php\n";
const STRICT_NEON: &str = "parameters:\n\tlevel: 3\n\texcludePaths:\n\t\t- fixtures\n";
const BASELINE_NEON: &str = "parameters:\n\tignoreErrors: []\n";

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

fn migrate_with(root: &Path, extra: &[&str]) -> (Outcome, String) {
    let mut arguments: Vec<OsString> = vec![
        "celerrate".into(),
        "migrate".into(),
        root.as_os_str().into(),
        "--from-phpstan".into(),
    ];
    arguments.extend(extra.iter().map(Into::into));
    let mut output = Vec::new();
    let outcome = run(arguments, &mut output, ColorMode::Plain);
    (outcome, String::from_utf8(output).unwrap())
}

fn migrate(root: &Path) -> (Outcome, String) {
    migrate_with(root, &[])
}

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
        ColorMode::Plain,
    );
    (outcome, String::from_utf8(output).unwrap())
}

fn phpstan_project() -> tempfile::TempDir {
    project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", FAILING_EXAMPLE),
        ("src/Suppressed.php", SUPPRESSED_EXAMPLE),
        ("phpstan.neon", PHPSTAN_NEON),
        ("build/strict.neon", STRICT_NEON),
        ("phpstan-baseline.neon", BASELINE_NEON),
    ])
}

#[test]
fn migrate_converts_reports_and_records_the_clean_slate() {
    let root = phpstan_project();
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");

    // The generated configuration.
    let configuration = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert!(configuration.contains("include = [\"src\"]"), "file was:\n{configuration}");
    assert!(
        configuration.contains("exclude = [\"build/fixtures\", \"src/Generated\"]"),
        "file was:\n{configuration}"
    );
    assert!(configuration.contains("CEL0034 = \"warning\""), "file was:\n{configuration}");

    // The report: level, untransposed keys, the ignored baseline.
    assert!(text.contains("level 5"), "report was:\n{text}");
    assert!(text.contains("ignoreErrors"), "report was:\n{text}");
    assert!(text.contains("bootstrapFiles"), "report was:\n{text}");
    assert!(text.contains("phpstan-baseline.neon"), "report was:\n{text}");

    // The clean slate: the finding is recorded, the suppressed one is
    // not (suppression is in-engine, upstream of recording).
    let baseline = std::fs::read_to_string(root.path().join("celerrate-baseline.toml")).unwrap();
    assert!(baseline.contains("path = \"src/Example.php\""), "file was:\n{baseline}");
    assert!(!baseline.contains("Suppressed"), "file was:\n{baseline}");
    assert!(text.contains("recorded 1 baseline entry"), "report was:\n{text}");
}

#[test]
fn migrate_never_touches_the_phpstan_files() {
    let root = phpstan_project();
    migrate(root.path());
    let neon = std::fs::read_to_string(root.path().join("phpstan.neon")).unwrap();
    assert_eq!(neon, PHPSTAN_NEON);
    let baseline = std::fs::read_to_string(root.path().join("phpstan-baseline.neon")).unwrap();
    assert_eq!(baseline, BASELINE_NEON);
}

#[test]
fn after_migrate_the_first_check_is_clean_and_only_new_problems_fail() {
    let root = phpstan_project();
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");

    let (first, text) = check(root.path());
    assert_eq!(first, Outcome::Clean, "report was:\n{text}");

    std::fs::write(
        root.path().join("src/Fresh.php"),
        "<?php\n\nstrlenn(\"fresh\");\n",
    )
    .unwrap();
    let (second, text) = check(root.path());
    assert_eq!(second, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("Fresh.php"), "report was:\n{text}");
}

#[test]
fn a_clean_project_records_no_baseline() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Clean.php", CLEAN_EXAMPLE),
        ("phpstan.neon", "parameters:\n\tlevel: 8\n\tpaths:\n\t\t- src\n"),
    ]);
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(!root.path().join("celerrate-baseline.toml").exists());
    assert!(text.contains("no findings"), "report was:\n{text}");
    // Level 8 keeps defaults: no severity section.
    let configuration = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert!(!configuration.contains("[severity]"), "file was:\n{configuration}");
}

#[test]
fn a_dist_source_is_discovered() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Clean.php", CLEAN_EXAMPLE),
        ("phpstan.neon.dist", "parameters:\n\tlevel: 6\n\tpaths:\n\t\t- src\n"),
    ]);
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("phpstan.neon.dist"), "report was:\n{text}");
}

#[test]
fn without_a_phpstan_configuration_migrate_is_a_usage_error() {
    let root = project(&[("composer.json", MANIFEST)]);
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
    assert!(text.contains("phpstan.neon"), "report was:\n{text}");
    assert!(!root.path().join("celerrate.toml").exists());
}

#[test]
fn an_existing_configuration_is_refused_without_force() {
    let root = phpstan_project();
    std::fs::write(root.path().join("celerrate.toml"), "# hand-written\n").unwrap();
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
    assert!(text.contains("--force"), "report was:\n{text}");
    let untouched = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert_eq!(untouched, "# hand-written\n");
}

#[test]
fn force_overwrites_deterministically() {
    let root = phpstan_project();
    let (first_outcome, _) = migrate(root.path());
    assert_eq!(first_outcome, Outcome::Clean);
    let first = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    let (second_outcome, text) = migrate_with(root.path(), &["--force"]);
    assert_eq!(second_outcome, Outcome::Clean, "report was:\n{text}");
    let second = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert_eq!(first, second);
}

#[test]
fn migrate_without_a_source_flag_is_a_usage_error() {
    let root = phpstan_project();
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "migrate".into(), root.path().as_os_str().into()],
        &mut output,
        ColorMode::Plain,
    );
    let text = String::from_utf8(output).unwrap();
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
    assert!(text.contains("--from-phpstan"), "report was:\n{text}");
}

#[test]
fn an_unusable_root_is_a_usage_error() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("vanished");
    let (outcome, text) = migrate(&missing);
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
}

#[test]
fn the_migration_report_snapshot() {
    let root = phpstan_project();
    let (_, text) = migrate(root.path());
    insta::assert_snapshot!("migrate_report", text);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --test migrate`
Expected: FAIL inside `run` at parse time ("unrecognized subcommand 'migrate'"), surfacing as `Outcome::UsageError` where `Clean` was expected.

- [ ] **Step 3: Add the `Migrate` variant to the clap surface**

In `crates/celerrate_cli/src/arguments.rs`, in `enum Command`, after `Check` and before `Explain`:

```rust
    /// Convert another tool's configuration to `celerrate.toml`,
    /// report what does not carry over, and record the baseline so
    /// only new problems fail.
    Migrate {
        /// The project root. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Migrate from PHPStan: `phpstan.neon`, its includes, its
        /// level.
        #[arg(long)]
        from_phpstan: bool,

        /// Overwrite an existing `celerrate.toml`.
        #[arg(long)]
        force: bool,
    },
```

In the same file's `mod tests`, add (following the existing let-else + `panic!` style):

```rust
    #[test]
    fn migrate_parses_its_flags() {
        let arguments =
            Arguments::try_parse_from(["celerrate", "migrate", "project", "--from-phpstan", "--force"])
                .unwrap();
        let Command::Migrate { path, from_phpstan, force } = arguments.command else {
            panic!("expected Command::Migrate");
        };
        assert_eq!(path, PathBuf::from("project"));
        assert!(from_phpstan);
        assert!(force);
    }

    #[test]
    fn migrate_defaults_to_the_current_directory() {
        let arguments = Arguments::try_parse_from(["celerrate", "migrate"]).unwrap();
        let Command::Migrate { path, from_phpstan, force } = arguments.command else {
            panic!("expected Command::Migrate");
        };
        assert_eq!(path, PathBuf::from("."));
        assert!(!from_phpstan);
        assert!(!force);
    }
```

Extend the existing hidden-subcommands help test to also assert `migrate` IS visible (add to its assertions: `assert!(text.contains("migrate"))`).

- [ ] **Step 4: Write the command flow and the report**

Replace the top of `crates/celerrate_cli/src/migrate/mod.rs` (keep the module doc; remove the `#![allow(dead_code)]` block and its comment) with:

```rust
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Outcome;
use crate::session::Session;

pub(crate) mod convert;
pub(crate) mod neon;
pub(crate) mod settings;

/// PHPStan's own configuration discovery order.
pub(crate) const SOURCE_FILE_NAMES: [&str; 3] =
    ["phpstan.neon", "phpstan.neon.dist", "phpstan.dist.neon"];

const TARGET_FILE_NAME: &str = "celerrate.toml";

/// The whole command: discover, convert, write, report, then record
/// the clean slate. The root has already been validated and
/// absolutized by the dispatcher.
pub(crate) fn execute(root: &Path, force: bool, output: &mut dyn Write) -> Outcome {
    let Some(source) = discover(root) else {
        let _ = writeln!(
            output,
            "error: no PHPStan configuration found in {}: expected one of {}",
            root.display(),
            SOURCE_FILE_NAMES.join(", "),
        );
        return Outcome::UsageError;
    };
    let target = root.join(TARGET_FILE_NAME);
    if target.exists() && !force {
        let _ = writeln!(
            output,
            "error: {TARGET_FILE_NAME} already exists; pass --force to overwrite it",
        );
        return Outcome::UsageError;
    }
    let settings = match settings::load(&source, root) {
        Ok(settings) => settings,
        Err(message) => {
            let _ = writeln!(output, "error: {message}");
            return Outcome::InternalError;
        }
    };
    let conversion = convert::convert(&settings);
    if let Err(error) = crate::cache::pack::write_atomically(&target, conversion.toml.as_bytes()) {
        let _ = writeln!(output, "error: could not write {TARGET_FILE_NAME}: {error}");
        return Outcome::InternalError;
    }
    let source_name = source
        .file_name()
        .map_or_else(|| SOURCE_FILE_NAMES[0].to_owned(), |name| name.to_string_lossy().into_owned());
    let _ = render_report(output, &source_name, &conversion, &settings);
    record_clean_slate(root, output)
}

fn discover(root: &Path) -> Option<PathBuf> {
    SOURCE_FILE_NAMES
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file())
}

fn render_report(
    output: &mut dyn Write,
    source: &str,
    conversion: &convert::Conversion,
    settings: &settings::Settings,
) -> std::io::Result<()> {
    writeln!(output, "migrated {source} to {TARGET_FILE_NAME}")?;
    writeln!(
        output,
        "  include: {}, exclude: {}",
        crate::render::count(conversion.include.len(), "path", "paths"),
        crate::render::count(conversion.exclude.len(), "path", "paths"),
    )?;
    writeln!(output, "  {}", conversion.level_note)?;
    for (path, reason) in &conversion.dropped {
        writeln!(output, "  dropped {path}: {reason}")?;
    }
    if !settings.untransposed.is_empty() {
        writeln!(output, "not carried over:")?;
        for entry in &settings.untransposed {
            writeln!(output, "  {} ({}): {}", entry.key, entry.origin, explanation(&entry.key))?;
        }
    }
    if !settings.problems.is_empty() {
        writeln!(output, "not parsed:")?;
        for problem in &settings.problems {
            writeln!(output, "  {problem}")?;
        }
    }
    if !settings.ignored_includes.is_empty() {
        writeln!(output, "ignored includes (never parsed; delete them once the baseline is in place):")?;
        for include in &settings.ignored_includes {
            writeln!(output, "  {include}")?;
        }
    }
    Ok(())
}

/// One honest line per key the migration does not carry over. The
/// report is the migration documentation: generated, never silent.
fn explanation(key: &str) -> &'static str {
    match key {
        "ignoreErrors" => {
            "message patterns over PHPStan's vocabulary do not translate; the recorded baseline carries the continuity"
        }
        "bootstrap" | "bootstrapFiles" => "Celerrate does not execute project code before analysis",
        "stubFiles" => "PHPStan stub files are not consumed; Celerrate ships its own stubs",
        "scanFiles" | "scanDirectories" => "symbol discovery follows Composer autoloading",
        "phpVersion" => "set `php` under `[project]` in celerrate.toml if the Composer range is not right",
        "tmpDir" | "parallel" => "Celerrate manages its own cache and parallelism",
        "services" | "rules" | "conditionalTags" => {
            "PHPStan extensions have no Celerrate equivalent; first-party plugins are enabled by default"
        }
        _ => "no Celerrate equivalent in v0.1",
    }
}

/// The spec's step four: always run the analysis, record the baseline
/// when there are findings, no file when there are none. Mirrors the
/// check pipeline exactly (suppression is in-engine, upstream; the
/// configuration diagnostics of the generated file are never merged
/// into the recorded slice, so they cannot be baselined).
fn record_clean_slate(root: &Path, output: &mut dyn Write) -> Outcome {
    let mut session = Session::start(root);
    let inputs = session.inputs();
    let outcome = crate::single_pass(&mut session, || crate::analysis::analyze(&inputs));
    session.absorb_outcome(&outcome);
    let diagnostics = crate::suggest::enrich(&session, &outcome.diagnostics);
    match crate::baseline::record(&session, &diagnostics) {
        Ok(Some(recorded)) => {
            let _ = writeln!(
                output,
                "recorded {} to {}",
                crate::render::count(recorded, "baseline entry", "baseline entries"),
                crate::baseline::BASELINE_FILE_NAME,
            );
        }
        Ok(None) => {
            let _ = writeln!(output, "no findings: no baseline needed");
        }
        Err(error) => {
            let _ = writeln!(
                output,
                "error: could not write {}: {error}",
                crate::baseline::BASELINE_FILE_NAME,
            );
            return Outcome::InternalError;
        }
    }
    crate::cache::persist(&mut session, &outcome);
    if crate::configuration::diagnostic_count(&session) > 0 {
        let _ = writeln!(
            output,
            "warning: the generated {TARGET_FILE_NAME} produced configuration diagnostics; run `celerrate check`",
        );
    }
    if !session.internal_errors.is_empty() {
        let _ = crate::render::render_internal_errors(output, &session);
        return Outcome::InternalError;
    }
    let _ = writeln!(output, "run `celerrate check`: from here, only new problems fail");
    Outcome::Clean
}
```

Visibility check at implementation time: `suggest::enrich`, `cache::persist`, `cache::pack::write_atomically`, `render::count`, `render::render_internal_errors`, `configuration::diagnostic_count`, `baseline::record`, and `session.internal_errors` are all already used across modules of this crate; if any turns out narrower than `pub(crate)` (for example `pub(super)`), widen it to `pub(crate)` in the same change, nothing more.

- [ ] **Step 5: Add the dispatch arm**

In `crates/celerrate_cli/src/lib.rs`, in the `match arguments.command`, after the `Check` arm (mirror the `Check` arm's root-validation lines exactly as they exist today):

```rust
        Command::Migrate { path, from_phpstan, force } => {
            if !from_phpstan {
                let _ = writeln!(output, "error: migrate needs a source; pass --from-phpstan");
                return Outcome::UsageError;
            }
            if let Some(message) = unusable_root(&path) {
                let _ = writeln!(output, "error: {message}");
                return Outcome::UsageError;
            }
            let root = match absolute_root(&path) {
                Ok(root) => root,
                Err(message) => {
                    let _ = writeln!(output, "error: {message}");
                    return Outcome::UsageError;
                }
            };
            migrate::execute(&root, force, output)
        }
```

(Copy the exact `unusable_root`/`absolute_root` handling from the `Check` arm; the shape above is the intent, today's code is the letter.)

- [ ] **Step 6: Run the integration tests to verify they pass**

Run: `cargo test --package celerrate_cli --test migrate`
Expected: PASS, with the snapshot test writing `crates/celerrate_cli/tests/snapshots/migrate__migrate_report.snap` on first run (review it: it must show the summary lines, the three untransposed keys at most, the ignored baseline include, `recorded 1 baseline entry`, and the closing `run \`celerrate check\`` line; then accept it as the pinned report). If `migrate_converts_reports_and_records_the_clean_slate` finds a different entry count (the fixture fires one `strlenn` finding), read the actual report and adjust the fixture expectation only after understanding which diagnostics fired.

- [ ] **Step 7: Run the whole crate's tests**

Run: `cargo test --package celerrate_cli`
Expected: PASS. Watch specifically: the arguments unit tests (new variant), the help-visibility test, and `tests/check.rs` snapshots (they must NOT change; `migrate` alters no check behavior). If `check__help_line.snap` or any existing snapshot moves, that is a bug in this task, not a snapshot to re-bless.

- [ ] **Step 8: Full mechanical suite and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo xtask dependency-shape && cargo xtask emission-scan`

```bash
git add crates/celerrate_cli/src/ crates/celerrate_cli/tests/
git commit -m "✨ feat(cli): add celerrate migrate --from-phpstan end to end"
```

---

### Task 5: Documentation, drift gate, changelog, closure

**Files:**
- Create: `docs/migration.md`
- Modify: `crates/celerrate_cli/tests/documentation.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: the level table and behavior fixed in Tasks 3 and 4; the `workspace_page` helper in `tests/documentation.rs`.
- Produces: the drift-gated migration page.

- [ ] **Step 1: Write the failing drift test**

In `crates/celerrate_cli/tests/documentation.rs`, following the existing test style and using the existing `workspace_page` helper:

```rust
#[test]
fn the_migration_page_documents_the_command_and_its_level_table() {
    let page = workspace_page("migration.md");
    for token in [
        "--from-phpstan",
        "--force",
        "phpstan.neon",
        "celerrate-baseline.toml",
        "includes",
        "ignoreErrors",
    ] {
        assert!(page.contains(token), "docs/migration.md must mention {token}");
    }
    for identifier in [
        "CEL0030", "CEL0031", "CEL0032", "CEL0033", "CEL0034", "CEL0035", "CEL0036",
        "CEL0037", "CEL0038",
    ] {
        assert!(page.contains(identifier), "the level table must list {identifier}");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_cli --test documentation`
Expected: FAIL (`docs/migration.md` does not exist; `workspace_page` will surface that as a read failure or an empty page, per its existing behavior).

- [ ] **Step 3: Write `docs/migration.md`**

Match the tone and structure of the existing `docs/` pages (see `docs/output-formats.md`). Content, in this order:

1. **Title and pitch**: `# Migrating from PHPStan` and the one-command story: `celerrate migrate --from-phpstan` converts the configuration, reports what does not carry over, and records the baseline so the first `celerrate check` is clean and only new problems fail.
2. **What the command reads**: `phpstan.neon` (or `phpstan.neon.dist`, `phpstan.dist.neon`), its `includes` resolved recursively; only `parameters.paths`, `parameters.excludePaths`, and `parameters.level` are consumed. What does not parse or does not carry over is listed in the report, line by line.
3. **What it writes**: `celerrate.toml` (refusing to overwrite without `--force`) and, when there are findings, `celerrate-baseline.toml`. It never modifies `phpstan.neon` or any PHPStan baseline: rollback stays free, and the listed baseline includes can be deleted once the Celerrate baseline is in place.
4. **The level table**, exactly as in this plan's "committed level table" section (the markdown table with the nine identifiers CEL0030 to CEL0038 and the three typed families named), plus one line: severities stay editable afterward in `[severity]`.
5. **What is not converted, and why**: message-based `ignoreErrors` (a foreign diagnostic vocabulary; the recorded baseline carries the continuity instead), PHPStan baseline files (same reason), bootstrap and stub files, extension configuration. Inline `@phpstan-ignore` comments need no migration: Celerrate honors them at analysis time (link `docs/phpdoc-bridge.md`).
6. **After migrating**: run `celerrate check`; the baseline workflow reference (link the baseline documentation page).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --package celerrate_cli --test documentation`
Expected: PASS.

- [ ] **Step 5: Update the changelog**

In `CHANGELOG.md` under `## [Unreleased]` in the `### Added` section (create the section if absent, matching the file's existing style):

```markdown
- `celerrate migrate --from-phpstan`: convert `phpstan.neon` (includes,
  paths, level) to `celerrate.toml`, report everything that does not
  carry over, and record `celerrate-baseline.toml` so the first check
  is clean and only new problems fail.
```

- [ ] **Step 6: Full verification, including the corpus gates**

Run, in order:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo xtask dependency-shape
cargo xtask emission-scan
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: everything green; the corpus snapshot and the mixed-rate baseline byte-identical (this plan changes no default-path behavior and no types; any delta is a bug in this plan's wiring).

- [ ] **Step 7: Commit**

```bash
git add docs/migration.md crates/celerrate_cli/tests/documentation.rs CHANGELOG.md
git commit -m "📝 docs(migration): document the PHPStan migration and its level table"
```

---

## Self-review notes (spec coverage)

- Spec section 5, step 1 (minimal NEON parser, recursive cycle-guarded includes, NEON merge semantics, resilience): Tasks 1 and 2.
- Step 2 (`paths` to `include`, `excludePaths` to `exclude`, the committed level table, `--force` refusal): Tasks 3 and 4.
- Step 3 (key-by-key report of what is not carried over, generated, never silent): Tasks 2 and 4 (the `untransposed`/`problems`/`ignored_includes` channels and `render_report`).
- Step 4 (always run the analysis; record the baseline only when there are findings): Task 4 (`record_clean_slate`; `baseline::record` already refuses to create an empty file).
- The amendment (no PHPStan baseline conversion; baseline includes listed by name, never parsed; `phpstan.neon` and PHPStan baselines never touched): Task 2 (`ignored_include`) and Task 4 (`migrate_never_touches_the_phpstan_files`).
- Closure gate 4 (PHPStan fixture, one command, clean first run, only new problems fail): Task 4 (`after_migrate_the_first_check_is_clean_and_only_new_problems_fail`).
- Spec section 9's migrate bullet (neon with recursive includes, scattered baselines ignored, inline suppressions, introduced regression): Task 4's fixture covers all four.
- Out of scope, deliberately: the README one-liner and install-channel documentation belong to the release work (spec section 8), not here; no new CEL identifiers; no `celerrate_config` changes.
