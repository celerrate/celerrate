# Cold-Run Performance Diagnostic Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce the three measured answers of the diagnostic spec (the corpus floor, the missing-cores attribution, the Amdahl accounting) plus the two closing deliverables (the prioritized lever list and the ambition amendment proposal), landing no performance lever.

**Architecture:** A measurement campaign, not a feature. Every task either runs a measurement with the existing tooling (the release binary's `--verbose` phase channel, `cargo xtask benchmark`, hyperfine, the macOS `sample` profiler) or builds a throwaway probe on a scratch branch that is never merged. Results accumulate in one measurement document, committed after every task so they survive across sessions and subagents.

**Tech Stack:** Rust workspace, rayon (global pool, so `RAYON_NUM_THREADS` applies), salsa, hyperfine, macOS `sample`, `cargo xtask benchmark` (the pinned comparison protocol).

**Spec:** `.claude/superpowers/specs/2026-08-07-cold-run-performance-diagnostic-design.md`

## Global Constraints

- **This plan lands no performance lever.** Scratch branches (`scratch-mimalloc-probe`, `scratch-parse-floor`) are never merged, never pushed, and never the base of another branch. The only commits that land on the working branch are measurement-document commits.
- The working branch is `docs-cold-run-performance-diagnostic` (already created, carries the spec).
- The measurement document is `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (created in Task 1). Every measurement task appends its section and commits it before the task ends.
- **Session discipline (spec section 4):** machine otherwise idle; any A/B comparison measured back to back in one session; each session opens and closes with a control run (three cold `celerrate check` runs on the pinned corpus, medians recorded); a control drift above ~10 % between a session's open and close invalidates that session's comparisons and the session is redone. Every figure written to the document carries its date, commit, and spread.
- Cold means `rm -rf .celerrate` in the corpus directory before the run. The corpus directory is `target/comparison-corpus/fc96d0d4eae383e8c6f1f54f19cf592c221a62e3` (prepared by Task 1); the release binary is reached from there as `../../release/celerrate`.
- Never pass `--bless` to any xtask command. The corpus snapshot and mixed-rate baseline are untouched by this plan.
- Determinism rules are unaffected: `RAYON_NUM_THREADS` is read by rayon at global-pool initialization, outside every salsa query; probes never touch query code.
- All file content in English, full words. Commits: gitmoji + Conventional Commits, repository-configured identity, no Claude attribution.
- Measurement runs are long. Never run two measurements concurrently, and never run cargo builds while a measurement is timing.

---

### Task 1: The measurement document and the session anchor

**Files:**
- Create: `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`

**Interfaces:**
- Produces: the measurement document every later task appends to, and the session-anchor figures (PHPStan cold median, Celerrate cold median, ratio) later tasks cite as the campaign's opening reference.

- [ ] **Step 1: Build the release binary at the working-branch HEAD**

Run from the repository root:

```bash
git checkout docs-cold-run-performance-diagnostic
cargo build --release
git rev-parse --short HEAD
```

Expected: a successful build; note the short commit hash for the document header.

- [ ] **Step 2: Create the measurement document skeleton**

Create `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`:

```markdown
# Cold-Run Performance Diagnostic: Measurements

Spec: `.claude/superpowers/specs/2026-08-07-cold-run-performance-diagnostic-design.md`
Machine: the reference 10-core machine used by every published figure
Corpus: pinned PrestaShop comparison corpus
(`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`), equalized file set
Binary: `target/release/celerrate`, built at commit `<short hash from Step 1>`

## Protocol

- Cold run: `rm -rf .celerrate` in the corpus directory, then
  `../../release/celerrate check .` from the corpus directory.
- Three repetitions minimum per measurement point; medians reported.
- Session discipline: machine otherwise idle; every A/B measured back
  to back in one session; each session opens and closes with a control
  (three cold `check` runs, median recorded); a control drift above
  ~10 % between open and close invalidates the session's comparisons.
- Every figure carries its session date, commit, and spread.

## 1. Session anchor (Task 1)

<!-- Task 1 appends here -->
```

- [ ] **Step 3: Run the session anchor: one full protocol comparison**

This prepares the comparison corpus if absent (first run also does a Composer install; do not time anything until it completes) and measures both tools cold under the published protocol:

```bash
cargo xtask benchmark
```

Expected: hyperfine output with a PHPStan cold median in the historical 31 s to 39 s band and a Celerrate cold median in the 4.8 s to 5.5 s band. If either lands far outside its band, the machine is not idle; find the cause before continuing.

- [ ] **Step 4: Record the anchor**

Append to section 1 of the measurement document: the date, the command, the PHPStan and Celerrate cold medians with their hyperfine standard deviations, and the ratio. State explicitly that this anchor is the campaign's opening reference and that any task citing a cross-session figure must say so.

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): open the cold-run diagnostic measurement record"
```

---

### Task 2: The fixed process cost

**Files:**
- Modify: `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append section 2)

**Interfaces:**
- Consumes: the release binary from Task 1.
- Produces: the fixed-cost figures (empty-project cold `check`, `--version`) that Task 3 subtracts when reconciling wall clock against the phase sum, and that Task 7 subtracts when reading the floor.

- [ ] **Step 1: Create the empty probe project**

```bash
mkdir -p /tmp/celerrate-empty-probe
cd /tmp/celerrate-empty-probe
printf '{}\n' > composer.json
```

An empty project with a manifest: discovery succeeds, the walk finds zero PHP files, and the run exercises startup (including embedded-stub loading), configuration, discovery, and teardown with no per-file work.

- [ ] **Step 2: Measure the empty-project cold check**

```bash
cd /tmp/celerrate-empty-probe
hyperfine --warmup 1 --runs 10 --prepare 'rm -rf .celerrate' \
  '<repository root>/target/release/celerrate check .'
```

Expected: a stable sub-second mean. Record mean and standard deviation.

- [ ] **Step 3: Measure the bare process cost**

```bash
hyperfine --warmup 1 --runs 10 '<repository root>/target/release/celerrate --version'
```

This isolates process start and teardown below discovery and stub loading. Record mean and standard deviation.

- [ ] **Step 4: Record and interpret**

Append section 2 to the measurement document: both figures, and the derived split — bare process cost (`--version`), plus the increment to an empty `check` (startup work that scales with the binary, not the corpus: embedded-stub loading, discovery, cache directory handling). State the number that matters downstream: the fixed cost floor any cold run on any corpus pays.

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): record the fixed process cost of a cold check"
```

---

### Task 3: Wall-clock versus phase-sum reconciliation

**Files:**
- Modify: `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append section 3)

**Interfaces:**
- Consumes: the fixed-cost figures from Task 2.
- Produces: the reconciliation table (wall clock = eight phases + attributed overhead + residue) that Task 9's Amdahl accounting starts from.

- [ ] **Step 1: Three instrumented cold runs**

From the corpus directory (`target/comparison-corpus/fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`):

```bash
rm -rf .celerrate
time ../../release/celerrate check . --verbose > /dev/null
```

Repeat three times. Capture, per run: the shell's wall-clock report and every `verbose: phase ...` stderr line.

- [ ] **Step 2: Build the reconciliation table**

Append section 3 with two tables. First, the phase table (the eight phases, three runs, medians), in the exact format of the previous record (`.claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md`, section 6). Second, the reconciliation:

| Component | Median (ms) | Source |
| --- | ---: | --- |
| Eight-phase sum | measured | phase table |
| Fixed process cost | measured | Task 2 |
| Unaccounted residue | wall − sum − fixed | derived |

- [ ] **Step 3: Chase the residue if it is large**

If the unaccounted residue exceeds ~300 ms, identify it before moving on: the known candidates are `--verbose` stderr formatting (re-run once without `--verbose`, compare wall clocks) and cache-directory deletion timing (move the `rm -rf .celerrate` cost outside the timed region — it already is, confirm). Record what the residue is, or record that it resisted attribution and its size.

- [ ] **Step 4: Commit**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): reconcile the cold wall clock against the phase sum"
```

---

### Task 4: The scaling curve on the default allocator

**Files:**
- Modify: `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append section 4)

**Interfaces:**
- Produces: the thread-count scaling table (wall clock and per-phase medians at 1, 2, 4, 8, 10 threads) and the fan-out efficiency figures Tasks 5, 6, and 9 consume; the stagnation point (the smallest N where adding threads stops paying) that Task 5 profiles.

- [ ] **Step 1: Open the session with a control**

Three cold runs at default threads from the corpus directory, medians recorded as the session-open control:

```bash
rm -rf .celerrate && time ../../release/celerrate check . --verbose > /dev/null
```

- [ ] **Step 2: The curve**

For each N in 1, 2, 4, 8, 10, three cold runs:

```bash
rm -rf .celerrate
RAYON_NUM_THREADS=<N> time ../../release/celerrate check . --verbose > /dev/null
```

Capture wall clock and all phase lines per run. This is fifteen cold runs (roughly 3 to 25 minutes of machine time depending on the single-thread cost); keep the machine idle throughout.

- [ ] **Step 3: Close the session with a control**

Repeat Step 1. If the close control's median drifts more than ~10 % from the open control's, the session is invalid: record that fact, discard the curve, and redo the task.

- [ ] **Step 4: Record and read the curve**

Append section 4: the raw table (N × phases × three runs), then the reading —

- Per-phase speedup from 1 to 10 threads for the parallel phases (analysis fan-out, persist collect entries, file read + input set).
- Effective cores at 10 threads: single-thread fan-out median divided by ten-thread fan-out median.
- The stagnation point: the smallest N beyond which the fan-out improves by less than ~10 %.
- The shape verdict the spec names: early plateau (contention) versus constant shallow slope (serial residue inside the phase). State which the curve shows, per phase.

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): record the cold-run thread scaling curve"
```

---

### Task 5: Sampling profiles at the stagnation point

**Files:**
- Modify: `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append section 5)
- Scratch output: `/tmp/celerrate-profiles/` (not committed)

**Interfaces:**
- Consumes: the stagnation point from Task 4.
- Produces: the worker-time attribution table (allocator / salsa memo access / memo wait / productive work percentages) that decides between local optimization and architectural rework, and the `stub_*` self-time check the spec requires.

- [ ] **Step 1: Capture profiles during cold runs**

macOS's built-in `sample` needs no installation. From the corpus directory, three captures at 10 threads and three at the Task 4 stagnation point:

```bash
mkdir -p /tmp/celerrate-profiles
rm -rf .celerrate
../../release/celerrate check . > /dev/null & sample $! 4 -file /tmp/celerrate-profiles/cold-10t-run1.txt; wait
```

(For the stagnation-point captures, prefix with `RAYON_NUM_THREADS=<N>`.) The 4-second window inside a ~5 s cold run lands mostly in the analysis fan-out and persist phases, which is where the workers live; the walk and read phases contribute few worker samples.

- [ ] **Step 2: Categorize worker samples**

For each profile, take the rayon worker threads' call stacks and bucket every sample:

- **Allocator**: frames in `malloc`, `free`, `realloc`, `nanov2_*`, `szone_*`.
- **Salsa memo access**: frames in salsa's runtime, shard, or ingredient code that are lock or map operations (`parking_lot`, `RwLock`, shard lookup) rather than query execution.
- **Memo wait**: `__psynch_cvwait` (or `_pthread_cond_wait`) reached under salsa's block-on-in-progress-memo path — the pattern the previous effort's diagnosis saw when nine workers idled behind one worker's sequential parse.
- **Productive work**: everything else (lexing, parsing, lowering, inference, serialization frames).

Produce, per capture, the four buckets as percentages of total worker samples. Note: bucket by reading the stacks, not by grepping function names blindly — a `malloc` leaf under a lock acquisition belongs to its parent bucket. Sum the main thread separately; it mostly waits during the fan-out and would dilute the worker signal.

- [ ] **Step 3: The `stub_*` check**

Search every profile for self time in `stub_symbol_table`, `stub_frontier`, and `stub_signature_table` (`crates/celerrate_semantics/src/index.rs`). The previous record found none; confirm or report the change.

- [ ] **Step 4: Record**

Append section 5: the capture protocol, the per-capture bucket table, the medians across captures at each thread count, the `stub_*` finding, and the reading: which bucket owns the missing cores, and with what confidence. If the buckets differ sharply between 10 threads and the stagnation point, say what that difference implies (contention grows with threads; serial residue does not).

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): attribute the fan-out's worker time by sampling profile"
```

---

### Task 6: The mimalloc A/B scaling curve (scratch branch)

**Files:**
- Scratch branch `scratch-mimalloc-probe` (from `docs-cold-run-performance-diagnostic`): modify `crates/celerrate_cli/Cargo.toml`, `crates/celerrate_cli/src/main.rs`, `Cargo.lock`
- Modify (working branch): `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append section 6)

**Interfaces:**
- Consumes: the default-allocator curve from Task 4 (re-anchored in this session per the discipline below).
- Produces: the allocation-bound verdict — whether the allocator changes the scaling slope — that Task 9's lever list and architectural call depend on.

- [ ] **Step 1: Build the probe on a scratch branch**

```bash
git checkout -b scratch-mimalloc-probe docs-cold-run-performance-diagnostic
cargo add mimalloc --package celerrate_cli
```

In `crates/celerrate_cli/src/main.rs`, after the imports:

```rust
#[global_allocator]
static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

```bash
cargo build --release
cp target/release/celerrate /tmp/celerrate-mimalloc
git add -A && git commit -m "🔧 chore(cli): swap the global allocator to mimalloc (scratch probe)"
git checkout docs-cold-run-performance-diagnostic
cargo build --release
```

The probe binary is preserved at `/tmp/celerrate-mimalloc`; the working tree is back on the default allocator. Do not run `cargo deny` on the scratch branch; the licence pass belongs to a future landing effort, not this probe.

- [ ] **Step 2: Same-session A/B curve**

Both binaries, N in 1, 4, 10, three cold runs each, alternating binaries at each N so machine drift cannot masquerade as an allocator effect. From the corpus directory:

```bash
rm -rf .celerrate
RAYON_NUM_THREADS=<N> time ../../release/celerrate check . --verbose > /dev/null
rm -rf .celerrate
RAYON_NUM_THREADS=<N> time /tmp/celerrate-mimalloc check . --verbose > /dev/null
```

Eighteen cold runs total. Open and close the session with the standard control.

- [ ] **Step 3: Record and read**

Append section 6: both curves side by side (wall clock and fan-out phase, per N), then the two comparisons that matter —

- **Level**: mimalloc's 10-thread wall clock against default's (the previous effort saw about −0.9 s; confirm or revise).
- **Slope**: fan-out speedup from 1 to 10 threads, default versus mimalloc. If mimalloc's speedup is materially higher, the contention is allocation-bound and a cheap local lever exists; if the slopes match, the missing cores are not in the allocator and the structural question stands. State the verdict in those terms.

- [ ] **Step 4: Commit the record; the scratch branch stays local**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): record the mimalloc A/B scaling curves"
```

The `scratch-mimalloc-probe` branch is neither pushed nor merged; note its local commit hash in section 6 for provenance.

---

### Task 7: The parse floor and the ratio ceiling (scratch branch)

**Files:**
- Scratch branch `scratch-parse-floor` (from `docs-cold-run-performance-diagnostic`): create `crates/celerrate_cli/src/parse_floor.rs`, modify `crates/celerrate_cli/src/lib.rs`, `crates/celerrate_cli/src/main.rs`
- Modify (working branch): `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append section 7)

