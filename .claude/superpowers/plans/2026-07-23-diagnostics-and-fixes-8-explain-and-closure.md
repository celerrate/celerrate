# Diagnostics and Fixes Part 8: `celerrate explain` and Closure — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the `celerrate explain` subcommand, write the embedded explain page for every one of the 42 registered identifiers with an executable-example harness and a declared exemption list, and close the diagnostics-and-fixes sub-project (gates verified, spec and CHANGELOG updated, no release).

**Architecture:** Page content lives in `celerrate_diagnostics` (the registry already reserves an `explain: Option<&'static ExplainPage>` field on every entry); a composition-root harness in `celerrate_cli` runs every page's failing and fixed example through the full product pipeline; once all pages exist the field flips to mandatory, making "every identifier has a page" a type-level invariant. The CLI gains a public `explain` subcommand that is pure formatting over the registry, with no analysis session.

**Tech Stack:** Rust, clap (derive), insta snapshots, tempfile fixtures, the existing `celerrate_cli::run` in-process product harness.

**Spec:** `.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md`, sections 10 (explain), 1 (closure gates), 12 item 8. Parent: `.claude/superpowers/specs/2026-07-09-celerrate-design.md`.

**Branch:** `feat-diagnostics-explain-8` off `main` (house pattern: part 7 used `feat-diagnostics-renderer-7`).

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result`. Test modules may locally `#[allow]` these lints (the repository's standard block, see any existing test module).
- TDD: failing test → minimal implementation → refactor. No production code without a test that demanded it.
- Strict layering: page content lives in `celerrate_diagnostics` (bottom layer, no new dependencies); the harness lives at the composition root (`celerrate_cli` tests); the `explain` subcommand consumes only `celerrate_diagnostics::REGISTRY`.
- Determinism: `celerrate explain` output is a pure function of the binary — no TTY probing, no color, no environment reads.
- The pinned corpus snapshot is `0 notices, 0 diagnostics` and must not change (`cargo xtask corpus`); the mixed-rate baseline must not change (`cargo xtask mixed-rate`). Both need `cargo xtask fetch-corpus` first. Nothing in this part touches analysis, so any delta is a bug.
- The exit-code contract does not move: 0 clean, 1 any span-anchored diagnostic, 2 internal error or usage error. `explain` exits 0 on success, 2 on an unknown identifier.
- Everything in English, full words, no abbreviated names (standard acronyms fine).
- Commits: gitmoji + Conventional Commits (e.g. `✨ feat(cli): add the celerrate explain subcommand`), authored with the repository-configured identity, no Claude attribution.
- **No release at closure.** No version bump, no tag, no release workflow. The next public event is v0.1 (sub-project 5). Closure is: gates verified, spec amended, CHANGELOG updated.

## Design decisions fixed by this plan

1. **Pages live in `celerrate_diagnostics::pages`**, one module per producing area (`source`, `syntax`, `semantic`, `project`, `typed`, `reporting`), as `pub(crate) const` items the registry references with `&pages::<module>::CEL####` (const promotion gives the `&'static` the field needs). The registry stays the single wiring point.
2. **Executable-fixture format.** An example without marker lines is one file, `src/Example.php`, wrapped with the default manifest `{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}` (the seeded-defect manifest). An example containing lines that start with `//// ` is split at those markers into exactly the files given, nothing implicit — which is how a fixture with no `composer.json` (CEL0025) or a different PHP floor (CEL0021's fixed example) is expressed. Markers render as-is in `celerrate explain` output, which is informative for project-shaped examples.
3. **Harness assertions are identifier-substring presence/absence over the full CLI report.** The failing example must put the identifier's text in the report; the fixed example must not. This deliberately does not require `Outcome::DiagnosticsReported` (project notices are exit-code-neutral) and does not require the fixed example to be perfectly clean (only free of the page's own identifier).
4. **The exemption list is first-class data**: `ExampleExemption { id, reason }` in `celerrate_diagnostics::explain`, one entry per waived identifier, reason mandatory. Initial members: CEL0001 (environment: the 4 GiB cap cannot be fixture-committed), CEL0039 and CEL0040 (environment: permission-based IO errors break under root and on Windows CI), CEL0022 (content: the shipped stub blob carries no symbol removed inside the supported 8.1–8.5 window — the same waiver the seeded-defect suite documents). CEL0013 joins with reason "defensive backstop" only if Task 5's reachability probe proves no source triggers it. Exempt identifiers still carry a full page.
5. **Forced-active provision is a guard test, not machinery.** The spec's harness forces nursery rules active; no nursery rule exists today, and building force-activation now would be untested machinery. Instead the harness carries a guard test asserting every core rule is `Tier::Default`, whose failure message names the extension required. The pages-cannot-lie property survives: a nursery rule cannot land without tripping the guard.
6. **`explain` output format** (snapshot-pinned, colorless): `CEL####: <family>` header, blank line, the `why` text, blank line, `failing example:` with the example indented four spaces, `fixed example:` likewise, blank line, the `configuration` text. Lowercase section labels match the report's own `for more information, run …` voice.
7. **Unknown identifier is a usage error** (exit 2) with a two-line message; lookup normalizes the argument to ASCII uppercase so `celerrate explain cel0030` works.
8. **The mandatory flip is one task** (Task 6), after all 42 pages exist: `explain` becomes `&'static ExplainPage`, the interim `documented()` helper becomes the only constructor (renamed back to `registered()`), and the content gate becomes a four-sections-non-empty test over the whole registry.

## File Structure

```
crates/celerrate_diagnostics/src/explain.rs        modify: ExampleExemption + EXECUTABLE_EXAMPLE_EXEMPTIONS + tests
crates/celerrate_diagnostics/src/pages/mod.rs      create: module index
crates/celerrate_diagnostics/src/pages/source.rs   create: CEL0001
crates/celerrate_diagnostics/src/pages/syntax.rs   create: CEL0002–CEL0017
crates/celerrate_diagnostics/src/pages/semantic.rs create: CEL0018–CEL0024
crates/celerrate_diagnostics/src/pages/project.rs  create: CEL0025–CEL0029, CEL0039, CEL0040
crates/celerrate_diagnostics/src/pages/typed.rs    create: CEL0030–CEL0038
crates/celerrate_diagnostics/src/pages/reporting.rs create: CEL0041, CEL0042
crates/celerrate_diagnostics/src/registry.rs       modify: documented() helper, page wiring, final mandatory flip
crates/celerrate_diagnostics/src/lib.rs            modify: mod pages; export ExampleExemption + EXECUTABLE_EXAMPLE_EXEMPTIONS
crates/celerrate_cli/tests/explain_pages.rs        create: the executable-page harness + tier guard
crates/celerrate_cli/src/arguments.rs              modify: Explain variant + parse test
crates/celerrate_cli/src/explain.rs                create: page formatting
crates/celerrate_cli/src/lib.rs                    modify: mod explain; dispatch arm
crates/celerrate_cli/tests/explain.rs              create: subcommand integration tests + snapshot
README.md                                          modify: explain sample
docs/diagnostics.md                                modify: explain pointer sentence
CHANGELOG.md                                       modify: Unreleased/Added entries
.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md  modify: Status + closing amendment
.claude/superpowers/specs/2026-07-09-celerrate-design.md              modify: amendment-history bullet
```

