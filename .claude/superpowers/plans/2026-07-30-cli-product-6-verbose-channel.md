# CLI Product 6: The Verbose Channel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The global `--verbose` flag: one stderr line per widened
foreign directive (the sub-project 4 debt), plus a run-summary line of
already-available meta-information, with every machine output
byte-identical with or without the flag.

**Architecture:** `ResolvedDirective` gains an in-memory `widened_by`
field (the written identifiers whose correspondence fate was `Unmapped`)
computed where the fates are already visible, in
`celerrate_semantics::suppression_directives`; the field is never
persisted, so nothing enters the cache. A new `celerrate_cli::verbose`
module derives the widened set fresh from that query over the reported
files (so the lines are independent of cache state), renders through
pure functions, and emits through a thin `eprintln!` wrapper — the exact
model `cache::statistics` already uses. The flag is a clap global
argument threaded into the single-pass check (all four output formats)
and into every `--watch` cycle.

**Tech Stack:** Rust, clap (derive), salsa queries already in place
(`suppression_directives`, `line_index`), tempfile fixtures for tests.

**Spec:** `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`
section 7 (and the debt wording in
`.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md`,
"Surfacing widened directives to the user (a verbose channel) is
sub-project 5 product surface").

**Branch:** work on `feat-cli-verbose-channel`, from `main`.

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
- TDD: failing test first, minimal implementation, refactor. No
  production code without a test that demanded it.
- Strict layering: the dependency DAG gains no new edge in this plan
  (`celerrate_cli` already depends on `celerrate_semantics` and
  `celerrate_db`; `celerrate_semantics` gains only a field).
- Determinism: no wall-clock, randomness, or environment reads inside
  queries. The verbose channel lives entirely at the orchestration
  layer.
- The spec's transverse decisions, verbatim: `--verbose` is a global
  flag, full word, alias `-v`; widened-directive lines go to **stderr**;
  machine outputs (JSON, SARIF, GitHub) stay **byte-identical** with or
  without `--verbose`; widening is **not a diagnostic** (no CEL code);
  verbose content is **not a stable surface**; **nothing enters the
  cache**.
- Everything written in English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits, repository-configured
  identity, no Claude attribution.
- The full mechanical suite guards every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`. The final task adds `cargo deny check` and the
  xtask gates.

## Context an implementer needs

The bridge (`celerrate_phpdoc_bridge`) marks every written foreign
identifier through its correspondence table as `Mapped { codes }`,
`ScopeWide` (only `@psalm-suppress all`), or `Unmapped`
(`crates/celerrate_phpdoc_bridge/src/correspondence.rs`,
`crates/celerrate_phpdoc_bridge/src/directives.rs`). The matcher
(`crates/celerrate_semantics/src/comment_directives.rs`) turns those
marks into a `SuppressionFilter` in `filter_of`: any `Unmapped` entry on
a foreign directive widens the filter to `All`. Today the fate is then
dropped — `ResolvedDirective.identifiers` keeps only the written
strings — which is exactly the debt this plan pays.

A wrapped identifier list appends the synthetic unmapped identifier
`<identifier list continues on the next line>`
(`directives.rs`, constant `WRAPPED_LIST_CONTINUES`); its documentation
already says "If a verbose channel ever echoes a directive's written
identifiers, this reads as the reason the directive widened". This plan
makes that sentence true; do not special-case the synthetic identifier.

The persistent cache stores directive records
(`crates/celerrate_cli/src/cache/stored.rs`, `StoredDirective`, schema
7). The new field is deliberately **not** persisted: on a warm serve the
reconstructed `ResolvedDirective` carries an empty `widened_by`, and the
verbose channel never reads reconstructed directives — it re-derives
from the live query. The warm/cold equivalence harness
(`crates/celerrate_cli/tests/cache_equivalence.rs`) compares composed
diagnostics, never directive structs, so the asymmetry is invisible to
it by construction.

Stderr convention in this codebase: pure `render` functions, unit
tested, plus a thin `eprintln!` emitter that no test drives
(`crates/celerrate_cli/src/cache/statistics.rs` is the model;
`report_excluded_plugins` in `crates/celerrate_cli/src/lib.rs` is the
other precedent). `run()` takes only the stdout stream; stderr is
process-global. Follow that convention; do not refactor `run()`'s
signature.

---

### Task 1: Carry the unmapped fate on `ResolvedDirective`

**Files:**

- Modify: `crates/celerrate_semantics/src/comment_directives.rs`
- Modify: `crates/celerrate_rules/src/phases.rs` (constructor sites,
  around lines 673, 955, 972, 1013, 1020 — a test helper and test
  literals)
- Modify: `crates/celerrate_cli/src/cache/stored.rs` (`to_directive`
  around line 991, plus test literals around lines 1448, 1463, 1567)

**Interfaces:**

- Consumes: `SuppressionIdentifier` (existing enum, same file),
  `DirectiveOrigin` (existing enum, same file).
- Produces: `ResolvedDirective.widened_by: Vec<String>` — the written
  identifiers whose fate was `Unmapped`, in written order; and
  `pub(crate) fn widened_by(origin: DirectiveOrigin, identifiers:
  &[SuppressionIdentifier]) -> Vec<String>` in
  `comment_directives.rs`. Task 3 reads the field.

- [ ] **Step 1: Write the failing tests**

In the `tests` module of
`crates/celerrate_semantics/src/comment_directives.rs`:

```rust
#[test]
fn widened_by_answers_the_unmapped_written_identifiers_in_order() {
    let identifiers = vec![
        SuppressionIdentifier::mapped("class.notFound".to_owned(), vec!["CEL0018".to_owned()]),
        SuppressionIdentifier::unmapped("something.else".to_owned()),
        SuppressionIdentifier::unmapped("another.one".to_owned()),
    ];
    assert_eq!(
        widened_by(DirectiveOrigin::Foreign, &identifiers),
        vec!["something.else".to_owned(), "another.one".to_owned()],
    );
}

#[test]
fn an_explicit_scope_wide_identifier_is_not_a_widening() {
    // `@psalm-suppress all` is the user's own decision, not a fallback.
    let identifiers = vec![SuppressionIdentifier::scope_wide("all".to_owned())];
    assert!(widened_by(DirectiveOrigin::Foreign, &identifiers).is_empty());
}

#[test]
fn a_bare_foreign_directive_is_not_a_widening() {
    // `@phpstan-ignore-line` is blanket by design, not widened.
    assert!(widened_by(DirectiveOrigin::Foreign, &[]).is_empty());
}

#[test]
fn a_native_directive_never_widens() {
    // An unknown native identifier suppresses nothing (CEL0041's job).
    let identifiers = vec![SuppressionIdentifier::native("CEL9999".to_owned())];
    assert!(widened_by(DirectiveOrigin::Native, &identifiers).is_empty());
}
```

And extend the existing query-level test
`the_query_resolves_anchor_scope_and_origin_per_directive` (the
`FakeProvider`'s `@fake` marker carries the unmapped identifier
`fake.identifier`) with one assertion after the `identifiers` one:

```rust
assert_eq!(directive.widened_by, vec!["fake.identifier".to_owned()]);
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics comment_directives`
Expected: compilation failure — `widened_by` is not defined and
`ResolvedDirective` has no such field. A compile failure of the test
target is the red state here.

- [ ] **Step 3: Implement**

In `crates/celerrate_semantics/src/comment_directives.rs`:

Add the field to `ResolvedDirective` (after `identifiers`):

```rust
    /// The written identifiers whose correspondence fate was
    /// `SuppressionIdentifier::Unmapped`, in written order: the reason
    /// this directive's filter widened to `All`. Empty for native
    /// directives (unknown native identifiers never widen), for fully
    /// mapped lists, for a bare foreign directive (blanket by design,
    /// not widened), and for the explicit scope-wide entry
    /// (`@psalm-suppress all`). Presentation data for the verbose
    /// channel; deliberately not persisted — `StoredDirective` does not
    /// carry it, and the channel re-derives it from this query.
    pub widened_by: Vec<String>,
```

Add the helper next to `filter_of`:

```rust
/// The written identifiers that widened a foreign directive to
/// scope-wide suppression: the `Unmapped` entries, in written order.
/// The companion of `filter_of`: whenever this answers non-empty for a
/// foreign directive, `filter_of` answers `All`.
pub(crate) fn widened_by(
    origin: DirectiveOrigin,
    identifiers: &[SuppressionIdentifier],
) -> Vec<String> {
    match origin {
        DirectiveOrigin::Foreign => identifiers
            .iter()
            .filter_map(|identifier| match identifier {
                SuppressionIdentifier::Unmapped { written } => Some(written.clone()),
                _ => None,
            })
            .collect(),
        DirectiveOrigin::Native => Vec::new(),
    }
}
```

Populate it in `suppression_directives` where the `ResolvedDirective` is
pushed (the `identifiers: Vec<SuppressionIdentifier>` binding is still
in scope there; compute `widened_by(origin, &identifiers)` **before**
the `identifiers.iter().map(...)` conversion consumes the list by
reference — both are borrows, order only matters for readability):

```rust
                        directives.push(ResolvedDirective {
                            anchor: token.text_range(),
                            scope: resolved,
                            filter: filter_of(origin, &identifiers),
                            widened_by: widened_by(origin, &identifiers),
                            identifiers: identifiers
                                .iter()
                                .map(|identifier| identifier.written().to_owned())
                                .collect(),
                            origin,
                        });
```

- [ ] **Step 4: Fix every other `ResolvedDirective { .. }` literal**

The struct is not `#[non_exhaustive]`, so every literal construction now
needs the field. Add `widened_by: Vec::new(),` to:

- the three test literals in `comment_directives.rs`'s own `tests`
  module (`an_only_filter_admits_exactly_its_codes_on_its_scope`,
  `the_end_of_file_exception_survives_in_admits`,
  `an_empty_scope_at_the_end_of_file_admits_only_the_end_position`);
- the test helper and test literals in
  `crates/celerrate_rules/src/phases.rs` (around lines 673, 955, 972,
  1013, 1020);
- `crates/celerrate_cli/src/cache/stored.rs`:
  - in `StoredDirective::to_directive` (around line 991), with this
    comment:

    ```rust
                identifiers: self.identifiers.clone(),
                // The fates are not persisted: the verbose channel
                // derives widening fresh from `suppression_directives`,
                // never from a reconstructed record, so nothing enters
                // the cache for it.
                widened_by: Vec::new(),
    ```

  - the test literals (around lines 1448, 1463, 1567), plain
    `widened_by: Vec::new(),`.

Let the compiler find any site this list missed:
`cargo build --workspace --all-targets` and fix each the same way.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics -p celerrate_rules -p celerrate_cli`
Expected: PASS, including the existing directive-record round-trip
(`a_directive_record_round_trips` in `stored.rs`: both sides carry an
empty `widened_by`, so equality holds) and the cache-equivalence
harness (it compares diagnostics, never directive structs).

- [ ] **Step 6: Lints, format, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_semantics/src/comment_directives.rs \
        crates/celerrate_rules/src/phases.rs \
        crates/celerrate_cli/src/cache/stored.rs
git commit -m "✨ feat(semantics): record which written identifiers widened a directive"
```

---

### Task 2: The global `--verbose` flag

**Files:**

- Modify: `crates/celerrate_cli/src/arguments.rs`

**Interfaces:**

- Produces: `Arguments.verbose: bool` — global, `--verbose` or `-v`,
  valid before or after any subcommand, default `false`. Task 4 reads
  it in `run()`.

- [ ] **Step 1: Write the failing test**

In the `tests` module of `crates/celerrate_cli/src/arguments.rs`:

```rust
    /// The design fixes the surface: a global flag, the full word,
    /// alias `-v`, accepted on either side of the subcommand.
    #[test]
    fn verbose_is_global_and_defaults_off() {
        let arguments = Arguments::try_parse_from(["celerrate", "check"]).unwrap();
        assert!(!arguments.verbose);
        let arguments = Arguments::try_parse_from(["celerrate", "check", "--verbose"]).unwrap();
        assert!(arguments.verbose);
        let arguments = Arguments::try_parse_from(["celerrate", "check", "-v"]).unwrap();
        assert!(arguments.verbose);
        let arguments =
            Arguments::try_parse_from(["celerrate", "explain", "CEL0030", "-v"]).unwrap();
        assert!(arguments.verbose);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p celerrate_cli arguments`
Expected: compilation failure — no field `verbose` on `Arguments`.

- [ ] **Step 3: Implement**

On the `Arguments` struct in `arguments.rs`:

```rust
pub struct Arguments {
    #[command(subcommand)]
    pub command: Command,

    /// Report analysis meta-information on stderr: widened foreign
    /// directives, files analyzed, cache traffic. Not a stable surface.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
}
```

Note: clap's derived `--version` uses the short flag `-V` (uppercase),
so `-v` is free; the parse test above is what proves it (a clash would
be a clap builder panic at parse time).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli arguments`
Expected: PASS. Also run `cargo test -p celerrate_cli` whole: the
`--help` guard tests (`mixed_rate_is_hidden_from_help` and any snapshot
over help text) must still pass; if a pinned help snapshot changed
because the global flag now appears in it, inspect the delta (it must be
exactly the new `--verbose` line) and accept it — verify-then-accept.

- [ ] **Step 5: Lints, format, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/arguments.rs
git commit -m "✨ feat(cli): add the global --verbose flag"
```

---

### Task 3: The verbose module: collection and rendering

**Files:**

- Create: `crates/celerrate_cli/src/verbose.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (module declaration only:
  add `pub mod verbose;` to the module list)
- Modify: `crates/celerrate_cli/src/render.rs` (visibility only:
  `fn display_path` at line 440 becomes `pub(crate) fn display_path`)

**Interfaces:**

- Consumes: `ResolvedDirective.widened_by` (task 1),
  `crate::session::Session` (fields `database`, `statistics`; method
  `inputs()` whose `reported: Arc<[SourceFile]>` is the project's own
  files), `crate::render::display_path(session, file_id) -> String`
  (project-relative), `celerrate_semantics::suppression_directives(db,
  file)`, `celerrate_db::line_index(db, file)` (0-based
  `line_column`), `crate::cache::statistics::CacheStatistics`.
- Produces: `WidenedDirective { path: String, line: u32, identifiers:
  Vec<String> }`, `widened_directives(&Session) ->
  Vec<WidenedDirective>`, `render_widened(&WidenedDirective) ->
  String`, `render_run_summary(&CacheStatistics, analyzed: usize) ->
  String`, `report(&Session)`. Task 4 calls `report`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_cli/src/verbose.rs` with the module
documentation, empty production surface, and the tests first — the
file will not compile until step 3, which is the red state:

```rust
//! The verbose channel: meta-reporting about the analysis, on stderr,
//! behind the global `--verbose` flag. Two kinds of lines: one per
//! widened foreign directive (the sub-project 4 debt: a foreign
//! directive with any unmapped identifier falls back to scope-wide
//! suppression, and the user now sees it), and one run summary of
//! already-available meta-information (files analyzed, cache verdict
//! traffic).
//!
//! Stderr because this is meta-reporting about the analysis, not an
//! analysis result: the machine formats stay byte-identical with or
//! without the flag. Widening is deliberately not a diagnostic - a CEL
//! code here would recreate the false-positive storm on imported
//! codebases. The content is not a stable surface, and nothing here
//! enters the queries or the persistent cache: the widened marks are
//! derived fresh from `suppression_directives`, so the lines are
//! independent of which files the cache happened to serve this run.
//! The module follows the `cache::statistics` convention: pure render
//! functions, unit tested, plus a thin `eprintln!` emitter.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::session::Session;

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

    #[test]
    fn an_unmapped_identifier_yields_one_widened_entry() {
        let root = project(&[(
            "a.php",
            "<?php\nnew MissingOne(); // @phpstan-ignore some.unknownIdentifier\n",
        )]);
        let session = Session::start(root.path());
        assert_eq!(
            widened_directives(&session),
            vec![WidenedDirective {
                path: "a.php".to_owned(),
                line: 2,
                identifiers: vec!["some.unknownIdentifier".to_owned()],
            }],
        );
    }

    #[test]
    fn a_fully_mapped_directive_is_not_reported() {
        let root = project(&[(
            "a.php",
            "<?php\nnew MissingOne(); // @phpstan-ignore class.notFound\n",
        )]);
        let session = Session::start(root.path());
        assert!(widened_directives(&session).is_empty());
    }

    #[test]
    fn an_explicit_scope_wide_suppression_is_not_reported() {
        // `@psalm-suppress all` is the user's own decision, not a
        // fallback the channel should second-guess.
        let root = project(&[(
            "a.php",
            "<?php\n/* @psalm-suppress all */\nnew MissingOne();\n",
        )]);
        let session = Session::start(root.path());
        assert!(widened_directives(&session).is_empty());
    }

    #[test]
    fn a_wrapped_list_reports_the_synthetic_continuation_identifier() {
        let root = project(&[(
            "a.php",
            "<?php\n/**\n * @psalm-suppress UndefinedClass,\n * UndefinedFunction\n */\nclass Service {}\n",
        )]);
        let session = Session::start(root.path());
        let widened = widened_directives(&session);
        assert_eq!(widened.len(), 1);
        assert_eq!(
            widened[0].identifiers,
            vec!["<identifier list continues on the next line>".to_owned()],
        );
        // The line is the carrying comment's first line.
        assert_eq!(widened[0].line, 2);
    }

    #[test]
    fn entries_are_sorted_by_path_then_line() {
        let root = project(&[
            (
                "b.php",
                "<?php\nnew MissingOne(); // @phpstan-ignore second.unknown\n",
            ),
            (
                "a.php",
                "<?php\nnew MissingOne(); // @phpstan-ignore first.unknown\n\nnew MissingTwo(); // @phpstan-ignore third.unknown\n",
            ),
        ]);
        let session = Session::start(root.path());
        let widened = widened_directives(&session);
        let keys: Vec<(&str, u32)> = widened
            .iter()
            .map(|entry| (entry.path.as_str(), entry.line))
            .collect();
        assert_eq!(keys, vec![("a.php", 2), ("a.php", 4), ("b.php", 2)]);
    }

    #[test]
    fn the_widened_line_names_file_line_identifier_and_consequence() {
        let directive = WidenedDirective {
            path: "src/a.php".to_owned(),
            line: 3,
            identifiers: vec!["some.unknown".to_owned()],
        };
        assert_eq!(
            render_widened(&directive),
            "verbose: src/a.php:3: unmapped identifier `some.unknown`: \
             the directive widens to scope-wide suppression",
        );
    }

    #[test]
    fn several_unmapped_identifiers_share_one_line() {
        let directive = WidenedDirective {
            path: "a.php".to_owned(),
            line: 2,
            identifiers: vec!["first.unknown".to_owned(), "second.unknown".to_owned()],
        };
        assert_eq!(
            render_widened(&directive),
            "verbose: a.php:2: unmapped identifiers `first.unknown`, \
             `second.unknown`: the directive widens to scope-wide suppression",
        );
    }

    #[test]
    fn the_run_summary_carries_the_counters() {
        use std::sync::atomic::Ordering;
        let statistics = crate::cache::statistics::CacheStatistics::default();
        statistics.verdicts_served.fetch_add(3, Ordering::Relaxed);
        statistics.verdicts_absent.fetch_add(2, Ordering::Relaxed);
        assert_eq!(
            render_run_summary(&statistics, 5),
            "verbose: 5 files analyzed; verdicts 3 served / 0 discarded / \
             2 absent from the cache",
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli verbose`
Expected: compilation failure — `widened_directives`,
`WidenedDirective`, `render_widened`, `render_run_summary` do not exist
(and `pub mod verbose;` is not yet declared, so add the declaration in
`lib.rs` first and re-run to see the real red).

- [ ] **Step 3: Implement**

Production code at the top of `crates/celerrate_cli/src/verbose.rs`
(above the tests module):

```rust
use std::sync::atomic::Ordering;

use crate::cache::statistics::CacheStatistics;
use crate::session::Session;

/// One widened foreign directive, presentation-ready. The derived
/// order (path, then line, then identifiers) is the report order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WidenedDirective {
    /// The carrying file, project-relative, as the renderer displays it.
    pub path: String,
    /// The carrying comment's first line, 1-based.
    pub line: u32,
    /// The unmapped written identifiers, in written order.
    pub identifiers: Vec<String>,
}

/// Every widened foreign directive across the reported files, sorted.
/// Derived fresh from `suppression_directives`, deliberately: the
/// answer is a pure function of the sources and the registered
/// bridge, independent of which files the cache served this run. On a
/// warm run this parses files the analysis itself skipped; that is the
/// price of asking, paid only under `--verbose`.
pub fn widened_directives(session: &Session) -> Vec<WidenedDirective> {
    let database = &session.database;
    let mut widened = Vec::new();
    for &file in session.inputs().reported.iter() {
        for directive in celerrate_semantics::suppression_directives(database, file) {
            if directive.widened_by.is_empty() {
                continue;
            }
            let file_id = file.file_id(database);
            let index = celerrate_db::line_index(database, file);
            widened.push(WidenedDirective {
                path: crate::render::display_path(session, file_id),
                line: index.line_column(directive.anchor.start()).line + 1,
                identifiers: directive.widened_by.clone(),
            });
        }
    }
    widened.sort();
    widened
}

/// One line per widened directive: the file, the directive's line, the
/// unmapped identifiers, and the consequence. Not a stable surface.
pub fn render_widened(directive: &WidenedDirective) -> String {
    let noun = if directive.identifiers.len() == 1 {
        "identifier"
    } else {
        "identifiers"
    };
    let identifiers = directive
        .identifiers
        .iter()
        .map(|identifier| format!("`{identifier}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "verbose: {}:{}: unmapped {noun} {identifiers}: the directive \
         widens to scope-wide suppression",
        directive.path, directive.line,
    )
}

