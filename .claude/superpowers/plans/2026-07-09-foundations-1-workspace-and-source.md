# Foundations Part 1: Workspace, Repository Hygiene, Source Crate — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Cargo workspace that compiles under the zero-panic lint policy, a repository carrying every open-source hygiene file from the spec, CI green from day 1, and the first real crate (`celerrate_source`) built with TDD.

**Architecture:** Cargo workspace with strict layering (spec section 3); `celerrate_source` is the bottom layer (source text primitives: spans re-exported from `text-size`, line/column index). Repository standards per spec section 10. The zero-panic policy (spec section 8) is enforced through workspace-level lints, verified to actually fire.

**Tech Stack:** Rust (edition 2024), `text-size` crate, GitHub Actions, cargo-deny.

## Global Constraints

- Zero-panic policy: `unsafe_code = "forbid"`; Clippy `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` at **deny** (spec section 8). Test modules may locally `#[allow]` these.
- Dual license: `MIT OR Apache-2.0`, copyright JDevelop (spec section 1).
- All files written in English. No abbreviated names (full words; standard acronyms fine).
- Commits: gitmoji + Conventional Commits (`<emoji> <type>(<scope>): <summary>`). No AI attribution lines.
- Commit identity: commits are authored with the GitHub noreply email. Before the first commit, `git config user.email` must print `5817251+jh3ady@users.noreply.github.com`; if it does not, set it for this repository only: `git config user.email "5817251+jh3ady@users.noreply.github.com"`.
- Public contact email (code of conduct, security policy): `contact@jdevelop.io`.
- TDD for all Rust code: failing test first, minimal implementation, pass, commit.
- The GitHub repository URL is not yet known: no badges or absolute repository links in this plan's files; they are added at publication (see the publication checklist at the end of this plan).
- Network access is required by Task 2 (`cargo deny check` downloads the RustSec advisory database) and Tasks 3-4 (`curl` to apache.org and contributor-covenant.org).
- Deliberate deferrals: the source-file representation (file identifiers, loading, encoding handling) belongs to `celerrate_source` per spec section 3 but is deferred to the Foundations Part 2 plan; continuous fuzzing (spec section 8) is deferred until the parser exists (sub-project 1).

---

### Task 1: Cargo workspace bootstrap with enforced zero-panic policy

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Create: `crates/celerrate_source/Cargo.toml`
- Create: `crates/celerrate_source/src/lib.rs`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: a building workspace; `crates/*` membership pattern; workspace lint table that every later crate inherits via `[lints] workspace = true`.

- [ ] **Step 1: Write the workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.94"
license = "MIT OR Apache-2.0"
authors = ["JDevelop"]

[workspace.dependencies]
text-size = "1"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
indexing_slicing = "deny"
panic = "deny"
```

- [ ] **Step 2: Write `rust-toolchain.toml` and `rustfmt.toml`**

`rust-toolchain.toml` (the version is pinned, per spec section 10 — `"stable"` would float with every release; bump it deliberately, together with `rust-version` in the workspace `Cargo.toml` and the `toolchain` inputs in the CI workflow):

```toml
[toolchain]
channel = "1.94"
components = ["clippy", "rustfmt"]
```

`rustfmt.toml`:

```toml
style_edition = "2024"
```

- [ ] **Step 3: Create the `celerrate_source` crate stub**

`crates/celerrate_source/Cargo.toml`:

```toml
[package]
name = "celerrate_source"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true

[lints]
workspace = true
```

`crates/celerrate_source/src/lib.rs`:

```rust
//! Source text primitives for the Celerrate toolchain: text sizes, ranges,
//! and line/column indexing. This is the bottom layer of the workspace:
//! it depends on no other Celerrate crate.
```

- [ ] **Step 4: Add build artifacts to `.gitignore`**

Append to the existing `.gitignore`:

```gitignore
# Rust build artifacts
/target/
```

- [ ] **Step 5: Verify the workspace builds**

Run: `cargo check --workspace`
Expected: `Finished` with no errors.

- [ ] **Step 6: Verify the lint policy actually fires**

Temporarily append to `crates/celerrate_source/src/lib.rs`:

```rust
pub fn lint_canary() -> u32 {
    "42".parse::<u32>().unwrap()
}
```

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: FAIL with ``error: used `unwrap()` on a `Result` value`` (denied by `clippy::unwrap_used`; the message quotes `unwrap()` and `Result` in backticks).

- [ ] **Step 7: Remove the canary and verify clean**

Delete the `lint_canary` function.

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: both PASS — exit 0, no warnings, no formatting diff (cargo still prints its usual `Checking`/`Finished` lines).

- [ ] **Step 8: Verify the commit identity and commit**

Run: `git config user.email`
Expected: `5817251+jh3ady@users.noreply.github.com`. If it prints anything else, run `git config user.email "5817251+jh3ady@users.noreply.github.com"` first (repository-local configuration).

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml crates/ .gitignore
git commit -m "🎉 feat: bootstrap Cargo workspace with zero-panic lint policy"
```

