# Republish the Comparison and Take the Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-measure the pinned PHPStan comparison on the reference machine now that the check pipeline is parallel, replace every published figure derived from the pre-parallelism binary, raise the gate floor to match, and date the `0.1.0` entry so the tag can be taken.

**Architecture:** No new machinery. The comparison corpus, the harness, the two workflows and the gate all exist and are exercised; what is stale is the set of numbers they were calibrated against, and the tag decision those numbers justified. This plan changes one constant, the prose that quotes it, and the release entry.

**Why now:** the published comparison (2.90x wall clock, 14.9x CPU) was measured at commit `8ab4af1`, before the check-pipeline work. That work landed and moved the cold median from about 14 s to about 5 s. The repository therefore under-states its own tool by more than a factor of two, and `COLD_RATIO_FLOOR` is half of a ratio that no longer exists. The tag was withheld on the strength of the old figure; the decision to take it is the user's and has been given.

**Tech Stack:** Rust (xtask), hyperfine, Composer/PHPStan, Markdown.

**Specs:** `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` (the sub-project this closes), `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md` (the protocol being re-run), `.claude/superpowers/specs/2026-07-09-celerrate-design.md` section 7 (the ambition being annotated).

## Global Constraints

- Zero panic, mechanically enforced: no `unwrap`/`expect`/indexing/`panic` in production code; test modules may locally `#[allow]`.
- The analysis corpus is untouched: `xtask/corpus.pin`, the corpus snapshot and the mixed-rate baseline must be byte-identical at the end.
- No analysis behaviour changes in this plan. It edits one numeric constant, documentation, and the changelog. A diff that touches `crates/` is out of scope and is a defect.
- **Living documents versus dated records.** Every *living* document states the new measurement, and they must agree with each other exactly: the repository publishing two different answers is the failure this plan exists to remove. A *dated record* of past work is never rewritten to the new figures — that would falsify the account of what was measured on the day it was measured. Where a dated record's conclusion has since been overtaken, it gets an appended, dated note saying so; its original body stands.

  Living, and in scope: `README.md`, `benchmarks/PROTOCOL.md`, `CHANGELOG.md`, `xtask/src/benchmark.rs`, `.github/workflows/release.yml`, `.github/workflows/benchmark.yml`, section 7 of `.claude/superpowers/specs/2026-07-09-celerrate-design.md`, and `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`.

  Dated records, not to be rewritten: `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md` (closed 2026-08-03; its section 11 is the evidence of what was measured then), `.claude/superpowers/specs/2026-08-03-check-pipeline-performance-design.md` (its problem statement is the correct pre-work baseline), and everything under `.claude/superpowers/plans/`.
