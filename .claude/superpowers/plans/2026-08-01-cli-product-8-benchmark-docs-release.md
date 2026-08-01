# CLI Product 8: Benchmark, Documentation, Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the CLI Product v0.1 sub-project: the pinned PHPStan comparison protocol with its CI ratio gate, the README and `docs/` pass, the CHANGELOG, and the `v0.1.0` tag.

**Architecture:** A new `cargo xtask benchmark` command measures PHPStan and Celerrate cold on the same corpus working tree in the same run and gates the ratio (at least 20x) in CI; the sub-second incremental claim is held by the existing `cargo xtask bench` protocol run on the reference machine. Documentation is a README landing-page rewrite plus three new documents (configuration, baseline, CI integration). The release is the version bump, the 0.1.0 CHANGELOG entry, the tag that triggers the existing release workflow, and a new workflow job that splits the Composer package to a read-only mirror for Packagist.

**Tech Stack:** Rust 1.94 (xtask, hyperfine-driven measurement), PHPStan pinned via Composer, GitHub Actions.

**Design spec:** `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` (sections 8, 9, 10) and the parent `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (published performance targets).

## Decisions taken with the user (2026-08-01)

These resolve ambiguities in the spec and bind this plan:

1. **The CI gate is the ratio only.** CI asserts the cold ratio (PHPStan
   median / Celerrate median, same runner, same run) is at least 20x. The
   sub-second incremental claim is verified by the protocol run on the
   reference machine and guarded structurally in CI by the existing
   `cargo xtask bench --ceilings` job. Rationale: the spec itself says
   "the gate is the ratio, not wall-clock", and the existing warm
   ceilings sit at 3.0 s because shared runners cannot hold sub-second.
2. **`v0.0.3` stays untagged.** The CHANGELOG `[0.0.3]` link is repointed
   at the merge commit that closed that release
   (`1fe4ef8277b11c1dc5a72a0a6cf7d8c77b4f2fb7`); `v0.1.0` is the first
   published release.
3. **The comparison is measured at matched scope.** Both tools analyze the
   same working tree root, the same file set: PHPStan's generated
   configuration lists the corpus working tree itself, not its `src/`
   subdirectory, so the volume PHPStan sees equals the volume
   `celerrate check .` sees. Rationale: the parent design's claim is worded
   "roughly 20x faster than PHPStan **at matched scope**", and an
   unmatched scope collapses the measurement into a comparison of PHP's
   startup cost. Measured on the reference machine: at the corpus's `src/`
   alone (34 files, 3424 lines) PHPStan takes about 2.1 s against
   Celerrate's 0.17 s, while at matched full-tree scope (9447 files,
   1302218 lines) PHPStan takes about 52 s against Celerrate's 1.5 s. Only
   the matched-scope figure can carry a 20x floor. PHPStan's memory limit
   rises to `4G` for the larger scope.
4. **Packagist ships via an automated subtree split.** A `release.yml`
   job pushes the split of `packages/composer-bootstrap` to the
   read-only mirror `celerrate/composer-bootstrap` and repeats the tag
   there. The push authenticates with a write-enabled deploy key on the
   mirror (no personal access token: `gh` cannot create one, but it can
   create the repository, register the key, and set the secret — Task
   11 scripts all of it). Only the initial Packagist submission stays
   manual (it needs the user's packagist.org account).

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` forbidden. Test modules may locally `#[allow]`.
- TDD: failing test, minimal implementation, refactor. Documentation tasks substitute a stated verification step for the test cycle.
- Everything in files is English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits, repository-configured identity, no Claude attribution, and no reference to plans, phases, or task numbers in any public-facing text (commits, documentation, README, CHANGELOG).
- Rust toolchain 1.94 (pinned in `rust-toolchain.toml`; every workflow uses `toolchain: "1.94"`).
- The corpus snapshot (`xtask/corpus-snapshot.txt`) and the mixed-rate baseline (`xtask/mixed-rate-baseline.txt`) must not move: no analysis code changes in this plan.
- `xtask` depends on no `celerrate_*` crate; it only spawns external tools and the built binary.
- Work happens on one branch off `main` (suggested name: `feat-cli-release`), one pull request, merged before the tag.

---

### Task 1: The pinned PHPStan harness dependency

**Files:**
- Create: `benchmarks/phpstan/composer.json`
- Create: `benchmarks/phpstan/composer.lock` (generated, committed)
- Create: `benchmarks/phpstan/.gitignore`

**Interfaces:**
- Produces: a deterministic `composer install` target at `benchmarks/phpstan/` whose `vendor/bin/phpstan` Task 2 invokes. The exact PHPStan version is pinned by the committed lock file.

- [ ] **Step 1: Write the Composer package**

`benchmarks/phpstan/composer.json`:

```json
{
    "description": "The pinned PHPStan installation the benchmark comparison protocol measures.",
    "require": {
        "php": ">=8.2",
        "phpstan/phpstan": "^2.1"
    },
    "config": {
        "sort-packages": true
    }
}
```

`benchmarks/phpstan/.gitignore`:

```
/vendor/
```

- [ ] **Step 2: Generate and commit the lock file**

Run: `composer update --no-interaction --working-dir benchmarks/phpstan`
Expected: `benchmarks/phpstan/composer.lock` exists and names one exact `phpstan/phpstan` version (record it; the protocol document in Task 4 states it).

- [ ] **Step 3: Verify the pinned install reproduces**

Run: `rm -rf benchmarks/phpstan/vendor && composer install --no-interaction --working-dir benchmarks/phpstan && php benchmarks/phpstan/vendor/bin/phpstan --version`
Expected: prints `PHPStan - PHP Static Analysis Tool <version>` matching the lock file.

