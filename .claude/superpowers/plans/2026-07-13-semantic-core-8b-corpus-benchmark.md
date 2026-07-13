# Semantic Core Part 8b: The Corpus and the Benchmark — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The pinned symfony/demo corpus in CI (snapshot regression plus the anti-false-positive contract), the committed benchmark protocol with its `cargo xtask bench` harness and CI guard rail, and the protocol run that produces the published warm one-edit number.

**Architecture:** Everything new lives in `xtask` and CI configuration; no `celerrate_*` crate changes. The existing pin mechanism (`xtask/phpstorm-stubs.pin`) is generalized into a shared `xtask::pin` module, then reused for `xtask/corpus.pin` pointing at symfony/demo. `cargo xtask corpus` fetches the corpus, installs its vendor tree from its own lock file, runs the release binary over it, enforces the anti-false-positive contract mechanically (no `CEL0018`/`CEL0019`/`CEL0020` in the report, ever, even under `--bless`), and compares the complete output against a committed snapshot. `cargo xtask bench` prepares a disposable working copy, primes the cache, applies the scripted edit, and lets hyperfine measure the built binary end to end — process startup and cache loading included, because that is how the flagship number is defined. The same harness with `--ceilings` is the CI guard rail. `benchmarks/PROTOCOL.md` fixes everything a publishable number requires, and the final task runs the protocol on the maintainer's machine, records the number, and closes the two measured decisions 8a left open (symbol-index pack economics, diagnostics pack economics) as amendments to the design spec.

**Tech Stack:** Rust (xtask only), hyperfine (external tool, dev and CI only — not a crate dependency), composer + PHP (corpus vendor install, preinstalled on `ubuntu-latest`), serde_json (parsing hyperfine's JSON export), GitHub Actions.

**Branch:** `semantic-core-8b-corpus-benchmark`, from `main`. Spec: `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md` (sections 3 and 4), plus the 8a plan's deferral note: the symbol-index pack's economics are measured here before any code is written.

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result`; test modules may locally `#[allow]`. This applies to `xtask` too (it is a workspace member).
- xtask depends on no `celerrate_*` crate: it only spawns processes (`git`, `cargo`, `composer`, `hyperfine`, and the built `celerrate` binary), so a broken build can never prevent regenerating what fixes it.
- The corpus is pinned, never floating: bumping it is a deliberate `corpus.pin` change with a human-reviewed snapshot diff.
- The anti-false-positive contract: the corpus snapshot contains no unknown-symbol diagnostics (`CEL0018`, `CEL0019`, `CEL0020`). Any such diagnostic on symfony/demo is a false positive and a priority bug — the tooling refuses to bless a violating snapshot.
- No PHPStan comparison anywhere in this part: the protocol document states the scope asymmetry and defers the matched-scope comparison to v0.1.
- The corpus and the benchmarks stay on Linux and macOS; Windows support is 8c's test matrix, not this part.
- TDD where a seam exists (parsing, the contract scan, the working-copy filter, median extraction, ceilings); process-spawning orchestration is verified by running it, consistent with the existing `xtask::stubs` style.
- Everything in English, full words, no abbreviated names (standard acronyms fine).
- Commits: gitmoji + Conventional Commits, repository-configured identity, no AI attribution.
- Every task ends with: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check` all green.

## File Structure

```
xtask/src/pin.rs                 create: Pin, parse, read, fetch_snapshot (generalized from stubs.rs)
xtask/src/stubs.rs               modify: consume xtask::pin, drop the moved code
xtask/src/corpus.rs              create: corpus pin, vendor install, snapshot check, contract
xtask/src/bench.rs               create: working copy, priming, scenarios, hyperfine, ceilings
xtask/src/lib.rs                 modify: modules, release_binary helper, doc comment
xtask/src/main.rs                modify: fetch-corpus, corpus [--bless], bench [--ceilings]
xtask/Cargo.toml                 modify: serde_json
xtask/corpus.pin                 create: symfony/demo at a pinned SHA
xtask/corpus-snapshot.txt        create: the committed expected report (blessed in Task 4)
.github/workflows/corpus.yml     create: snapshot job + bench guard-rail job
benchmarks/PROTOCOL.md           create: the committed benchmark protocol and its results
.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md
                                 modify: amendment history (measured decisions, Task 9)
```

---

### Task 1: The shared pin module

Generalize the pin mechanism so the corpus can reuse it. Pure refactor: `parse_pin`/`StubsPin` and the fetch-into-staging logic move from `xtask/src/stubs.rs` into a new `xtask/src/pin.rs`; behavior is unchanged and the existing tests move with the code.

**Files:**
- Create: `xtask/src/pin.rs`
- Modify: `xtask/src/stubs.rs`
- Modify: `xtask/src/lib.rs`

**Interfaces:**
- Produces: `xtask::pin::Pin { repository: String, commit: String }`, `xtask::pin::parse(text: &str) -> Result<Pin>`, `xtask::pin::read(path: &Path) -> Result<Pin>`, `xtask::pin::fetch_snapshot(pin: &Pin, directory: &Path) -> Result<()>`. Task 2 consumes all four.
- Consumes: `xtask::Result`, `xtask::workspace_root()` (existing).

- [ ] **Step 1: Write the failing test**

Create `xtask/src/pin.rs` with only the test module (the moved tests, renamed to the new API):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::parse;

    const VALID_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_valid_pin_parses() {
        let pin = parse(&format!(
            "# a comment\n\
             repository = https://github.com/JetBrains/phpstorm-stubs\n\
             \n\
             commit = {VALID_SHA}\n",
        ))
        .unwrap();
        assert_eq!(
            pin.repository,
            "https://github.com/JetBrains/phpstorm-stubs"
        );
        assert_eq!(pin.commit, VALID_SHA);
    }

    #[test]
    fn a_missing_key_is_rejected() {
        assert!(parse("repository = https://example.com/repo").is_err());
        assert!(parse(&format!("commit = {VALID_SHA}")).is_err());
    }

    #[test]
    fn a_short_or_non_hexadecimal_commit_is_rejected() {
        assert!(parse("repository = r\ncommit = abc123").is_err());
        assert!(
            parse("repository = r\ncommit = zzzz456789abcdef0123456789abcdef01234567").is_err()
        );
    }

    #[test]
    fn unknown_keys_are_rejected_to_catch_typos() {
        assert!(parse(&format!("repo = r\ncommit = {VALID_SHA}")).is_err());
    }
}
```

Register the module in `xtask/src/lib.rs` below `pub mod codegen;`:

```rust
pub mod pin;
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package xtask`
Expected: FAIL to compile with "cannot find function `parse`".

- [ ] **Step 3: Move the implementation**

Fill `xtask/src/pin.rs` above the test module (the bodies are `stubs.rs`'s `parse_pin`, `fetch` and `run_git`, generalized):

```rust
//! The pin mechanism shared by every vendored snapshot: a committed
//! `key = value` file naming a repository and a commit, fetched
//! shallowly into `target/`, bumped deliberately, never floating.

use std::path::Path;
use std::process::Command;

use crate::Result;

/// A parsed pin file: one repository, one full-length commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub repository: String,
    pub commit: String,
}

/// Reads and parses a committed pin file.
pub fn read(path: &Path) -> Result<Pin> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse(&text)
}

/// Parses a pin file: `key = value` lines, `#` comments, both
/// `repository` and a full-length hexadecimal `commit` required.
pub fn parse(text: &str) -> Result<Pin> {
    let mut repository = None;
    let mut commit = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("malformed pin line: {line}").into());
        };
        match key.trim() {
            "repository" => repository = Some(value.trim().to_owned()),
            "commit" => commit = Some(value.trim().to_owned()),
            unknown => return Err(format!("unknown pin key: {unknown}").into()),
        }
    }
    let repository = repository.ok_or("pin file misses the repository key")?;
    let commit = commit.ok_or("pin file misses the commit key")?;
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the pinned commit must be a full 40-character SHA".into());
    }
    Ok(Pin { repository, commit })
}