- Everything committed is English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits, repository-configured identity, no Claude attribution.
- Public-facing text never references plans, specs, phases, or tasks by number; referencing public issues (#118, #124) is fine.
- Execution branch: `feat-republish-comparison`, created from `main`.

---

### Task 1: The reference re-measurement — DONE (2026-08-06)

Empirical, commits nothing, and runs on the reference machine named in `benchmarks/PROTOCOL.md` (the Apple M5 this project develops on), so the controller performs it rather than a subagent. Three full `cargo xtask benchmark` runs against the release binary built from the branch head, each run's hyperfine exports preserved before the next overwrites them.

**Outcome.** Recorded below, and consumed verbatim by every later task. Values are pooled exactly as the committed protocol already pools them: the wall-clock medians over all timed runs (nine PHPStan, fifteen Celerrate), the CPU column as the median of the three full runs' own CPU totals, because hyperfine reports one CPU total per invocation rather than per timed run.

- `MEASURED_COMMIT` = `4bc0156`, `celerrate --version` reports `0.1.0`
- `MEASURED_PHPSTAN_MEDIAN` = 39.058 s, `MEASURED_CELERRATE_MEDIAN` = 4.874 s
- `MEASURED_RATIO` = 8.01x (8.0135 exactly)
- `MEASURED_PHPSTAN_CPU` = 242.5 s, `MEASURED_CELERRATE_CPU` = 22.0 s
- `MEASURED_CPU_RATIO` = 11.0x
- `PER_RUN_RATIOS` = 7.52x, 8.04x, 7.93x (a 6.87 % span)
- `EFFECTIVE_CORES` = Celerrate 4.51 of 10, PHPStan 6.21 of 10
- `REPORTED_FILE_COUNT` = `COUNTED_FILE_COUNT` = 6932, on all three runs
- Raw timed runs — PHPStan: 39.315, 36.594, 36.787 / 40.327, 38.768, 35.553 / 39.391, 41.926, 39.058. Celerrate: 5.350, 4.891, 5.034, 4.866, 4.874 / 4.834, 4.823, 4.764, 5.052, 4.586 / 5.212, 4.969, 4.766, 5.016, 4.752.
- Derived floor: `MEASURED_RATIO / 2` floored to one decimal = **4.0**

**Two figures move in opposite directions, and both are published.** The wall-clock ratio rises from 2.90x to 8.01x. The CPU ratio *falls*, from 14.9x to 11.0x: Celerrate's own CPU cost rose from 17.0 s to 22.0 s, which is the price of the parallelism that bought the wall clock. Its effective core count went from 1.27 of 10 to 4.51 of 10. No later task may quote the wall-clock improvement without the CPU regression beside it.

**Acceptance.** The spec's section 4 criterion is the *ratio's* stability across the three runs, target under 10 %: measured at 6.87 %, met. The subsidiary per-tool per-run spreads, `(max - min) / min`, are PHPStan 7.43 %, 13.43 %, 7.34 % and Celerrate 9.95 %, 10.16 %, 9.68 % — two of six marginally exceed 10 %. That exceedance is published rather than dropped, exactly as section 11 of the comparison-corpus design published the previous run's (Celerrate 19.93 % and 16.42 %), and this run is tighter than that one on every axis. The published figure is the pooled median with the observed range beside it, never the best of three.

**Discarded attempts.** Two earlier three-run sets were thrown away rather than published: the first contaminated by a search subagent running concurrently (PHPStan 53.7 / 91.7 / 39.7 s), the second by Spotlight indexing the 93 209 files the harness creates under `target/` plus a leaked recursive grep (PHPStan 64-69 s). Both were caught by the absolute figures diverging from the protocol's own history, not by the spread alone. The fix that made the machine measurable again was killing the leaked process and adding `target/.metadata_never_index`; the reference conditions then reproduced the independently recorded 4.813 s Celerrate cold time to within 1.3 %.

---

### Task 2: The gate floor

**Files:**
- Modify: `xtask/src/benchmark.rs` (the `COLD_RATIO_FLOOR` value and its documentation comment, lines 28-44)

**Interfaces:**
- Consumes: every `MEASURED_*` value from Task 1.
- Produces: the committed floor the two workflows gate on. No signature changes.

- [ ] **Step 1: Set the value**

`COLD_RATIO_FLOOR` becomes `MEASURED_RATIO / 2`, rounded down to one decimal, keeping the existing rationale (half the measured ratio, so runner variance cannot fail a healthy build while a regression that halves the advantage still does).

- [ ] **Step 2: Rewrite the measurement record in its documentation**

The comment currently records the 2026-08-03 run in full: pooled wall-clock medians over twenty-four timed runs, the CPU column's different pooling, the ratio range, and the sentence attributing the wall-clock/CPU gap to Celerrate being effectively single-threaded. Replace the figures with Task 1's and rewrite that last explanation: the pipeline is parallel now, so whatever gap remains between the two ratios must be described as Task 1 actually measured it, not as the old single-threaded story. Keep the pooling explanation (hyperfine reports one CPU total per invocation, not per timed run) — that is a property of the tool, not of the old run.

Do not invent an explanation for the gap. State the measured numbers and, if the cause is not established by this plan's measurement, say that plainly rather than reasoning about it in a committed comment.

- [ ] **Step 3: Verify the gate passes**

Run: `cargo xtask benchmark --gate`
Expected: exit 0, printing a ratio comfortably above the new floor. This is a fourth full measurement; record its ratio in the report as corroboration of Task 1's three.

- [ ] **Step 4: Lint, format, commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
git add xtask/src/benchmark.rs
git commit -m "📈 fix(xtask): re-derive the cold ratio floor from the parallel pipeline"
```

---

### Task 3: The published documents

**Files:**
- Modify: `benchmarks/PROTOCOL.md` (the measurement paragraph and stale commit at line 66, the results table at lines 77-79, the "single-threaded" explanation at line 84, the per-run spread paragraph at lines 88-95 including the floor headroom sentence)
- Modify: `README.md` (lines 119-121: the wall-clock ratio, the per-run range, the CPU ratio and its "single-threaded today" clause)
- Modify: `CHANGELOG.md` (lines 180-181, inside the body that becomes the 0.1.0 entry: it already publishes 2.90x and 14.9x)
- Modify: `.github/workflows/release.yml` (line 75) and `.github/workflows/benchmark.yml` (line 41), which carry the same duplicated comment justifying the PHP-version margin by "~2.9x"

The two benchmark SVG assets the README references were checked and carry no comparison figure, only Celerrate's own scenario medians. Leave them alone.

**Interfaces:**
- Consumes: every `MEASURED_*` value from Task 1; the floor from Task 2.

- [ ] **Step 1: Read both documents end to end first**

Not only the lines named above. The inventory in the brief is the checklist, but a document that reads as two voices — a corrected table under an uncorrected paragraph — is the failure this task exists to prevent.

- [ ] **Step 2: Replace the protocol's comparison figures**

The measurement paragraph names a date and a commit; both change. The table's six cells change. The paragraph explaining why both ratios are published stays in substance — the wall clock is what you wait through, the CPU column is what the engines cost — but its causal sentence ("they differ by roughly a factor of five because Celerrate is effectively single-threaded today") describes a binary that no longer exists and must state what this measurement shows instead. The per-run spread paragraph is replaced wholesale by Task 1's runs, including the sentence deriving the gate floor's headroom.

- [ ] **Step 3: Replace the README's statement**

One sentence in the same place, carrying the new wall-clock ratio, the new CPU ratio, and the observed per-run range, pointing at `benchmarks/PROTOCOL.md` as it does today. Drop the "single-threaded today where PHPStan forks workers" clause and say what is now true.

- [ ] **Step 4: Correct the changelog body and the two workflow comments**

`CHANGELOG.md` lines 180-181 publish the old ratios inside what becomes the 0.1.0 entry. The workflow comment, duplicated verbatim in `release.yml` and `benchmark.yml`, justifies a PHP-version margin by the size of the ratio; the new ratio is larger, so the justification still holds and only its figure changes. Both copies must move together — a margin argued from two different numbers in two workflows is worse than either.

- [ ] **Step 5: Verify the documents agree**

Grep the working tree for every old figure (`2.9`, `14.9`, `38.92`, `13.41`, `253.4`, `17.0 s`, `2.70x`, `2.97x`, `8ab4af1`) and confirm each surviving hit is inside a dated record this plan deliberately leaves standing, not inside a living document. Record the surviving hits and why each is legitimate.

- [ ] **Step 6: Commit**

```bash
git add benchmarks/PROTOCOL.md README.md CHANGELOG.md .github/workflows/
git commit -m "📝 docs(benchmarks): republish the comparison against the parallel pipeline"
```

---

### Task 4: The internal specs

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (section 7, the paragraph after the published performance targets, lines 586-601)
- Modify: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` (the status line, the state of play, gate 7 if its figures moved)

**Interfaces:**
- Consumes: every `MEASURED_*` value from Task 1.

- [ ] **Step 1: Re-annotate the parent design**

Section 7's ambition ("at least ~20x faster than PHPStan on a cold full analysis") is **not** amended, in either direction. What changes is the position paragraph beneath it: the figures, and the two reasons it gave for why the measurement did not test the ambition. Both reasons have been acted on — the quadratic did-you-mean pass was fixed and the pipeline parallelised — so repeating them would be false. State the new measured position, and state honestly what the remaining gap to the ambition is now attributable to, or that this plan's measurement does not establish it.

The paragraph's closing pointer currently sends the reader to the comparison-corpus design for "the estimate of what removing that churn and parallelising the rest would do to the wall-clock ratio". That estimate has since been tested rather than merely proposed, so the pointer must be reworded to say so, while still pointing at the same section. Do not edit that section itself: it is a dated record, and its estimate is now checkable against this plan's measurement precisely because it was left as written.

- [ ] **Step 2: Close the release spec**

In `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`:
- The status line becomes `Status: Closed (v0.1.0, 2026-08-06)`, replacing "Gates held, tag withheld" and the sentences explaining the withholding.
- The state of play: the tag is no longer withheld. Record what changed — the measured ratio the withholding decision was taken against, the work that moved it, and the new figure — and that the closure criterion's "~20x" is still not reached, since claiming otherwise would be the kind of drift this project's protocol exists to prevent. The decision to publish 0.1 at the measured figure is recorded as a decision, not as the criterion being met.
- Gate 7: update any figure it quotes.

- [ ] **Step 3: Commit**

```bash
git add .claude/superpowers/specs/
git commit -m "📝 docs: record the closure of the v0.1 sub-project"
```

---

### Task 5: The release entry

**Files:**
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing from Task 1 directly; runs after Tasks 2-4 so the entry describes a tree whose numbers are already corrected.

- [ ] **Step 1: Date the entry**

Change `## [Unreleased]` (line 8) to `## [0.1.0] - 2026-08-06` and add a fresh empty `## [Unreleased]` section above it. The existing body — the configuration crate, the baseline, the migration, the output formats, the distribution work and the check-pipeline speedup — is the 0.1.0 body and does not move.

- [ ] **Step 2: Confirm the entry's figures were already corrected**

The 0.1.0 body's comparison figures are corrected in Task 3, not here, so that every document carrying them moves in one commit. This step only verifies it happened: the entry must quote Task 1's ratios and no others before it is dated. If it does not, stop — dating an entry that publishes a superseded number is precisely the drift this plan removes.

- [ ] **Step 3: Verify the extraction**

Run: `cargo xtask release-notes 0.1.0`
Expected: prints the 0.1.0 body, exit 0. Record the first and last lines of the output in the report as evidence the section boundaries are right.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "🔖 chore(release): date the 0.1.0 entry"
```

---

### Task 6: Gates and the pull request

**Interfaces:**
- Consumes: everything above.

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

Every command must pass. `corpus` and `mixed-rate` passing unchanged is what proves this plan changed no analysis behaviour. Record each command's verdict in the report; a suite reported green in aggregate hides the one that was not run.

- [ ] **Step 2: Push and open the pull request**

```bash
git push -u origin feat-republish-comparison
gh pr create --title "📈 feat(benchmarks): republish the comparison and date the 0.1.0 release" \
  --body "..."
```

The body states the old and new figures side by side, names the work that moved them, says plainly that the closure criterion's ~20x is still not reached and that publishing 0.1 at this figure is a decision, and closes #124.

**Execution stops here.** The merge, the tag and the release are the user's.

- [ ] **Step 3 (user): Merge, then run the weekly workflow once by hand**

```bash
gh workflow run Benchmark
gh run watch
```

- [ ] **Step 4 (user): Tag v0.1.0**

```bash
git checkout main && git pull --ff-only
git tag v0.1.0
git push origin v0.1.0
gh run watch
gh release view v0.1.0
```

---

## Self-review notes

- The plan's own risk is inconsistency, not complexity: six documents quote one set of numbers. Tasks 2 through 5 therefore all consume Task 1's recorded values verbatim rather than re-deriving them, and Task 6's gate run is the last check that the committed floor and the published ratio still agree.
- Two sentences in the existing prose assert causes that the check-pipeline work invalidated (the single-threaded engine, the quadratic did-you-mean pass). Tasks 2, 3 and 4 each name theirs explicitly, because replacing a number while leaving its explanation standing is the likeliest way this plan half-lands.
- The ambition in the parent design is annotated, never amended. That rule survives this plan unchanged.