---

### Task 2: cargo-deny and the CI pipeline

**Files:**
- Create: `deny.toml`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the workspace from Task 1.
- Produces: CI jobs `lint`, `test`, `deny` that every later task must keep green.

- [ ] **Step 1: Install cargo-deny locally (if absent) and write `deny.toml`**

Run: `cargo deny --version || cargo install cargo-deny`

`deny.toml`:

```toml
[licenses]
allow = ["MIT", "Apache-2.0"]

[advisories]
yanked = "deny"

[bans]
multiple-versions = "warn"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

- [ ] **Step 2: Verify cargo-deny passes locally**

Run: `cargo deny check`
Expected: PASS (`advisories ok`, `bans ok`, `licenses ok`, `sources ok`) with no warnings. The advisories check downloads the RustSec database, so network access is required. If a transitive dependency uses another permissive license (for example `BSD-3-Clause` or `Unicode-3.0`), add that exact identifier to the `allow` list — never a copyleft license without an explicit decision, and never an identifier no dependency uses yet (cargo-deny warns about unmatched allowances).

- [ ] **Step 3: Write `.github/workflows/ci.yml`**

Actions are referenced by their latest major-version tag (current at the time of writing: checkout v7.0.0, rust-cache v2.9.1, cargo-deny-action v2.0.20). `dtolnay/rust-toolchain` is referenced by its `v1` tag, so the explicit `toolchain` input is required; keep it in sync with `rust-toolchain.toml`.

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

env:
  CARGO_TERM_COLOR: always

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace

  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: EmbarkStudios/cargo-deny-action@v2
```

- [ ] **Step 4: Validate the workflow syntax locally**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS (the workflow mirrors these exact commands; actual CI execution is verified on first push once the GitHub repository exists).

- [ ] **Step 5: Commit**

```bash
git add deny.toml .github/workflows/ci.yml
git commit -m "👷 ci: add lint, test, and cargo-deny pipeline"
```

---

### Task 3: Legal files and landing page

**Files:**
- Create: `LICENSE-MIT`
- Create: `LICENSE-APACHE`
- Create: `README.md`
- Create: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing.
- Produces: the dual-license files referenced by `Cargo.toml`'s `license` field and by the README's license section.

- [ ] **Step 1: Write `LICENSE-MIT`**

```text
MIT License

Copyright (c) 2026 JDevelop

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Fetch the canonical Apache-2.0 text**

Run: `curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE-APACHE`
Verify: `grep -q "Apache License" LICENSE-APACHE && grep -q "Version 2.0, January 2004" LICENSE-APACHE && echo OK`
Expected: `OK`. (The file's first line is blank; the centered title is on line 2 — do not check the first line.) Leave the appendix's `[yyyy] [name of copyright owner]` placeholders untouched: the appendix is application instructions, not the license grant, and leaving it pristine is the Rust-ecosystem convention.

- [ ] **Step 3: Write `README.md`**

```markdown
# Celerrate

An extremely fast, all-in-one toolchain for PHP, written in Rust.

> **Status: early development.** Celerrate is not yet usable. The design is
> settled, the engine is being built. Watch the repository to follow along.

## What is Celerrate?

Celerrate aims to replace the fragmented PHP tooling ecosystem with a single
coherent toolchain built on one engine:

- **`celerrate check`** — static analysis with interprocedural type
  inference and fine-grained incremental computation, plus lint, taint
  (security), and architecture rule groups. One command answers "is my code
  OK?".
- **`celerrate format`** — an opinionated, lossless formatter.
- **`celerrate lsp`** — a language server built on the same engine.
- **`celerrate migrate` / `celerrate generate`** — automated refactoring and
  semantic code generation.