**Interfaces:**
- Consumes: the fixed process cost from Task 2; a same-session `cargo xtask benchmark` anchor.
- Produces: the corpus floor (read + lex + parse at 10 threads, plus fixed cost) and the ratio ceiling (same-session PHPStan median ÷ floor) — the central input to Task 9's ambition amendment.

- [ ] **Step 1: Build the probe on a scratch branch**

```bash
git checkout -b scratch-parse-floor docs-cold-run-performance-diagnostic
```

Create `crates/celerrate_cli/src/parse_floor.rs`. Scratch code: it is never merged, so the zero-panic lints may be locally allowed at the top of the file, and no test demands it. It reuses the exact discovery-and-walk slice `Session::start` uses (`session.rs:151-271`), so the file set is the same 6932 files by construction:

```rust
//! Scratch probe: the incompressible cost of this corpus. Walks the
//! project exactly as `Session::start` does, then reads, lexes, and
//! parses every file under rayon, discarding results. Never merged.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stderr)]

use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;

pub fn run(root: &Path) {
    let root = celerrate_vfs::normalize_path(root, root);
    let mut vfs = celerrate_vfs::Vfs::default();
    let loaded_configuration = crate::configuration::load(&root, &mut vfs);
    let configuration_model = loaded_configuration
        .as_ref()
        .map(|loaded| loaded.configuration.clone())
        .unwrap_or_default();
    let discovery = celerrate_project::discover(&root, &configuration_model);

    let started = Instant::now();
    let walk = celerrate_vfs::enumerate_php_files(
        &discovery.walk_roots(),
        &discovery.excluded_roots,
    );
    let walk_elapsed = started.elapsed();

    let started = Instant::now();
    let sources: Vec<String> = walk
        .files
        .par_iter()
        .map(|path| std::fs::read_to_string(path).unwrap_or_default())
        .collect();
    let read_elapsed = started.elapsed();

    let started = Instant::now();
    sources.par_iter().for_each(|source| {
        let _ = celerrate_syntax::parse(source);
    });
    let parse_elapsed = started.elapsed();

    eprintln!("floor: files: {}", walk.files.len());
    eprintln!("floor: walk: {}ms", walk_elapsed.as_millis());
    eprintln!("floor: read: {}ms", read_elapsed.as_millis());
    eprintln!("floor: parse: {}ms", parse_elapsed.as_millis());
}
```

