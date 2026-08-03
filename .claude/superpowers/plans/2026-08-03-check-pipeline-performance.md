# Check Pipeline Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the cold `check` median on the pinned PrestaShop comparison corpus from 13.41 s to at or under 6 s, behavior-identical, to unblock the `v0.1.0` release.

**Architecture:** Three levers landed one at a time with a measurement between each: remove the allocation churn in `suggest::enrich` (precomputed candidate pools, reused edit-distance rows), parallelize the persist entry collection with the salsa handle-clone pattern already used by the analysis fan-out, and parallelize the walk's file reads. A permanent per-phase timing channel behind `--verbose` carries every measurement.

**Tech Stack:** Rust workspace, rayon (already a workspace dependency), salsa (storage is `Send` but not `Sync`), hyperfine via `cargo xtask benchmark`.

**Spec:** `.claude/superpowers/specs/2026-08-03-check-pipeline-performance-design.md`

## Global Constraints

- Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Test modules may locally `#[allow]` these lints. No new panic path anywhere, including inside rayon closures.
- Every change is behavior-preserving: the corpus snapshot (`cargo xtask corpus`) and the mixed-rate baseline (`cargo xtask mixed-rate`) must pass unchanged at every task. Never pass `--bless` in this plan.
- Determinism: no wall-clock reads, randomness, or environment reads inside any salsa query. Wall-clock reads are legal only in orchestration code (the `cache::persist` / `CacheStatistics` precedent).
- All identifiers and comments in English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits (`⚡️ perf(cli): ...`), authored with the repository-configured identity, no Claude attribution.
- The working branch is `perf-check-pipeline` (already created, carries the spec).
- Measurement machine: this reference machine (10 cores). Cold-run measurements need an otherwise idle machine; do not run other heavy jobs concurrently.
- Measurement log: every measurement task appends to `.claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md` (created in Task 3) and commits it, so results survive across sessions and subagents.

---

### Task 1: Per-phase timing counters (`phases.rs`)

**Files:**
- Create: `crates/celerrate_cli/src/phases.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (module declaration list near the top, alongside `mod verbose;`)

**Interfaces:**
- Produces: `crate::phases::{Phase, PhaseTimings}`. `Phase` is a fieldless enum with variants `Walk`, `ReadAndSetInputs`, `Analysis`, `Enrich`, `Render`, `PersistCollectEntries`, `PersistCollectSignatures`, `PersistPackWrites`. `PhaseTimings` is `Default`, thread-shareable behind `Arc`, with `pub fn record(&self, phase: Phase, elapsed: std::time::Duration)` and `pub fn render_lines(&self) -> Vec<String>` (eight lines, one per phase, in the declaration order above).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_cli/src/phases.rs` with only the test module first (the types referenced do not exist yet, so the crate will not compile — that is this cycle's red):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::time::Duration;

    use super::{Phase, PhaseTimings};

    #[test]
    fn every_phase_renders_one_line_in_pipeline_order() {
        let timings = PhaseTimings::default();
        let lines = timings.render_lines();
        assert_eq!(
            lines,
            vec![
                "verbose: phase filesystem walk: 0ms",
                "verbose: phase file read + input set: 0ms",
                "verbose: phase analysis fan-out: 0ms",
                "verbose: phase suggest enrich: 0ms",
                "verbose: phase render report: 0ms",
                "verbose: phase persist: collect entries: 0ms",
                "verbose: phase persist: collect signatures: 0ms",
                "verbose: phase persist: pack writes: 0ms",
            ],
        );
    }

    #[test]
    fn recording_adds_milliseconds_to_the_named_phase_only() {
        let timings = PhaseTimings::default();
        timings.record(Phase::Enrich, Duration::from_millis(120));
        timings.record(Phase::Enrich, Duration::from_millis(30));
        let lines = timings.render_lines();
        assert_eq!(lines[3], "verbose: phase suggest enrich: 150ms");
        assert_eq!(lines[0], "verbose: phase filesystem walk: 0ms");
    }

    #[test]
    fn a_sub_millisecond_duration_still_renders_as_zero() {
        let timings = PhaseTimings::default();
        timings.record(Phase::Walk, Duration::from_micros(400));
        assert_eq!(
            timings.render_lines()[0],
            "verbose: phase filesystem walk: 0ms",
        );
    }
}
```

Declare the module in `crates/celerrate_cli/src/lib.rs`, next to the existing `mod verbose;` declaration:

```rust
mod phases;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli phases -- --nocapture`
Expected: compilation failure, `Phase` and `PhaseTimings` not found.

- [ ] **Step 3: Write the implementation**

Above the test module in `phases.rs`:

```rust
//! Per-phase wall-clock timings for one `check` pass, printed on the
//! `--verbose` channel. Meta-reporting only, like `verbose.rs`: the
//! machine formats stay byte-identical with or without the flag,
//! nothing here enters a salsa query, and the line format is not a
//! stable surface. Wall-clock reads happen at the recording call
//! sites, which are all orchestration code — the same legality
//! argument as `cache::persist`'s own timer. Under `--watch` the
//! counters accumulate across cycles rather than resetting; the
//! channel reports totals for the session, which is what profiling a
//! long-running watch wants anyway.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The measured phases of one `check` pass, in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Walk,
    ReadAndSetInputs,
    Analysis,
    Enrich,
    Render,
    PersistCollectEntries,
    PersistCollectSignatures,
    PersistPackWrites,
}

impl Phase {
    /// Every phase, in the order the lines print.
    const ALL: [Phase; 8] = [
        Phase::Walk,
        Phase::ReadAndSetInputs,
        Phase::Analysis,
        Phase::Enrich,
        Phase::Render,
        Phase::PersistCollectEntries,
        Phase::PersistCollectSignatures,
        Phase::PersistPackWrites,
    ];