- [ ] **Step 4: Commit**

```bash
git add benchmarks/phpstan/composer.json benchmarks/phpstan/composer.lock benchmarks/phpstan/.gitignore
git commit -m "📌 chore(benchmarks): pin the PHPStan installation the comparison measures"
```

---

### Task 2: The comparison harness, `cargo xtask benchmark`

**Files:**
- Create: `xtask/src/benchmark.rs`
- Modify: `xtask/src/bench.rs` (visibility only: `fn copy_directory` and `fn ensure_hyperfine` become `pub(crate) fn`)
- Modify: `xtask/src/lib.rs` (add `pub mod benchmark;` to the module list, and mention the comparison in the crate doc comment)
- Modify: `xtask/src/main.rs` (dispatch arms and the usage string)
- Test: unit tests inside `xtask/src/benchmark.rs`

**Interfaces:**
- Consumes: `crate::workspace_root()`, `crate::release_binary()`, `crate::corpus::prepare() -> Result<PathBuf>`, `crate::bench::{copy_directory, ensure_hyperfine, median_seconds}`, the Task 1 Composer package.
- Produces: `cargo xtask benchmark` (measure and print) and `cargo xtask benchmark --gate` (fail when the cold ratio is under 20x). Public functions for Task 4's protocol text: `benchmark::phpstan_version(&str) -> Result<String>`, `benchmark::cold_ratio(f64, f64) -> Result<f64>`, `benchmark::under_ratio_floor(f64, f64) -> Option<String>`, `benchmark::phpstan_configuration(&Path, &Path) -> String`.

- [ ] **Step 1: Write the failing unit tests**