---

### Task 1: The executable-page harness, the exemption declaration, and the first page (CEL0018)

**Files:**
- Modify: `crates/celerrate_diagnostics/src/explain.rs`
- Create: `crates/celerrate_diagnostics/src/pages/mod.rs`, `crates/celerrate_diagnostics/src/pages/semantic.rs`
- Modify: `crates/celerrate_diagnostics/src/registry.rs`, `crates/celerrate_diagnostics/src/lib.rs`
- Create: `crates/celerrate_cli/tests/explain_pages.rs`

**Interfaces:**
- Consumes: `ExplainPage` (exists, `crates/celerrate_diagnostics/src/explain.rs:6`), `REGISTRY` and `find_page` (exist, `registry.rs:40,121`), `celerrate_cli::run` / `ColorMode` (exist), `celerrate_rules::core_rules()` and `celerrate_rules::Tier` (exist).
- Produces: `pub struct ExampleExemption { pub id: DiagnosticId, pub reason: &'static str }` and `pub const EXECUTABLE_EXAMPLE_EXEMPTIONS: &[ExampleExemption]` (exported from `celerrate_diagnostics`); the interim `const fn documented(id, family, owner, explain: &'static ExplainPage) -> RegisteredDiagnostic` in `registry.rs`; `pub(crate) const CEL0018: ExplainPage` in `pages/semantic.rs`; the harness test file `explain_pages.rs` whose `every_written_page_example_is_honest` test all later page tasks run against.

- [ ] **Step 1: Write the failing exemption-shape tests**

Append to the `tests` module in `crates/celerrate_diagnostics/src/explain.rs` (the module exists at the bottom of the file):

```rust
    #[test]
    fn every_exemption_names_a_registered_identifier_and_a_reason() {
        use crate::{EXECUTABLE_EXAMPLE_EXEMPTIONS, find_identifier};
        for exemption in EXECUTABLE_EXAMPLE_EXEMPTIONS {
            assert!(
                find_identifier(exemption.id.as_str()).is_some(),
                "{} is exempt but not registered",
                exemption.id.as_str(),
            );
            assert!(
                !exemption.reason.trim().is_empty(),
                "{} is exempt without a reason",
                exemption.id.as_str(),
            );
        }
    }

    #[test]
    fn exemptions_are_sorted_and_unique() {
        use crate::EXECUTABLE_EXAMPLE_EXEMPTIONS;
        let identifiers: Vec<&str> = EXECUTABLE_EXAMPLE_EXEMPTIONS
            .iter()
            .map(|exemption| exemption.id.as_str())
            .collect();
        let mut sorted = identifiers.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(identifiers, sorted, "keep the exemption list sorted and unique");
    }
```

- [ ] **Step 2: Run and verify the tests fail to compile**

Run: `cargo test -p celerrate_diagnostics`
Expected: compile error — `EXECUTABLE_EXAMPLE_EXEMPTIONS` not found.

- [ ] **Step 3: Declare the exemption vocabulary**

In `crates/celerrate_diagnostics/src/explain.rs`, above the tests module, add (and add `use crate::identifier::DiagnosticId;` at the top of the file):

```rust
/// One identifier whose executable example is waived. The page itself
/// stays mandatory; only the harness execution is skipped, and the
/// reason is part of the declaration so a waiver is reviewable, never
/// an accident (spec section 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExampleExemption {
    pub id: DiagnosticId,
    pub reason: &'static str,
}

/// The declared exemption list, in identifier order. Grown only by
/// the page tasks that justify each entry; the closing spec amendment
/// records the final contents.
pub const EXECUTABLE_EXAMPLE_EXEMPTIONS: &[ExampleExemption] = &[];
```