## Why another tool?

- **Speed as a feature.** A Rust core, parallel by default, incremental by
  construction: full analyses in seconds, single-file updates in
  milliseconds.
- **Diagnostics that teach.** Rust-quality diagnostics: annotated spans,
  the engine's reasoning, concrete suggestions, and safe automatic fixes.
- **One engine, many tools.** Types, lint, security taint analysis, and
  architecture rules are rule groups over the same semantic model — not
  separate tools to install and configure.
- **Extensible by design.** First-party plugins in Rust, community plugins
  through a sandboxed WASM API, with extension points at every layer.

## Compatibility

Celerrate targets PHP 8.1+ projects. It defines its own type annotation
norm and ships a first-party PHPStan/Psalm syntax bridge, enabled by
default, so existing annotated codebases work on day 1.

## Roadmap

One pillar at a time, in this order:

1. **`celerrate check`** — the static analysis engine is the first public
   deliverable; the lint, taint, and architecture rule groups build on it.
2. **`celerrate format`** — the formatter, once the lossless syntax tree
   is proven by the analyzer.
3. **`celerrate lsp`** — the language server, reusing the same
   incremental engine.
4. **`celerrate migrate` / `celerrate generate`** — refactoring and code
   generation, last because they lean on everything above.

## License

Dual-licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option. "Celerrate" is a trademark of JDevelop.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
```

- [ ] **Step 4: Write `CHANGELOG.md`**

```markdown
# Changelog

All notable changes to Celerrate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cargo workspace with the zero-panic lint policy.
- `celerrate_source`: source text primitives (spans, line/column index).
```

- [ ] **Step 5: Commit**

```bash
git add LICENSE-MIT LICENSE-APACHE README.md CHANGELOG.md
git commit -m "📄 docs: add dual license, README, and changelog"
```

---

### Task 4: Community files, templates, and the repository CLAUDE.md

**Files:**
- Create: `CONTRIBUTING.md`
- Create: `CODE_OF_CONDUCT.md`
- Create: `SECURITY.md`
- Create: `.github/ISSUE_TEMPLATE/config.yml`
- Create: `.github/ISSUE_TEMPLATE/bug_report.yml`
- Create: `.github/ISSUE_TEMPLATE/false_positive.yml`
- Create: `.github/ISSUE_TEMPLATE/internal_error.yml`
- Create: `.github/ISSUE_TEMPLATE/rule_proposal.yml`
- Create: `.github/PULL_REQUEST_TEMPLATE.md`
- Create: `CLAUDE.md`

**Interfaces:**
- Consumes: commit conventions and lint commands defined in Tasks 1-2.
- Produces: the `internal_error.yml` template that the CLI crash reporter (future sub-project 5) links to.

- [ ] **Step 1: Write `CONTRIBUTING.md`**

```markdown
# Contributing to Celerrate

Thank you for considering a contribution. This document is the contract for
working on the codebase.

## Development setup