Create `xtask/src/benchmark.rs` containing only the test module below, referencing the functions Step 3 defines (they do not exist yet, so the compile failure is the red state):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{cold_ratio, phpstan_configuration, phpstan_version, under_ratio_floor};

    #[test]
    fn the_phpstan_version_is_the_trailing_dotted_number() {
        let output = "PHPStan - PHP Static Analysis Tool 2.1.22\n";
        assert_eq!(phpstan_version(output).unwrap(), "2.1.22");
    }

    #[test]
    fn version_output_without_a_number_is_an_error_not_a_panic() {
        assert!(phpstan_version("").is_err());
        assert!(phpstan_version("PHPStan crashed").is_err());
    }

    #[test]
    fn the_cold_ratio_divides_the_medians() {
        assert_eq!(cold_ratio(30.0, 1.5).unwrap(), 20.0);
    }

    #[test]
    fn a_non_positive_celerrate_median_is_an_error() {
        assert!(cold_ratio(30.0, 0.0).is_err());
        assert!(cold_ratio(30.0, -1.0).is_err());
    }

    #[test]
    fn a_ratio_under_the_floor_is_named() {
        let failure = under_ratio_floor(19.9, 20.0).unwrap();
        assert!(failure.contains("19.9"));
        assert!(failure.contains("20"));
        assert!(under_ratio_floor(20.0, 20.0).is_none());
    }

    #[test]
    fn the_generated_configuration_pins_level_paths_and_temporary_directory() {
        let configuration = phpstan_configuration(
            std::path::Path::new("/work/corpus"),
            std::path::Path::new("/work/phpstan-tmp"),
        );
        assert!(configuration.contains("level: 5"));
        assert!(configuration.contains("- \"/work/corpus\""));
        assert!(configuration.contains("tmpDir: \"/work/phpstan-tmp\""));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package xtask benchmark`
Expected: compilation failure, the functions do not exist yet. (Add `pub mod benchmark;` to `xtask/src/lib.rs` now so the module is reachable.)

- [ ] **Step 3: Write the implementation**

The module body of `xtask/src/benchmark.rs` (above the test module):

```rust
//! The comparison harness behind the published PHPStan ratio: measure
//! PHPStan and Celerrate cold on the same corpus working tree in the
//! same run, and gate the ratio, not wall-clock — shared runners are
//! too noisy for absolute thresholds, but a ratio taken on one machine
//! in one run survives them. The sub-second incremental claim is held
//! by `cargo xtask bench` on the reference machine, not here.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// The pinned PHPStan rule level: level 5 is the closest match to the
/// enabled Celerrate families (unknown symbols and members, argument
/// checks); the residual asymmetry is disclosed in the protocol.
const PHPSTAN_RULE_LEVEL: u8 = 5;
const PHPSTAN_MEMORY_LIMIT: &str = "4G";
const PHPSTAN_COLD_RUNS: u32 = 3;
const CELERRATE_COLD_RUNS: u32 = 5;

/// The published claim's floor: at least 20x faster than PHPStan on a
/// cold full analysis. Gated as a same-machine ratio.
const COLD_RATIO_FLOOR: f64 = 20.0;

/// Runs the comparison and prints the medians and the ratio. With
/// `gate`, a ratio under the floor fails the run.
pub fn run(gate: bool) -> Result<()> {
    crate::bench::ensure_hyperfine()?;
    let root = crate::workspace_root()?;
    let corpus = crate::corpus::prepare()?;
    let binary = crate::release_binary()?;
    let phpstan = install_phpstan(&root)?;
    let version = installed_phpstan_version(&phpstan)?;

    let benchmark_directory = root.join("target/benchmark");
    let working = benchmark_directory.join("corpus");
    if working.exists() {
        std::fs::remove_dir_all(&working)?;
    }
    crate::bench::copy_directory(&corpus, &working)?;

    // The PHPStan temporary directory (its result cache) and the
    // generated configuration live outside the working tree: nothing
    // foreign enters the tree Celerrate analyzes.
    let temporary = benchmark_directory.join("phpstan-tmp");
    let configuration_path = benchmark_directory.join("phpstan.neon");
    std::fs::write(
        &configuration_path,
        phpstan_configuration(&working, &temporary),
    )?;

    println!(
        "phpstan {version}, rule level {PHPSTAN_RULE_LEVEL}, result cache off, \
         memory limit {PHPSTAN_MEMORY_LIMIT}"
    );

    let phpstan_command = format!(
        "'php' '{}' analyse --configuration '{}' --no-progress --memory-limit {PHPSTAN_MEMORY_LIMIT}",
        phpstan.display(),
        configuration_path.display(),
    );
    let phpstan_median = measure(
        &working,
        &phpstan_command,
        &format!("rm -rf '{}'", temporary.display()),
        PHPSTAN_COLD_RUNS,
        &benchmark_directory.join("phpstan-cold.json"),
    )?;

    let celerrate_command = format!("'{}' check .", binary.display());
    let celerrate_median = measure(
        &working,
        &celerrate_command,
        "rm -rf .celerrate",
        CELERRATE_COLD_RUNS,
        &benchmark_directory.join("celerrate-cold.json"),
    )?;

    let ratio = cold_ratio(phpstan_median, celerrate_median)?;
    println!("{:<16} {:>10}", "scenario", "median");
    println!("{:<16} {:>9.3}s", "phpstan cold", phpstan_median);
    println!("{:<16} {:>9.3}s", "celerrate cold", celerrate_median);
    println!("cold ratio: {ratio:.1}x");

    if gate {
        if let Some(failure) = under_ratio_floor(ratio, COLD_RATIO_FLOOR) {
            return Err(failure.into());
        }
    }
    Ok(())
}

/// One hyperfine invocation in the working tree. `--ignore-failure`
/// because both tools exit 1 when they report findings — a completed
/// analysis, not a failed one.
fn measure(
    working: &Path,
    command: &str,
    prepare: &str,
    runs: u32,
    export: &Path,
) -> Result<f64> {
    let status = Command::new("hyperfine")
        .current_dir(working)
        .args(["--ignore-failure", "--runs", &runs.to_string()])
        .arg("--export-json")
        .arg(export)
        .arg("--prepare")
        .arg(prepare)
        .arg(command)
        .status()?;
    if !status.success() {
        return Err(format!("hyperfine failed for: {command}").into());
    }
    crate::bench::median_seconds(&std::fs::read_to_string(export)?)
}

/// Installs the pinned PHPStan from the committed lock file and returns
/// the path of its executable.
fn install_phpstan(root: &Path) -> Result<PathBuf> {
    let package_directory = root.join("benchmarks/phpstan");
    let status = Command::new("composer")
        .current_dir(&package_directory)
        .args(["install", "--no-interaction", "--no-progress"])
        .status()?;
    if !status.success() {
        return Err("composer install failed for benchmarks/phpstan".into());
    }
    Ok(package_directory.join("vendor/bin/phpstan"))
}

fn installed_phpstan_version(phpstan: &Path) -> Result<String> {
    let output = Command::new("php").arg(phpstan).arg("--version").output()?;
    phpstan_version(&String::from_utf8_lossy(&output.stdout))
}

/// Extracts the version from `phpstan --version` output
/// ("PHPStan - PHP Static Analysis Tool 2.1.22").
pub fn phpstan_version(output: &str) -> Result<String> {
    output
        .split_whitespace()
        .last()
        .filter(|token| {
            !token.is_empty()
                && token
                    .chars()
                    .all(|character| character.is_ascii_digit() || character == '.')
        })
        .map(str::to_owned)
        .ok_or_else(|| format!("unreadable PHPStan version output: {output:?}").into())
}

/// The generated PHPStan configuration: pinned level, the whole corpus
/// working tree — the same file set `celerrate check .` walks, so the
/// two tools are compared at matched scope — and a result cache
/// directory outside the analyzed tree, wiped before every timed run so
/// every run is cold.
pub fn phpstan_configuration(analyzed_directory: &Path, temporary_directory: &Path) -> String {
    format!(
        "parameters:\n    level: {PHPSTAN_RULE_LEVEL}\n    paths:\n        - \"{}\"\n    tmpDir: \"{}\"\n",
        analyzed_directory.display(),
        temporary_directory.display(),
    )
}

/// The published ratio: PHPStan cold median over Celerrate cold median.
pub fn cold_ratio(phpstan_median: f64, celerrate_median: f64) -> Result<f64> {
    if celerrate_median <= 0.0 {
        return Err("the Celerrate cold median is not positive".into());
    }
    Ok(phpstan_median / celerrate_median)
}

/// The gate comparison, named on failure.
pub fn under_ratio_floor(ratio: f64, floor: f64) -> Option<String> {
    (ratio < floor).then(|| {
        format!("the cold ratio ({ratio:.1}x) is under its {floor:.0}x floor")
    })
}
```

In `xtask/src/bench.rs`, change the two visibilities (no other change):

```rust
pub(crate) fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
```

```rust
pub(crate) fn ensure_hyperfine() -> Result<()> {
```

In `xtask/src/main.rs`, add two arms next to the `bench` arms and extend the usage string with `benchmark [--gate]`:

```rust
["benchmark"] => xtask::benchmark::run(false),
["benchmark", "--gate"] => xtask::benchmark::run(true),
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package xtask benchmark`
Expected: all six new tests PASS.

- [ ] **Step 5: Run the harness end to end once**

Run: `cargo xtask benchmark`
Expected: prints the PHPStan version line, both cold medians, and the ratio. (This takes several minutes: three PHPStan cold runs.) Then `cargo xtask benchmark --gate` exits 0 (on the reference machine the ratio must clear 20x; if it does not, STOP and report — the published claim fails and that is a product finding, not a harness bug).

- [ ] **Step 6: Lint and format**

Run: `cargo clippy --package xtask --all-targets -- -D warnings && cargo fmt --all`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add xtask/src/benchmark.rs xtask/src/bench.rs xtask/src/lib.rs xtask/src/main.rs
git commit -m "✨ feat(xtask): measure the PHPStan comparison behind a cold-ratio gate"
```

---

### Task 3: The CI ratio gate

**Files:**
- Modify: `.github/workflows/corpus.yml` (new `benchmark` job; update the comment block above the jobs that lists which contexts are required)

**Interfaces:**
- Consumes: `cargo xtask benchmark --gate` from Task 2.
- Produces: a `benchmark` check context on every pull request and push to `main`.

- [ ] **Step 1: Add the job**

Append to `.github/workflows/corpus.yml`, following the required-context pattern (`if` on every step, not on the job — see the file's own comment about issue #89), after the `bench` job:

```yaml
  benchmark:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/cache@v4
        with:
          path: target/corpus
          key: corpus-${{ hashFiles('xtask/corpus.pin') }}
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: shivammathur/setup-php@v2
        with:
          php-version: "8.4"
          tools: composer
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: sudo apt-get update && sudo apt-get install --yes hyperfine
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo xtask benchmark --gate
```

Update the comment block at the top of the `jobs:` section: the snapshot, bench, **benchmark**, and phpdoc-cases contexts are required.

- [ ] **Step 2: Verify the workflow parses**

Run: `python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/corpus.yml'))"` (or `actionlint` if installed)
Expected: no error.

- [ ] **Step 3: Commit and verify on the branch**

```bash
git add .github/workflows/corpus.yml
git commit -m "👷 ci: hold the cold PHPStan ratio floor in the corpus workflow"
git push --set-upstream origin feat-cli-release
```

Then watch the `benchmark` job on the pushed branch (`gh run watch` or `gh pr checks` once the pull request exists): it must go green with a ratio printed in its log. If the ratio is under 20x on the runner while it clears comfortably locally, STOP and report the two numbers — the floor is the spec's claim and must not be quietly lowered.

---

### Task 4: The reference protocol run and the protocol rewrite

**Files:**
- Modify: `benchmarks/PROTOCOL.md`

**Interfaces:**
- Consumes: `cargo xtask bench`, `cargo xtask memory`, `cargo xtask benchmark` from Task 2.
- Produces: the measured numbers (cold, warm scenarios, PHPStan comparison, ratio, peak memory) that Task 8's README and Task 10's CHANGELOG cite. Record them in the task report.

This task runs on the reference machine (the Apple M5 the protocol already names). All numbers below marked «measured» come from this run; never carry forward a stale number.

- [ ] **Step 1: Run the full protocol**

Run, in order, recording every output:

```bash
cargo xtask fetch-corpus
cargo xtask bench          # five scenario medians
cargo xtask memory         # cold and warm peak RSS
cargo xtask benchmark      # PHPStan version, both cold medians, the ratio
php --version
php -r 'var_dump(ini_get("opcache.enable_cli"));'
git rev-parse --short HEAD
```

Also rerecord the per-scenario cache statistics: one manual `CELERRATE_CACHE_STATS=1` run per scenario, as the existing Results section does.
Expected: warm body-edit median under 1.0 s and the ratio at least 20x. If either target fails, STOP and report the numbers.

- [ ] **Step 2: Rewrite `benchmarks/PROTOCOL.md`**

Precise edits, section by section:

1. **Hardware and toolchain**: add two lines — the PHP version used by the comparison (from `php --version`) and the opcache state (`opcache.enable_cli` off is PHP's CLI default; state what was measured).
2. **New section "The comparison"**, after "Peak memory": everything the parent design requires the pinned protocol to state, with the measured values:
   - PHPStan version: the exact version from `benchmarks/phpstan/composer.lock` (Task 1).
   - Rule level: 5, and why (the closest match to the enabled Celerrate families: unknown symbols and members, argument checks; Celerrate additionally runs its nullability and version-gating families — disclosed, not hidden).
   - Result cache: explicitly off — the `tmpDir` lives outside the analyzed tree and is removed before every timed run.
   - Parallelism: PHPStan's default auto-detected worker count; Celerrate's default thread pool. Both defaults, both recorded.
   - Analyzed paths, matched scope: both tools walk the same corpus working tree root, vendor included (9447 files, 1302218 lines). State that the scope is matched deliberately, and why: at the corpus's `src/` alone (34 files, 3424 lines) both tools finish in about two seconds or less, where PHP's interpreter startup and autoloader dominate PHPStan's wall-clock and the measurement stops describing analysis throughput. State the small-scope numbers too, as the honest disclosure that the ratio narrows on small inputs.
   - Memory limit passed to PHPStan (`4G`, required by the matched scope), invocation line, run counts (3 PHPStan cold, 5 Celerrate cold), aggregate (median).
   - The harness: `cargo xtask benchmark`, reproducible by a third party.
3. **Replace "What is not compared"** with a short "The gate in CI" section: the v0.0.x position (no comparison published) is superseded by this protocol; in CI the gate is the same-machine ratio (floor 20x, `cargo xtask benchmark --gate`) because shared runners cannot hold absolute wall-clock; the sub-second incremental target is held on the reference machine by this protocol run and guarded structurally in CI by `cargo xtask bench --ceilings`.
4. **Substance**: fix the stale numbers to match `xtask/mixed-rate-baseline.txt`: 1046 of 4233 expressions (24.7 %) and 56 of 758 element positions (7.4 %).
5. **Results**: new run date and commit, the five-scenario table, a comparison table (PHPStan cold median, Celerrate cold median, ratio), the rerecorded cache statistics, updated peak RSS if it moved.
6. **Trajectory**: append this run against the 2026-07-18 one; replace the closing sentence (the "deliberately not published" comparison is now published above).

- [ ] **Step 3: Verify internal consistency**

Check: every number in the document matches Step 1's outputs; the PHPStan version matches the lock file; the substance numbers match `xtask/mixed-rate-baseline.txt` exactly (`grep -E "^expressions|^element-positions" xtask/mixed-rate-baseline.txt`).

- [ ] **Step 4: Commit**

```bash
git add benchmarks/PROTOCOL.md
git commit -m "📝 docs(benchmarks): publish the pinned PHPStan comparison protocol"
```

---

### Task 5: `docs/configuration.md`

**Files:**
- Create: `docs/configuration.md`
- Modify: `docs/diagnostics.md` (one cross-link, see Step 2)

**Interfaces:**
- Consumes: the shipped `celerrate.toml` surface. Verify every key name and behavior against `crates/celerrate_config/src/` and its tests before writing — the document describes the code, not the spec.
- Produces: the configuration reference Task 8's README links.

- [ ] **Step 1: Write the document**

Structure and content (prose in the repository's documentation voice — see `docs/migration.md` for tone):

1. **Discovery**: one `celerrate.toml` at the project root, next to `composer.json`. No tree-walking, no includes, no global user file. Zero configuration is fully supported: without the file, behavior is identical.
2. **The full surface**, as one annotated example (verify the exact key names in `crates/celerrate_config`):

```toml
[project]
php = "8.2"                 # optional; collapses the detected version range to a point
include = ["src", "tests"]  # optional; default: the Composer autoload roots
exclude = ["src/Generated"] # optional; subtracted from include

[rules.null-dereference]
enabled = false             # opt out of a Default-tier rule

[rules.some-nursery-rule]
enabled = true              # opt in to a Nursery-tier rule

[severity]
"CEL0034" = "warning"       # per-identifier remap, error <-> warning only
```

3. **`[project]`**: what each key does; how `php` interacts with the range detected from `composer.json`.
4. **`[rules]`**: activation is per rule; `enabled` is the only recognized key today (any other key produces a configuration diagnostic); the rule-name list with tiers — take the names from the "Rule names" subsection of `docs/diagnostics.md`; valid no-ops (`enabled = true` on a Default rule, `enabled = false` on a Nursery rule).
5. **`[severity]`**: `error` and `warning` only; no third state; resilience identifiers (parse errors, project notices) are neither disableable nor remappable — a remap entry on one is a configuration error.
6. **What is deliberately not configurable**: the baseline path, cache, threads, output format (CLI surface); per-identifier disabling (that is what suppression and the baseline are for).
7. **Errors are diagnostics**: unknown keys, unknown rule names, and invalid remaps produce CEL0043 to CEL0049, each with an explain page (`celerrate explain CEL0043`); a typo never silently disables nothing.
8. **Configuration and the cache**: the configuration participates in the cache key; changing it invalidates cached verdicts, an unchanged file keeps the warm path.

- [ ] **Step 2: Cross-link**

In `docs/diagnostics.md`, at the top of the "Configuration (CEL0043–CEL0049)" section (verify the exact heading), add one line pointing to `configuration.md` for the file's reference.

- [ ] **Step 3: Verify against the code**

For each documented behavior, name the test in `crates/celerrate_config` (or `celerrate_cli`) that pins it; anything you cannot anchor to a test or to observed CLI behavior does not go in the document. Run `cargo run --release --package celerrate_cli -- check` variants against a scratch fixture with a `celerrate.toml` if any behavior is unclear.

- [ ] **Step 4: Commit**

```bash
git add docs/configuration.md docs/diagnostics.md
git commit -m "📝 docs(configuration): document the celerrate.toml surface"
```

---

### Task 6: `docs/baseline.md`

**Files:**
- Create: `docs/baseline.md`
- Modify: `docs/diagnostics.md` (one cross-link from the baseline-notices section)

**Interfaces:**
- Consumes: the shipped baseline behavior. Verify flag names against `crates/celerrate_cli/src/arguments.rs` (`--baseline`, `--ignore-baseline`) and behaviors against `crates/celerrate_cli/src/baseline/` tests.
- Produces: the baseline guide Task 8's README and Task 7's CI document link.

- [ ] **Step 1: Write the document**

Structure and content:

1. **What it is**: adopt Celerrate on an existing codebase without fixing everything first; existing findings are recorded and hidden, only new problems fail the build.
2. **Recording**: `celerrate check --baseline` writes (or rewrites) `celerrate-baseline.toml` at the project root. Combining `--baseline` with `--fix`, `--fix-suggestions`, or `--watch` is a usage error.
3. **Applying**: a present file is applied automatically; a summary line announces how many findings were hidden; `--ignore-baseline` runs strict. Filtering happens after analysis and suppression, before rendering and the exit code — the baseline never enters the analysis cache.
4. **The file**: paste a real recorded file — generate one from a two-finding fixture (`celerrate check --baseline` in a scratch project), and show it: versioned header, deterministically sorted entries, reviewable diffs. Entries carry no line numbers: the key is the relative path, the CEL identifier, the enclosing symbol, the full message, and a count.
5. **The invariants, honestly stated**: an entry survives line movement (no line in the key); it dies with its diagnostic — a fix or a reworded message makes it obsolete, and obsolete entries are reported by an exit-neutral notice (name the exact identifier from the "Baseline notices" section of `docs/diagnostics.md` — CEL0050 or CEL0051, verify which is which) advising a re-record, never pruned silently; a count of N never hides occurrence N+1.
6. **Owned failure modes**: renaming a method orphans its entries and the findings resurface — noisy but honest; an engine upgrade that rewords messages does the same; re-record after either.
7. **Interaction with suppression**: suppression filters first (in-engine), the baseline second; adding a suppression makes the matching baseline entry obsolete — intended.
8. **In CI**: commit the file; pointer to `docs/ci.md` (Task 7).

- [ ] **Step 2: Cross-link and verify**

Add the pointer line in `docs/diagnostics.md`'s baseline-notices section. Verify each documented behavior against the baseline property tests (search `crates/celerrate_cli` for the baseline test module; the invariants above each have a test).

- [ ] **Step 3: Commit**

```bash
git add docs/baseline.md docs/diagnostics.md
git commit -m "📝 docs(baseline): document recording, applying, and obsolescence"
```

---

### Task 7: `docs/ci.md`

**Files:**
- Create: `docs/ci.md`
- Modify: `docs/output-formats.md` (one cross-link from its "GitHub Actions" section)

**Interfaces:**
- Consumes: the exit-code table in `docs/output-formats.md` (verify the exact codes there — do not invent them), `--output=github`, `--output=sarif`, the baseline document from Task 6, the install channels from `docs/installation.md`.
- Produces: the CI integration guide Task 8's README links.

- [ ] **Step 1: Write the document**

Structure and content:

1. **The short version**: a complete, copy-pastable GitHub Actions workflow:

```yaml
name: Celerrate
on:
  push:
    branches: [main]
  pull_request:
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Celerrate
        run: curl --fail --location https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh
      - name: Check
        run: ~/.local/bin/celerrate check --output=github .
```

   Note the Composer alternative for Composer projects: `composer require --dev celerrate/celerrate` in the project, then `vendor/bin/celerrate check --output=github .` (no separate install step). Verify the install script's default install directory and the raw URL against `docs/installation.md` and `install.sh` before committing.
2. **Exit codes**: reproduce the table from `docs/output-formats.md` (copied exactly), and what each means for a CI gate.
3. **Pull-request annotations**: `--output=github` emits workflow commands natively rendered on the diff; nothing to configure.
4. **SARIF upload** for GitHub code scanning:

```yaml
      - name: Check
        run: ~/.local/bin/celerrate check --output=sarif . > celerrate.sarif || true
      - uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: celerrate.sarif
```

   (The `|| true` is explained: exit 1 means findings were reported and the upload should still happen; gate on the upload or a separate plain run.)
5. **The baseline in CI**: commit `celerrate-baseline.toml`; CI runs plain `celerrate check` and only new problems fail; re-record locally with `--baseline` when adopting fixes. Link `docs/baseline.md`.
6. **Caching**: cache `.celerrate/` between runs for warm incremental analysis (an `actions/cache` example keyed on `composer.lock`), with the honest caveat that cold runs are already fast (link the protocol).

- [ ] **Step 2: Verify every command**

Run the workflow's commands locally where possible: the install one-liner in a scratch directory (against the latest existing release), `celerrate check --output=github` and `--output=sarif` on a fixture, confirming flags and output shapes. Fix anything that does not match.

- [ ] **Step 3: Commit**

```bash
git add docs/ci.md docs/output-formats.md
git commit -m "📝 docs(ci): document the CI integration, annotations, and the baseline flow"
```

---

### Task 8: The README landing page

**Files:**
- Modify: `README.md`
- Modify: `assets/benchmark-light.svg`, `assets/benchmark-dark.svg`
- Modify: `docs/installation.md`

**Interfaces:**
- Consumes: the measured numbers from Task 4 (cold, warm body-edit, PHPStan cold, the ratio), the documents from Tasks 5–7.
- Produces: the v0.1.0 landing page. Task 10's CHANGELOG links assume the README section names written here.

- [ ] **Step 1: Rewrite the stale claims**

Precise edits to `README.md`:

1. The blockquote `> Early preview (v0.0.3).` becomes a v0.1.0 statement (for example: `> v0.1.0 — the first public release.`), keeping the sentence that follows it honest about scope.
2. The headline claim keeps its shape but takes Task 4's measured numbers, and gains the comparison: state the measured ratio against PHPStan (with "at the pinned protocol" linking `benchmarks/PROTOCOL.md`).
3. **Installation**: remove the "from v0.1.0" hedge — `composer require --dev celerrate/celerrate` is now simply the Composer channel. Link `docs/installation.md`.
4. **Quick start**: add the migration one-liner (`celerrate migrate --from-phpstan`) with one sentence and a link to `docs/migration.md`.
5. **Performance**: update the table with Task 4's numbers and add the PHPStan comparison (PHPStan cold, Celerrate cold, the ratio); keep the protocol link.
6. **What works today**: add the product surface shipped since: `celerrate.toml` (link `docs/configuration.md`), the baseline (link `docs/baseline.md`), `--output=json|sarif|github` (link `docs/output-formats.md`), `celerrate migrate --from-phpstan` (link `docs/migration.md`), `--fix` and `celerrate explain`, CI integration (link `docs/ci.md`).
7. **What it does not do yet**: delete the baseline and output-formats lines (they shipped); keep and refresh the honest gaps: no LSP or editor integration, no formatter, no lint or style group, no taint analysis, Windows is tier 2, no Homebrew, Docker image, or GitHub Action yet.
8. **Roadmap**: align with the parent design's post-v0.1 sequence (daemon and LSP, taint analysis, lint group, formatter, more distribution channels), without dates.

- [ ] **Step 2: Update the two benchmark SVG assets**

Read `assets/benchmark-light.svg` first and mirror its structure and style. Both variants get: the bars updated to Task 4's numbers, plus a PHPStan cold bar on a linear scale — PHPStan's bar is the full-width reference, the Celerrate bars are proportionally tiny; that visual contrast is the message. Each bar labeled with its seconds. Keep both files small and hand-legible.

- [ ] **Step 3: Update `docs/installation.md`**

Remove the "(from v0.1.0)" qualifier from the Composer section; the channel is live at the release this branch produces.

- [ ] **Step 4: Verify the links**

Run: `grep -oE '\((docs|benchmarks|assets)/[^)]+\)' README.md | tr -d '()' | while read -r path; do [ -f "$path" ] || echo "MISSING $path"; done`
Expected: no output. Render the README locally (or via `gh` preview) and check both SVGs display.

- [ ] **Step 5: Commit**

```bash
git add README.md assets/benchmark-light.svg assets/benchmark-dark.svg docs/installation.md
git commit -m "📝 docs(readme): rewrite the README as the v0.1.0 landing page"
```

---

### Task 9: The Composer split in the release workflow

**Files:**
- Modify: `.github/workflows/release.yml` (new `split-composer` job)
- Create: `packages/composer-bootstrap/README.md`

**Interfaces:**
- Consumes: the existing `publish` job; the repository secret `COMPOSER_SPLIT_SSH_KEY` (the private half of a deploy key with write access on the mirror `celerrate/composer-bootstrap`) — both provisioned with `gh` in Task 11.
- Produces: on every `v*` tag, the mirror's `main` and the repeated tag — what Packagist indexes as `celerrate/celerrate`.

- [ ] **Step 1: Write the mirror's README**

`packages/composer-bootstrap/README.md` — this is what the Packagist page and the mirror repository show:

1. Title `# celerrate/celerrate`, one sentence: the Composer bootstrap package that installs the platform's `celerrate` binary and exposes `vendor/bin/celerrate`.
2. The install line: `composer require --dev celerrate/celerrate`.
3. How it works, three sentences: on install the plugin downloads the binary matching the package version from the GitHub release, verifies its SHA-256 checksum against the release's `SHA256SUMS`, refuses a mismatch.
4. A pointer: development happens in the monorepo (`https://github.com/celerrate/celerrate`, directory `packages/composer-bootstrap/`); this repository is a read-only split — issues and pull requests go to the monorepo.
5. License line: MIT OR Apache-2.0.

- [ ] **Step 2: Add the split job**

Append to `.github/workflows/release.yml`:

```yaml
  split-composer:
    if: startsWith(github.ref, 'refs/tags/')
    needs: publish
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
      - name: Push the package split and its tag to the read-only mirror
        env:
          SPLIT_KEY: ${{ secrets.COMPOSER_SPLIT_SSH_KEY }}
        run: |
          mkdir -p ~/.ssh
          printf '%s\n' "$SPLIT_KEY" > ~/.ssh/composer-split
          chmod 600 ~/.ssh/composer-split
          # GitHub's published Ed25519 host key, pinned rather than scanned.
          echo "github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl" >> ~/.ssh/known_hosts
          export GIT_SSH_COMMAND="ssh -i ~/.ssh/composer-split -o IdentitiesOnly=yes"
          mirror="git@github.com:celerrate/composer-bootstrap.git"
          split="$(git subtree split --prefix packages/composer-bootstrap HEAD)"
          git push --force "$mirror" "${split}:refs/heads/main"
          git push "$mirror" "${split}:refs/tags/${GITHUB_REF_NAME}"
```

The force push is deliberate and documented by the job name: the mirror is read-only output, never a merge target. `git subtree` ships with git on the runners and creates no new authorship. Before committing, verify the pinned host key against https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/githubs-ssh-key-fingerprints (the repository's checksum stance applies to host keys too).

- [ ] **Step 3: Verify locally what can be verified**

Run: `git subtree split --prefix packages/composer-bootstrap HEAD` locally.
Expected: prints a commit SHA; `git show --stat <sha>` shows `composer.json` at the split's root (Packagist's requirement). Then parse the workflow: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml packages/composer-bootstrap/README.md
git commit -m "👷 ci(release): split the Composer package to its read-only mirror"
```

---

### Task 10: The 0.1.0 CHANGELOG and the version bump

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `Cargo.toml` (workspace version)
- Modify: `Cargo.lock` (regenerated)

**Interfaces:**
- Consumes: the `[Unreleased]` block (already written release-note style), `cargo xtask release-notes` (extracts the `## [version]` block; `release.yml` greps `^version = "0.1.0"$` in `Cargo.toml` at the tag).
- Produces: the `[0.1.0]` entry the GitHub release notes are generated from.

- [ ] **Step 1: Cut the entry**

In `CHANGELOG.md`:

1. Insert a fresh, empty `## [Unreleased]` heading above the current one, and retitle the current block `## [0.1.0] - <date>` — use the planned tag date (adjust to the actual date at Step 4 of Task 11 if it slips).
2. Sweep the new `[0.1.0]` block for links into the repository pinned at `main` or unpinned; repoint them at `v0.1.0` (the convention the `[0.0.3]` entry set: `.../blob/v0.1.0/docs/...`).
3. Add to the `[0.1.0]` Added section, if the sweep finds them missing: the PHPStan comparison protocol and its CI ratio gate (`cargo xtask benchmark`), and the new documents (`docs/configuration.md`, `docs/baseline.md`, `docs/ci.md`) — these ship in this release too.
4. Replace the link references at the bottom:

```
[Unreleased]: https://github.com/celerrate/celerrate/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/celerrate/celerrate/releases/tag/v0.1.0
[0.0.3]: https://github.com/celerrate/celerrate/commit/1fe4ef8277b11c1dc5a72a0a6cf7d8c77b4f2fb7
[0.0.2]: https://github.com/celerrate/celerrate/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/celerrate/celerrate/releases/tag/v0.0.1
```

   (`[0.1.0]` is a tag link, not a compare link: a `v0.0.2...v0.1.0` compare would misattribute the 0.0.3 work. `[0.0.3]` points at the merge commit that closed that version; the tag was never pushed and stays unpushed.)

- [ ] **Step 2: Bump the version**

In `Cargo.toml`: `[workspace.package] version = "0.0.3"` becomes `version = "0.1.0"`. Then run `cargo check --workspace` to regenerate `Cargo.lock`.

- [ ] **Step 3: Verify the release-notes extraction**

Run: `cargo xtask release-notes 0.1.0 | head -20 && cargo xtask release-notes 0.1.0 | tail -5`
Expected: exit 0, prints the full `[0.1.0]` body, no trailing link-reference lines. Also verify the tag gate: `grep '^version = "0.1.0"$' Cargo.toml` exits 0.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "🔖 chore(release): cut the 0.1.0 changelog and bump the workspace version"
```

---

### Task 11: Closure — gates, spec, tag, and the published release

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` (status and closure record)

**Interfaces:**
- Consumes: everything above, merged to `main`.
- Produces: the `v0.1.0` tag, the GitHub release, the Packagist package, the closed sub-project.

- [ ] **Step 1: Run the full gate suite locally**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo xtask dependency-shape
cargo xtask emission-scan
cargo xtask compile-stubs --check
cargo xtask phpdoc-cases --check
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
cargo xtask bench --ceilings
cargo xtask benchmark --gate
```

Expected: every command exits 0. The corpus snapshot and mixed-rate baseline are byte-identical (this plan touches no analysis code); any delta is a STOP-and-report.

- [ ] **Step 2: Record the closure in the spec**

In `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`: change `Status: Draft` to `Status: Closed (v0.1.0, <date>)`, and append a short closure section walking the nine closure gates of section 1, each with where it is held (the test, workflow job, or document). Record the two deviations as amendments: the CI gate asserts the cold ratio only (the sub-second incremental target is held by the protocol run on the reference machine), and Packagist publication rides a subtree-split mirror (`celerrate/composer-bootstrap`). Commit: `📝 docs(spec): record the v0.1.0 closure and its two release amendments`.

- [ ] **Step 3: Open the pull request and merge**

Push, open the pull request describing the release (benchmark protocol, documentation pass, release plumbing — plain words, no internal artifact references), wait for every check including the new `benchmark` context, merge into `main`.

- [ ] **Step 4: Provision the release plumbing with `gh` (scripted; confirm with the user before running — it creates a public repository and edits branch protection)**

```bash
gh repo create celerrate/composer-bootstrap --public \
  --description "Read-only split of the celerrate/celerrate Composer package; development happens in celerrate/celerrate."
ssh-keygen -t ed25519 -N "" -C "celerrate release split" -f "$SCRATCH/composer-split"
gh repo deploy-key add "$SCRATCH/composer-split.pub" --repo celerrate/composer-bootstrap \
  --title "release split push" --allow-write
gh secret set COMPOSER_SPLIT_SSH_KEY --repo celerrate/celerrate < "$SCRATCH/composer-split"
rm "$SCRATCH/composer-split" "$SCRATCH/composer-split.pub"
gh api --method POST repos/celerrate/celerrate/branches/main/protection/required_status_checks/contexts \
  -f "contexts[]=benchmark"
```

`$SCRATCH` is any private scratch directory; the key pair exists only long enough to be registered, and the private half lives nowhere but the repository secret. Verify: `gh repo view celerrate/composer-bootstrap` succeeds, `gh secret list --repo celerrate/celerrate` shows `COMPOSER_SPLIT_SSH_KEY`, and `gh api repos/celerrate/celerrate/branches/main/protection/required_status_checks/contexts` lists `benchmark`.

- [ ] **Step 5: Tag (confirm with the user immediately before pushing)**

```bash
git checkout main && git pull
git tag v0.1.0
git push origin v0.1.0
```

- [ ] **Step 6: Watch the release workflow to completion**

`gh run watch` on the Release workflow: five builds green, `publish` green (tag/version gate, checksums, attestations, release created with notes from the CHANGELOG), `split-composer` green. Then verify: the release page lists the five archives plus `SHA256SUMS`; `gh api repos/celerrate/composer-bootstrap/tags` lists `v0.1.0`; the mirror's root shows `composer.json` and the README.

- [ ] **Step 7: Submit to Packagist (manual, one time)**

The user submits `https://github.com/celerrate/composer-bootstrap` on packagist.org (the package name `celerrate/celerrate` comes from its `composer.json`) and enables the GitHub hook for auto-updates.

- [ ] **Step 8: Post-release smoke tests**

In a scratch directory:

```bash
curl --fail --location https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh
~/.local/bin/celerrate --version   # expected: 0.1.0
```

And in a scratch Composer project (once Packagist has indexed, usually minutes):

```bash
composer require --dev celerrate/celerrate
vendor/bin/celerrate --version     # expected: 0.1.0
```

Report both outputs verbatim. The sub-project is closed when both channels deliver the released binary.