`discover` is what `session.rs` imports from `celerrate_project`, and `parse` is re-exported at `celerrate_syntax`'s crate root (`pub use parse::{Parse, parse}`); if any signature differs from the above, follow the imports at the top of `session.rs` — the probe must call exactly what `Session::start` calls. Wire it in `main.rs` before the normal dispatch:

```rust
fn main() -> ExitCode {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("__parse-floor")) {
        celerrate_cli::parse_floor_probe(std::path::Path::new("."));
        return ExitCode::SUCCESS;
    }
    // ... existing body unchanged
```

and in `lib.rs` expose it (`mod parse_floor;` plus `pub fn parse_floor_probe(root: &Path) { parse_floor::run(root) }`).

```bash
cargo build --release
cp target/release/celerrate /tmp/celerrate-parse-floor
git add -A && git commit -m "🔧 chore(cli): add the parse-floor probe (scratch)"
git checkout docs-cold-run-performance-diagnostic
cargo build --release
```

- [ ] **Step 2: Sanity-check the file set**

From the corpus directory:

```bash
/tmp/celerrate-parse-floor __parse-floor
```

Expected: `floor: files: 6932`. If the count differs from the file count a normal `check --verbose` reports on the same corpus, the probe walks a different set and its floor is invalid; fix the probe before measuring.

