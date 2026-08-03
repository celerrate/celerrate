# Benchmark Comparison Corpus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pin a comparison corpus whose first-party code is large enough to separate Celerrate from PHPStan, publish the measured cold ratio, wire the gate weekly and into the release workflow, and unblock the `v0.1.0` tag.

**Architecture:** A second pin file (`xtask/comparison-corpus.pin`) feeds the existing `cargo xtask benchmark` harness through new `comparison_*` functions in `xtask/src/corpus.rs`; the analysis corpus, its snapshot, and the mixed-rate baseline do not move. The harness generates both tools' configurations so they analyse the same file set. The gate runs in a new weekly workflow and as a required job in the release workflow, never per pull request.

**Amended 2026-08-03**, after Task 1's scouting and the investigation it triggered. Two changes run through Tasks 3, 4, 5 and 7: the harness must *enforce* equal reported work rather than assume it, and the publication states both the wall-clock and the CPU ratio while leaving the parent design's ~20x ambition unamended. Section 11 of the spec carries the evidence.

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

### Task 1: Scout the comparison candidate — DONE (2026-08-03)

Empirical, commits nothing. Produces the values every later task consumes: the pinned repository and commit, the first-party file count, the trial medians, and their spread. The spec's acceptance criteria decide; if Shopware 6 fails, the same steps run on the fallbacks in order.

**Outcome.** Shopware and phpMyAdmin were both rejected; PrestaShop 9.0.3 is the corpus. Scouting also falsified the design's equal-reported-work assumption, which is why Tasks 3, 4, 5 and 7 below differ from their original text. The full record is `.superpowers/sdd/2026-08-02-benchmark-comparison-corpus/task-1-report.md` and `investigation-ratio-gap.md`; the design amendment is section 11 of the spec. The values later tasks consume:

- `SCOUTED_REPOSITORY` = `https://github.com/PrestaShop/PrestaShop`
- `SCOUTED_COMMIT` = `fc96d0d4eae383e8c6f1f54f19cf592c221a62e3` (tag 9.0.3)
- `FIRST_PARTY_COUNT` = 6932, `TOTAL_COUNT` = 24033
- `PHPSTAN_MEMORY_LIMIT` stays `2G` (PHPStan finished well under it)
- Corrected-protocol reference figures, three cold runs each: PHPStan 39.52 s wall / 264.5 s CPU, Celerrate 13.67 s wall / 16.8 s CPU, wall ratio 2.89x, CPU ratio 11.6x

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
fn the_comparison_pin_is_committed_and_names_the_scouted_corpus() {
    // Exact values, not a prefix: the medians published against this
    // pin are only meaningful for the commit they were measured on, so
    // a silent edit here must fail the suite rather than quietly
    // invalidate every number in the protocol.
    let pin = super::comparison_pin().unwrap();
    assert_eq!(pin.repository, "https://github.com/PrestaShop/PrestaShop");
    assert_eq!(pin.commit, "fc96d0d4eae383e8c6f1f54f19cf592c221a62e3");
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
repository = https://github.com/PrestaShop/PrestaShop
commit = fc96d0d4eae383e8c6f1f54f19cf592c221a62e3
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
- Modify: `xtask/src/benchmark.rs` (module documentation lines 1-7, the `prepare` call line 41, the `COLD_RATIO_FLOOR` documentation lines 25-34, the `phpstan_configuration` documentation lines 156-172; a new `celerrate_configuration` function beside it, with unit tests in the existing `mod tests`)

`PHPSTAN_MEMORY_LIMIT` stays `2G`: PHPStan finished comfortably under it on the pinned corpus.

**Interfaces:**
- Consumes: `xtask::corpus::prepare_comparison()` from Task 2.
- Produces: `cargo xtask benchmark [--gate]` measuring the comparison corpus, both tools analysing the same file set. Signatures of `run`, `cold_ratio`, `under_ratio_floor`, `phpstan_configuration` unchanged.

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

- [ ] **Step 1b: Equalise the analysed file set (test-first)**

This step exists because scouting falsified the design's equal-reported-work assumption (spec section 11). Celerrate discovers through Composer autoload; PrestaShop loads its 326-file `classes/` directory through its own runtime autoloader, so Celerrate reported on 5922 files where PHPStan analysed 6926, and the resulting spurious diagnostics cost it 8 seconds of did-you-mean work. Both tools must be handed the same tree.

Write the failing unit tests first, in the existing `mod tests` beside the `phpstan_configuration` tests:

```rust
#[test]
fn the_celerrate_configuration_includes_the_whole_tree() {
    let text = super::celerrate_configuration();
    assert!(text.contains("[project]"));
    assert!(text.contains(r#"include = ["."]"#));
}
```

Then add, beside `phpstan_configuration`:

```rust
/// The configuration written into the corpus working tree so Celerrate
/// analyzes exactly what PHPStan is given. Discovery walks Composer's
/// autoload roots, and a real application routinely loads part of its own
/// code through a runtime autoloader Composer never declares: on the
/// pinned corpus that hid 1010 of 6932 first-party files from Celerrate
/// while PHPStan saw them, which is not a comparison. Pinning the include
/// set to the whole tree restores equal reported work; vendor stays
/// indexed for reflection, as it is for PHPStan.
fn celerrate_configuration() -> String {
    "[project]\ninclude = [\".\"]\n".to_string()
}
```

and write it into the working tree in `run()`, after the `copy_directory` call:

```rust
    std::fs::write(working.join("celerrate.toml"), celerrate_configuration())?;
```

Update the neighbouring comment that claims "nothing foreign enters the tree Celerrate analyzes": PHPStan's cache and configuration still live outside the tree, but Celerrate's own configuration is now written into it deliberately, and the comment must say so.

- [ ] **Step 2: Run the unit tests, lint, format**

Run: `cargo test -p xtask && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: all xtask tests PASS (none encode the corpus choice).

- [ ] **Step 3: Commit**

```bash
git add xtask/src/benchmark.rs
git commit -m "✨ feat(xtask): measure the comparison corpus in the benchmark harness"
```

---

### Task 3b: Make corpus preparation survive a real application's tree

Added 2026-08-03, after the first reference run failed before it measured anything. The corpus machinery was written for symfony/demo and carries two assumptions a real application breaks. Both must be fixed before any measurement is trustworthy.

**Files:**
- Modify: `xtask/src/corpus.rs` (`install_vendor`'s guard, plus a unit test)
- Modify: `xtask/src/bench.rs` (`copy_directory`'s symlink handling, plus a unit test)

**Interfaces:**
- Consumes: nothing new.
- Produces: `prepare_comparison()` reaching a complete working tree on the pinned corpus. Signatures unchanged.

- [ ] **Step 1: Guard on composer's own artefact, not on the directory**

`install_vendor` skips the install when `vendor/` is a directory. PrestaShop commits `vendor/.htaccess`, so that directory exists the instant the snapshot is checked out and `composer install` never runs: the vendor tree stays empty and every measurement built on it is meaningless.

Write the failing test first, then guard on `vendor/autoload.php` instead — the file composer itself always produces, which no corpus commits. Keep the rest of the function, its flags and its documentation as they are; extend the documentation to say why the artefact and not the directory.

- [ ] **Step 2: Do not abort the copy on an unresolvable symlink**

`copy_directory` follows symlinks with `fs::copy`, which fails with `ENOENT` when the target is missing. PrestaShop commits exactly one symlink, `tests/Resources/modules/ps_apiresources`, pointing at `modules/ps_apiresources` — a package composer places only through the `composer/installers` plugin, which `--no-scripts --no-plugins` deliberately disables so that no code from the corpus ever runs. The symlink therefore dangles by design, and a dangling symlink must not be able to abort a benchmark.

Write the failing test first (a temporary directory holding a symlink to a nonexistent path), then skip entries whose metadata cannot be resolved, and document the reasoning at the skip. Do not weaken the hermetic flags to make the target exist: not executing corpus code is a deliberate project rule, and one absent test fixture does not justify breaking it.

- [ ] **Step 3: Prove the analysis corpus did not move**

Both functions are shared with the analysis corpus, so this task can silently break it. Run, and record the output:

```bash
cargo test -p xtask
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

`corpus` and `mixed-rate` must pass unchanged. A snapshot or baseline delta here is a failure of this task, not a result to bless.

- [ ] **Step 4: Lint, format, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
git add xtask/src/corpus.rs xtask/src/bench.rs
git commit -m "🐛 fix(xtask): prepare corpora that commit a vendor placeholder or a dangling symlink"
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

Also record from those exports, for publication in Task 5: each tool's CPU time (`user + system` in the hyperfine JSON) and the derived `MEASURED_CPU_RATIO`. Scouting measured 264.5 s against 16.8 s, a CPU ratio of 11.6x against a wall-clock ratio of 2.89x; the gap is Celerrate running at 1.1 effective cores against PHPStan's 6.7, and section 7 of the spec publishes both.

Sanity-check that the equalisation from Task 3 actually took effect before trusting the number: `celerrate check` on the working tree should report on roughly 6932 files, not roughly 5922. `target/benchmark/celerrate-cold.json` plus a single `--verbose` run gives this. If the counts still diverge, stop — the ratio is not measuring what it claims.

- [ ] **Step 2: Set the floor to half the measured ratio**

Replace the provisional value: `COLD_RATIO_FLOOR` becomes `MEASURED_RATIO / 2`, rounded down to one decimal (a measured 2.9x gives `1.4`). Replace the documentation's last sentence ("The value below is provisional until the reference run lands.") with the measurement record:

```rust
/// Reference measurement (2026-08-03, the protocol machine): PHPStan
/// <MEASURED_PHPSTAN_MEDIAN>s wall, Celerrate <MEASURED_CELERRATE_MEDIAN>s
/// wall, ratio <MEASURED_RATIO>x; on CPU consumed, <MEASURED_CPU_RATIO>x.
/// The gap between the two ratios is parallelism: Celerrate is
/// effectively single-threaded today and PHPStan forks workers.
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

### Task 4a: Stabilise the reference measurement

Added 2026-08-03. The first reference run met every criterion except the one that decides whether a number is publishable at all. Celerrate's five cold runs came out at 14.21, 11.59, 11.67, 12.15 and 12.12 seconds: a 22.66 % spread driven entirely by the first run, where the spec's acceptance criterion is under 10 %. That is the same instability that disqualified symfony/demo, and it is why two consecutive full measurements reported 2.7x and 3.0x.

The cause is that `measure()` passes hyperfine no warmup run, so the first timed run of each tool absorbs the cold page cache. `--prepare` still wipes Celerrate's own cache before every run including warmups, so a warmup costs nothing in cold-analysis fidelity: what it discards is filesystem warm-up, not analysis work.

**Files:**
- Modify: `xtask/src/benchmark.rs` (`measure`'s hyperfine invocation; `COLD_RATIO_FLOOR`'s value and the last sentence of its documentation)

- [ ] **Step 1: Give each measurement a warmup run**

Add `--warmup 1` to the hyperfine invocation in `measure()`, and document at the call why it is there: the first timed run otherwise pays for the cold page cache and inflates the spread past the stability criterion, while `--prepare` keeps every timed run cold in the sense that matters, namely Celerrate's own cache and PHPStan's result cache.

This change is not unit-testable — it is one flag on an external process invocation, and the honest verification is the measured spread in Step 2, not an assertion that a string contains a flag. Do not write a test that asserts the argument list; it would restate the implementation rather than verify behaviour.

- [ ] **Step 2: Re-run the reference measurement and check the criterion**

Run `cargo xtask benchmark` once. Then, from `target/benchmark/phpstan-cold.json` and `target/benchmark/celerrate-cold.json`, compute each tool's spread as `(max - min) / min`.

**Both spreads must be under 10 %.** If either is not, stop and report: the corpus or the machine is not delivering a stable measurement, and no floor derived from it is worth committing.

Record the raw times of every run in the report, not only the medians. The previous run recorded medians alone, which is why its instability went unnoticed until the exports were read directly.

- [ ] **Step 3: Set the floor from the stabilised ratio**

`COLD_RATIO_FLOOR` becomes the new measured ratio divided by two, rounded down to one decimal. Update the reference-measurement sentence in its documentation to the new figures, keeping the two-ratio form (wall clock and CPU) and the sentence explaining that the gap between them is parallelism.

- [ ] **Step 4: Verify the gate, lint, format, commit**

```bash
cargo xtask benchmark --gate
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
git add xtask/src/benchmark.rs
git commit -m "📈 fix(xtask): warm up the benchmark so the published ratio is stable"
```

The gate run is a second full measurement. Record its ratio too: two consecutive full runs agreeing within a few percent is the evidence that the warmup did its job, and their divergence is what this task exists to remove.

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

- Repository: https://github.com/PrestaShop/PrestaShop
- Commit: `fc96d0d4eae383e8c6f1f54f19cf592c221a62e3` (tag 9.0.3,
  committed in `xtask/comparison-corpus.pin`)
- Vendor tree: installed from the corpus's own `composer.lock`, same
  flags as the analysis corpus
- Size: 24033 PHP files, of which 6932 are first-party and the rest are
  the installed vendor tree

Both tools are handed the same file set, and the harness enforces it
rather than assuming it. Celerrate discovers a project through Composer's
autoload roots, but PrestaShop loads its 326-file `classes/` directory
through its own runtime autoloader, which Composer never declares: left
alone, Celerrate would report on 5922 files while PHPStan analysed 6926.
The harness therefore writes a `celerrate.toml` pinning
`[project] include = ["."]` into the corpus working tree. Vendor is
indexed for reflection on both sides but analysed by neither. PHPStan is
pinned in `benchmarks/phpstan/composer.lock`, at rule level 5, with its
result cache wiped before every timed run.

The harness is `cargo xtask benchmark`; `--gate` fails under the
committed floor (`COLD_RATIO_FLOOR` in `xtask/src/benchmark.rs`), half
the reference ratio below.

Measured on the reference machine, cold. The figures are the pooled
medians of three full runs: nine timed PHPStan runs and fifteen timed
Celerrate runs.

| | wall clock | CPU consumed |
| --- | ---: | ---: |
| PHPStan | 38.92 s | 237.5 s |
| Celerrate | 13.41 s | 16.8 s |
| ratio | **2.90x** | **14.2x** |

Both ratios are published because either one alone misleads. The wall
clock is what you wait through; the CPU column is what the engines cost.
They differ by roughly a factor of five because Celerrate is effectively
single-threaded today while PHPStan forks worker processes: Celerrate
wins the wall clock while using an order of magnitude less machine.
Parallelising it is tracked work, not a claim made here.

The three full runs gave ratios of 2.97x, 2.95x and 2.70x. The spread is
Celerrate's, not PHPStan's: its five timed runs step from about 12.3 s to
about 13.8-14.3 s partway through and stay there, which has the shape of
frequency scaling under sustained load on this machine. PHPStan's own
spread stayed between 1.1 % and 6.1 % throughout. The published figure is
therefore the pooled median rather than any single run, and the gate
floor sits far below the worst of them: 1.4, cleared by 1.93x even at
2.70x.

The gate runs weekly (`.github/workflows/benchmark.yml`) and as a
required job before any release publishes (`.github/workflows/release.yml`).
```

Then delete or rewrite every withheld-comparison statement found in Step 1 so the document reads as one voice: the comparison exists, here is the number.

- [ ] **Step 3: State the ratio in the README**

Replace the README's no-published-comparison statement with one sentence in the same place, filling the values:

```markdown
On the pinned comparison corpus (6932 first-party PHP files), a cold
`celerrate check` completes 2.9x faster than PHPStan at rule level 5 on
the same file set, using 14x less CPU to do it: Celerrate is
single-threaded today where PHPStan forks workers. The pinned protocol
and the full numbers live in
[benchmarks/PROTOCOL.md](benchmarks/PROTOCOL.md).
```

Check `docs/installation.md` and the two benchmark SVG assets referenced by the README for any stale comparison claim; update only what carries one.

- [ ] **Step 3b: Align the floor's committed record with what is published**

`COLD_RATIO_FLOOR`'s documentation in `xtask/src/benchmark.rs` currently records the single reference run it was first derived from. Rewrite that reference-measurement sentence to the published figures: PHPStan 38.92 s and Celerrate 13.41 s wall, ratio 2.90x, CPU ratio 14.2x, stated as the pooled medians of three full runs, and note that the observed per-run ratios ranged 2.70x to 2.97x. The value `1.4` does not change (2.902 / 2 = 1.451, floored). A committed comment that quotes a better number than the protocol publishes is exactly the kind of drift this branch exists to remove.

- [ ] **Step 4: Annotate the parent design**

In `.claude/superpowers/specs/2026-07-09-celerrate-design.md`, find the "at least ~20x faster than PHPStan" target in section 7. **Annotate it; do not amend it down.** Record the measured `<MEASURED_RATIO>x` wall-clock and `<MEASURED_CPU_RATIO>x` CPU figures on the pinned comparison corpus (2026-08-03), and state plainly why the ambition still stands: 62 % of Celerrate's wall clock is a quadratic did-you-mean pass in the presentation layer, and the engine runs at 1.1 effective cores of 10. The measurement does not test the claim the ambition makes. Section 11 of `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md` carries the evidence and the estimates; point at it rather than restating them.

No defensive rewording in either direction: the measured number goes in as it is, and so does the reason it is not the ambition's verdict.

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
- Amendment 3: append that the comparison is no longer withheld — it is published on the pinned comparison corpus (name repository and commit) with both measured ratios, and the gate runs weekly and before releases. Note in the same breath that the harness now enforces the equal file set rather than assuming it, and that the parent design's ~20x ambition stands unamended for the reason recorded in section 11 of the comparison-corpus design.
- Gate 7 in the state-of-play section: rewrite as fully held, naming `xtask/comparison-corpus.pin`, `benchmarks/PROTOCOL.md`, `.github/workflows/benchmark.yml`, and the release gate.
- The "State of play" introduction: record that both open items (the published comparison, the tag) are resolved.

In `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`: `Status: Closed (implemented, <today's date>)`.

- [ ] **Step 3: Date the CHANGELOG entry**

In `CHANGELOG.md`, change `## [Unreleased]` to `## [0.1.0] - <today's date>` and add a fresh empty `## [Unreleased]` section above it. Verify the extraction:

Run: `cargo xtask release-notes 0.1.0`
Expected: prints the 0.1.0 body, exit 0.

- [ ] **Step 3b: Open the performance issue**

The scouting investigation identified concrete, measured performance work that this design deliberately does not do. File it so it is not lost, and reference it from the protocol's parallelism note:

```bash
gh issue create --title "⚡️ Parallelise the check pipeline and fix the quadratic did-you-mean pass" \
  --body "..."
```

The body states the measured facts, not a plan: Celerrate runs at 1.1 effective cores of 10 on the pinned comparison corpus; `suggest::enrich` is 62 % of the cold wall clock and re-clones an 18000-name pool plus reallocates its edit-distance matrix per candidate; the persist, index and read phases are serial and embarrassingly parallel. Estimated cumulative effect from the measured per-phase costs: a 13.67 s cold run lands near 4.5-6 s. Link the evidence.

- [ ] **Step 4: Commit and open the pull request**

```bash
git add .claude/superpowers/specs/ CHANGELOG.md
git commit -m "🔖 chore(release): date the 0.1.0 entry and record the closure"
git push -u origin feat-comparison-corpus
gh pr create --title "📈 feat(benchmarks): publish the PHPStan comparison on a corpus that carries it" \
  --body "Pins a comparison corpus whose first-party code separates the two analyzers, publishes the measured cold ratio in benchmarks/PROTOCOL.md and the README, and gates it weekly and before releases. Closes #118"
```

The pull request body states both ratios and says plainly that the harness now enforces the equal file set, so a reviewer meets the correction rather than discovering it.

**Execution stops here.** Steps 5 through 7 below are the user's: they own the merge, the tag, and the release. Report the pull request URL and hand over.

- [ ] **Step 5 (user): Run the weekly workflow once by hand**

After the merge:

```bash
gh workflow run Benchmark
gh run watch
```

Expected: the `comparison-gate` job completes green within its budget.

- [ ] **Step 6 (user): Tag v0.1.0**

```bash
git checkout main && git pull --ff-only
git tag v0.1.0
git push origin v0.1.0
gh run watch
```

Expected: the release workflow runs `build` (five targets), `benchmark-gate`, `publish` (checksums, attestation, GitHub release), and `split-composer`, all green. Confirm the release exists: `gh release view v0.1.0`.

- [ ] **Step 7 (user): Confirm the issue closed**

`gh issue view 118` shows CLOSED (the pull request body closes it on merge). If not, close it with a comment linking the release.

---

## Self-review notes

- Spec coverage: section 3 → Task 2; section 4 → Task 1; section 5 → Tasks 3-4; section 6 → Task 6; section 7 → Task 5; section 8 → Task 7; section 9 → Tasks 2-4 test steps.
- The analysis-corpus invariant is verified twice: Task 7 Step 1 (`corpus`, `mixed-rate` unchanged) and by construction (no task touches `xtask/corpus.pin`).
- `<ANGLE_BRACKET>` values are data dependencies produced by Tasks 1 and 4, each defined in a Produces block — not placeholders. Task 1's are now resolved and inlined; Task 4's are still pending its run.
- The equal-file-set correction is verified twice: by the unit test in Task 3 Step 1b, and by Task 4 Step 1's reported-file-count check on the real run. A green ratio with divergent counts is a false pass, which is exactly the failure that produced the original 1.95x.