/// Fetches the pinned snapshot into `directory` if it is not already
/// present. The checkout lands in a staging directory first and is
/// renamed only when complete, so an interrupted fetch never
/// masquerades as a snapshot.
pub fn fetch_snapshot(pin: &Pin, directory: &Path) -> Result<()> {
    if directory.exists() {
        println!("snapshot already present at {}", directory.display());
        return Ok(());
    }
    let staging = directory.with_file_name(format!("{}.staging", pin.commit));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    run_git(&staging, &["init", "--quiet"])?;
    run_git(
        &staging,
        &[
            "fetch",
            "--quiet",
            "--depth",
            "1",
            &pin.repository,
            &pin.commit,
        ],
    )?;
    run_git(&staging, &["checkout", "--quiet", "--detach", "FETCH_HEAD"])?;
    std::fs::rename(&staging, &directory)?;
    println!("fetched {} at {}", pin.repository, pin.commit);
    Ok(())
}

fn run_git(directory: &Path, arguments: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .status()?;
    if !status.success() {
        return Err(format!("git {} failed", arguments.join(" ")).into());
    }
    Ok(())
}
```

Rewrite `xtask/src/stubs.rs` to consume it — delete `StubsPin`, `parse_pin`, the body of `fetch`, `run_git`, and the whole test module; what remains:

```rust
//! The pinned phpstorm-stubs snapshot: fetch it at the pinned commit
//! and drive the stub compiler. Network happens only here — never in
//! a build script, never in a query. The pin is bumped deliberately,
//! like the corpus SHA.

use std::path::PathBuf;
use std::process::Command;

use crate::Result;
use crate::pin::Pin;

/// Reads and parses the committed pin file.
pub fn pin() -> Result<Pin> {
    crate::pin::read(&crate::workspace_root()?.join("xtask/phpstorm-stubs.pin"))
}

/// Where the pinned snapshot lives: under `target/`, so it is already
/// gitignored and swept by `cargo clean`.
pub fn snapshot_directory() -> Result<PathBuf> {
    Ok(crate::workspace_root()?
        .join("target/phpstorm-stubs")
        .join(pin()?.commit))
}

/// Fetches the pinned snapshot if it is not already present.
pub fn fetch() -> Result<()> {
    crate::pin::fetch_snapshot(&pin()?, &snapshot_directory()?)
}
```

Keep `compile` exactly as it is today (it only calls `fetch`, `workspace_root`, and `snapshot_directory`, all of which kept their signatures).

- [ ] **Step 4: Run the tests and the stubs check**

Run: `cargo test --package xtask`
Expected: PASS (the four moved tests, green).

Run: `cargo xtask compile-stubs --check`
Expected: exits 0 — the refactor did not change what the stubs pipeline does.

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add xtask/src/pin.rs xtask/src/stubs.rs xtask/src/lib.rs
git commit -m "♻️ refactor(xtask): extract the pin mechanism into a shared module"
```

---

### Task 2: The corpus pin and `cargo xtask fetch-corpus`

`xtask/corpus.pin` names symfony/demo at a full SHA; `cargo xtask fetch-corpus` fetches it shallowly into `target/corpus/<sha>` and installs its vendor tree from its own committed lock file.

**Files:**
- Create: `xtask/corpus.pin`
- Create: `xtask/src/corpus.rs`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/src/main.rs`

**Interfaces:**
- Consumes: `xtask::pin::{Pin, read, fetch_snapshot}` (Task 1).
- Produces: `xtask::corpus::pin() -> Result<Pin>`, `xtask::corpus::snapshot_directory() -> Result<PathBuf>`, `xtask::corpus::prepare() -> Result<PathBuf>` (fetch + vendor install, returns the corpus root). Tasks 3 and 6 consume `prepare`.

- [ ] **Step 1: Resolve the SHA to pin**

Run: `git ls-remote https://github.com/symfony/demo.git refs/heads/main`
Expected: one line, `<sha>\trefs/heads/main`. Note the 40-character SHA; it is `<CORPUS_SHA>` below.