- [ ] **Step 3: Measure the floor**

Three runs at 10 threads and three at 1 thread (the 1-thread figure is the parse work's CPU cost, useful for the Amdahl table):

```bash
/tmp/celerrate-parse-floor __parse-floor
RAYON_NUM_THREADS=1 /tmp/celerrate-parse-floor __parse-floor
```

No `.celerrate` involvement; the probe reads only. Medians per line.

- [ ] **Step 4: Anchor the same session with the full protocol**

```bash
cargo xtask benchmark
```

This gives the session's own PHPStan cold median for the ceiling division, per the same-session discipline.

- [ ] **Step 5: Record the floor and the ceiling**

Append section 7: the probe's provenance (scratch commit hash), the raw lines, and the derived figures —

- **Corpus floor** = walk + read + parse medians at 10 threads + the Task 2 fixed process cost.
- **Ratio ceiling** = this session's PHPStan cold median ÷ the corpus floor.
- The explicit reading the spec demands: is ~20x above or below the ceiling on this machine and corpus, and by how much? Also state the floor's own caveat: a real analyzer does more than parse (name resolution, inference, rendering, cache writes), so the true reachable ceiling is below this bound; the bound is what no optimization can beat.

- [ ] **Step 6: Commit**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): record the corpus parse floor and the ratio ceiling"
```

---

### Task 8: Conditional: the shared-nothing process-level bound

**Decision gate, evaluated first:** build this only if, after Tasks 5 and 6, the salsa share of the missing cores is still ambiguous — concretely, if the profile's salsa buckets (memo access + memo wait) hold under ~15 % of worker time AND the mimalloc slope verdict was negative, yet the fan-out still stagnates well below 10 effective cores with no bucket owning the loss. If Tasks 5 and 6 produced a clear owner, append one line to the measurement document saying this probe was not needed and why, commit, and skip to Task 9.

**Files:**
- Modify: `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append section 8)
- Scratch output: `/tmp/celerrate-partitions/` (not committed)