    fn label(self) -> &'static str {
        match self {
            Phase::Walk => "filesystem walk",
            Phase::ReadAndSetInputs => "file read + input set",
            Phase::Analysis => "analysis fan-out",
            Phase::Enrich => "suggest enrich",
            Phase::Render => "render report",
            Phase::PersistCollectEntries => "persist: collect entries",
            Phase::PersistCollectSignatures => "persist: collect signatures",
            Phase::PersistPackWrites => "persist: pack writes",
        }
    }
}

/// Accumulated milliseconds per phase. Atomics for the same reason as
/// `CacheStatistics`: the value is shared through `Arc` with call
/// sites that hold `&Session`, and a relaxed counter is all a
/// telemetry total needs.
#[derive(Debug, Default)]
pub struct PhaseTimings {
    walk: AtomicU64,
    read_and_set_inputs: AtomicU64,
    analysis: AtomicU64,
    enrich: AtomicU64,
    render: AtomicU64,
    persist_collect_entries: AtomicU64,
    persist_collect_signatures: AtomicU64,
    persist_pack_writes: AtomicU64,
}

impl PhaseTimings {
    /// The counter behind one phase. A match, not an index: the
    /// workspace denies `indexing_slicing`, and the match is exhaustive
    /// by construction.
    fn counter(&self, phase: Phase) -> &AtomicU64 {
        match phase {
            Phase::Walk => &self.walk,
            Phase::ReadAndSetInputs => &self.read_and_set_inputs,
            Phase::Analysis => &self.analysis,
            Phase::Enrich => &self.enrich,
            Phase::Render => &self.render,
            Phase::PersistCollectEntries => &self.persist_collect_entries,
            Phase::PersistCollectSignatures => &self.persist_collect_signatures,
            Phase::PersistPackWrites => &self.persist_pack_writes,
        }
    }

    /// Adds an elapsed duration to a phase's total, saturating rather
    /// than panicking on the absurd overflow case.
    pub fn record(&self, phase: Phase, elapsed: Duration) {
        let milliseconds = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        self.counter(phase).fetch_add(milliseconds, Ordering::Relaxed);
    }