/// The run summary: already-available meta-information, no format
/// commitment. Not a stable surface.
pub fn render_run_summary(statistics: &CacheStatistics, analyzed: usize) -> String {
    let load = |counter: &std::sync::atomic::AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        "verbose: {analyzed} files analyzed; verdicts {} served / {} \
         discarded / {} absent from the cache",
        load(&statistics.verdicts_served),
        load(&statistics.verdicts_discarded),
        load(&statistics.verdicts_absent),
    )
}

/// Prints every verbose line to stderr. The caller gates on the flag;
/// this function only speaks (the `cache::statistics::report` model:
/// the render functions above carry the tests, this wrapper stays
/// thin).
pub fn report(session: &Session) {
    for directive in widened_directives(session) {
        eprintln!("{}", render_widened(&directive));
    }
    let analyzed = session.inputs().reported.len();
    eprintln!("{}", render_run_summary(&session.statistics, analyzed));
}
```

In `crates/celerrate_cli/src/lib.rs`, add `pub mod verbose;` to the
module list (alphabetical: between `pub mod suggest;` and
`pub mod watch;`).

In `crates/celerrate_cli/src/render.rs`, change line 440:

```rust
pub(crate) fn display_path(session: &Session, file: FileId) -> String {
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli verbose`
Expected: PASS. If the line assertions are off by one, the fix belongs
in the test expectation only after re-deriving the fixture line by hand:
`line_column` is 0-based and the `+ 1` in the collector is the
conversion, matching `output::model::position`.

- [ ] **Step 5: Lints, format, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/verbose.rs \
        crates/celerrate_cli/src/lib.rs \
        crates/celerrate_cli/src/render.rs
git commit -m "✨ feat(cli): collect and render widened foreign directives"
```

---

### Task 4: Wire `--verbose` into check and watch; prove machine outputs unchanged

**Files:**

- Modify: `crates/celerrate_cli/src/lib.rs` (the `run()` check arm)
- Modify: `crates/celerrate_cli/src/watch.rs` (`watch` at line 240,
  `iteration` at line 310, `completed_cycle` at line 109, plus their
  in-crate test callers — compiler-guided)
- Create: `crates/celerrate_cli/tests/verbose.rs`

**Interfaces:**

- Consumes: `Arguments.verbose` (task 2), `crate::verbose::report`
  (task 3).
- Produces: `watch::watch(session, output, color, mode, verbose)` — the
  signature gains a trailing `verbose: bool`, threaded to
  `completed_cycle` the same way `mode: crate::baseline::Mode` already
  is.

- [ ] **Step 1: Write the failing end-to-end test**

Create `crates/celerrate_cli/tests/verbose.rs`:

```rust
//! The verbose channel end to end: stdout is byte-identical with and
//! without `--verbose` in every output format — the flag speaks only
//! on stderr, so the machine formats cannot move (the spec's
//! transverse decision, pinned here).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

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

fn check(root: &Path, extra: &[&str]) -> (Outcome, Vec<u8>) {
    let mut output = Vec::new();
    let mut arguments: Vec<std::ffi::OsString> =
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()];
    arguments.extend(extra.iter().map(|argument| argument.into()));
    let outcome = run(arguments, &mut output, ColorMode::Plain);
    (outcome, output)
}

#[test]
fn stdout_is_byte_identical_with_and_without_verbose_in_every_format() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore some.unknownIdentifier\nnew MissingTwo();\n",
    )]);
    // One throwaway run so both compared runs are equally warm: the
    // report is warm/cold byte-identical by contract, but this test
    // must not depend on that contract to isolate its own claim.
    let _ = check(root.path(), &[]);
    for format in ["human", "json", "sarif", "github"] {
        let (outcome_without, without) = check(root.path(), &["--output", format]);
        let (outcome_with, with) = check(root.path(), &["--output", format, "--verbose"]);
        assert_eq!(outcome_without, outcome_with, "{format}");
        assert_eq!(without, with, "{format}: stdout must not move");
    }
}

#[test]
fn verbose_with_a_machine_format_is_not_a_usage_error() {
    let root = project(&[("a.php", "<?php\n$clean = 1;\n")]);
    let (outcome, _) = check(root.path(), &["--output", "json", "--verbose"]);
    assert_eq!(outcome, Outcome::Clean);
}
```

- [ ] **Step 2: Run the test to verify the state**

Run: `cargo test -p celerrate_cli --test verbose`
Expected: PASS already — the flag parses (task 2) and nothing writes to
stdout. This test is the pin that keeps step 3's wiring honest: after
wiring, it must still pass, proving the emission went to stderr and
nowhere else. The red state for the wiring itself is manual: run
`cargo run -p celerrate_cli -- check <fixture> --verbose` on a fixture
with an unmapped identifier and observe that no verbose line appears
yet.

- [ ] **Step 3: Wire the single-pass check arm**

In `crates/celerrate_cli/src/lib.rs`, inside `run()`, read the flag
before the `match` moves `arguments.command`:

```rust
    let verbose = arguments.verbose;
    match arguments.command {
```

In the `Command::Check` arm, three insertions:

1. The watch path passes the flag through (signature change lands in
   step 4):

```rust
            if watch {
                return watch::watch(&mut session, output, color, mode, verbose);
            }
```

2. The machine-format early return, immediately after
   `session.statistics.report();` and before `return verdict;`:

```rust
                session.statistics.report();
                if verbose {
                    verbose::report(&session);
                }
                return verdict;
```

3. The human-format tail, immediately after the final
   `session.statistics.report();` and before the closing
   `Outcome::of(...)`:

```rust
            session.statistics.report();
            if verbose {
                verbose::report(&session);
            }
            Outcome::of(
```

- [ ] **Step 4: Thread the flag through watch**

In `crates/celerrate_cli/src/watch.rs`, add a trailing `verbose: bool`
parameter to `watch` (line 240), `iteration` (line 310), and
`completed_cycle` (line 109), passing it down exactly as
`mode: crate::baseline::Mode` already travels. In `completed_cycle`,
immediately after the existing `session.statistics.report();`
(line 174):

```rust
    session.statistics.report();
    if verbose {
        crate::verbose::report(session);
    }
```

Fix every in-crate caller the compiler names (watch's own test module
and any integration test driving `watch`/`iteration` directly) by
passing `false`, except where a test exists specifically for the
verbose path.

- [ ] **Step 5: Run the full crate suite**

Run: `cargo test -p celerrate_cli`
Expected: PASS — including `tests/verbose.rs` (stdout unchanged),
`tests/output_json.rs`, `tests/output_sarif.rs`,
`tests/output_github.rs`, `tests/output_equivalence.rs` (none of them
know the flag exists, which is the point), and the watch suite with the
threaded parameter.

Manual verification of the actual emission (the part in-process tests
cannot capture, per the codebase's stderr convention):

```bash
cd "$(mktemp -d)" && printf '<?php\nnew MissingOne(); // @phpstan-ignore some.unknownIdentifier\n' > a.php
cargo run -p celerrate_cli --manifest-path <repository>/Cargo.toml -- check . --verbose 2>&1 >/dev/null
```

Expected on stderr, exactly two lines:

```text
verbose: a.php:2: unmapped identifier `some.unknownIdentifier`: the directive widens to scope-wide suppression
verbose: 1 files analyzed; verdicts 0 served / 0 discarded / 1 absent from the cache
```

(The counter split may differ on a re-run in the same directory — warm
verdicts move to `served` — but the widened line must be identical.)

- [ ] **Step 6: Lints, format, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/lib.rs \
        crates/celerrate_cli/src/watch.rs \
        crates/celerrate_cli/tests/verbose.rs
git commit -m "✨ feat(cli): report widened directives and run meta-information under --verbose"
```

---

### Task 5: Documentation and the full gate suite

**Files:**

- Modify: `docs/phpdoc-bridge.md` (append a section)

**Interfaces:**

- Consumes: the shipped behavior of tasks 1-4.
- Produces: user-facing documentation of the verbose channel; a fully
  green mechanical suite over the branch.

- [ ] **Step 1: Document the channel where foreign directives are documented**

Read `docs/phpdoc-bridge.md` first to place the section coherently
(after the widening/correspondence discussion), then append:

```markdown
## The verbose channel

A foreign directive that names an identifier the correspondence table
does not map falls back to scope-wide suppression: the existing
decision in the code is honored rather than re-reported. By default
that widening is silent. The global `--verbose` flag (alias `-v`)
makes it visible: each widened directive produces one line on stderr
naming the file, the directive's line, the unmapped written
identifiers, and the consequence:

    verbose: src/Service.php:42: unmapped identifier `some.futureCode`: the directive widens to scope-wide suppression

A wrapped identifier list (one that continues past the tag's own line)
reports the synthetic reason
`<identifier list continues on the next line>`.

`--verbose` also prints a run summary (files analyzed, cache verdict
traffic). Everything the flag adds goes to stderr: the machine formats
(`--output=json`, `sarif`, `github`) are byte-identical with or
without it. Verbose content is not a stable surface; do not parse it.

Widening is deliberately not a diagnostic. A CEL code here would turn
every suppression of a code Celerrate does not emit yet into a warning
storm on imported codebases. The verbose channel informs whoever asks,
without judging.
```

Check `crates/celerrate_cli/tests/documentation.rs` afterwards: if it
walks `docs/` for structural properties, the new section must satisfy
them (run it: `cargo test -p celerrate_cli --test documentation`).

- [ ] **Step 2: Run the full mechanical suite**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
cargo xtask dependency-shape
cargo xtask emission-scan
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: everything green. In particular the corpus snapshot and the
mixed-rate baseline must be **unchanged** — the whole feature is
stderr-side presentation, so any delta there is a defect in the
implementation, not a snapshot to re-bless.

- [ ] **Step 3: Commit**

```bash
git add docs/phpdoc-bridge.md
git commit -m "📝 docs(bridge): document the verbose widened-directive channel"
```

---

## What this plan deliberately does not do

- **No `error: &mut dyn Write` refactor of `run()`.** The codebase's
  stderr convention is direct `eprintln!` behind tested pure render
  functions (`cache::statistics`, `report_excluded_plugins`); the
  verbose channel follows it. Threading a second stream through `run`,
  `watch`, and every test is a cross-cutting refactor with no consumer
  beyond marginally different test ergonomics.
- **No persistence of the widened fates.** `StoredDirective` (schema 7)
  is untouched: the spec says nothing enters the cache, and the channel
  re-derives from the live query instead, at the cost of parsing
  cache-served files only when `--verbose` is actually passed.
- **No CEL identifier for widening.** Twice rejected (sub-project 4
  section 8, sub-project 5 section 7): the channel informs, it does not
  judge.
- **No stability commitment on verbose content.** The spec says so
  explicitly; the docs section repeats it, and the rendered strings can
  change without ceremony.
- **No "product polish" beyond the spec'd surface.** The design's
  step 6 title mentions polish; its section 7 commits exactly the
  widened-directive lines and the meta-information home, and this plan
  implements exactly that. Anything else found along the way is an
  issue to file, not scope to absorb.