**Interfaces:**
- Consumes: the Task 4 ten-thread cold median as the comparison point.
- Produces: an upper bound on what a PHPStan-style isolated-worker architecture could gain, cited by Task 9 wherever the architectural option is weighed.

- [ ] **Step 1: Partition the corpus by top-level directory**

No code. In the corpus directory, list the top-level directories Celerrate analyzes (the walk roots minus exclusions) and group them into 4 partitions of roughly equal PHP-file counts (`find <dir> -name '*.php' | wc -l` per directory; greedy bin-packing by hand is fine). Record the partition in the measurement document. Perfect balance is not required; the imbalance is part of the honest reading.

- [ ] **Step 2: One corpus copy per partition**

Configuration is discovery-only (`celerrate.toml` at the project root; `arguments.rs` has no flag pointing elsewhere), so each partition gets its own corpus copy carrying its own `celerrate.toml` — which also keeps the four `.celerrate` cache directories separate. On APFS, `cp -Rc` clones instead of copying, so the four copies are near-instant and share storage:

```bash
for K in 1 2 3 4; do
  cp -Rc <corpus directory> /tmp/celerrate-partitions/corpus-$K
done
```

In each `/tmp/celerrate-partitions/corpus-<K>/celerrate.toml`, write an `include` list naming that partition's directories, in the syntax `docs/configuration.md` documents. Verify one partition alone runs and reports only its own files (its `--verbose` walk count must be well below 6932 and the four counts must sum to 6932):