- [ ] **Step 2: Write the failing test**

Create `xtask/src/corpus.rs` with the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    #[test]
    fn the_committed_corpus_pin_parses_and_names_the_corpus() {
        let pin = super::pin().unwrap();
        assert!(
            pin.repository.contains("symfony/demo"),
            "the corpus is symfony/demo, per the design: {}",
            pin.repository,
        );
    }
}
```

Register the module in `xtask/src/lib.rs` below `pub mod codegen;` (keep the list alphabetical):

```rust
pub mod corpus;
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test --package xtask`
Expected: FAIL to compile with "cannot find function `pin`".

- [ ] **Step 4: Write the pin file and the implementation**

Create `xtask/corpus.pin` (substitute the SHA from Step 1):

```
# The pinned regression and benchmark corpus: symfony/demo, a real user
# project with application code, a composer.json, and a full vendor tree
# from its committed lock file. Bump deliberately: change the commit,
# run `cargo xtask corpus --bless`, and commit the regenerated snapshot
# together with this file after reviewing the diff.
repository = https://github.com/symfony/demo
commit = <CORPUS_SHA>
```

Fill `xtask/src/corpus.rs` above the test module:

```rust
//! The pinned regression and benchmark corpus: symfony/demo at a
//! committed SHA, fetched shallowly, its vendor tree installed from its
//! own lock file. The corpus is both the anti-false-positive regression
//! surface and the benchmark subject; bumping it is a deliberate pin
//! change with a human-reviewed snapshot diff, never a floating HEAD.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;
use crate::pin::Pin;

/// Reads and parses the committed corpus pin.
pub fn pin() -> Result<Pin> {
    crate::pin::read(&crate::workspace_root()?.join("xtask/corpus.pin"))
}

/// Where the corpus lives: under `target/`, so it is already gitignored
/// and swept by `cargo clean`.
pub fn snapshot_directory() -> Result<PathBuf> {
    Ok(crate::workspace_root()?
        .join("target/corpus")
        .join(pin()?.commit))
}

/// Fetches the corpus and installs its vendor tree; returns the corpus
/// root, ready to be analyzed.
pub fn prepare() -> Result<PathBuf> {
    let directory = snapshot_directory()?;
    crate::pin::fetch_snapshot(&pin()?, &directory)?;
    install_vendor(&directory)?;
    Ok(directory)
}

/// Runs `composer install` from the corpus's committed lock file, once:
/// a present vendor directory is trusted, because the lock file pins
/// the tree exactly. `--no-scripts` and `--no-plugins` keep the install
/// hermetic (no code from the corpus runs), and `--ignore-platform-reqs`
/// decouples it from the local PHP extension set: Celerrate never
/// executes the corpus, it only reads it.
fn install_vendor(directory: &Path) -> Result<()> {
    if directory.join("vendor").is_dir() {
        return Ok(());
    }
    let status = Command::new("composer")
        .current_dir(directory)
        .args([
            "install",
            "--no-interaction",
            "--no-progress",
            "--no-scripts",
            "--no-plugins",
            "--ignore-platform-reqs",
        ])
        .status()
        .map_err(|error| format!("cannot run composer (is it installed?): {error}"))?;
    if !status.success() {
        return Err("composer install failed".into());
    }
    Ok(())
}
```

In `xtask/src/main.rs`, add the dispatch arm (before the `_` arm) and extend the usage line:

```rust
(Some("fetch-corpus"), None) => xtask::corpus::prepare().map(|_| ()),
```

```rust
eprintln!(
    "usage: cargo xtask <codegen | fetch-stubs | compile-stubs [--check] | fetch-corpus>"
);
```

- [ ] **Step 5: Run the tests, then the real fetch**

Run: `cargo test --package xtask`
Expected: PASS.

Run: `cargo xtask fetch-corpus`
Expected: fetches symfony/demo at the pin, runs `composer install`, exits 0. Verify: `ls target/corpus/<CORPUS_SHA>/vendor` shows the installed packages, and `target/corpus/<CORPUS_SHA>/src/Controller/BlogController.php` exists (Task 6's scripted edit targets it; if symfony/demo has renamed it at the pinned SHA, pick another stable file under `src/` now and use it consistently in Task 6 and in `benchmarks/PROTOCOL.md`).

Run: `cargo xtask fetch-corpus` a second time.
Expected: "snapshot already present", no re-install, exits 0.

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add xtask/corpus.pin xtask/src/corpus.rs xtask/src/lib.rs xtask/src/main.rs
git commit -m "✨ feat(xtask): pin the symfony/demo corpus and fetch it on demand"
```

---

### Task 3: The snapshot check and the anti-false-positive contract

`cargo xtask corpus` runs the release binary over the corpus, refuses any unknown-symbol diagnostic (a false positive on correct code, a priority bug — refused even under `--bless`), and compares the complete report against `xtask/corpus-snapshot.txt`. `--bless` writes the snapshot.

**Files:**
- Modify: `xtask/src/corpus.rs`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/src/main.rs`

**Interfaces:**
- Consumes: `xtask::corpus::prepare()` (Task 2).
- Produces: `xtask::release_binary() -> Result<PathBuf>` in `lib.rs` (builds `celerrate` in release mode, returns its path; Task 6 consumes it), `xtask::corpus::check_snapshot(bless: bool) -> Result<()>`, `xtask::corpus::unknown_symbol_violations(report: &str) -> Vec<String>`.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module of `xtask/src/corpus.rs`:

```rust
use super::unknown_symbol_violations;