    /// One line per phase, in pipeline order, zeros included: a phase
    /// that never ran (a machine-format pass skips rich rendering)
    /// still prints, so the reader sees the whole pipeline shape.
    pub fn render_lines(&self) -> Vec<String> {
        Phase::ALL
            .iter()
            .map(|&phase| {
                format!(
                    "verbose: phase {}: {}ms",
                    phase.label(),
                    self.counter(phase).load(Ordering::Relaxed),
                )
            })
            .collect()
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli phases`
Expected: 3 passed.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --package celerrate_cli --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/phases.rs crates/celerrate_cli/src/lib.rs
git commit -m "✨ feat(cli): add per-phase timing counters"
```

---

### Task 2: Wire the timings through session, check path, persist, and verbose

**Files:**
- Modify: `crates/celerrate_cli/src/session.rs` (struct `Session` around line 100-141, `Session::start` around line 147-265)
- Modify: `crates/celerrate_cli/src/lib.rs` (the `Command::Check` arm, around lines 156-242)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`persist_timed`, around lines 124-229)
- Modify: `crates/celerrate_cli/src/verbose.rs` (`report_to`, around line 120)
- Test: `crates/celerrate_cli/src/verbose.rs` (existing test module)

**Interfaces:**
- Consumes: `crate::phases::{Phase, PhaseTimings}` from Task 1.
- Produces: `Session.phases: Arc<PhaseTimings>` (public field, like `Session.statistics`). Verbose output gains the eight phase lines after the run summary line.

- [ ] **Step 1: Write the failing test**

In `crates/celerrate_cli/src/verbose.rs`'s test module, add (reuse the existing `project` fixture helper already in that module):

```rust
#[test]
fn the_phase_lines_print_after_the_run_summary() {
    let root = project(&[(
        "composer.json",
        r#"{"require": {"php": "^8.1"}}"#,
    )]);
    let session = Session::start(root.path());
    let mut output = Vec::new();
    report_to(&session, &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    let summary_position = text.find("verbose: 0 project files").unwrap();
    let walk_position = text.find("verbose: phase filesystem walk:").unwrap();
    assert!(summary_position < walk_position, "summary first, then phases");
    for label in [
        "verbose: phase filesystem walk:",
        "verbose: phase file read + input set:",
        "verbose: phase analysis fan-out:",
        "verbose: phase suggest enrich:",
        "verbose: phase render report:",
        "verbose: phase persist: collect entries:",
        "verbose: phase persist: collect signatures:",
        "verbose: phase persist: pack writes:",
    ] {
        assert!(text.contains(label), "missing {label} in {text}");
    }
}
```

Note: check the exact run-summary wording produced by `render_run_summary` for an empty project against the existing tests in that module; if the rendered count text differs from `"verbose: 0 project files"`, match the existing expectation, not this literal.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_cli the_phase_lines_print_after_the_run_summary`
Expected: FAIL (the phase lines are not printed yet; the `phases` field does not exist).

- [ ] **Step 3: Implement the wiring**

In `session.rs`:

1. Add the import and the field on `Session` (next to `statistics`):

```rust
use crate::phases::PhaseTimings;
```

```rust
    /// The session's per-phase timings, shared with the persist layer.
    /// Never read by analysis; rendered to stderr under `--verbose`.
    pub phases: Arc<PhaseTimings>,
```

2. In `Session::start`, create `let phases = Arc::new(PhaseTimings::default());` next to the `statistics` creation, add `phases: phases.clone(),` to the `Self { ... }` initializer, and time the walk and the load (the two statements at the end of `start`, currently lines 259-263):

```rust
        // Wall-clock reads, legal here: `start` is orchestration, never
        // a salsa query, and the readings feed only the verbose channel.
        let started = std::time::Instant::now();
        let walk = enumerate_php_files(
            &session.discovery.walk_roots(),
            &session.discovery.excluded_roots,
        );
        session
            .phases
            .record(crate::phases::Phase::Walk, started.elapsed());
        let started = std::time::Instant::now();
        session.load(&walk);
        session
            .phases
            .record(crate::phases::Phase::ReadAndSetInputs, started.elapsed());
        session
```

In `lib.rs`, inside the `Command::Check` arm (non-watch path):

1. Time the analysis (currently `let outcome = single_pass(&mut session, || analysis::analyze(&inputs));`):

```rust
            let started = std::time::Instant::now();
            let outcome = single_pass(&mut session, || analysis::analyze(&inputs));
            session
                .phases
                .record(phases::Phase::Analysis, started.elapsed());
```

2. Time the enrichment (currently `diagnostics: suggest::enrich(&session, &outcome.diagnostics),`) — hoist the call so the timer wraps only it:

```rust
            let started = std::time::Instant::now();
            let enriched = suggest::enrich(&session, &outcome.diagnostics);
            session
                .phases
                .record(phases::Phase::Enrich, started.elapsed());
            let mut presented = analysis::AnalysisOutcome {
                diagnostics: enriched,
                panicked: outcome.panicked.clone(),
            };
```

3. Time the rich rendering (the `render::render_report(...)` call) and, on the machine-format path, the `crate::output::write(...)` call, both under `phases::Phase::Render`, with the same `Instant::now()` / `record` bracket as above. Keep the existing error handling exactly as it is; only wrap the call.

In `cache/mod.rs`, inside `persist_timed` (the session is `&mut Session`, so `session.phases` is reachable):

1. Bracket the first `analysis::isolated(|| collect_entries(...))` call with a timer recording `Phase::PersistCollectEntries`.
2. Bracket the second `analysis::isolated(|| collect_signature_entries(...))` call with a timer recording `Phase::PersistCollectSignatures`.
3. Bracket the block from the first `write_when_changed` call through the fourth with one timer recording `Phase::PersistPackWrites`.

Shape for each (record even on the early-return path, mirroring the function's own "the timer wraps the call" discipline):

```rust
    let started = std::time::Instant::now();
    let collected = crate::analysis::isolated(|| collect_entries(&session.sources, &inputs, &panicked));
    session
        .phases
        .record(crate::phases::Phase::PersistCollectEntries, started.elapsed());
    let Ok((trees, member_trees, verdicts)) = collected else {
        return;
    };
```

In `verbose.rs`, at the end of `report_to`, after the run-summary `writeln!`:

```rust
    for line in session.phases.render_lines() {
        writeln!(output, "{line}")?;
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli`
Expected: all pass, including the new wiring test and every existing verbose/session/cache test.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add -u
git commit -m "✨ feat(cli): time the check phases behind --verbose"
```

---

### Task 3: Baseline measurement and fresh per-phase profile

No production code. This task establishes the numbers every later task compares against. The issue #124 phase table predates the file-set equalization, so this fresh profile is what actually budgets the levers.

**Files:**
- Create: `.claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md`

- [ ] **Step 1: Build and prepare the comparison working tree**

```bash
cargo build --release --package celerrate_cli
cargo xtask benchmark
```

`cargo xtask benchmark` fetches the pinned PrestaShop corpus, installs its vendor tree, copies it to `target/benchmark/corpus`, writes the equalizing `celerrate.toml` (`[project] include = ["."]`), and prints the official hyperfine medians (PHPStan cold, Celerrate cold, ratio). Record all three figures. This run takes several minutes (PHPStan side included); that is expected.

- [ ] **Step 2: Take three instrumented cold runs**

From the workspace root:

```bash
cd target/benchmark/corpus
for i in 1 2 3; do
  rm -rf .celerrate
  ../../release/celerrate check . --verbose > /dev/null
done
cd ../../..
```

The phase lines print on stderr, one block of eight per run. If the binary path differs, take it from `ls target/release/celerrate`.

- [ ] **Step 3: Record the baseline**

Create `.claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md` with: the date, the machine, the `cargo xtask benchmark` medians (PHPStan cold, Celerrate cold, ratio), and a table of the three instrumented runs' eight phase lines each, plus the median per phase. State which phases the three levers will attack and what the equalized budget actually is (this replaces the issue's pre-equalization table as the working budget).

- [ ] **Step 4: Sanity-check the budget**

If the fresh profile contradicts the spec's assumption — for example `suggest::enrich` is no longer a top phase on the equalized corpus — STOP and report to the user before implementing Task 4: the lever order may need to change. Otherwise continue.

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md
git commit -m "📝 docs: record the equalized check pipeline phase baseline"
```

---

### Task 4: Reused edit-distance rows (`DistanceScratch`)

**Files:**
- Modify: `crates/celerrate_cli/src/suggest.rs` (`bounded_distance`, lines 32-83, and its test coverage)

**Interfaces:**
- Consumes: nothing new.
- Produces: `struct DistanceScratch` (private to `suggest.rs`, `Default`) and `fn bounded_distance_pooled(written: &[char], candidate: &[char], bound: usize, scratch: &mut DistanceScratch) -> Option<usize>`. The existing `fn bounded_distance(written: &str, candidate: &str, bound: usize) -> Option<usize>` remains, as a thin wrapper, so every existing distance test keeps compiling unchanged. Task 5 consumes both.

- [ ] **Step 1: Write the failing test**

In the `suggest.rs` test module:

```rust
#[test]
fn the_pooled_distance_agrees_with_the_string_form_and_survives_scratch_reuse() {
    let mut scratch = super::DistanceScratch::default();
    let cases: [(&str, &str, usize); 6] = [
        ("svae", "save", 2),
        ("nmae", "name", 2),
        ("php_eol", "PHP_EOL", 2),
        ("draft", "active", 2),
        ("a", "abcd", 2),
        ("Activ", "Active", 2),
    ];
    for (written, candidate, bound) in cases {
        let written_lowercase: Vec<char> = written.to_lowercase().chars().collect();
        let candidate_lowercase: Vec<char> = candidate.to_lowercase().chars().collect();
        // The same scratch across every case: reuse must not leak one
        // computation's rows into the next.
        assert_eq!(
            super::bounded_distance_pooled(
                &written_lowercase,
                &candidate_lowercase,
                bound,
                &mut scratch,
            ),
            super::bounded_distance(written, candidate, bound),
            "{written} vs {candidate}",
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_cli the_pooled_distance_agrees`
Expected: compilation failure, `DistanceScratch` and `bounded_distance_pooled` not found.

- [ ] **Step 3: Implement**

Add above `bounded_distance`:

```rust
/// The three matrix rows `bounded_distance_pooled` works in, owned
/// outside the call so one allocation serves every candidate of a
/// pass. The profiling behind issue #124 attributed most of the
/// enrich phase to reallocating exactly these rows per candidate.
#[derive(Debug, Default)]
struct DistanceScratch {
    before_previous: Vec<usize>,
    previous: Vec<usize>,
    current: Vec<usize>,
}
```

Then move the body of `bounded_distance` into the pooled form. The algorithm is byte-for-byte the same restricted Damerau-Levenshtein; only the storage changes: rows are cleared and refilled in place, and the row rotation swaps buffers instead of allocating.

```rust
/// [`bounded_distance`] over pre-lowercased characters and caller-owned
/// rows: the hot form. The length rejection lives here so no caller
/// can forget it.
fn bounded_distance_pooled(
    written: &[char],
    candidate: &[char],
    bound: usize,
    scratch: &mut DistanceScratch,
) -> Option<usize> {
    if written.len().abs_diff(candidate.len()) > bound {
        return None;
    }
    scratch.before_previous.clear();
    scratch.before_previous.extend(0..=candidate.len());
    scratch.previous.clear();
    scratch.previous.extend(0..=candidate.len());
    for (row, written_character) in written.iter().enumerate() {
        scratch.current.clear();
        scratch.current.push(row + 1);
        for (column, candidate_character) in candidate.iter().enumerate() {
            // The `get` fallbacks are unreachable (the rows are dense
            // by construction); they exist because indexing is denied
            // and a wrong answer here is caught by the tests anyway.
            let substitution = scratch.previous.get(column).copied().unwrap_or(usize::MAX - 1)
                + usize::from(written_character != candidate_character);
            let insertion = scratch.current.get(column).copied().unwrap_or(usize::MAX - 1) + 1;
            let deletion = scratch
                .previous
                .get(column + 1)
                .copied()
                .unwrap_or(usize::MAX - 1)
                + 1;
            let mut best = substitution.min(insertion).min(deletion);
            if row > 0 && column > 0 {
                let previous_written = written.get(row - 1);
                let previous_candidate = candidate.get(column - 1);
                if previous_written == Some(candidate_character)
                    && previous_candidate == Some(written_character)
                {
                    // Adjacent transposition: `..ab` -> `..ba` costs 1,
                    // read off the diagonal two rows up (dense by
                    // construction whenever `row > 0`).
                    let transposition = scratch
                        .before_previous
                        .get(column - 1)
                        .copied()
                        .unwrap_or(usize::MAX - 1)
                        + 1;
                    best = best.min(transposition);
                }
            }
            scratch.current.push(best);
        }
        if scratch.current.iter().min().copied().unwrap_or(0) > bound {
            return None;
        }
        std::mem::swap(&mut scratch.before_previous, &mut scratch.previous);
        std::mem::swap(&mut scratch.previous, &mut scratch.current);
    }
    scratch
        .previous
        .last()
        .copied()
        .filter(|&distance| distance <= bound)
}
```

Then shrink `bounded_distance` to the wrapper (its doc comment stays where it is, on the wrapper, since it documents the algorithm's semantics):

```rust
fn bounded_distance(written: &str, candidate: &str, bound: usize) -> Option<usize> {
    let written: Vec<char> = written.to_lowercase().chars().collect();
    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    bounded_distance_pooled(&written, &candidate, bound, &mut DistanceScratch::default())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli suggest`
Expected: all pass, including every pre-existing distance test.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --package celerrate_cli --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/suggest.rs
git commit -m "⚡️ perf(cli): reuse the edit-distance rows across candidates"
```

---

### Task 5: Precomputed candidate pools, no per-diagnostic clones

**Files:**
- Modify: `crates/celerrate_cli/src/suggest.rs` (`CandidatePools`, `did_you_mean`, `did_you_mean_across_keys`, `symbol_did_you_mean`, `member_did_you_mean`)

**Interfaces:**
- Consumes: `DistanceScratch`, `bounded_distance_pooled` from Task 4.
- Produces: `struct PoolEntry { original: String, folded: String, lowercase: Vec<char> }` (private), pools of `Vec<PoolEntry>` inside `CandidatePools`, and `fn did_you_mean_pooled(written: &str, pool: &[PoolEntry], excluded_folded: Option<&str>, scratch: &mut DistanceScratch) -> (DidYouMean, Option<usize>)`. The public contract of the module (`pub fn enrich`) is untouched.

- [ ] **Step 1: Write the failing test**

In the test module:

```rust
#[test]
fn the_pooled_did_you_mean_agrees_with_the_vector_form() {
    let mut scratch = super::DistanceScratch::default();
    let cases: [(&str, &[&str]); 4] = [
        ("svae", &["save", "wave", "unrelated"]),
        ("sive", &["sove", "save", "sove"]),
        ("svae", &["unrelated"]),
        ("Activ", &["Active", "Passive"]),
    ];
    for (written, candidates) in cases {
        let owned: Vec<String> = candidates.iter().map(|name| (*name).to_owned()).collect();
        let pool = super::pool_entries(owned.clone(), |name| name.to_owned());
        let (pooled, _) = super::did_you_mean_pooled(written, &pool, None, &mut scratch);
        assert_eq!(pooled, super::did_you_mean(written, owned), "{written}");
    }
}

#[test]
fn a_fold_excluded_entry_never_becomes_a_candidate() {
    let mut scratch = super::DistanceScratch::default();
    let pool = super::pool_entries(
        vec!["save".to_owned(), "wave".to_owned()],
        |name| format!("folded::{name}"),
    );
    let (outcome, _) =
        super::did_you_mean_pooled("svae", &pool, Some("folded::save"), &mut scratch);
    // `save` is fold-excluded; `wave` is at distance 2, outside the
    // short-name bound of 1: nothing survives.
    assert_eq!(outcome, super::DidYouMean::Nothing);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli pooled_did_you_mean`
Expected: compilation failure, `pool_entries` and `did_you_mean_pooled` not found.

- [ ] **Step 3: Implement**

1. The entry and its constructor:

```rust
/// One pool name with everything the per-candidate loop needs,
/// computed once at pool construction: the fold key the exclusion
/// compares, and the lowercased characters the distance walks. Before
/// this existed, both were recomputed for the whole pool on every
/// diagnostic — the dominant cost of the enrich phase (issue #124).
struct PoolEntry {
    original: String,
    folded: String,
    lowercase: Vec<char>,
}

/// Builds a pool from declared names and the space's fold function.
fn pool_entries(names: Vec<String>, fold: impl Fn(&str) -> String) -> Vec<PoolEntry> {
    names
        .into_iter()
        .map(|name| PoolEntry {
            folded: fold(&name),
            lowercase: name.to_lowercase().chars().collect(),
            original: name,
        })
        .collect()
}
```

2. The pooled core. It returns the outcome and its minimal distance so `did_you_mean_across_keys` stops recomputing the winner's distance:

```rust
/// [`did_you_mean`] over a precomputed pool: no clone, no re-fold, one
/// scratch for every candidate. `excluded_folded` is the per-diagnostic
/// part of an otherwise shared pool: a name folding equal to the
/// attempted key would have resolved, so it is skipped inline. Answers
/// the outcome and its minimal distance (`None` exactly when the
/// outcome is `Nothing`).
fn did_you_mean_pooled(
    written: &str,
    pool: &[PoolEntry],
    excluded_folded: Option<&str>,
    scratch: &mut DistanceScratch,
) -> (DidYouMean, Option<usize>) {
    let bound = distance_bound(written);
    let written_lowercase: Vec<char> = written.to_lowercase().chars().collect();
    let mut minimum: Option<usize> = None;
    let mut names: Vec<&str> = Vec::new();
    for entry in pool {
        if excluded_folded == Some(entry.folded.as_str()) {
            continue;
        }
        let Some(distance) =
            bounded_distance_pooled(&written_lowercase, &entry.lowercase, bound, scratch)
        else {
            continue;
        };
        match minimum {
            Some(best) if distance > best => {}
            Some(best) if distance == best => {
                if !names.contains(&entry.original.as_str()) {
                    names.push(&entry.original);
                }
            }
            _ => {
                minimum = Some(distance);
                names = vec![&entry.original];
            }
        }
    }
    names.sort_unstable();
    let outcome = match names.len() {
        0 => DidYouMean::Nothing,
        1 => names
            .pop()
            .map_or(DidYouMean::Nothing, |name| DidYouMean::Unique(name.to_owned())),
        _ => DidYouMean::Tie(names.into_iter().map(str::to_owned).collect()),
    };
    (outcome, minimum)
}
```

3. `did_you_mean` becomes the vector-form wrapper the tests exercise (delete its old body):

```rust
fn did_you_mean(written: &str, candidates: Vec<String>) -> DidYouMean {
    let pool = pool_entries(candidates, |name| name.to_owned());
    did_you_mean_pooled(written, &pool, None, &mut DistanceScratch::default()).0
}
```

4. `CandidatePools` reworked: the three symbol slots and the member map hold `Vec<PoolEntry>`, and the struct owns the pass-wide scratch:

```rust
    classes: Option<Vec<PoolEntry>>,
    functions: Option<Vec<PoolEntry>>,
    constants: Option<Vec<PoolEntry>>,
    members: HashMap<(String, MemberKind), Vec<PoolEntry>>,
    reference_cache: HashMap<FileId, Vec<Reference>>,
    /// The pass-wide distance rows, shared by every diagnostic.
    scratch: DistanceScratch,
```

Replace the old `get` accessor with a split-borrow accessor (the pool slice and the scratch are disjoint fields, so one `&mut self` hands both out):

```rust
    /// The declared pool of `space` plus the shared distance scratch,
    /// split-borrowed so one call feeds `did_you_mean_pooled` directly.
    fn symbol_pool(&mut self, space: SymbolSpace) -> (&[PoolEntry], &mut DistanceScratch) {
        let session = self.session;
        let slot = match space {
            SymbolSpace::ClassLike => &mut self.classes,
            SymbolSpace::Function => &mut self.functions,
            SymbolSpace::Constant => &mut self.constants,
        };
        let entries = slot.get_or_insert_with(|| {
            pool_entries(declared_pool(session, space), |name| {
                folded_symbol_key(space, name)
            })
        });
        (entries.as_slice(), &mut self.scratch)
    }

    /// The member pool of (`class_key`, `kind`) plus the shared
    /// scratch, same split-borrow shape as `symbol_pool`.
    fn member_pool(
        &mut self,
        class_key: &str,
        kind: MemberKind,
    ) -> (&[PoolEntry], &mut DistanceScratch) {
        let session = self.session;
        let entries = self
            .members
            .entry((class_key.to_owned(), kind))
            .or_insert_with(|| {
                pool_entries(member_candidates(session, class_key, kind), |name| {
                    folded_member_key(kind, name)
                })
            });
        (entries.as_slice(), &mut self.scratch)
    }
```

5. `did_you_mean_across_keys` loses its filtered-clone loop and its winner-distance recomputation:

```rust
fn did_you_mean_across_keys(
    attempted: Vec<String>,
    pool: &[PoolEntry],
    space: SymbolSpace,
    scratch: &mut DistanceScratch,
) -> Option<(String, DidYouMean)> {
    let mut best: Option<(String, DidYouMean, usize)> = None;
    for key in attempted {
        let folded_key = folded_symbol_key(space, &key);
        let (outcome, distance) =
            did_you_mean_pooled(&key, pool, Some(folded_key.as_str()), scratch);
        let Some(distance) = distance else { continue };
        let replace = match &best {
            Some((_, _, best_distance)) => distance < *best_distance,
            None => true,
        };
        if replace {
            best = Some((key, outcome, distance));
        }
    }
    best.map(|(key, outcome, _)| (key, outcome))
}
```

6. `symbol_did_you_mean` call site becomes:

```rust
    let written = span_text(session, file, range)?;
    let attempted = attempted_keys(session, pools, file, range, space)?;
    let (pool, scratch) = pools.symbol_pool(space);
    let (winning_key, outcome) = did_you_mean_across_keys(attempted, pool, space, scratch)?;
```

(the guard logic below it is unchanged).

7. `member_did_you_mean`: replace the pool fetch, candidate clone, and emptiness check with:

```rust
    let class_key = receiver_class_key(session, pools, file, range, &receiver);
    let written_key = folded_member_key(kind, &member);
    let (pool, scratch) = pools.member_pool(&class_key, kind);
    let (outcome, _) = did_you_mean_pooled(&member, pool, Some(written_key.as_str()), scratch);
    match outcome {
```

(the `Unique`/`Tie`/`Nothing` arms below are unchanged; the old `if candidates.is_empty() { return None; }` disappears — an empty pool yields `Nothing`, the same `None` enrichment).

Delete whatever is now unused (the old `CandidatePools::get`, the old filtered-clone code). Update `CandidatePools::new` for the new fields (`scratch: DistanceScratch::default()`).

- [ ] **Step 4: Run the full package tests**

Run: `cargo test --package celerrate_cli`
Expected: all pass — in particular every fixture-driven enrichment test (`an_unknown_class_with_one_near_declaration_gains_an_applicable_suggestion`, the guard tests, the member tests) passes unchanged. They are the behavior pin.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/suggest.rs
git commit -m "⚡️ perf(cli): precompute the did-you-mean candidate pools"
```

---

### Task 6: Measure lever 1 and hold the behavior gates

No production code.

- [ ] **Step 1: Run the behavior gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: everything passes, snapshot and baseline byte-identical. If `corpus` reports a delta, the enrich rework changed a suggestion: STOP, do not bless, diagnose the divergence in `suggest.rs` against the pre-change behavior.

- [ ] **Step 2: Take three instrumented cold runs**

```bash
cargo build --release --package celerrate_cli
cd target/benchmark/corpus
for i in 1 2 3; do rm -rf .celerrate; ../../release/celerrate check . --verbose > /dev/null; done
cd ../../..
```

- [ ] **Step 3: Record and commit**

Append to the measurements file: the per-phase table of the three runs, the enrich phase before/after, and the new total. Then:

```bash
git add .claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md
git commit -m "📝 docs: record the enrich lever measurement"
```

If the cold total is already at or under 6 s across the three runs, note it, skip Tasks 7-10, and go straight to Task 11 (final protocol measurement) — the remaining levers stay unbuilt (YAGNI), per the spec's stop-at-target rule.

---

### Task 7: Parallelize persist entry collection

**Files:**
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`collect_entries`, lines 240-307)

**Interfaces:**
- Consumes: `AnalysisInputs` (already `Clone`; cloning hands a thread its own salsa handle — the `analysis::analyze` pattern), `rayon::prelude::*`.
- Produces: same signature, same sorted output: `fn collect_entries(&BTreeMap<FileId, SourceFile>, &AnalysisInputs, &BTreeSet<FileId>) -> (TreeEntries, MemberTreeEntries, VerdictEntries)`.

- [ ] **Step 1: Write the failing determinism test**

In the `cache/mod.rs` test module (reuse that module's existing fixture helpers for building a session; mirror how `persist_outcomes_are_counted` at line 1073 builds one). The test pins what parallelism could break — cross-run identity of the collected entries:

```rust
#[test]
fn collecting_entries_twice_yields_identical_sorted_entries() {
    // Several files so the parallel collection has real fan-out.
    let root = project(&[
        ("composer.json", r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#),
        ("src/Alpha.php", "<?php\nnamespace App;\nclass Alpha {}\n"),
        ("src/Beta.php", "<?php\nnamespace App;\nclass Beta {}\n"),
        ("src/Gamma.php", "<?php\nnamespace App;\nclass Gamma {}\n"),
        ("src/Delta.php", "<?php\nnamespace App;\nnew Alpha();\n"),
    ]);
    let session = Session::start(root.path());
    let inputs = session.inputs();
    let panicked = std::collections::BTreeSet::new();
    let first = collect_entries(&session.sources, &inputs, &panicked);
    let second = collect_entries(&session.sources, &inputs, &panicked);
    assert_eq!(first.0, second.0);
    assert_eq!(first.1, second.1);
    assert_eq!(first.2, second.2);
    assert!(!first.0.is_empty(), "the fixture must actually collect trees");
}
```

Adapt the fixture-building lines to the helpers that actually exist in that test module (`project`, or the pattern the existing persist tests use); if the entry types do not derive `PartialEq`, compare their serialized forms with the same encoding `write_when_changed` uses instead.

- [ ] **Step 2: Run the test to verify it fails, then passes on the serial code**

Run: `cargo test --package celerrate_cli collecting_entries_twice`
This test passes on the current serial implementation — that is the point: write it, watch it pass on serial code (a red step is not possible for a pure refactor; the test is the invariant carrier), and keep it green through the rewrite.

- [ ] **Step 3: Rewrite `collect_entries` with rayon**

```rust
fn collect_entries(
    sources: &BTreeMap<FileId, SourceFile>,
    inputs: &AnalysisInputs,
    panicked: &BTreeSet<FileId>,
) -> (TreeEntries, MemberTreeEntries, VerdictEntries) {
    use rayon::prelude::*;

    // The salsa storage is `Send` but not `Sync` (see
    // `analysis::analyze`): every task's handle clone is made up front
    // on this thread and handed to rayon as owned data. The queries
    // underneath are memoized from the pass that just ran, so the
    // parallel work is mostly the `Stored*::of` conversions.
    let tree_tasks: Vec<(SourceFile, AnalysisInputs)> = sources
        .iter()
        .filter(|(file_id, _)| !panicked.contains(file_id))
        .map(|(_, &file)| (file, inputs.clone()))
        .collect();
    let (mut trees, mut member_trees): (TreeEntries, MemberTreeEntries) = tree_tasks
        .into_par_iter()
        .map(|(file, inputs)| {
            let database = &inputs.database;
            let hash = celerrate_db::content_hash(database, file);
            (
                (
                    hash,
                    StoredItemTree::of(celerrate_semantics::item_tree(database, file)),
                ),
                (
                    hash,
                    StoredMemberTree::of(celerrate_semantics::member_tree(database, file)),
                ),
            )
        })
        .unzip();
    sort_entries(&mut trees);
    sort_entries(&mut member_trees);

    let verdict_tasks: Vec<(SourceFile, AnalysisInputs)> = inputs
        .reported
        .iter()
        .filter(|file| !panicked.contains(&file.file_id(&inputs.database)))
        .map(|&file| (file, inputs.clone()))
        .collect();
    let mut verdicts: VerdictEntries = verdict_tasks
        .into_par_iter()
        .map(|(file, inputs)| {
            let database = &inputs.database;
            let file_id = file.file_id(database);
            let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
            // Mirrors `analyze_one` (see the pre-parallel history of
            // this comment): a validated hit's whole entry is only
            // reused byte-for-byte when every stored diagnostic still
            // re-interns AND the typed half itself validated.
            let stored = match verdict::lookup_verdict(&inputs, file) {
                verdict::VerdictLookup::Hit {
                    verdict: stored,
                    typed: verdict::TypedOutcome::Served,
                } if stored.diagnostics.iter().all(|diagnostic| {
                    diagnostic.to_diagnostic(file_id, content_length).is_some()
                }) && stored.directives_convert(content_length).is_some() =>
                {
                    stored.clone()
                }
                _ => composed_verdict(&inputs, file),
            };
            (celerrate_db::content_hash(database, file), stored)
        })
        .collect();
    sort_entries(&mut verdicts);

    (trees, member_trees, verdicts)
}
```

Keep the function's existing doc comment (the panicked-file skip rationale) — it still holds: panicked files are filtered before any query runs, now before the clone instead of inside a serial loop. A worker panic propagates through rayon's `collect` to the calling thread, where the existing `analysis::isolated` wrapper in `persist_timed` catches it exactly as before.

- [ ] **Step 4: Run the tests**

Run: `cargo test --package celerrate_cli`
Expected: all pass, including the determinism test and every existing persist/watch test.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/cache/mod.rs
git commit -m "⚡️ perf(cli): parallelise persist entry collection"
```

---

### Task 8: Measure lever 2 and hold the behavior gates

No production code. Same shape as Task 6.

- [ ] **Step 1: Run the behavior gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: pass, byte-identical. A `corpus` delta here means the parallel collection changed a persisted verdict: STOP and diagnose (the sorted-entry invariant or the clone pattern is wrong).

- [ ] **Step 2: Take three instrumented cold runs** (same commands as Task 6 Step 2) **and one warm run**

The warm run (no `rm -rf .celerrate`, run the check a second time) guards the spec's secondary risk: the per-file handle clones must not make a no-change persist slower. Compare its `persist: collect entries` line against the baseline's warm behavior.

- [ ] **Step 3: Record and commit**

Append the tables to the measurements file; note cold total, the `persist: collect entries` phase before/after, and the warm figure.

```bash
git add .claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md
git commit -m "📝 docs: record the persist lever measurement"
```

If the cold total is at or under 6 s across the three runs, skip Task 9 (unbuilt reserve, YAGNI) and go to Task 11.

---

### Task 9: Parallelize the walk's file reads

**Files:**
- Modify: `crates/celerrate_cli/src/session.rs` (`load`, lines 395-459)

**Interfaces:**
- Consumes: `rayon::prelude::*`.
- Produces: same `fn load(&mut self, walk: &Walk)` signature and observable behavior: same `sources` map, same `internal_errors` in walk order, same salsa input values.

- [ ] **Step 1: Write the failing test**

In `session.rs`'s test module (reuse the `project` fixture helper). The invariant parallelism could break is the walk-order determinism of the recorded read failures:

```rust
#[test]
fn unreadable_files_are_recorded_in_walk_order() {
    use std::os::unix::fs::PermissionsExt;
    let root = project(&[
        ("composer.json", r#"{"require": {"php": "^8.1"}}"#),
        ("alpha.php", "<?php\n"),
        ("beta.php", "<?php\n"),
        ("gamma.php", "<?php\n"),
    ]);
    for name in ["alpha.php", "gamma.php"] {
        let path = root.path().join(name);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    let session = Session::start(root.path());
    let unreadable: Vec<String> = session
        .internal_errors
        .iter()
        .filter_map(|error| match error {
            InternalError::FileUnreadable { path, .. } => {
                Some(path.file_name().unwrap().to_string_lossy().into_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(unreadable, vec!["alpha.php", "gamma.php"]);
}
```

(Adjust the fixture manifest to whatever shape makes the walk reach root-level PHP files in the existing tests — `a_project_without_a_manifest_still_starts_and_says_so` at line 613 shows the discovery default; follow it. If the test runs as root where permission bits do not bite, guard with the same pattern the existing `an_unreadable_file_is_recorded_and_the_run_still_continues` test at line 672 uses — read that test first and mirror its fixture and its guards exactly.)

- [ ] **Step 2: Run the test to verify it passes on serial code**

Run: `cargo test --package celerrate_cli unreadable_files_are_recorded_in_walk_order`
Same note as Task 7: this is the invariant carrier for a pure refactor; green before, must stay green after.

- [ ] **Step 3: Rewrite the read loop**

In `load`, replace the head of the `for path in &walk.files` loop: the reads fan out first, the mutations stay serial and in walk order.

```rust
        // The reads fan out; everything that mutates (`internal_errors`,
        // the VFS, the salsa inputs) stays on this thread, in walk
        // order. Rayon's indexed `collect` preserves input order, so
        // the zip below reunites each path with its own read and the
        // recorded failures keep their serial-era order.
        let read_outcomes: Vec<Result<Vec<u8>, String>> = {
            use rayon::prelude::*;
            walk.files
                .par_iter()
                .map(|path| std::fs::read(path).map_err(|error| error.to_string()))
                .collect()
        };
        let mut wanted: BTreeMap<FileId, SourceFile> = BTreeMap::new();
        for (path, outcome) in walk.files.iter().zip(read_outcomes) {
            let contents = match outcome {
                Ok(contents) => contents,
                Err(reason) => {
                    self.internal_errors.push(InternalError::FileUnreadable {
                        path: path.clone(),
                        reason,
                    });
                    Vec::new()
                }
            };
            let file_id = self.vfs.set_file_contents(path, Some(contents.clone()));
            // ... the rest of the loop body is unchanged ...
        }
```

No `Cargo.toml` change is needed: rayon is already a dependency of `celerrate_cli`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --package celerrate_cli`
Expected: all pass, including the walk-order test and the existing unreadable-file and watch tests.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_cli/src/session.rs
git commit -m "⚡️ perf(cli): parallelise the walk's file reads"
```

---

### Task 10: Measure lever 3, decide

No production code. Same gate commands as Task 8 Step 1, same three instrumented cold runs, append to the measurements file, commit as `📝 docs: record the file-read lever measurement`.

Decision rule:

- Cold total at or under 6 s across the three instrumented runs → go to Task 11.
- Cold total above 6 s → STOP. Do not start the reserve levers (fan-out serial diagnosis, filesystem walk) on your own: per the spec they begin with a diagnosis whose findings the user weighs. Report the measured table, the gap to the target, and which phase now dominates.

---

### Task 11: Final protocol measurement and close-out

No production code.

- [ ] **Step 1: Full gate suite**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: everything green, snapshots byte-identical.

- [ ] **Step 2: Three full protocol runs**

```bash
cargo xtask benchmark
cargo xtask benchmark
cargo xtask benchmark
```

Each prints the PHPStan cold median, the Celerrate cold median, and the ratio. The published figure per the protocol is the median across the runs. This takes a while (each run measures PHPStan too); run them back to back on an otherwise idle machine.

- [ ] **Step 3: Record the final result**

Append to the measurements file: the three runs' medians and ratios, the pooled verdict against the 6 s target, and the before/after summary (13.41 s baseline → final). Commit:

```bash
git add .claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md
git commit -m "📝 docs: record the final check pipeline measurement"
```

- [ ] **Step 4: Report on the issue**

Post a comment on issue #124 with: the new per-phase table (median of the instrumented runs), the new cold medians and ratio from the protocol runs, and which levers landed. Plain description of what changed and what was measured — no references to plan or task numbers.

```bash
gh issue comment 124 --repo celerrate/celerrate --body-file <the drafted comment>
```

Do not close the issue and do not touch `benchmarks/PROTOCOL.md`: publishing the new figure and the release are the follow-up effort, out of this plan's scope.

- [ ] **Step 5: Hand off the branch**

The branch `perf-check-pipeline` now carries the spec, the instrumentation, the levers, and the measurement log. Use superpowers:finishing-a-development-branch to integrate (pull request against `main`).
