# Benchmark Comparison Corpus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pin a comparison corpus whose first-party code is large enough to separate Celerrate from PHPStan, publish the measured cold ratio, wire the gate weekly and into the release workflow, and unblock the `v0.1.0` tag.

**Architecture:** A second pin file (`xtask/comparison-corpus.pin`) feeds the existing `cargo xtask benchmark` harness through new `comparison_*` functions in `xtask/src/corpus.rs`; the analysis corpus, its snapshot, and the mixed-rate baseline do not move. The gate runs in a new weekly workflow and as a required job in the release workflow, never per pull request.

**Tech Stack:** Rust (xtask), hyperfine, Composer/PHPStan, GitHub Actions.

**Spec:** `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`

## Global Constraints

- Zero panic, mechanically enforced: no `unwrap`/`expect`/indexing/`panic` in production code; test modules may locally `#[allow]` (existing xtask test modules already do).
- TDD: failing test before implementation wherever a unit is testable.
- The analysis corpus is untouched: `xtask/corpus.pin`, the corpus snapshot, and the mixed-rate baseline must be byte-identical at the end (`cargo xtask corpus` and `cargo xtask mixed-rate` both pass unchanged).
- Everything committed is English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits, repository-configured identity, no Claude attribution.
- Public-facing text (commits, README, PROTOCOL.md, workflow comments) never references plans, specs, phases, or tasks by number; referencing public issues (#118) is fine.
- Execution branch: `feat-comparison-corpus`, created from `main` after the spec/plan documentation pull request merges.

---

### Task 1: Scout the comparison candidate

Empirical, commits nothing. Produces the values every later task consumes: the pinned repository and commit, the first-party file count, the trial medians, and their spread. The spec's acceptance criteria decide; if Shopware 6 fails, the same steps run on the fallbacks in order.

**Files:**
- No repository changes. Working area: `target/scouting/` (gitignored under `target/`).

**Interfaces:**
- Produces: `SCOUTED_REPOSITORY` (URL), `SCOUTED_COMMIT` (40-hex SHA), `FIRST_PARTY_COUNT`, `TOTAL_COUNT`, the three cold wall-clock times of each tool, and the per-tool spread. Later tasks reference these names.

- [ ] **Step 1: Pick the candidate commit**

The candidate is the latest stable (numeric, non-RC) release tag of `shopware/shopware`. List the peeled tag SHAs and take the newest:

```bash
git ls-remote --tags https://github.com/shopware/shopware.git \
  | grep -E 'refs/tags/v6\.[0-9]+\.[0-9]+\.[0-9]+\^\{\}$' \
  | sort -t/ -k3 -V | tail -3
```

Record the last line: the first column is `SCOUTED_COMMIT`, the tag name (without `^{}`) is the human label. `SCOUTED_REPOSITORY` is `https://github.com/shopware/shopware`.

- [ ] **Step 2: Fetch the candidate the way the pin machinery will**

```bash
ROOT="$(git rev-parse --show-toplevel)"
SCOUT="$ROOT/target/scouting/shopware"
mkdir -p "$SCOUT" && cd "$SCOUT"
git init --quiet
git fetch --quiet --depth 1 https://github.com/shopware/shopware.git "$SCOUTED_COMMIT"
git checkout --quiet --detach FETCH_HEAD
```

- [ ] **Step 3: Check the committed lock file criterion**

```bash
test -f "$SCOUT/composer.lock" && echo "lock: present" || echo "lock: MISSING"
```

A missing `composer.lock` fails the candidate outright: the vendor tree must install reproducibly from the corpus's own lock, as the analysis corpus does.

- [ ] **Step 4: Install the vendor tree**

```bash
cd "$SCOUT"
composer install --no-interaction --no-progress --no-scripts --no-plugins --ignore-platform-reqs
```

- [ ] **Step 5: Count the corpus**

```bash
cd "$SCOUT"
find . -path ./vendor -prune -o -name '*.php' -print | wc -l   # FIRST_PARTY_COUNT
find . -name '*.php' | wc -l                                   # TOTAL_COUNT
```

Criterion: `FIRST_PARTY_COUNT >= 3000`.

- [ ] **Step 6: Trial-run Celerrate once**

```bash
cd "$ROOT"
cargo build --release
target/release/celerrate check "$SCOUT"; echo "exit: $?"
```

Criterion: the run completes with exit 0 (clean) or 1 (findings). An internal-error exit or a crash fails the candidate.

- [ ] **Step 7: Trial-run pinned PHPStan once**

Install the pinned PHPStan and generate the same configuration shape the harness generates (level, root path, vendor excluded, temporary directory outside the tree):

```bash
composer install --no-interaction --no-progress --working-dir="$ROOT/benchmarks/phpstan"
cat > "$ROOT/target/scouting/phpstan.neon" <<EOF
parameters:
    level: 5
    paths:
        - "$SCOUT"
    excludePaths:
        - "$SCOUT/vendor"
    tmpDir: "$ROOT/target/scouting/phpstan-tmp"
EOF
php "$ROOT/benchmarks/phpstan/vendor/bin/phpstan" analyse \
  --configuration "$ROOT/target/scouting/phpstan.neon" \
  --no-progress --memory-limit 2G; echo "exit: $?"
```

Criterion: exit 0 or 1 (a finished analysis). If PHPStan dies on its memory limit, retry once with `--memory-limit 4G`; if that finishes, record that the harness constant `PHPSTAN_MEMORY_LIMIT` must move to `4G` (consumed by Task 3). Any other crash fails the candidate.

- [ ] **Step 8: Measure three cold runs of each tool**

```bash
cd "$SCOUT"
hyperfine --ignore-failure --runs 3 \
  --prepare "rm -rf $ROOT/target/scouting/phpstan-tmp" \
  --export-json "$ROOT/target/scouting/phpstan-cold.json" \
  "php $ROOT/benchmarks/phpstan/vendor/bin/phpstan analyse --configuration $ROOT/target/scouting/phpstan.neon --no-progress --memory-limit 2G"
hyperfine --ignore-failure --runs 3 \
  --prepare "rm -rf .celerrate" \
  --export-json "$ROOT/target/scouting/celerrate-cold.json" \
  "$ROOT/target/release/celerrate check ."
```

(Use `--memory-limit 4G` above if Step 7 required it.) From each JSON's `results[0].times`, compute the per-tool spread `(max - min) / min`. Criterion: both spreads under 10 %. Record both medians and the ratio of medians.

- [ ] **Step 9: Decide, and stop if a fallback fired**

If every criterion passed, Shopware 6 is the corpus; hand `SCOUTED_REPOSITORY`, `SCOUTED_COMMIT`, the counts, medians, and spreads to Task 2. If any criterion failed, repeat Steps 1-8 with the next candidate, in order:

- PrestaShop: `https://github.com/PrestaShop/PrestaShop`, stable tags match `refs/tags/[0-9]+\.[0-9]+\.[0-9]+\^\{\}$` (no suffix).
- phpMyAdmin: `https://github.com/phpmyadmin/phpmyadmin`, stable tags match `refs/tags/RELEASE_[0-9_]+\^\{\}$`.

**STOP before committing any pin if a fallback fired**: report to the user which candidate failed, on which criterion, with the numbers, and wait for their go-ahead on the fallback.

---

### Task 2: The comparison pin and its preparation path

**Files:**
- Create: `xtask/comparison-corpus.pin`
- Modify: `xtask/src/corpus.rs` (new functions next to `pin()`/`snapshot_directory()`/`prepare()`, lines 13-33; new tests in its existing `mod tests`)

**Interfaces:**
- Consumes: `SCOUTED_REPOSITORY`, `SCOUTED_COMMIT` from Task 1.
- Produces: `xtask::corpus::comparison_pin() -> Result<Pin>`, `xtask::corpus::comparison_snapshot_directory() -> Result<PathBuf>`, `xtask::corpus::prepare_comparison() -> Result<PathBuf>` (fetches and returns the corpus root, vendor installed).

- [ ] **Step 1: Write the failing tests**

In the existing `mod tests` at the bottom of `xtask/src/corpus.rs`:

```rust
#[test]
fn the_comparison_pin_is_committed_and_well_formed() {
    let pin = super::comparison_pin().unwrap();
    assert!(pin.repository.starts_with("https://github.com/"));
}

#[test]
fn the_comparison_snapshot_lives_in_its_own_directory() {
    let directory = super::comparison_snapshot_directory().unwrap();
    let text = directory.display().to_string();
    assert!(text.contains("comparison-corpus"));
    assert!(text.ends_with(&super::comparison_pin().unwrap().commit));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p xtask comparison`
Expected: compilation fails, `comparison_pin` and `comparison_snapshot_directory` not found.

- [ ] **Step 3: Write the pin file and the implementation**

`xtask/comparison-corpus.pin` (commit SHA from Task 1):

```
# The pinned PHPStan comparison corpus: a large real application whose
# first-party code is big enough that rule-checking dominates both
# tools' wall clocks (issue #118). This pin serves the published cold
# ratio only: the analysis corpus, its diagnostic snapshot, and the
# mixed-rate baseline live under xtask/corpus.pin and never move with
# this file. Bump deliberately, re-run the reference protocol
# (benchmarks/PROTOCOL.md), and publish the re-measured ratio together
# with the bump.
repository = <SCOUTED_REPOSITORY>
commit = <SCOUTED_COMMIT>
```

In `xtask/src/corpus.rs`, after `prepare()`:

```rust
/// Reads and parses the committed comparison-corpus pin.
pub fn comparison_pin() -> Result<Pin> {
    crate::pin::read(&crate::workspace_root()?.join("xtask/comparison-corpus.pin"))
}

/// Where the comparison corpus lives: separate from the analysis
/// corpus, so bumping either pin never invalidates the other's
/// snapshot.
pub fn comparison_snapshot_directory() -> Result<PathBuf> {
    Ok(crate::workspace_root()?
        .join("target/comparison-corpus")
        .join(comparison_pin()?.commit))
}

/// Fetches the comparison corpus and installs its vendor tree; returns
/// the corpus root, ready to be measured.
pub fn prepare_comparison() -> Result<PathBuf> {
    let directory = comparison_snapshot_directory()?;
    crate::pin::fetch_snapshot(&comparison_pin()?, &directory)?;
    install_vendor(&directory)?;
    Ok(directory)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p xtask comparison`
Expected: both tests PASS.

- [ ] **Step 5: Lint and format**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

- [ ] **Step 6: Commit**

```bash
git add xtask/comparison-corpus.pin xtask/src/corpus.rs
git commit -m "✨ feat(xtask): pin the PHPStan comparison corpus"
```

---

### Task 3: Point the harness at the comparison corpus

**Files:**
- Modify: `xtask/src/benchmark.rs` (module documentation lines 1-7, the `prepare` call line 41, the `COLD_RATIO_FLOOR` documentation lines 25-34, the `phpstan_configuration` documentation lines 156-172; and `PHPSTAN_MEMORY_LIMIT` line 21 only if Task 1 Step 7 required 4G)

**Interfaces:**
- Consumes: `xtask::corpus::prepare_comparison()` from Task 2.
- Produces: `cargo xtask benchmark [--gate]` measuring the comparison corpus. Signatures of `run`, `cold_ratio`, `under_ratio_floor`, `phpstan_configuration` unchanged.

- [ ] **Step 1: Swap the corpus and rewrite the stale documentation**

In `run()`, replace:

```rust
    let corpus = crate::corpus::prepare()?;
```

with:

```rust
    let corpus = crate::corpus::prepare_comparison()?;
```

Replace the module documentation (lines 1-7) with:

```rust
//! The PHPStan comparison harness: measure PHPStan and Celerrate cold
//! on the same working tree in the same run, and gate the ratio, not
//! wall-clock — shared runners are too noisy for absolute thresholds,
//! but a ratio taken on one machine in one run survives them. The
//! subject is the pinned comparison corpus
//! (`xtask/comparison-corpus.pin`), not the analysis corpus: a
//! publishable ratio needs first-party code large enough that
//! rule-checking dominates both wall clocks (issue #118). The
//! sub-second incremental claim is held by `cargo xtask bench` on the
//! reference machine, not here.
```

Replace the `COLD_RATIO_FLOOR` documentation (lines 25-34, keep the value `20.0` for now; Task 4 sets the measured value) with:

```rust
/// The gate floor for the cold ratio. Set from the reference run on
/// the comparison corpus: half the measured median ratio, so
/// shared-runner variance does not fail a healthy build while a real
/// regression — anything that halves the advantage — still does. The
/// value below is provisional until the reference run lands.
```

In the `phpstan_configuration` documentation (lines 156-172), replace the sentence citing symfony/demo's counts ("Pointing PHPStan at the tree root ... 51 project files Celerrate reports on.") with:

```text
Pointing PHPStan at the tree root without the exclusion would
rule-check the entire installed vendor tree nobody asks it to check,
against only the first-party files Celerrate reports on.
```

and drop the sentence about "the corpus's own `phpstan.dist.neon`" (it described symfony/demo specifically).

If Task 1 Step 7 recorded the 4G requirement, change line 21 to `const PHPSTAN_MEMORY_LIMIT: &str = "4G";` and reword its documentation comment to name the comparison corpus as what sized it.

- [ ] **Step 2: Run the unit tests, lint, format**

Run: `cargo test -p xtask && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: all xtask tests PASS (none encode the corpus choice).

- [ ] **Step 3: Commit**

```bash
git add xtask/src/benchmark.rs
git commit -m "✨ feat(xtask): measure the comparison corpus in the benchmark harness"
```

---

### Task 4: The reference run and the ratio floor

Runs on the reference machine named in `benchmarks/PROTOCOL.md` (the Apple M5 this project develops on).

**Files:**
- Modify: `xtask/src/benchmark.rs:34` (the `COLD_RATIO_FLOOR` value and the final sentence of its documentation)

**Interfaces:**
- Consumes: the harness from Task 3.
- Produces: `MEASURED_PHPSTAN_MEDIAN`, `MEASURED_CELERRATE_MEDIAN`, `MEASURED_RATIO` (consumed by Task 5), and the committed floor.

- [ ] **Step 1: Run the reference measurement**

Run: `cargo xtask benchmark`
Record from the output: `phpstan cold` median (`MEASURED_PHPSTAN_MEDIAN`), `celerrate cold` median (`MEASURED_CELERRATE_MEDIAN`), `cold ratio` (`MEASURED_RATIO`). The hyperfine exports stay in `target/benchmark/` for inspection.

- [ ] **Step 2: Set the floor to half the measured ratio**

Replace the provisional value: `COLD_RATIO_FLOOR` becomes `MEASURED_RATIO / 2`, rounded down to one decimal (for example, a measured 8.7x gives `4.3`). Replace the documentation's last sentence ("The value below is provisional until the reference run lands.") with the measurement record:

```rust
/// Reference measurement (2026-08-02, the protocol machine): PHPStan
/// <MEASURED_PHPSTAN_MEDIAN>s, Celerrate <MEASURED_CELERRATE_MEDIAN>s,
/// ratio <MEASURED_RATIO>x.
```

- [ ] **Step 3: Verify the gate passes**

Run: `cargo xtask benchmark --gate`
Expected: exits 0, printing a ratio comfortably above the floor.

- [ ] **Step 4: Lint, format, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
git add xtask/src/benchmark.rs
git commit -m "📈 feat(xtask): gate the cold ratio on the reference measurement"
```

---

### Task 5: The published documents

**Files:**
- Modify: `benchmarks/PROTOCOL.md` (new comparison-corpus section; replace the withheld-comparison statements)
- Modify: `README.md` (state the measured ratio)
- Modify: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (amend the "~20x" ambition in section 7)

**Interfaces:**
- Consumes: `SCOUTED_REPOSITORY`, `SCOUTED_COMMIT`, `FIRST_PARTY_COUNT`, `TOTAL_COUNT` from Task 1; `MEASURED_*` from Task 4.

- [ ] **Step 1: Read both public documents first**

Read `benchmarks/PROTOCOL.md` and `README.md` end to end. Find every statement that the comparison is withheld, unmeasured, or waiting on a corpus (search: `withheld`, `withdrawn`, `unmeasured`, `#118`, `PHPStan`). Those statements are what this task replaces; leave the absolute-numbers sections (the five scenario medians, memory) untouched.

- [ ] **Step 2: Add the comparison section to the protocol**

After the existing `## Corpus` section, add (filling the recorded values):

```markdown
## Comparison corpus

The published PHPStan ratio is measured on a second pinned corpus,
separate from the analysis corpus above, because a comparison needs
first-party code large enough that rule-checking dominates both wall
clocks (issue #118 records why symfony/demo cannot carry one).

- Repository: <SCOUTED_REPOSITORY>
- Commit: `<SCOUTED_COMMIT>` (committed in `xtask/comparison-corpus.pin`)
- Vendor tree: installed from the corpus's own `composer.lock`, same
  flags as the analysis corpus
- Size: <TOTAL_COUNT> PHP files, of which <FIRST_PARTY_COUNT> are
  first-party and the rest are the installed vendor tree

Both tools do the same reported work: Celerrate parses and indexes the
whole tree and rule-checks the first-party files; PHPStan (pinned in
`benchmarks/phpstan/composer.lock`, rule level 5, result cache wiped
before every timed run, vendor excluded from analysis but loaded for
reflection) is given exactly the first-party set. The harness is
`cargo xtask benchmark`; `--gate` fails under the committed floor
(`COLD_RATIO_FLOOR` in `xtask/src/benchmark.rs`), half the reference
ratio below.

Measured on the reference machine (medians, cold):

| | median |
| --- | --- |
| PHPStan | <MEASURED_PHPSTAN_MEDIAN> s |
| Celerrate | <MEASURED_CELERRATE_MEDIAN> s |
| ratio | <MEASURED_RATIO>x |

The gate runs weekly (`.github/workflows/benchmark.yml`) and as a
required job before any release publishes (`.github/workflows/release.yml`).
```

Then delete or rewrite every withheld-comparison statement found in Step 1 so the document reads as one voice: the comparison exists, here is the number.

- [ ] **Step 3: State the ratio in the README**

Replace the README's no-published-comparison statement with one sentence in the same place, filling the values:

```markdown
On the pinned comparison corpus (<FIRST_PARTY_COUNT> first-party PHP
files), a cold `celerrate check` completes <MEASURED_RATIO>x faster
than PHPStan at rule level 5 on the same first-party set; the pinned
protocol and the full numbers live in
[benchmarks/PROTOCOL.md](benchmarks/PROTOCOL.md).
```

Check `docs/installation.md` and the two benchmark SVG assets referenced by the README for any stale comparison claim; update only what carries one.

- [ ] **Step 4: Amend the parent design**

In `.claude/superpowers/specs/2026-07-09-celerrate-design.md`, find the "at least ~20x faster than PHPStan" target in section 7 and annotate it in place: the ambition is replaced by the measured `<MEASURED_RATIO>x` on the pinned comparison corpus (2026-08-02), met or not, without defensive rewording.

- [ ] **Step 5: Commit**

```bash
git add benchmarks/PROTOCOL.md README.md docs/ .claude/superpowers/specs/2026-07-09-celerrate-design.md
git commit -m "📝 docs(benchmarks): publish the measured PHPStan comparison"
```

---

### Task 6: The two workflows

**Files:**
- Create: `.github/workflows/benchmark.yml`
- Modify: `.github/workflows/release.yml` (new `benchmark-gate` job; `publish.needs` gains it)
- Modify: `.github/workflows/corpus.yml` (the "There is no PHPStan comparison job" comment block)

**Interfaces:**
- Consumes: `cargo xtask benchmark --gate` from Task 4.
- Produces: the `comparison-gate` weekly job and the `benchmark-gate` release job.

- [ ] **Step 1: Write the weekly workflow**

`.github/workflows/benchmark.yml`:

```yaml
name: Benchmark

on:
  schedule:
    - cron: "17 5 * * 1"
  workflow_dispatch:

permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always

jobs:
  comparison-gate:
    runs-on: ubuntu-latest
    # A hung PHPStan run would otherwise hold the runner for GitHub's
    # six-hour default. The budget covers a cold Cargo cache, a cold
    # corpus fetch with its composer install, the release build, and
    # the timed runs themselves.
    timeout-minutes: 120
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - uses: Swatinem/rust-cache@v2
      - uses: actions/cache@v4
        with:
          path: target/comparison-corpus
          key: comparison-corpus-${{ hashFiles('xtask/comparison-corpus.pin') }}
      - uses: shivammathur/setup-php@v2
        with:
          php-version: "8.4"
          tools: composer
      - run: sudo apt-get update && sudo apt-get install --yes hyperfine
      - run: cargo xtask benchmark --gate
```

- [ ] **Step 2: Add the release gate**

In `.github/workflows/release.yml`, add after the `build` job a `benchmark-gate` job with exactly the same steps as `comparison-gate` above (checkout, toolchain `1.94`, rust-cache, the comparison-corpus cache, setup-php `8.4` with composer, hyperfine, `cargo xtask benchmark --gate`, `timeout-minutes: 120`), and change the `publish` job's dependency line from:

```yaml
    needs: build
```

to:

```yaml
    needs: [build, benchmark-gate]
```

- [ ] **Step 3: Repoint the corpus workflow comment**

In `.github/workflows/corpus.yml`, replace the seven-line comment block starting "# There is no PHPStan comparison job." with:

```yaml
  # The PHPStan comparison gate does not run per pull request: it runs
  # weekly in benchmark.yml and as a required job before a release
  # publishes in release.yml. See benchmarks/PROTOCOL.md.
```

- [ ] **Step 4: Verify the workflows parse**

Run: `ruby -ryaml -e 'YAML.load_file(".github/workflows/benchmark.yml"); YAML.load_file(".github/workflows/release.yml"); YAML.load_file(".github/workflows/corpus.yml"); puts "ok"'`
Expected: `ok`. If `actionlint` is installed, run it too.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/benchmark.yml .github/workflows/release.yml .github/workflows/corpus.yml
git commit -m "👷 ci(benchmarks): gate the cold ratio weekly and before releases"
```

---

### Task 7: Gates, closure, and the tag

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` (status, amendment 3, gate 7, state of play)
- Modify: `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md` (status)
- Modify: `CHANGELOG.md` (date the 0.1.0 entry)

**Interfaces:**
- Consumes: everything above, merged.

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

Every command must pass. `corpus` and `mixed-rate` passing unchanged proves the analysis corpus did not move.

- [ ] **Step 2: Close the release spec**

In `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`:
- Status line: `Status: Closed (v0.1.0, <today's date>)`.
- Amendment 3: append that the comparison is no longer withheld — it is published on the pinned comparison corpus (name repository and commit) with the measured ratio, and the gate runs weekly and before releases.
- Gate 7 in the state-of-play section: rewrite as fully held, naming `xtask/comparison-corpus.pin`, `benchmarks/PROTOCOL.md`, `.github/workflows/benchmark.yml`, and the release gate.
- The "State of play" introduction: record that both open items (the published comparison, the tag) are resolved.

In `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`: `Status: Closed (implemented, <today's date>)`.

- [ ] **Step 3: Date the CHANGELOG entry**

In `CHANGELOG.md`, change `## [Unreleased]` to `## [0.1.0] - <today's date>` and add a fresh empty `## [Unreleased]` section above it. Verify the extraction:

Run: `cargo xtask release-notes 0.1.0`
Expected: prints the 0.1.0 body, exit 0.

- [ ] **Step 4: Commit and open the pull request**

```bash
git add .claude/superpowers/specs/ CHANGELOG.md
git commit -m "🔖 chore(release): date the 0.1.0 entry and record the closure"
git push -u origin feat-comparison-corpus
gh pr create --title "📈 feat(benchmarks): publish the PHPStan comparison on a corpus that carries it" \
  --body "Pins a comparison corpus whose first-party code separates the two analyzers, publishes the measured cold ratio in benchmarks/PROTOCOL.md and the README, and gates it weekly and before releases. Closes #118"
```

Wait for CI; merge on green (user merges or approves the merge).

- [ ] **Step 5: Run the weekly workflow once by hand**

After the merge:

```bash
gh workflow run Benchmark
gh run watch
```

Expected: the `comparison-gate` job completes green within its budget.

- [ ] **Step 6: Tag v0.1.0**

```bash
git checkout main && git pull --ff-only
git tag v0.1.0
git push origin v0.1.0
gh run watch
```

Expected: the release workflow runs `build` (five targets), `benchmark-gate`, `publish` (checksums, attestation, GitHub release), and `split-composer`, all green. Confirm the release exists: `gh release view v0.1.0`.

- [ ] **Step 7: Confirm the issue closed**

`gh issue view 118` shows CLOSED (the pull request body closes it on merge). If not, close it with a comment linking the release.

---

## Self-review notes

- Spec coverage: section 3 → Task 2; section 4 → Task 1; section 5 → Tasks 3-4; section 6 → Task 6; section 7 → Task 5; section 8 → Task 7; section 9 → Tasks 2-4 test steps.
- The analysis-corpus invariant is verified twice: Task 7 Step 1 (`corpus`, `mixed-rate` unchanged) and by construction (no task touches `xtask/corpus.pin`).
- `<ANGLE_BRACKET>` values are data dependencies produced by Tasks 1 and 4, each defined in a Produces block — not placeholders.