#[test]
fn the_unknown_symbol_family_is_caught_line_by_line() {
    let report = "src/A.php:3:1 CEL0018 unknown class \\App\\Missing\n\
                  src/B.php:9:5 CEL0024 match expressions require PHP 8.0\n\
                  src/C.php:1:1 CEL0019 unknown function \\missing()\n\
                  src/D.php:2:2 CEL0020 unknown constant \\MISSING\n\
                  0 notices, 4 diagnostics\n";
    let violations = unknown_symbol_violations(report);
    assert_eq!(violations.len(), 3);
    assert!(violations.iter().all(|line| !line.contains("CEL0024")));
}

#[test]
fn a_clean_report_has_no_violations() {
    let report = "src/B.php:9:5 CEL0024 match expressions require PHP 8.0\n\
                  0 notices, 1 diagnostic\n";
    assert!(unknown_symbol_violations(report).is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package xtask`
Expected: FAIL to compile with "cannot find function `unknown_symbol_violations`".

- [ ] **Step 3: Implement**

Add to `xtask/src/lib.rs` (below `workspace_root`; add `use std::process::Command;` to the imports):

```rust
/// Builds the release binary and returns its path. Every corpus and
/// benchmark run goes through the optimized build: the numbers and the
/// snapshot must describe what users download, not a debug build.
pub fn release_binary() -> Result<PathBuf> {
    let root = workspace_root()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .current_dir(&root)
        .args(["build", "--release", "--package", "celerrate_cli"])
        .status()?;
    if !status.success() {
        return Err("the release build failed".into());
    }
    Ok(root.join("target/release/celerrate"))
}
```

Add to `xtask/src/corpus.rs`:

```rust
/// The identifiers of the unknown-symbol family. symfony/demo is
/// correct code: any of these in its report is a false positive, which
/// the umbrella design classifies as a priority bug, not an opinion.
const UNKNOWN_SYMBOL_IDENTIFIERS: [&str; 3] = ["CEL0018", "CEL0019", "CEL0020"];

/// The committed expected report.
pub fn snapshot_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("xtask/corpus-snapshot.txt"))
}

/// Every report line carrying an unknown-symbol diagnostic. The
/// identifier is the second field of the diagnostic line format
/// (`path:line:column identifier message`); a plain substring match is
/// enough because the identifiers never appear in message text.
pub fn unknown_symbol_violations(report: &str) -> Vec<String> {
    report
        .lines()
        .filter(|line| {
            UNKNOWN_SYMBOL_IDENTIFIERS
                .iter()
                .any(|identifier| line.contains(identifier))
        })
        .map(str::to_owned)
        .collect()
}

/// Runs the release binary over the corpus and holds the report to its
/// two contracts: no unknown-symbol diagnostic anywhere (refused even
/// under `--bless`), and byte-for-byte agreement with the committed
/// snapshot. Exit code 1 from the binary means diagnostics were
/// reported, which is a completed analysis; anything above 1 is not.
pub fn check_snapshot(bless: bool) -> Result<()> {
    let corpus = prepare()?;
    let binary = crate::release_binary()?;
    let output = Command::new(&binary).arg("check").arg(&corpus).output()?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "celerrate check did not complete (exit {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
        )
        .into());
    }
    let actual = String::from_utf8(output.stdout)
        .map_err(|error| format!("the report is not valid UTF-8: {error}"))?;

    let violations = unknown_symbol_violations(&actual);
    if !violations.is_empty() {
        return Err(format!(
            "the corpus report contains {} unknown-symbol diagnostic(s); each is a \
             false positive on correct code and a priority bug:\n{}",
            violations.len(),
            violations.join("\n"),
        )
        .into());
    }

    let path = snapshot_path()?;
    if bless {
        std::fs::write(&path, &actual)?;
        println!("blessed {}", path.display());
        return Ok(());
    }

    let expected = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read {}: {error}; run `cargo xtask corpus --bless` and review the result",
            path.display(),
        )
    })?;
    if actual != expected {
        let actual_path = crate::workspace_root()?.join("target/corpus/actual-snapshot.txt");
        std::fs::write(&actual_path, &actual)?;
        // Exit code 1 from `git diff` means "differences", which is the point.
        let _ = Command::new("git")
            .args(["--no-pager", "diff", "--no-index"])
            .arg(&path)
            .arg(&actual_path)
            .status();
        return Err(
            "the corpus report diverged from the committed snapshot; review the diff above \
             and, if the change is intended, run `cargo xtask corpus --bless`"
                .into(),
        );
    }
    println!("the corpus report matches the committed snapshot");
    Ok(())
}
```

In `xtask/src/main.rs`, add the arms and extend the usage line:

```rust
(Some("corpus"), None) => xtask::corpus::check_snapshot(false),
(Some("corpus"), Some("--bless")) => xtask::corpus::check_snapshot(true),
```

```rust
eprintln!(
    "usage: cargo xtask <codegen | fetch-stubs | compile-stubs [--check] | fetch-corpus | corpus [--bless] | bench [--ceilings]>"
);
```

(The `bench` arm arrives in Task 6; naming it in the usage line now keeps this string from churning.)

- [ ] **Step 4: Run the tests**

Run: `cargo test --package xtask`
Expected: PASS.

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add xtask/src/corpus.rs xtask/src/lib.rs xtask/src/main.rs
git commit -m "✨ feat(xtask): check the corpus report against a committed snapshot"
```

---

### Task 4: Blessing the first snapshot

Run the check against the real corpus for the first time, hold the report to the contract, and commit the blessed snapshot. This task is a gate: if the contract fails, the false positives are bugs to fix before this plan can continue.

**Files:**
- Create: `xtask/corpus-snapshot.txt`

**Interfaces:**
- Consumes: `cargo xtask corpus --bless` (Task 3).

- [ ] **Step 1: Run the first bless**

Run: `cargo xtask corpus --bless`

**If it fails with unknown-symbol violations: STOP.** Each violation is a false positive on correct code and a priority bug, per the design. Do not weaken the contract, do not hand-edit the snapshot. Report the violating lines to your human partner, then debug each one with the superpowers:systematic-debugging skill (the fix belongs in the analyzer, most likely `celerrate_semantics` resolution or `celerrate_project` autoload mapping, each fix TDD with a minimal reproduction lifted from the corpus). Re-run the bless once the report is clean.