```bash
cd /tmp/celerrate-partitions/corpus-1 && rm -rf .celerrate
<repository root>/target/release/celerrate check . --verbose > /dev/null
```

- [ ] **Step 3: Measure the shared-nothing cold run**

Three repetitions: all 4 partitions as 4 concurrent processes, cold, the measurement being the wall clock until the last one exits:

```bash
for K in 1 2 3 4; do rm -rf /tmp/celerrate-partitions/corpus-$K/.celerrate; done
time ( for K in 1 2 3 4; do ( cd /tmp/celerrate-partitions/corpus-$K && <repository root>/target/release/celerrate check . > /dev/null ) & done; wait )
```

- [ ] **Step 4: Record the bound honestly**

Append section 8: the partition table, the three wall clocks and median, against the Task 4 ten-thread median from the same session (re-run the control if this is a new session). State the two built-in caveats verbatim from the spec: this bound excludes the merge and determinism costs a real implementation would pay, and each process redundantly pays stub loading and startup (which inflates, not deflates, the bound's honesty as an upper limit on the gain).

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): bound the shared-nothing architecture gain"
```

---

### Task 9: Synthesis: the core-second map, the lever list, and the ambition amendment

**Files:**
- Modify: `.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md` (append the closing sections)
- Modify (only after user approval, Step 4): `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (the performance ambition passage, currently the "at least ~20x" wording around its section 9/benchmark discussion)