In `crates/celerrate_diagnostics/src/lib.rs`, extend the explain re-export to `pub use explain::{ExampleExemption, ExplainPage};` and re-export the list alongside (match the file's existing re-export style; `EXECUTABLE_EXAMPLE_EXEMPTIONS` comes from `explain`).

Run: `cargo test -p celerrate_diagnostics` — Expected: PASS (both new tests are vacuous on an empty list; that is fine, they bite from Task 2 on).

- [ ] **Step 4: Write the CEL0018 page and wire it**

Create `crates/celerrate_diagnostics/src/pages/mod.rs`:

```rust
//! The embedded explain pages, one module per producing area. Each
//! page is a `const` the registry references; the executable-page
//! harness at the composition root (`celerrate_cli/tests/
//! explain_pages.rs`) keeps every non-exempt example honest.

pub(crate) mod semantic;
```

Create `crates/celerrate_diagnostics/src/pages/semantic.rs`:

```rust
//! Pages for the semantic families: unknown symbols (CEL0018 to
//! CEL0020) and version gating (CEL0021 to CEL0024).

use crate::explain::ExplainPage;

pub(crate) const CEL0018: ExplainPage = ExplainPage {
    why: "\
The referenced class does not exist under any name the project can
resolve: it is neither declared in the project, nor autoloadable
through Composer, nor part of the PHP distribution for the supported
version range. At runtime the reference throws an `Error` (class not
found), so the code path cannot execute at all.",
    failing_example: "\
<?php
namespace App;

function f(): void { $x = new MissingService(); }
",
    fixed_example: "\
<?php
namespace App;

class MissingService {}

function f(): void { $x = new MissingService(); }
",
    configuration: "\
Reported by the `unknown-symbols` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0018 (reason)` on or above the line.",
};
```

In `crates/celerrate_diagnostics/src/lib.rs` add `mod pages;` next to the other module declarations.

In `crates/celerrate_diagnostics/src/registry.rs` add the interim constructor below `registered()` (add `#[allow(dead_code)]` nowhere — it is used immediately):

```rust
/// The interim constructor while pages land family by family; Task 6
/// of part 8 deletes `registered`, makes the field mandatory, and
/// renames this back to `registered`.
const fn documented(
    id: &'static str,
    family: &'static str,
    owner: &'static str,
    explain: &'static ExplainPage,
) -> RegisteredDiagnostic {
    RegisteredDiagnostic {
        id: DiagnosticId::new(id),
        family,
        owner,
        explain: Some(explain),
    }
}
```

Add `use crate::pages;` at the top and change the CEL0018 entry:

```rust
    documented(
        "CEL0018",
        "unknown class",
        "celerrate_rules",
        &pages::semantic::CEL0018,
    ),
```

Replace the now-false test `no_page_is_registered_yet` in `explain.rs` with:

```rust
    #[test]
    fn a_written_page_is_found_and_an_unknown_identifier_has_none() {
        assert!(find_page(DiagnosticId::new("CEL0018")).is_some());
        assert!(find_page(DiagnosticId::new("CEL9999")).is_none());
    }
```

Run: `cargo test -p celerrate_diagnostics` — Expected: PASS.

- [ ] **Step 5: Write the harness (failing first against a deliberately broken page is not needed — the harness IS the red/green loop for every later page)**

Create `crates/celerrate_cli/tests/explain_pages.rs`:

```rust
//! The executable explain-page harness (design section 10): every
//! written page's failing example must fire its identifier through
//! the full product pipeline, and its fixed example must not.
//! Identifiers on the declared exemption list keep the page
//! requirement but waive execution. An explain page outside the
//! exemption can neither lie nor rot.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use celerrate_cli::{ColorMode, run};
use celerrate_diagnostics::{EXECUTABLE_EXAMPLE_EXEMPTIONS, REGISTRY};

/// The manifest wrapped around a plain-snippet example. Pages that
/// need another PHP range or another file set carry their own files
/// through `//// ` markers (see `fixture_files`).
const DEFAULT_MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

/// Splits an example into its fixture files. A line starting with
/// `//// ` opens a new file at the path that follows; with markers
/// the file set is exactly what the example declares (so a fixture
/// without `composer.json` is expressible). Without markers the
/// whole example is one `src/Example.php` plus the default manifest.
fn fixture_files(example: &str) -> Vec<(String, String)> {
    if !example.lines().any(|line| line.starts_with("//// ")) {
        return vec![
            ("composer.json".to_string(), DEFAULT_MANIFEST.to_string()),
            ("src/Example.php".to_string(), example.to_string()),
        ];
    }
    let mut files: Vec<(String, String)> = Vec::new();
    for line in example.lines() {
        if let Some(path) = line.strip_prefix("//// ") {
            files.push((path.trim().to_string(), String::new()));
        } else if let Some((_, contents)) = files.last_mut() {
            contents.push_str(line);
            contents.push('\n');
        } else {
            panic!("example text before the first `//// ` marker: {line}");
        }
    }
    files
}

/// Runs `celerrate check` over the example's fixture and returns the
/// full plain-color report.
fn report_for(example: &str) -> String {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in fixture_files(example) {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    let mut output = Vec::new();
    run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.path().as_os_str().into(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    String::from_utf8(output).unwrap()
}

#[test]
fn every_written_page_example_is_honest() {
    let mut failures = Vec::new();
    for entry in REGISTRY {
        // Task 6 of part 8 makes the field mandatory; this binding
        // then becomes `let page = entry.explain;`.
        let Some(page) = entry.explain else { continue };
        if EXECUTABLE_EXAMPLE_EXEMPTIONS
            .iter()
            .any(|exemption| exemption.id == entry.id)
        {
            continue;
        }
        let identifier = entry.id.as_str();
        let failing = report_for(page.failing_example);
        if !failing.contains(identifier) {
            failures.push(format!(
                "{identifier}: the failing example does not fire it:\n{failing}"
            ));
        }
        let fixed = report_for(page.fixed_example);
        if fixed.contains(identifier) {
            failures.push(format!(
                "{identifier}: the fixed example still fires it:\n{fixed}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}

/// The spec's harness forces nursery rules active before running
/// their pages; no nursery rule exists, so that machinery does not
/// either. This guard fails the moment the first nursery rule lands,
/// naming the extension required.
#[test]
fn every_core_rule_is_default_tier_so_the_default_active_set_covers_all_pages() {
    for (metadata, _) in celerrate_rules::core_rules() {
        assert_eq!(
            metadata.tier,
            celerrate_rules::Tier::Default,
            "rule `{}` is outside the default active set; teach \
             explain_pages.rs to force it active before its \
             identifiers' pages can stay executable",
            metadata.name,
        );
    }
}
```

If `celerrate_rules` is not yet a dev-visible dependency of `celerrate_cli` for tests, it is already a regular dependency (the CLI is the composition root) — no Cargo.toml change expected. Check `core_rules()`'s exact return type in `crates/celerrate_rules/src/rules/mod.rs` and destructure accordingly (the composition root at `crates/celerrate_cli/src/plugins.rs:151` maps over `(metadata, implementation)` pairs).

- [ ] **Step 6: Run the harness**

Run: `cargo test -p celerrate_cli --test explain_pages`
Expected: PASS — `every_written_page_example_is_honest` exercises CEL0018's two examples through the full pipeline (failing fires, fixed does not), and the tier guard passes because all eight core rules are `Default`.

If the CEL0018 examples misbehave, fix the page content, not the harness assertions.

- [ ] **Step 7: Workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: all green. (`cargo test -p celerrate_diagnostics` covers the changed registry tests; the full run catches anything the wiring broke.)

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_diagnostics crates/celerrate_cli/tests/explain_pages.rs
git commit -m "✨ feat(diagnostics): add the executable explain-page harness and the CEL0018 page"
```

---

### Task 2: Pages for the semantic and version-gating families (CEL0019–CEL0024)

**Files:**
- Modify: `crates/celerrate_diagnostics/src/pages/semantic.rs`
- Modify: `crates/celerrate_diagnostics/src/registry.rs` (six entries move to `documented(...)`)
- Modify: `crates/celerrate_diagnostics/src/explain.rs` (CEL0022 exemption entry)

**Interfaces:**
- Consumes: `documented(...)`, `ExplainPage`, the harness from Task 1.
- Produces: `pub(crate) const CEL0019 … CEL0024: ExplainPage` in `pages/semantic.rs`; the CEL0022 entry in `EXECUTABLE_EXAMPLE_EXEMPTIONS`.

- [ ] **Step 1: Write the six pages**

Follow the CEL0018 shape exactly: `why` states the runtime consequence in two to four sentences; `failing_example` is the executable trigger; `fixed_example` is the same code corrected; `configuration` names the owning rule, group, tier, the severity, and the native suppression form. The failing examples come from the seeded-defect suite (`crates/celerrate_cli/tests/seeded_defects.rs`) — copy them verbatim, they are proven to fire:

| Identifier | Failing example (source) | Fixed example | Configuration line names |
| --- | --- | --- | --- |
| CEL0019 unknown function | `seeded_defects.rs` CEL0019 fixture (`missing_helper();`) | declare `function missing_helper(): void {}` above the call | `unknown-symbols`, error |
| CEL0020 unknown constant | CEL0020 fixture (`return MISSING_LIMIT;`) | add `const MISSING_LIMIT = 10;` | `unknown-symbols`, error |
| CEL0021 symbol not available | CEL0021 fixture (`\json_validate('{}')` under the default `^8.1` manifest; introduced in 8.3) | marker fixture raising the floor: `//// composer.json` with `{"require": {"php": "^8.3"}, "autoload": {"psr-4": {"App\\": "src/"}}}` + `//// src/Example.php` with the same call | `symbol-version-gating`, error |
| CEL0022 symbol removed | authored, not executed (exempt): e.g. `\each($array)` under a manifest whose range spans the removal in PHP 8.0 | the `foreach` equivalent | `symbol-version-gating`, error |
| CEL0023 symbol deprecated | CEL0023 fixture (`\utf8_encode('x')`, deprecated 8.2) | `\mb_convert_encoding('x', 'UTF-8', 'ISO-8859-1');` | `symbol-version-gating`, warning |
| CEL0024 syntax construct not available | CEL0024 fixture (`readonly class Point {}` under `^8.1`; readonly classes are 8.2) | marker fixture raising the floor to `"php": "^8.2"` with the same class | `syntax-version-gating`, error |

For CEL0021's and CEL0024's `why`, state the version fact used by the example (for example "`json_validate` exists only from PHP 8.3; the manifest's `^8.1` admits versions where the call fails at runtime with an undefined-function error").

- [ ] **Step 2: Wire the registry and declare the CEL0022 exemption**

Move the CEL0019–CEL0024 registry entries from `registered(...)` to `documented(..., &pages::semantic::CEL00XX)`.

In `explain.rs`, the exemption list gains its first entry:

```rust
pub const EXECUTABLE_EXAMPLE_EXEMPTIONS: &[ExampleExemption] = &[ExampleExemption {
    id: DiagnosticId::new("CEL0022"),
    reason: "the shipped stub blob carries no symbol whose removal falls \
             inside the supported 8.1 to 8.5 window; the framework-path \
             fixture in celerrate_rules covers recall (same waiver as the \
             seeded-defect suite)",
}];
```

- [ ] **Step 3: Run the harness and iterate until every example is honest**

Run: `cargo test -p celerrate_cli --test explain_pages`
Expected: PASS. A failure names the identifier and prints the report it got — fix the example (or, for a fixed example that still fires, correct the code further), never the assertion.

Also run: `cargo test -p celerrate_diagnostics`
Expected: PASS (the exemption-shape tests now bite on the CEL0022 entry).

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_diagnostics
git commit -m "✨ feat(diagnostics): write the explain pages for the semantic and version-gating families"
```

---

### Task 3: Pages for the typed families (CEL0030–CEL0038)

**Files:**
- Create: `crates/celerrate_diagnostics/src/pages/typed.rs` (add `pub(crate) mod typed;` to `pages/mod.rs`)
- Modify: `crates/celerrate_diagnostics/src/registry.rs` (nine entries)

**Interfaces:**
- Consumes: `documented(...)`, the harness.
- Produces: `pub(crate) const CEL0030 … CEL0038: ExplainPage` in `pages/typed.rs`.

- [ ] **Step 1: Write the nine pages**

All nine failing examples exist, proven, in `crates/celerrate_cli/tests/seeded_defects.rs` — copy them verbatim. One page written out in full as the module's exemplar:

```rust
pub(crate) const CEL0030: ExplainPage = ExplainPage {
    why: "\
The receiver's inferred type declares no such method, in the project,
its ancestors, or the stubs for the supported PHP range. At runtime
the call throws an `Error` (call to undefined method) unless a magic
`__call` intercepts it; classes with magic methods are already
exempted conservatively, so what remains is a genuine typo or a
renamed member.",
    failing_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(User $u): void { $u->svae(); }
",
    fixed_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(User $u): void { $u->save(); }
",
    configuration: "\
Reported by the `unknown-members` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0030 (reason)` on or above the line.",
};
```

The remaining eight follow the same shape:

| Identifier | Failing example source | Fixed example | Rule, severity |
| --- | --- | --- | --- |
| CEL0031 unknown property | seeded fixture (`$u->nmae`) | `$u->name` | `unknown-members`, error |
| CEL0032 unknown class constant | seeded fixture (`Config::LIMTI`) | `Config::LIMIT` | `unknown-members`, error |
| CEL0033 unknown enum case | seeded fixture (`Status::Draft`) | `Status::Active` | `unknown-members`, error |
| CEL0034 possibly null dereference | seeded fixture (`?User` receiver, unguarded `$u->save()`) | guard first: `if ($u !== null) { $u->save(); }` | `null-dereference`, error |
| CEL0035 argument type mismatch | seeded fixture (`takes($p)` where `takes(int $n)`) | pass an `int` | `argument-checks`, error |
| CEL0036 too few arguments | seeded fixture (`pair(1)`) | `pair(1, 2)` | `argument-checks`, error |
| CEL0037 too many arguments | seeded fixture (`single(1, 2)`) | `single(1)` | `argument-checks`, error |
| CEL0038 unknown named argument | seeded fixture (`single(b: 1)`) | `single(a: 1)` | `argument-checks`, error |

Each `why` states the runtime consequence (undefined property reads yield a warning and `null` since PHP 8.0 becomes an `Error` for typed contexts, `TypeError`, `ArgumentCountError`, unknown named argument throws `Error`) — one honest sentence per page, verified against the PHP semantics the rule actually checks; when in doubt about a phrasing, check the rule's own doc comments in `crates/celerrate_rules/src/rules/`.

- [ ] **Step 2: Wire the registry**

Move CEL0030–CEL0038 to `documented(..., &pages::typed::CEL00XX)`.

- [ ] **Step 3: Run the harness and iterate**

Run: `cargo test -p celerrate_cli --test explain_pages` — Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_diagnostics
git commit -m "✨ feat(diagnostics): write the explain pages for the typed families"
```

---

### Task 4: Pages for the reporting rules and the project notices (CEL0041, CEL0042, CEL0025–CEL0029, CEL0039, CEL0040)

**Files:**
- Create: `crates/celerrate_diagnostics/src/pages/reporting.rs`, `crates/celerrate_diagnostics/src/pages/project.rs` (register both in `pages/mod.rs`)
- Modify: `crates/celerrate_diagnostics/src/registry.rs` (nine entries)
- Modify: `crates/celerrate_diagnostics/src/explain.rs` (CEL0039, CEL0040 exemptions)

**Interfaces:**
- Consumes: `documented(...)`, the harness, the marker fixture format from Task 1.
- Produces: `pub(crate) const CEL0041, CEL0042` in `pages/reporting.rs`; `CEL0025 … CEL0029, CEL0039, CEL0040` in `pages/project.rs`; two new exemption entries.

- [ ] **Step 1: Write the two reporting pages**

```rust
pub(crate) const CEL0041: ExplainPage = ExplainPage {
    why: "\
A suppression directive naming an identifier the tool does not know
suppresses nothing: a typo in a CEL code would otherwise silently
leave the directive inert while looking intentional. A known but
currently inactive identifier is not unknown.",
    failing_example: "\
<?php
namespace App;

// @celerrate-ignore CEL9999
function f(): void {}
",
    fixed_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(User $u): void {
    // @celerrate-ignore CEL0030 (renamed upstream, fix scheduled)
    $u->svae();
}
",
    configuration: "\
Reported by the `unknown-suppression-identifier` rule (correctness
group, default tier) as a warning, on native `@celerrate-ignore`
directives only; foreign directives (PHPStan, Psalm) legitimately
name identifiers Celerrate does not emit.",
};
```

CEL0042 (unused suppression): failing example is a native directive whose next line reports nothing:

```rust
    failing_example: "\
<?php
namespace App;

// @celerrate-ignore CEL0030
function f(): void {}
",
```

with the same fixed example as CEL0041 (a directive that suppresses a real finding); its `why` states the drift hazard (a suppression that outlives its finding hides the next real one at that site) and its `configuration` notes the two exemptions the rule itself carries: directives naming any identifier of an inactive rule are not evaluable, and suppressing CEL0042 itself counts as use.

- [ ] **Step 2: Write the seven project pages**

Project notices are spanless, exit-code-neutral, and render under the notice vocabulary — each `why` must say what the tool falls back to, since that is the finding's real content. Marker fixtures throughout. Confirm exact minimal contents against `crates/celerrate_project/src/notice.rs` (message texts at lines 84–114) and the project fixtures in `crates/celerrate_project` tests; the harness is the oracle.

```rust
pub(crate) const CEL0025: ExplainPage = ExplainPage {
    why: "\
No `composer.json` was found at the project root, so autoload
mappings and the PHP version constraint are unknown. Analysis
continues over the whole root with the default supported PHP range,
which is broader than what the project actually targets: version
gating loses precision until a manifest exists.",
    failing_example: "\
//// src/Example.php
<?php
namespace App;

function f(): void {}
",
    fixed_example: "\
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function f(): void {}
",
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};
```

(Watch the escaping: inside a normal Rust string the manifest's `App\\` needs `App\\\\` and each quote a backslash. Using a raw string for the whole example is not possible with markers containing quotes — it is: `r#"…"#` handles quotes fine; prefer raw strings and only escape when the content itself contains `"#`.)

The rest:

| Identifier | Failing fixture (markers) | Fixed fixture | Notes |
| --- | --- | --- | --- |
| CEL0026 invalid manifest | `composer.json` containing `[]` (valid JSON, not an object) + a `src/Example.php` | proper object manifest | falls back to defaults; say so in `why` |
| CEL0027 PHP version fallback | manifest without a `require.php` entry: `{"autoload": {"psr-4": {"App\\": "src/"}}}` | add `"require": {"php": "^8.1"}` | `why`: the default range is broader than the project's real floor |
| CEL0028 invalid PHP version constraint | manifest with `"php": "not-a-constraint"` | `"php": "^8.1"` | |
| CEL0029 invalid installed packages | valid manifest + `vendor/composer/installed.json` containing `not json` | same fixture with a minimal valid `installed.json` (check the minimal accepted shape in `crates/celerrate_project` tests) | `why`: vendor autoload is skipped |
| CEL0039 unreadable manifest | authored, not executed (exempt) — narrative example: a `composer.json` whose permissions deny reading | readable manifest | exempt: environment |
| CEL0040 unreadable installed packages | authored, not executed (exempt) — same, for `vendor/composer/installed.json` | readable file | exempt: environment |

- [ ] **Step 3: Declare the two environment exemptions**

Extend `EXECUTABLE_EXAMPLE_EXEMPTIONS` (keep identifier order: CEL0022 first, then CEL0039, CEL0040):

```rust
    ExampleExemption {
        id: DiagnosticId::new("CEL0039"),
        reason: "fires on a permission-based IO error, which cannot be \
                 committed as a fixture and does not reproduce under root \
                 or on Windows CI (spec section 10's environment class)",
    },
    ExampleExemption {
        id: DiagnosticId::new("CEL0040"),
        reason: "fires on a permission-based IO error, which cannot be \
                 committed as a fixture and does not reproduce under root \
                 or on Windows CI (spec section 10's environment class)",
    },
```

- [ ] **Step 4: Wire the registry, run the harness, iterate**

Move the nine entries to `documented(...)`.

Run: `cargo test -p celerrate_cli --test explain_pages && cargo test -p celerrate_diagnostics`
Expected: PASS. The CEL0041/CEL0042 fixed example must come out clean of their own identifiers (the directive is used, so CEL0042 stays silent; the identifier is known, so CEL0041 stays silent).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_diagnostics
git commit -m "✨ feat(diagnostics): write the explain pages for the reporting rules and project notices"
```

---

### Task 5: Pages for the source and syntax resilience families (CEL0001–CEL0017)

**Files:**
- Create: `crates/celerrate_diagnostics/src/pages/source.rs`, `crates/celerrate_diagnostics/src/pages/syntax.rs` (register both in `pages/mod.rs`)
- Modify: `crates/celerrate_diagnostics/src/registry.rs` (seventeen entries)
- Modify: `crates/celerrate_diagnostics/src/explain.rs` (CEL0001 exemption; CEL0013 only if the probe demands it)

**Interfaces:**
- Consumes: `documented(...)`, the harness.
- Produces: `pub(crate) const CEL0001` in `pages/source.rs`; `CEL0002 … CEL0017` in `pages/syntax.rs`.

- [ ] **Step 1: Probe CEL0013 reachability**

Run: `grep -rn "CEL0013\|no progress" crates/celerrate_syntax/src crates/celerrate_syntax/tests`
If a unit test triggers it from source text, reuse that source as the failing example. If it is a defensive backstop no grammar-admitted input reaches, add the exemption entry (identifier order: before CEL0022):

```rust
    ExampleExemption {
        id: DiagnosticId::new("CEL0013"),
        reason: "the parser's no-progress guard is a defensive backstop \
                 that no grammar-admitted source reaches; the day a \
                 reproduction exists it becomes this page's example",
    },
```

Record the probe's outcome in the task's commit message body (one line).

- [ ] **Step 2: Write the CEL0001 page (exempt) and its exemption**

`pages/source.rs`: the `why` states the 4 GiB decoded-size engine cap and that the file is skipped with this diagnostic rather than analyzed partially; the examples are narrative (a comment describing a generated blob committed as PHP), since a 4 GiB fixture cannot be committed. Exemption entry (first in the list):

```rust
    ExampleExemption {
        id: DiagnosticId::new("CEL0001"),
        reason: "fires only on a file whose decoded size exceeds 4 GiB, \
                 which cannot be committed as a fixture (spec section \
                 10's environment class)",
    },
```

- [ ] **Step 3: Write the sixteen syntax pages**

Every page's `configuration` uses the resilience template: "Produced by the parser's error resilience in `celerrate_syntax`, not by a rule: it cannot be disabled, and analysis continues past the recovered region." The `why` explains what the parser expected and what it recovered to. Failing snippets below are starting points — **the producing crate's own tests are the authority**: for each identifier run `grep -rn "CEL00XX" crates/celerrate_syntax/src` and prefer a snippet mirroring the crate's own trigger; the harness then proves it.

| Identifier | Failing snippet (starting point) | Fixed snippet |
| --- | --- | --- |
| CEL0002 unexpected character | `<?php $x = 1 § 2;` | `<?php $x = 1 + 2;` |
| CEL0003 unterminated block comment | `<?php /* never closed` | close the comment |
| CEL0004 unterminated string | `<?php $s = 'never closed;` | close the quote |
| CEL0005 unterminated heredoc | `<?php $s = <<<TEXT` + newline + `never closed` | add the `TEXT;` terminator |
| CEL0006 unterminated interpolation | `<?php $s = "{$x";` (open `{$` at end of string) | `"{$x}"` |
| CEL0007 expected an expression | `<?php $x = ;` | `<?php $x = 1;` |
| CEL0008 expected a semicolon | `<?php $a = 1 $b = 2;` | add the semicolon |
| CEL0009 expected a specific token | `<?php function f(int $a { }` (missing `)`) | close the parameter list |
| CEL0010 unexpected token | `<?php class C {} }` | drop the stray `}` |
| CEL0011 nesting too deep | an expression nested one past the parser's cap: read the cap constant (`grep -rn "nesting" crates/celerrate_syntax/src`) and write the literal `<?php $x = ((((…1…))));` at cap + 1 | the same expression within the cap |
| CEL0012 non-associative operator chained | `<?php $x = 1 < 2 < 3;` | `<?php $x = 1 < 2 && 2 < 3;` |
| CEL0013 no progress | Step 1's outcome | Step 1's outcome |
| CEL0014 expected a member name | `<?php $u->;` (with `$u` defined) | name the member |
| CEL0015 expected a statement | from the crate's own trigger test | corrected |
| CEL0016 expected a type | `<?php function f(): { }` | `<?php function f(): void { }` |
| CEL0017 expected a declaration | from the crate's own trigger test | corrected |

For CEL0011, the page's `why` must state the cap's actual value so the monstrous example explains itself; keep the literal on one line rather than pretending it is idiomatic code.

Note the fixed examples must avoid firing the page's own identifier but may not need to be diagnostic-free; still, prefer clean snippets (a parse error usually cascades).

- [ ] **Step 4: Wire the registry, run the harness, iterate**

Move CEL0001–CEL0017 to `documented(...)`.

Run: `cargo test -p celerrate_cli --test explain_pages && cargo test -p celerrate_diagnostics`
Expected: PASS. Syntax recovery can emit several identifiers per snippet; only the page's own identifier is asserted, so cascades are tolerated in failing examples but the fixed example must be free of that identifier.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_diagnostics
git commit -m "✨ feat(diagnostics): write the explain pages for the source and syntax resilience families"
```

(Include the CEL0013 probe outcome as a body line, for example: `CEL0013 is a defensive backstop; exempted with reason.`)

---

### Task 6: Make the explain page mandatory on every registry entry

**Files:**
- Modify: `crates/celerrate_diagnostics/src/registry.rs`, `crates/celerrate_diagnostics/src/explain.rs`
- Modify: `crates/celerrate_cli/tests/explain_pages.rs` (drop the `Option` binding)

**Interfaces:**
- Consumes: all 42 pages (Tasks 1–5).
- Produces: `RegisteredDiagnostic.explain: &'static ExplainPage` (no `Option`); `find_page(id) -> Option<&'static ExplainPage>` (None only for unknown identifiers); the single constructor named `registered(id, family, owner, explain)`.

- [ ] **Step 1: Write the failing content-gate test**

In `crates/celerrate_diagnostics/src/registry.rs` tests, add (it fails to compile until the flip, which is the point):

```rust
    #[test]
    fn every_identifier_has_a_page_with_all_four_sections() {
        for entry in REGISTRY {
            let page = entry.explain;
            for (section, text) in [
                ("why", page.why),
                ("failing example", page.failing_example),
                ("fixed example", page.fixed_example),
                ("configuration", page.configuration),
            ] {
                assert!(
                    !text.trim().is_empty(),
                    "{} has an empty {section} section",
                    entry.id.as_str(),
                );
            }
        }
    }
```

Run: `cargo test -p celerrate_diagnostics` — Expected: compile error (`page.why` on an `Option`).

- [ ] **Step 2: Flip the field**

In `registry.rs`:
- `pub explain: &'static ExplainPage,` with the doc comment `/// The long-form explanation served by `celerrate explain`.` (drop the "Option only until part 8" note — part 8 is now).
- Delete the old three-argument `registered()` helper entirely (every entry now uses the four-argument form).
- Rename `documented` to `registered` (update its doc comment: it is no longer interim) and update all 42 call sites (mechanical rename).
- `find_page` becomes `.map(|entry| entry.explain)` instead of `.and_then(...)`; keep its doc: `None` now only means an unknown identifier.

In `explain.rs`: update the struct's module doc (the pages exist; the store is total) and the `failing_example` field doc (the harness exists — point at `celerrate_cli/tests/explain_pages.rs`).

In `crates/celerrate_cli/tests/explain_pages.rs`: replace `let Some(page) = entry.explain else { continue };` (and its Task-6 comment) with `let page = entry.explain;`.

- [ ] **Step 3: Run the full workspace**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS — in particular `every_identifier_has_a_page_with_all_four_sections` (the spec's mechanical page gate, now belt over the type-level suspenders) and the harness across all 42 entries minus exemptions.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_diagnostics crates/celerrate_cli/tests/explain_pages.rs
git commit -m "♻️ refactor(diagnostics): make the explain page mandatory on every registry entry"
```

---

### Task 7: The `celerrate explain` subcommand

**Files:**
- Modify: `crates/celerrate_cli/src/arguments.rs`
- Create: `crates/celerrate_cli/src/explain.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (module + dispatch arm)
- Create: `crates/celerrate_cli/tests/explain.rs`

**Interfaces:**
- Consumes: `celerrate_diagnostics::REGISTRY`, `RegisteredDiagnostic` (with the now-mandatory `explain`), `Outcome`, the `run` dispatch in `lib.rs:98`.
- Produces: `Command::Explain { identifier: String }`; `pub(crate) fn render_page(entry: &RegisteredDiagnostic, output: &mut dyn Write) -> io::Result<()>` in `crates/celerrate_cli/src/explain.rs`.

- [ ] **Step 1: Write the failing parse test**

In `crates/celerrate_cli/src/arguments.rs` tests (follow the existing test style at lines 60–141):

```rust
    #[test]
    fn explain_takes_an_identifier() {
        let arguments = Arguments::try_parse_from(["celerrate", "explain", "CEL0030"])
            .expect("explain must parse");
        match arguments.command {
            Command::Explain { identifier } => assert_eq!(identifier, "CEL0030"),
            other => panic!("expected explain, parsed {other:?}"),
        }
    }
```

(Adapt `expect`/`unwrap` usage to the module's existing allow-block.)

Run: `cargo test -p celerrate_cli --lib` — Expected: compile error, no `Explain` variant.

- [ ] **Step 2: Add the variant**

In the `Command` enum in `arguments.rs`, after `Check` (it is the second public subcommand; keep the hidden ones last):

```rust
    /// Explain a diagnostic identifier: why it fires, a failing and a
    /// fixed example, and its configuration notes.
    Explain {
        /// The identifier to explain, for example CEL0030.
        identifier: String,
    },
```

Run: `cargo test -p celerrate_cli --lib` — Expected: the parse test passes; the `run` dispatch now fails to compile with a non-exhaustive match — that is the next step's failing state.

- [ ] **Step 3: Implement the formatting module and the dispatch arm**

Create `crates/celerrate_cli/src/explain.rs`:

```rust
//! The `celerrate explain` subcommand: prints the embedded page for
//! one diagnostic identifier. Pure formatting over the registry — no
//! analysis session, no color, no environment reads.

use std::io::{self, Write};

use celerrate_diagnostics::RegisteredDiagnostic;

/// Renders `entry`'s page. Every registry entry carries a page (the
/// field is mandatory), so lookup failures cannot reach this function.
pub(crate) fn render_page(
    entry: &RegisteredDiagnostic,
    output: &mut dyn Write,
) -> io::Result<()> {
    let page = entry.explain;
    writeln!(output, "{}: {}", entry.id.as_str(), entry.family)?;
    writeln!(output)?;
    writeln!(output, "{}", page.why.trim_end())?;
    writeln!(output)?;
    writeln!(output, "failing example:")?;
    writeln!(output)?;
    write_indented(page.failing_example, output)?;
    writeln!(output)?;
    writeln!(output, "fixed example:")?;
    writeln!(output)?;
    write_indented(page.fixed_example, output)?;
    writeln!(output)?;
    writeln!(output, "{}", page.configuration.trim_end())?;
    Ok(())
}

fn write_indented(text: &str, output: &mut dyn Write) -> io::Result<()> {
    for line in text.trim_end().lines() {
        if line.is_empty() {
            writeln!(output)?;
        } else {
            writeln!(output, "    {line}")?;
        }
    }
    Ok(())
}
```

In `crates/celerrate_cli/src/lib.rs`: add `mod explain;` next to the other modules, and the dispatch arm in the `match arguments.command` block (template: the `GroundTruth` arm at `lib.rs:149`):

```rust
        Command::Explain { identifier } => {
            let normalized = identifier.to_ascii_uppercase();
            match celerrate_diagnostics::REGISTRY
                .iter()
                .find(|entry| entry.id.as_str() == normalized)
            {
                Some(entry) => {
                    if explain::render_page(entry, output).is_err() {
                        return Outcome::InternalError;
                    }
                    Outcome::Clean
                }
                None => {
                    let _ = writeln!(
                        output,
                        "error: unknown diagnostic identifier `{identifier}`",
                    );
                    let _ = writeln!(
                        output,
                        "identifiers look like CEL0030; a report names the ones it uses",
                    );
                    Outcome::UsageError
                }
            }
        }
```

(If `celerrate_diagnostics` is not already imported in `lib.rs`, use the crate path directly as shown — it is a direct dependency.)

Run: `cargo test -p celerrate_cli --lib` — Expected: PASS.

- [ ] **Step 4: Write the integration tests**

Create `crates/celerrate_cli/tests/explain.rs`:

```rust
//! `celerrate explain` end to end: a known identifier prints its
//! page, lookup is case-insensitive, an unknown identifier is a
//! usage error, and every registered identifier renders.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use celerrate_cli::{ColorMode, Outcome, run};

fn explain(identifier: &str) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "explain".into(), identifier.into()],
        &mut output,
        ColorMode::Plain,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn a_known_identifier_prints_its_page() {
    let (outcome, output) = explain("CEL0018");
    assert_eq!(outcome, Outcome::Clean);
    insta::assert_snapshot!(output);
}

#[test]
fn lookup_is_case_insensitive() {
    let (outcome, output) = explain("cel0018");
    assert_eq!(outcome, Outcome::Clean);
    assert!(output.starts_with("CEL0018: unknown class"));
}

#[test]
fn an_unknown_identifier_is_a_usage_error() {
    let (outcome, output) = explain("CEL9999");
    assert_eq!(outcome, Outcome::UsageError);
    assert!(output.contains("unknown diagnostic identifier `CEL9999`"));
    assert!(output.contains("identifiers look like CEL0030"));
}

#[test]
fn every_registered_identifier_prints_a_page() {
    for entry in celerrate_diagnostics::REGISTRY {
        let (outcome, output) = explain(entry.id.as_str());
        assert_eq!(outcome, Outcome::Clean, "{}", entry.id.as_str());
        assert!(
            output.starts_with(entry.id.as_str()),
            "{} page must open with its identifier",
            entry.id.as_str(),
        );
    }
}
```

Run: `cargo test -p celerrate_cli --test explain`
Expected: the snapshot test writes a new `.snap.new` — review that the rendered CEL0018 page matches design decision 6 (header, sections, four-space indentation), then accept with `cargo insta accept` (or move the file per the repository's insta workflow). All four tests then PASS.

- [ ] **Step 5: Full workspace gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): add the celerrate explain subcommand"
```

---

### Task 8: Documentation and CHANGELOG

**Files:**
- Modify: `README.md`, `docs/diagnostics.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: the shipped subcommand (Task 7); the existing drift test `every_registered_identifier_is_documented` (`crates/celerrate_cli/tests/documentation.rs:24`) keeps guarding `docs/diagnostics.md`.
- Produces: user-facing documentation of `celerrate explain`.

- [ ] **Step 1: README**

After the rustc-style report sample (whose trailer already reads `for more information, run \`celerrate explain CEL0018\``), add a short block showing the pointer being followed:

```markdown
Every identifier ships its page in the binary:

​```console
$ celerrate explain CEL0018
CEL0018: unknown class

The referenced class does not exist under any name the project can
resolve: …
​```
```

Truncate the sample with `…` after the first `why` lines — the README shows the shape, the binary is the source of truth. Match the README's existing console-block style.

- [ ] **Step 2: docs/diagnostics.md**

Add one sentence in the introduction: "Every identifier below also ships an embedded page: `celerrate explain CEL0030` prints why it fires, a failing and a fixed example, and its configuration notes."

- [ ] **Step 3: CHANGELOG**

Under `## [Unreleased]` / `### Added`, following the existing entry style:

```markdown
- `celerrate explain CEL####`: a full-word subcommand printing the
  embedded page for any registered identifier: why it fires, a failing
  example, a fixed example, and configuration notes. Lookup is
  case-insensitive; an unknown identifier is a usage error (exit 2).
- An embedded explain page for every registered identifier (CEL0001 to
  CEL0042). The registry field is mandatory (an identifier cannot be
  allocated without a page), and an executable-page harness runs every
  failing and fixed example through the full product pipeline; the
  declared exemption list (with reasons) covers the identifiers whose
  trigger is an environment condition or outside the shipped stubs'
  window.
```

(Adjust the exemption sentence to the final list from Tasks 2, 4, 5 — name the identifiers.)

- [ ] **Step 4: Run the documentation drift tests**

Run: `cargo test -p celerrate_cli --test documentation`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/diagnostics.md CHANGELOG.md
git commit -m "📝 docs: document celerrate explain in the README, diagnostics guide, and changelog"
```

---

### Task 9: Closure — gates, spec amendments

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md`
- Modify: `.claude/superpowers/specs/2026-07-09-celerrate-design.md`

**Interfaces:**
- Consumes: everything; this task runs the sub-project's closure gates (spec section 1) and records the outcome. **No release, no tag, no version bump.**

- [ ] **Step 1: Run every mechanical gate**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
cargo xtask emission-scan
cargo xtask dependency-shape
```

Expected: all green; `corpus` byte-identical (`0 notices, 0 diagnostics`); `mixed-rate` baseline unchanged. Any delta is a defect of this part (nothing here touches analysis) — stop and fix, never bless.

- [ ] **Step 2: Verify the eight closure gates of spec section 1 against their evidence**

Walk the list and confirm each is green in the run above; this is the checklist the closing amendment cites:

1. Behavior preservation per migrated family — `crates/celerrate_cli/tests/seeded_defects.rs` green, corpus snapshot byte-identical.
2. Explain page for every identifier with the declared exemption class — the mandatory field (Task 6), `every_identifier_has_a_page_with_all_four_sections`, the executable harness, `EXECUTABLE_EXAMPLE_EXEMPTIONS` with reasons.
3. No check family outside the framework — `crates/celerrate_cli/tests/registry.rs` + `cargo xtask emission-scan`.
4. Correspondence-table triage — the part 5 triage suite in `celerrate_phpdoc_bridge` (both dialects' catalogues mapped or explicitly unmapped) green under `cargo test --workspace`.
5. Rendering snapshot suite including the fault-injected fallback — the part 7 snapshots green.
6. Natural fixes wired through `--fix-suggestions` — the part 6 suite green.
7. Warm/cold equivalence extended to the `Reporting` phase — the part 5 harness green.
8. Mixed-rate baseline unchanged — `cargo xtask mixed-rate`.

If any gate is not actually covered by an existing green suite, that is a finding to fix before closure, not to paper over.

- [ ] **Step 3: Amend the sub-project spec**

In `.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md`:

- Change line 4 to: `Status: Closed (2026-07-23; all eight parts landed, closure gates verified; no release — the next public event is v0.1, sub-project 5)`
- Append to the amendment-history block a dated entry recording: part 4 landed without a committed plan document (PR #95); the final executable-example exemption list with each reason (CEL0001, CEL0022, CEL0039, CEL0040, and CEL0013 if Task 5 exempted it); the forced-active provision shipped as a tier guard rather than machinery, with the guard's location; the eight closure gates and where each is enforced.

- [ ] **Step 4: Amend the parent spec**

In `.claude/superpowers/specs/2026-07-09-celerrate-design.md`, append one bullet to the amendment history:

```markdown
- 2026-07-23 — sub-project 4 (diagnostics and fixes) closed: the rule
  framework with four phase traits and sealed contexts, the
  structured-edit library, identifier-level suppression with the
  native directive, the autofix engine, rustc-style rendering, and
  `celerrate explain` with an executable page per identifier. No
  version tagged; the next public event is v0.1 at the end of
  sub-project 5.
```

Also update the `Date:` line's amended date on line 3 if the file's convention does so for each amendment (follow the existing entries).

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/specs
git commit -m "📝 docs(specs): close the diagnostics-and-fixes sub-project"
```

- [ ] **Step 6: Hand off the branch**

The branch `feat-diagnostics-explain-8` is complete: use superpowers:finishing-a-development-branch (house pattern: PR to `main`, as parts 1–7 did).

---

## Spec coverage self-check (sections 10, 1, 12.8 → tasks)

| Spec requirement | Task |
| --- | --- |
| Full-word subcommand, content embedded in the binary (§10) | 7 |
| Page carries why, failing example, fixed example, configuration notes, owning rule (§10) | 1–5 (content), 6 (mandatory), 7 (rendered) |
| `RegisteredDiagnostic` gains a page pointer; composition-root test requires a page for every identifier, resilience included (§10) | field existed; 6 makes it mandatory + content gate |
| Executable pages: failing example fires, fixed does not, full registered set (§10) | 1 (harness), 2–5 (pages) |
| Rule under test forced active (§10) | 1 (tier guard, design decision 5) |
| Fixtures pin their PHP range via their own composer.json; project-level identifiers use a project-shaped fixture (§10) | 1 (marker format), 2/4 (uses) |
| Environment-condition exemption list, mechanically declared; page still required (§10) | 1 (declaration), 2/4/5 (entries) |
| ~40 pages written as a named workstream (§10) | 2, 3, 4, 5 |
| Explain discoverable from the report output (§9) | landed in part 7; pointed-at command ships in 7 |
| Closure gates verified (§1) | 9 |
| Spec and CHANGELOG updates at closure; no release (§1, §12.8) | 8, 9 |

Out of scope, restated: plugin-side identifier allocation and plugin-registered rules (deferred, shape pinned in §8); `celerrate.toml`, baseline, JSON/SARIF/GitHub formats, `migrate --from-phpstan` (sub-project 5); the style group, WASM host, formatter (later sub-projects); any version tag or release (the next public event is v0.1).