Expected (clean case): `blessed xtask/corpus-snapshot.txt`, exit 0.

- [ ] **Step 2: Review the blessed snapshot**

Read `xtask/corpus-snapshot.txt` in full. Sanity checks, all of which must hold:

- No `CEL0018`, `CEL0019`, `CEL0020` line (the tooling already enforced it; verify anyway).
- No `internal error:` line — a panic or an unreadable file inside the corpus is a bug or an environment problem to resolve, not something to commit as expected.
- Any remaining diagnostics are version-gating findings (`CEL0021`–`CEL0024`) or parse-level findings you can justify by reading the named file at the named position. Spot-check three.
- The summary line at the end (`N notices, M diagnostics`) matches the body.

If anything is unjustifiable, treat it as Step 1's STOP case.

- [ ] **Step 3: Verify the check now passes and is stable**

Run: `cargo xtask corpus`
Expected: `the corpus report matches the committed snapshot`, exit 0.

Run: `cargo xtask corpus` again (the cache written by the first run is now warm).
Expected: identical result — the report must not depend on cache state; if it does, that is an 8a regression to STOP on.

- [ ] **Step 4: Commit**

```bash
git add xtask/corpus-snapshot.txt
git commit -m "✅ test(xtask): bless the first corpus snapshot"
```

---

### Task 5: The corpus workflow in CI

`corpus.yml` runs the snapshot check on every pull request and on main, with the vendor tree cached by pin SHA. (The bench guard-rail job joins this file in Task 7.)

**Files:**
- Create: `.github/workflows/corpus.yml`

**Interfaces:**
- Consumes: `cargo xtask corpus` (Task 3), `xtask/corpus.pin` (Task 2).

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/corpus.yml` (mirroring `ci.yml`'s action versions and conventions; PHP and composer are preinstalled on `ubuntu-latest`):

```yaml
name: Corpus

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
  snapshot:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - uses: Swatinem/rust-cache@v2
      - uses: actions/cache@v4
        with:
          path: target/corpus
          key: corpus-${{ hashFiles('xtask/corpus.pin') }}
      - run: cargo xtask corpus
```

- [ ] **Step 2: Commit and verify on the pull request**

```bash
git add .github/workflows/corpus.yml
git commit -m "👷 ci(corpus): run the snapshot check on every pull request"
git push -u origin semantic-core-8b-corpus-benchmark
```

Open a draft pull request (`gh pr create --draft --title "Semantic core 8b: corpus and benchmark" --body "..."` — the body summarizes this plan) so the workflow runs. Watch it: `gh run watch` (or `gh pr checks`).
Expected: the `Corpus / snapshot` job is green. The first run fetches the corpus and installs vendor; a re-run restores it from the actions cache.

If the job fails while the local run passes, read the log before touching anything: the likely causes are a missing tool on the runner (composer, PHP) or a path assumption, not the snapshot itself.

---

### Task 6: The benchmark harness

`cargo xtask bench` measures the three protocol scenarios end to end with hyperfine on a disposable working copy of the corpus, and prints the three medians.

**Files:**
- Create: `xtask/src/bench.rs`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/Cargo.toml`

**Interfaces:**
- Consumes: `xtask::corpus::prepare()` (Task 2), `xtask::release_binary()` (Task 3).
- Produces: `xtask::bench::run(check_ceilings: bool) -> Result<()>`, `xtask::bench::median_seconds(json: &str) -> Result<f64>`, `xtask::bench::over_ceiling(name: &str, median: f64, ceiling: f64) -> Option<String>` (Task 7 consumes the ceilings path).

- [ ] **Step 1: Add the dependency**

In `xtask/Cargo.toml`, `[dependencies]`, add:

```toml
serde_json = { workspace = true }
```

(`serde_json = "1"` is already a workspace dependency.)

- [ ] **Step 2: Write the failing tests**

Create `xtask/src/bench.rs` with the test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{copy_directory, median_seconds, over_ceiling};

    #[test]
    fn the_median_is_read_from_hyperfine_json() {
        let json = r#"{"results": [{"command": "celerrate check .",
            "mean": 0.9, "stddev": 0.1, "median": 0.85,
            "min": 0.7, "max": 1.1, "times": [0.7, 0.85, 1.1]}]}"#;
        assert_eq!(median_seconds(json).unwrap(), 0.85);
    }

    #[test]
    fn json_without_a_median_is_an_error_not_a_panic() {
        assert!(median_seconds("{}").is_err());
        assert!(median_seconds("not json at all").is_err());
        assert!(median_seconds(r#"{"results": []}"#).is_err());
    }

    #[test]
    fn a_median_over_its_ceiling_is_named() {
        assert!(over_ceiling("warm one-edit", 3.2, 3.0).is_some());
        assert!(over_ceiling("warm one-edit", 2.9, 3.0).is_none());
    }

    #[test]
    fn the_working_copy_skips_the_git_directory_and_the_cache() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join(".git")).unwrap();
        std::fs::create_dir_all(source.path().join(".celerrate/cache")).unwrap();
        std::fs::create_dir_all(source.path().join("src")).unwrap();
        std::fs::write(source.path().join(".git/HEAD"), "ref").unwrap();
        std::fs::write(source.path().join("src/A.php"), "<?php").unwrap();
        std::fs::write(source.path().join("composer.json"), "{}").unwrap();

        let destination = tempfile::tempdir().unwrap();
        let copy = destination.path().join("corpus");
        copy_directory(source.path(), &copy).unwrap();

        assert!(copy.join("src/A.php").is_file());
        assert!(copy.join("composer.json").is_file());
        assert!(!copy.join(".git").exists());
        assert!(!copy.join(".celerrate").exists());
    }
}
```

Add to `xtask/Cargo.toml`, `[dev-dependencies]` (create the section if absent):

```toml
tempfile = { workspace = true }
```

Register the module in `xtask/src/lib.rs` (alphabetical):

```rust
pub mod bench;
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test --package xtask`
Expected: FAIL to compile with "cannot find function" for the three names.

- [ ] **Step 4: Implement the harness**

Fill `xtask/src/bench.rs` above the test module:

```rust
//! The benchmark harness behind `benchmarks/PROTOCOL.md`: prepare the
//! corpus, prime the cache, apply the scripted edit, and let hyperfine
//! measure the built binary end to end — process startup and cache
//! loading included, because that is how the flagship number is
//! defined. hyperfine rather than criterion, per the protocol: the
//! number is a full CLI run, and criterion measures in-process.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// The file the one-edit scenario touches, relative to the corpus
/// root, and the scripted edit itself: appended, so every span above
/// it stays put — one file changes, everything else is unchanged.
const EDIT_TARGET: &str = "src/Controller/BlogController.php";
const EDIT_TEXT: &str = "\\n// celerrate benchmark edit\\n";