**Interfaces:**
- Consumes: every prior section of the measurement document.
- Produces: the diagnostic's four deliverables (spec section 8); the input to the next implementation effort.

- [ ] **Step 1: The core-second map**

Append the closing synthesis to the measurement document: one table attributing the cold run's wall clock and its idle cores. Rows: each of the eight phases plus fixed cost plus residue. Columns: median cost, parallel today (yes/no), measured efficiency, loss owner (from Task 5's buckets and Task 6's verdict), and the evidence pointer (section number). Every cause of loss carries its quantified attribution; anything unattributed is labeled unattributed, not guessed.

- [ ] **Step 2: The Amdahl accounting and the lever list**

Two more closing sections:

- **Amdahl**: the best achievable cold total if every parallelizable phase ran at the measured fan-out efficiency, and separately at the mimalloc-improved efficiency if Task 6's verdict was positive. This is the local-optimization path's best case, stated as a number with its derivation shown.
- **Levers**: the prioritized list, each with estimated gain bounded by a measured figure (never intuition), class (local / architectural / dependency), and recommended order. Candidates the campaign has quantified: mimalloc (Task 6 level), whatever Task 5's dominant bucket implies, the remaining serial phases (walk, render, pack writes — Task 3 medians), fixed-cost reduction (Task 2), and the architectural option bounded by Task 8 if it ran. A lever without a measured bound does not go on the list.

- [ ] **Step 3: Draft the ambition amendment**

Append the proposal: the exact replacement wording for the parent design's "at least ~20x PHPStan on a cold run" passage, derived from the Task 7 ceiling and the Amdahl best case. The proposal must state the measured ceiling, the local-path best case, and the architectural-path bound (if measured), and put the proposed ambition figure inside what the evidence defends. Commit the measurement document:

```bash
git add .claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md
git commit -m "📝 docs(perf): close the cold-run diagnostic with the core-second map and lever list"
```

- [ ] **Step 4: STOP — user approval gate**

Present the amendment proposal and the lever list to the user. Do not touch `.claude/superpowers/specs/2026-07-09-celerrate-design.md` until the user approves the wording (spec section 2, last decision). This is a hard stop for the executing agent: report back and wait.

- [ ] **Step 5: Apply the approved amendment**

Once approved verbatim or as revised by the user: edit the parent design's ambition passage to the approved wording, adding a dated update note in its established dated-updates style (see the existing 2026-07-14 and 2026-08-06 notes near its top for the format).

```bash
git add .claude/superpowers/specs/2026-07-09-celerrate-design.md
git commit -m "📝 docs: revise the cold-run performance ambition on measured evidence"
```

---

## Execution notes

- Tasks 2 and 3 are quick; Tasks 4 to 7 are measurement-heavy (each is one continuous idle-machine session; do not interleave them with builds or other work). Task 8 may be skipped by its own gate.
- Task order matters: 4 before 5 (stagnation point), 5 and 6 before 8 (the gate), 2 before 3 and 7 (fixed cost), everything before 9.
- If any session's controls drift, the redo is the session, not the campaign: earlier committed sections stay valid.