- Install Rust via [rustup](https://rustup.rs/); the pinned toolchain in
  `rust-toolchain.toml` is picked up automatically.
- `cargo test --workspace` — run the test suite.
- `cargo clippy --workspace --all-targets -- -D warnings` — lints (CI runs
  exactly this).
- `cargo fmt --all` — formatting.
- `cargo deny check` — dependency license and advisory audit.

## Engineering rules

- **Test-driven development is the expected workflow**: write the failing
  test first, then the minimal implementation, then refactor.
- **Zero panic**: `unwrap`, `expect`, `panic!`, and slice indexing are denied
  by Clippy in production code. Use `Result` and total functions. Test
  modules may locally `#[allow]` these lints.
- **No `unsafe`**: forbidden workspace-wide.
- **Strict layering**: a crate only depends on crates below it in the layer
  diagram (see `CLAUDE.md`).
- **A reported false positive is a priority bug, not an opinion.**

## Commit conventions

Commits use gitmoji + Conventional Commits:

    <emoji> <type>(<optional scope>): <summary>

Example: `✨ feat(syntax): parse readonly class declarations`.
References: <https://gitmoji.dev/> and <https://www.conventionalcommits.org/>.

## Pull requests

- Keep pull requests focused: one concern per pull request.
- Every code change comes with tests; every user-visible change updates
  `CHANGELOG.md` under `[Unreleased]`.
- CI (lint, test, deny) must be green.

## Adding a rule or a diagnostic

The rule framework does not exist yet (it arrives with the rules
sub-project); this section will document the full workflow when it lands.
Until then, propose new rules through the "Rule proposal" issue template.

## Licensing of contributions

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as MIT OR Apache-2.0, without any
additional terms or conditions.
```

- [ ] **Step 2: Fetch the Contributor Covenant and set the contact**

Run: `curl -fsSL https://www.contributor-covenant.org/version/2/1/code_of_conduct/code_of_conduct.md -o CODE_OF_CONDUCT.md`

Then edit `CODE_OF_CONDUCT.md`: replace the `[INSERT CONTACT METHOD]` placeholder with `contact@jdevelop.io` (the file contains exactly one, in the Enforcement section).

Verify: `grep -q "^# Contributor Covenant Code of Conduct" CODE_OF_CONDUCT.md && ! grep -q "\[INSERT" CODE_OF_CONDUCT.md && echo OK`
Expected: `OK`. (The file's first line is blank; the heading is on line 2 — do not check the first line, and do not use `grep -c`, whose exit code is 1 when the count is 0.)

- [ ] **Step 3: Write `SECURITY.md`**

```markdown
# Security Policy

Celerrate will itself ship security analysis; we hold its own security to
the same standard.

## Reporting a vulnerability

Please do **not** open a public issue for security problems.

- Preferred: GitHub private vulnerability reporting ("Report a
  vulnerability" under the repository's Security tab).
- Alternative: email `contact@jdevelop.io`.

You will receive an acknowledgment within 72 hours. Please include a
reproduction if possible. We will coordinate a fix and disclosure timeline
with you.

## Supported versions

Until 1.0, only the latest released version receives security fixes.
```

- [ ] **Step 4: Write the issue template configuration and the four templates**

`.github/ISSUE_TEMPLATE/config.yml`:

```yaml
blank_issues_enabled: true
```

`.github/ISSUE_TEMPLATE/bug_report.yml`:

```yaml
name: Bug report
description: Something behaves incorrectly.
labels: ["bug"]
body:
  - type: textarea
    id: what-happened
    attributes:
      label: What happened?
      description: What did you do, what did you expect, what happened instead?
    validations:
      required: true
  - type: textarea
    id: reproduction
    attributes:
      label: Minimal reproduction
      description: The smallest PHP snippet or setup that triggers the problem.
      render: php
  - type: input
    id: version
    attributes:
      label: Celerrate version
      placeholder: output of `celerrate --version`, or the commit hash if built from source
```

`.github/ISSUE_TEMPLATE/false_positive.yml`:

```yaml
name: False positive
description: Celerrate reported a diagnostic on correct code. We treat these as priority bugs.
labels: ["false-positive", "bug"]
body:
  - type: input
    id: diagnostic-id
    attributes:
      label: Diagnostic identifier
      placeholder: e.g. CEL0231
    validations:
      required: true
  - type: textarea
    id: code
    attributes:
      label: The code that is incorrectly flagged
      render: php
    validations:
      required: true
  - type: textarea
    id: why-correct
    attributes:
      label: Why the code is correct
    validations:
      required: true
  - type: input
    id: version
    attributes:
      label: Celerrate version
    validations:
      required: true
```

`.github/ISSUE_TEMPLATE/internal_error.yml`:

```yaml
name: Internal error
description: Celerrate reported an internal error (this template is pre-filled by the crash reporter).
labels: ["internal-error", "bug"]
body:
  - type: textarea
    id: report
    attributes:
      label: Internal error report
      description: Paste the report printed by Celerrate.
      render: text
    validations:
      required: true
  - type: input
    id: version
    attributes:
      label: Celerrate version
    validations:
      required: true
```

`.github/ISSUE_TEMPLATE/rule_proposal.yml`:

```yaml
name: Rule proposal
description: Propose a new diagnostic rule.
labels: ["rule-proposal"]
body:
  - type: textarea
    id: motivation
    attributes:
      label: What problem does the rule catch?
    validations:
      required: true
  - type: textarea
    id: examples
    attributes:
      label: Failing and passing examples
      render: php
    validations:
      required: true
  - type: textarea
    id: fix
    attributes:
      label: Is an automatic fix possible? Is it safe?
```

- [ ] **Step 5: Write `.github/PULL_REQUEST_TEMPLATE.md`**

```markdown
## What does this change?

<!-- One or two sentences. Link the issue if one exists. -->

## Checklist

- [ ] Tests cover the change (test-driven: the test existed before the code).
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo fmt --all --check` passes.
- [ ] `CHANGELOG.md` updated under `[Unreleased]` (user-visible changes only).
```

- [ ] **Step 6: Write the repository `CLAUDE.md`**

```markdown
# Celerrate

A complete PHP toolchain written in Rust: static analysis (interprocedural,
incremental), lint, formatting, LSP, refactoring, security taint analysis.
Open source (MIT OR Apache-2.0) under JDevelop.

## Authoritative documents

- Design spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md`
- Implementation plans: `.claude/superpowers/plans/`

Read the spec before architectural work. It is the source of truth.

## Engineering rules (non-negotiable)

- **Zero panic, mechanically enforced**: Clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code`
  is forbidden. Production code returns `Result`; make invalid states
  unrepresentable. Test modules may locally `#[allow]` these lints.
- **TDD**: failing test → minimal implementation → refactor. No production
  code without a test that demanded it.
- **Strict layering**: a crate depends only on crates below it:

      celerrate_source      source files, spans, line index (bottom)
      celerrate_syntax      lexer + parser → lossless syntax tree
      celerrate_db          salsa query database
      celerrate_semantics   project discovery, symbol index, name resolution
      celerrate_types       inference and type system
      celerrate_rules       rule framework + diagnostics + structured edits
      celerrate_plugin      plugin API
      celerrate_cli         the binary (top)

- **Error resilience**: no user input may ever crash the tool; parsers and
  loaders produce diagnostics, never failures.
- **Determinism**: all analysis results are pure functions of their inputs
  (salsa requirement). No wall-clock time, no randomness, no environment
  reads inside queries.

## Local commands

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all`
- `cargo deny check`

## Conventions

- Everything is written in English. Full words, no abbreviated names
  (standard acronyms fine).
- Commits: gitmoji + Conventional Commits, e.g.
  `✨ feat(syntax): parse readonly class declarations`. Commits are
  authored with the repository-configured GitHub noreply email.
```

- [ ] **Step 7: Commit**

```bash
git add CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md .github/ISSUE_TEMPLATE/ .github/PULL_REQUEST_TEMPLATE.md CLAUDE.md
git commit -m "📄 docs: add community files, issue templates, and CLAUDE.md"
```

---

### Task 5: `celerrate_source` — spans and the line index (forward mapping)

**Files:**
- Modify: `crates/celerrate_source/Cargo.toml`
- Modify: `crates/celerrate_source/src/lib.rs`
- Create: `crates/celerrate_source/src/line_index.rs`
- Test: `crates/celerrate_source/tests/line_index.rs`

**Interfaces:**
- Consumes: the workspace dependency `text-size` declared in Task 1.
- Produces: `celerrate_source::{TextRange, TextSize}` (re-exports), `celerrate_source::LineCol { line: u32, col: u32 }` (both zero-based; `col` is a byte offset within the line), `celerrate_source::LineIndex` with `fn new(text: &str) -> LineIndex` and `fn line_col(&self, offset: TextSize) -> LineCol`. Task 6 adds `fn offset(&self, line_col: LineCol) -> Option<TextSize>`.

- [ ] **Step 1: Add the dependency and module wiring**

`crates/celerrate_source/Cargo.toml`, add:

```toml
[dependencies]
text-size = { workspace = true }
```

Replace `crates/celerrate_source/src/lib.rs` with:

```rust
//! Source text primitives for the Celerrate toolchain: text sizes, ranges,
//! and line/column indexing. This is the bottom layer of the workspace:
//! it depends on no other Celerrate crate.
//!
//! Offsets and ranges are byte-based and use the `text-size` types, which
//! cap file size at 4 GiB; source-file loading (added to this crate by a
//! later plan) is responsible for rejecting larger files before offsets
//! are ever constructed.

pub use text_size::{TextRange, TextSize};

mod line_index;

pub use line_index::{LineCol, LineIndex};
```

Create `crates/celerrate_source/src/line_index.rs` with only the types (no logic yet, so the failing test in Step 3 fails only on the missing methods, not on missing types):

```rust
use text_size::TextSize;

/// A zero-based line/column position. `col` is a byte offset within the
/// line, not a character count: multi-byte UTF-8 characters advance it by
/// their byte length. Rendering layers convert to user-facing columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// Maps byte offsets to line/column positions and back for one text.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<TextSize>,
}
```

The text length is deliberately not stored yet: nothing reads it before Task 6's `offset`, and an unread field would fail the `dead_code` lint under `-D warnings`. Task 6 adds it.

- [ ] **Step 2: Write the failing tests**

`crates/celerrate_source/tests/line_index.rs`:

The snippets below are already in rustfmt's output form for `style_edition = "2024"` (single-line `assert_eq!` calls with these arguments exceed the default call width and get split); paste them verbatim so `cargo fmt --all --check` stays clean.

```rust
use celerrate_source::{LineCol, LineIndex, TextSize};

#[test]
fn empty_text_maps_offset_zero_to_origin() {
    let index = LineIndex::new("");
    assert_eq!(
        index.line_col(TextSize::from(0)),
        LineCol { line: 0, col: 0 }
    );
}

#[test]
fn single_line_columns_are_byte_offsets() {
    let index = LineIndex::new("hello");
    assert_eq!(
        index.line_col(TextSize::from(3)),
        LineCol { line: 0, col: 3 }
    );
}

#[test]
fn newline_starts_a_new_line() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.line_col(TextSize::from(2)),
        LineCol { line: 0, col: 2 }
    );
    assert_eq!(
        index.line_col(TextSize::from(3)),
        LineCol { line: 1, col: 0 }
    );
    assert_eq!(
        index.line_col(TextSize::from(4)),
        LineCol { line: 1, col: 1 }
    );
}

#[test]
fn crlf_newline_keeps_carriage_return_on_its_line() {
    let index = LineIndex::new("ab\r\ncd");
    assert_eq!(
        index.line_col(TextSize::from(2)),
        LineCol { line: 0, col: 2 }
    );
    assert_eq!(
        index.line_col(TextSize::from(4)),
        LineCol { line: 1, col: 0 }
    );
}

#[test]
fn multibyte_characters_advance_columns_by_byte_length() {
    // 'é' is two bytes in UTF-8.
    let index = LineIndex::new("é\nx");
    assert_eq!(
        index.line_col(TextSize::from(2)),
        LineCol { line: 0, col: 2 }
    );
    assert_eq!(
        index.line_col(TextSize::from(3)),
        LineCol { line: 1, col: 0 }
    );
}

#[test]
fn offset_at_end_of_text_is_on_the_last_line() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.line_col(TextSize::from(5)),
        LineCol { line: 1, col: 2 }
    );
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_source --test line_index`
Expected: FAIL to compile with ``error[E0599]: no function or associated item named `new` found`` (the type exists, the behavior does not).

- [ ] **Step 4: Implement `new` and `line_col`**

Append to `crates/celerrate_source/src/line_index.rs`:

```rust
impl LineIndex {
    /// Builds the index in one pass over the text.
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![TextSize::from(0)];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter(|&(_, byte)| byte == b'\n')
                .map(|(position, _)| TextSize::from(position as u32 + 1)),
        );
        Self { line_starts }
    }

    /// Maps a byte offset to its line/column position. Offsets are expected
    /// to lie within the indexed text (`0..=len`); the end-of-text offset
    /// maps to the position just past the last character.
    pub fn line_col(&self, offset: TextSize) -> LineCol {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts.get(line).copied().unwrap_or_default();
        LineCol {
            line: line as u32,
            col: u32::from(offset) - u32::from(line_start),
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_source --test line_index`
Expected: PASS, 6 tests.

- [ ] **Step 6: Run the full gate and commit**

Run: `cargo fmt --all` (normalizes any formatting drift), then:
`cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check && cargo test --workspace`
Expected: all PASS (exit 0).

```bash
git add crates/celerrate_source/
git commit -m "✨ feat(source): add spans re-exports and line index forward mapping"
```

---

### Task 6: `celerrate_source` — reverse mapping (`offset`)

**Files:**
- Modify: `crates/celerrate_source/src/line_index.rs`
- Test: `crates/celerrate_source/tests/line_index.rs` (append)

**Interfaces:**
- Consumes: `LineIndex`, `LineCol` from Task 5.
- Produces: `LineIndex::offset(&self, line_col: LineCol) -> Option<TextSize>` — `None` when the line does not exist, the column runs past the end of the line, or the position is not representable (overflow). Also adds the stored text length (`len` field) that bounds the last line.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_source/tests/line_index.rs`:

```rust
#[test]
fn offset_roundtrips_every_char_boundary() {
    let text = "ab\r\ncd\né\nend";
    let index = LineIndex::new(text);
    for (position, _) in text.char_indices() {
        let offset = TextSize::from(position as u32);
        assert_eq!(index.offset(index.line_col(offset)), Some(offset));
    }
}

#[test]
fn offset_accepts_end_of_text() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.offset(LineCol { line: 1, col: 2 }),
        Some(TextSize::from(5))
    );
}