/// The CI guard rail's generous ceilings, in seconds. Shared runners
/// are too noisy to measure on, so these catch structural regressions
/// (the cache silently ceasing to work) and claim nothing more. The
/// local target for warm one-edit is sub-second; the ceiling is not
/// the target.
const COLD_CEILING_SECONDS: f64 = 30.0;
const WARM_NO_CHANGE_CEILING_SECONDS: f64 = 3.0;
const WARM_ONE_EDIT_CEILING_SECONDS: f64 = 3.0;

/// One protocol scenario: its name, how many timed runs, what runs
/// before each timed run, and the guard-rail ceiling.
struct Scenario {
    name: &'static str,
    runs: u32,
    prepare: Option<String>,
    ceiling_seconds: f64,
}

/// Runs the three protocol scenarios and prints their medians. With
/// `check_ceilings`, any median over its ceiling fails the run.
pub fn run(check_ceilings: bool) -> Result<()> {
    ensure_hyperfine()?;
    let corpus = crate::corpus::prepare()?;
    let binary = crate::release_binary()?;
    let root = crate::workspace_root()?;

    let bench_directory = root.join("target/bench");
    let working = bench_directory.join("corpus");
    if working.exists() {
        std::fs::remove_dir_all(&working)?;
    }
    copy_directory(&corpus, &working)?;

    // The pristine bytes of the edit target, kept outside the working
    // tree so the walk never sees them.
    let edit_target = working.join(EDIT_TARGET);
    let original = bench_directory.join("edit-target-original.bak");
    std::fs::copy(&edit_target, &original)?;

    let quoted_binary = quoted(&binary);
    let scenarios = [
        Scenario {
            name: "cold full",
            runs: 5,
            prepare: Some("rm -rf .celerrate".to_owned()),
            ceiling_seconds: COLD_CEILING_SECONDS,
        },
        Scenario {
            name: "warm no-change",
            runs: 10,
            prepare: None,
            ceiling_seconds: WARM_NO_CHANGE_CEILING_SECONDS,
        },
        Scenario {
            name: "warm one-edit",
            runs: 10,
            prepare: Some(format!(
                "cp {} {} && ({quoted_binary} check . > /dev/null || true) && printf '{}' >> {}",
                quoted(&original),
                quoted(&edit_target),
                EDIT_TEXT,
                quoted(&edit_target),
            )),
            ceiling_seconds: WARM_ONE_EDIT_CEILING_SECONDS,
        },
    ];

    // The no-change scenario needs a cache to not change against.
    prime(&binary, &working)?;

    let mut failures = Vec::new();
    println!("{:<16} {:>10}", "scenario", "median");
    for scenario in &scenarios {
        let export = bench_directory.join(format!(
            "{}.json",
            scenario.name.replace(' ', "-"),
        ));
        let median = run_scenario(&quoted_binary, &working, scenario, &export)?;
        println!("{:<16} {:>9.3}s", scenario.name, median);
        if check_ceilings {
            failures.extend(over_ceiling(
                scenario.name,
                median,
                scenario.ceiling_seconds,
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("; ").into());
    }
    Ok(())
}

/// One hyperfine invocation, in the working copy. `--ignore-failure`
/// because the binary exits 1 when it reports diagnostics, which is a
/// completed analysis, not a failed one.
fn run_scenario(
    quoted_binary: &str,
    working: &Path,
    scenario: &Scenario,
    export: &Path,
) -> Result<f64> {
    let mut command = Command::new("hyperfine");
    command
        .current_dir(working)
        .args(["--ignore-failure", "--runs", &scenario.runs.to_string()])
        .arg("--export-json")
        .arg(export);
    if let Some(prepare) = &scenario.prepare {
        command.arg("--prepare").arg(prepare);
    }
    command.arg(format!("{quoted_binary} check ."));
    let status = command.status()?;
    if !status.success() {
        return Err(format!("hyperfine failed for the {} scenario", scenario.name).into());
    }
    median_seconds(&std::fs::read_to_string(export)?)
}

/// Extracts the median, in seconds, from hyperfine's `--export-json`
/// output.
pub fn median_seconds(json: &str) -> Result<f64> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("unreadable hyperfine JSON: {error}"))?;
    value
        .get("results")
        .and_then(|results| results.get(0))
        .and_then(|result| result.get("median"))
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| "hyperfine JSON carries no median".into())
}

/// The guard-rail comparison, one scenario at a time.
pub fn over_ceiling(name: &str, median: f64, ceiling: f64) -> Option<String> {
    (median > ceiling).then(|| {
        format!("the {name} median ({median:.3}s) is over its {ceiling:.1}s ceiling")
    })
}

/// One analysis over the working copy, to write the cache the warm
/// scenarios start from. Exit 1 means diagnostics were reported, which
/// is a completed analysis.
fn prime(binary: &Path, working: &Path) -> Result<()> {
    let status = Command::new(binary)
        .arg("check")
        .arg(".")
        .current_dir(working)
        .stdout(std::process::Stdio::null())
        .status()?;
    if !matches!(status.code(), Some(0 | 1)) {
        return Err(format!("the priming run did not complete (exit {:?})", status.code()).into());
    }
    Ok(())
}