#[test]
fn offset_rejects_line_out_of_range() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(index.offset(LineCol { line: 2, col: 0 }), None);
}

#[test]
fn offset_rejects_column_past_end_of_line() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(index.offset(LineCol { line: 0, col: 7 }), None);
}

#[test]
fn offset_rejects_column_that_overflows() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.offset(LineCol {
            line: 1,
            col: u32::MAX
        }),
        None
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_source --test line_index`
Expected: FAIL to compile with ``error[E0599]: no method named `offset` found``.

- [ ] **Step 3: Store the text length and implement `offset`**

`offset` must know where the last line ends, so `LineIndex` now stores the text length. In `crates/celerrate_source/src/line_index.rs`:

First, add the field to the struct:

```rust
pub struct LineIndex {
    line_starts: Vec<TextSize>,
    len: TextSize,
}
```

Then extend `new` to fill it, and document the panic this introduces (`TextSize::of` is the only panicking path in the crate, and the crate documentation assigns oversized-input rejection to source-file loading):

```rust
    /// Builds the index in one pass over the text.
    ///
    /// # Panics
    ///
    /// Panics if `text` is larger than 4 GiB, the maximum size `TextSize`
    /// can represent. Rejecting oversized inputs before indexing is the
    /// responsibility of source-file loading (see the crate documentation).
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![TextSize::from(0)];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter(|&(_, byte)| byte == b'\n')
                .map(|(position, _)| TextSize::from(position as u32 + 1)),
        );
        Self {
            line_starts,
            len: TextSize::of(text),
        }
    }