/// Copies the corpus into a disposable working tree, skipping `.git`
/// (never analyzed) and `.celerrate` (each scenario controls the
/// cache). Symlinks (composer's `vendor/bin`) are copied by content,
/// which is fine: the tree is read, never executed.
fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" || name == ".celerrate" {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// hyperfine runs its command through a shell; the binary path is
/// quoted so a space in the workspace path cannot split it.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display())
}

/// A named check with an installation pointer beats a bare "No such
/// file or directory" from the spawn.
fn ensure_hyperfine() -> Result<()> {
    let found = Command::new("hyperfine")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok();
    if !found {
        return Err(
            "hyperfine is required: https://github.com/sharkdp/hyperfine#installation".into(),
        );
    }
    Ok(())
}
```

Note on `EDIT_TEXT`: the `\\n` escapes are literal backslash-n in the Rust string, because the string is interpolated into a `printf` format — `printf` turns them into newlines at run time.

In `xtask/src/main.rs`, add the arms (the usage line already names them since Task 3):

```rust
(Some("bench"), None) => xtask::bench::run(false),
(Some("bench"), Some("--ceilings")) => xtask::bench::run(true),
```

- [ ] **Step 5: Run the unit tests**

Run: `cargo test --package xtask`
Expected: PASS.

- [ ] **Step 6: Run the real harness once**

hyperfine must be installed locally (`brew install hyperfine` on macOS).

Run: `cargo xtask bench`
Expected: the table with three medians, exit 0. Sanity-check the shape, not the values: cold full is the largest; warm no-change is the smallest; warm one-edit sits between them. If warm no-change is not clearly below cold full, the cache is not being used — STOP and debug (8a regression) before continuing.

- [ ] **Step 7: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add xtask/src/bench.rs xtask/src/lib.rs xtask/src/main.rs xtask/Cargo.toml Cargo.lock
git commit -m "✨ feat(xtask): measure the three protocol scenarios with hyperfine"
```

---

### Task 7: The guard rail in CI

The bench job joins `corpus.yml`: the same three scenarios with the generous ceilings, catching structural regressions and claiming nothing more.

**Files:**
- Modify: `.github/workflows/corpus.yml`

**Interfaces:**
- Consumes: `cargo xtask bench --ceilings` (Task 6).

- [ ] **Step 1: Add the job**

Append to the `jobs:` section of `.github/workflows/corpus.yml`:

```yaml
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - uses: Swatinem/rust-cache@v2
      - uses: actions/cache@v4
        with:
          path: target/corpus
          key: corpus-${{ hashFiles('xtask/corpus.pin') }}
      - run: sudo apt-get update && sudo apt-get install --yes hyperfine
      - run: cargo xtask bench --ceilings
```

- [ ] **Step 2: Commit and verify on the pull request**

```bash
git add .github/workflows/corpus.yml
git commit -m "👷 ci(corpus): guard the three benchmark scenarios with ceilings"
git push
```

Watch the run (`gh pr checks`).
Expected: `Corpus / bench` green, its log showing three medians all under their ceilings. If a ceiling trips on a healthy run (a genuinely slow runner rather than a broken cache — check that warm no-change is still well below cold full in the log), raising that ceiling is the intended response; the guard rail only exists to catch the cache silently ceasing to work. Adjust the constant in `xtask/src/bench.rs`, note why in the commit message, and re-push.

---

### Task 8: The benchmark protocol document

`benchmarks/PROTOCOL.md` fixes everything the parent spec requires of a publishable number. The results section stays explicitly unfilled until Task 9's protocol run.

**Files:**
- Create: `benchmarks/PROTOCOL.md`

**Interfaces:**
- Consumes: `xtask/corpus.pin` (the SHA), the corpus on disk (the size counts), `cargo xtask bench` (the reproduction command).

- [ ] **Step 1: Measure the corpus size**

Run (substitute the pinned SHA):

```bash
find target/corpus/<CORPUS_SHA> -name '*.php' -not -path '*/.git/*' | wc -l
find target/corpus/<CORPUS_SHA> -name '*.php' -not -path '*/.git/*' -print0 | xargs -0 cat | wc -l
```

Expected: two numbers — PHP files and PHP lines, vendor tree included (that is what `celerrate check` analyzes). Note both.

- [ ] **Step 2: Describe the hardware**