```

Finally, append inside the `impl LineIndex` block:

```rust
    /// Maps a line/column position back to a byte offset. Returns `None`
    /// when the line does not exist, the column runs past the end of the
    /// line, or the position is not representable. The column one past the
    /// line's last byte is accepted: on interior lines it is the next
    /// line's start (which `line_col` reports as the next line's column
    /// zero), on the last line it is the end of text.
    pub fn offset(&self, line_col: LineCol) -> Option<TextSize> {
        let line = usize::try_from(line_col.line).ok()?;
        let line_start = self.line_starts.get(line).copied()?;
        let candidate = line_start.checked_add(TextSize::from(line_col.col))?;
        let line_end = self.line_starts.get(line + 1).copied().unwrap_or(self.len);
        (candidate <= line_end).then_some(candidate)
    }
```

`checked_add`, not `+`: `text-size`'s `Add` panics on overflow in debug builds and silently wraps in release builds, and `offset` is exactly the API that will receive untrusted positions (for example from an LSP client).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_source --test line_index`
Expected: PASS, 11 tests.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo fmt --all` (normalizes any formatting drift), then:
`cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check && cargo test --workspace`
Expected: all PASS (exit 0).

```bash
git add crates/celerrate_source/
git commit -m "✨ feat(source): add line index reverse mapping with bounds validation"
```

---

## Repository publication checklist (once the GitHub repository exists)

Not part of any task above; done when the repository is created and pushed.

- [ ] Verify the CI workflow runs green on the first push (`lint`, `test`, `deny`).
- [ ] Create the issue labels the templates reference: `bug`, `false-positive`, `internal-error`, `rule-proposal` (GitHub does not create labels from templates automatically; unknown labels are silently dropped).
- [ ] Enable private vulnerability reporting (repository Settings → Security), which SECURITY.md points contributors to.
- [ ] Add the repository URL where it was deliberately omitted: README badges, the `repository` field in the workspace `Cargo.toml`, and a compare link for `[Unreleased]` in CHANGELOG.md.
- [ ] Consider registering the "Celerrate" trademark (INPI/EUIPO): France and the EU are first-to-file jurisdictions, so the README's unregistered-trademark notice carries little enforceable weight on its own.