Run: `system_profiler SPHardwareDataType` and `sw_vers` (the maintainer's machine is a Mac).
Note: chip, core count, memory, storage kind, and the exact OS name and version.

- [ ] **Step 3: Write the document**

Create `benchmarks/PROTOCOL.md`. Angle-bracket fields are filled from Steps 1 and 2 and from the repository (no field may remain bracketed in the commit, except the Results section which says "not yet run"):

```markdown
# Benchmark protocol

Every published Celerrate performance number comes from this protocol,
run on the hardware named below, and is reproducible by a third party
following this document. A number that would not survive third-party
scrutiny is not published.

## Corpus

- Repository: https://github.com/symfony/demo
- Commit: `<CORPUS_SHA>` (committed in `xtask/corpus.pin`)
- Vendor tree: installed from the corpus's own `composer.lock` via
  `composer install --no-interaction --no-progress --no-scripts
  --no-plugins --ignore-platform-reqs`
- Size: <N> PHP files, <M> lines of PHP, vendor tree included — that is
  the tree `celerrate check` analyzes.

symfony/demo is the corpus because it has the exact shape
`celerrate check` is aimed at: a real user project, with application
code, a real `composer.json`, and the full Symfony vendor tree
installed from its lock file.

## Hardware and toolchain

- Machine: <chip>, <cores> cores, <memory> GiB memory, <storage>
- Operating system: <exact OS name and version>
- Rust toolchain: 1.94 (pinned in `rust-toolchain.toml`)
- Binary: `celerrate` built with `cargo build --release`, version
  `celerrate --version` reports at the commit the results name

## Method

The harness is `cargo xtask bench`. It fetches the corpus at the
pinned commit, installs the vendor tree, copies the corpus into a
disposable working tree (`target/bench/corpus`), builds the release
binary, and measures with [hyperfine](https://github.com/sharkdp/hyperfine),
which times the full process: startup, cache loading, analysis,
rendering. The umbrella design named criterion; criterion measures
in-process, and the flagship number is defined end to end, process
included, so hyperfine is the honest tool for this document. criterion
remains available for in-process query benchmarks when a later part
needs them.

- Aggregate: the median.
- hyperfine runs with `--ignore-failure`, because `celerrate check`
  exits 1 when it reports diagnostics — a completed analysis.
- "Cold" means no Celerrate cache (`.celerrate/` removed before every
  timed run); operating-system file caches are warm after the first
  run, and the protocol does not pretend otherwise.

## Scenarios

1. **Cold full** — 5 runs. Before each timed run: `rm -rf .celerrate`.
   Timed: `celerrate check .`. The complete analysis with nothing to
   reuse.
2. **Warm no-change** — 10 runs. The cache is primed once by a full
   run; nothing changes between runs. Timed: `celerrate check .`. The
   floor of the one-shot run.
3. **Warm one-edit** — 10 runs. Before each timed run: the edit target
   is restored to its pristine content, a full run primes the cache,
   then the scripted edit is applied. Timed: `celerrate check .`.
   **This is the flagship number**: a full CLI run, wall clock,
   process startup and cache loading included, exactly the execution
   mode a save-and-rerun user experiences. Target: sub-second.

The scripted edit appends one comment line
(`// celerrate benchmark edit`) to
`src/Controller/BlogController.php`: one file's content changes,
every other input is byte-identical.

## Reproduction

```sh
# prerequisites: rust 1.94, git, composer, php, hyperfine
cargo xtask bench
```

## What is not compared

No PHPStan (or other tool) comparison is published at v0.0.x: the
preview runs a handful of diagnostic families while PHPStan runs
hundreds of rules, and a cross-scope timing comparison would be
meaningless at best and misleading at worst. The matched-scope
comparison is the v0.1 claim.

## Results

Not yet run. Filled by the protocol run on the hardware above; the
README links here.
```

- [ ] **Step 4: Commit**

```bash
git add benchmarks/PROTOCOL.md
git commit -m "📝 docs(benchmarks): commit the benchmark protocol"
```

---

### Task 9: The protocol run, the number, and the measured decisions

Run the protocol on the maintainer's machine, record the results, and close the two measured decisions 8a left open: the symbol-index pack (deferred from 8a, "its economics are measured in part 8b's benchmark before any code is written") and the diagnostics pack ("if measurement says the class loses, the pack is deleted").

**Files:**
- Modify: `benchmarks/PROTOCOL.md` (the Results section)
- Modify: `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md` (the amendment history)

**Interfaces:**
- Consumes: `cargo xtask bench` (Task 6), `benchmarks/PROTOCOL.md` (Task 8).

- [ ] **Step 1: Quiesce the machine and run the protocol three times**

Close heavyweight applications; plug in; do not use the machine during the runs.

Run, three times in a row: `cargo xtask bench`
Expected: three tables. For each scenario take the median of the three reported medians as the protocol result. If the three disagree wildly (more than ~20% spread on a warm scenario), the machine was not quiet — rerun.

- [ ] **Step 2: Record the results**

Replace the Results section of `benchmarks/PROTOCOL.md`:

```markdown
## Results

Protocol run of <date>, at commit `<short sha>`:

| Scenario | Median |
| --- | --- |
| Cold full | <X.XX> s |
| Warm no-change | <X.XX> s |
| Warm one-edit | <X.XX> s |

The warm one-edit number is the published number; the README links
here. (Recorded as the median of three protocol runs; the raw
hyperfine exports live under `target/bench/` and are not committed.)
```

- [ ] **Step 3: Close the two measured decisions**

The decision rules, fixed here so the outcome is measured, not argued:

- **Symbol-index pack** (deferred from 8a): if the warm one-edit median is sub-second, the pack is not built — the flagship target is met without it, and an artifact class that is not needed to meet the target does not pay for itself. If the median is at or over one second, **STOP and report to your human partner** with the three numbers: building the pack (or something else) becomes a scope decision for the human, not this plan.
- **Diagnostics pack** (shipped in 8a): the class pays if warm no-change is materially below cold full (at or under half). If it is not, **STOP and report** — per the spec the losing pack is deleted, and that reopens 8a code this plan does not touch on its own.

- [ ] **Step 4: Record the amendment**

Append to the amendment history of `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md` (after the existing 2026-07-13 entry, matching its style), with the measured values substituted:

```markdown
- <date> — the part 8b protocol run on the corpus (<N> PHP files):
  cold full <X.XX> s, warm no-change <X.XX> s, warm one-edit <X.XX> s
  (medians per `benchmarks/PROTOCOL.md`). Decisions: the symbol-index
  pack is not built — the warm one-edit target (sub-second) is met
  without it, so the class does not pay for itself; the diagnostics
  pack stays — warm no-change is <ratio> of cold full, comfortably
  under the one-half criterion. Both by the drop-a-losing-class rule
  of section 2.
```

(If either STOP case fired in Step 3, this amendment instead records what was measured and that the decision escalated, and the plan pauses there.)

- [ ] **Step 5: Commit**

```bash
git add benchmarks/PROTOCOL.md .claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md
git commit -m "📝 docs(benchmarks): record the protocol run and the measured decisions"
```

- [ ] **Step 6: Finish the branch**

Run the full gate one last time: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

Mark the pull request ready (`gh pr ready`), confirm every check is green (`gh pr checks`), then use the superpowers:finishing-a-development-branch skill.
