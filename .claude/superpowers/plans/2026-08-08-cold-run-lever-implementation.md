# Cold-Run Lever Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the four mechanism-backed cold-run levers from the 2026-08-07 diagnostic (mimalloc, read fan-out cap, parallel walk, interning traffic), each accepted or rejected by its own measurement, then republish the comparison and release v0.1.1.

**Architecture:** Each lever is one small change at one named location: the global allocator at the binary's composition root (`main.rs`), a dedicated thread pool for the file read (`session.rs`), a level-synchronous parallel walk (`walk.rs`), and a targeted reduction of salsa interning traffic found by profiling. Levers land sequentially, each on its own branch with a same-session A/B measurement against its base, merged before the next begins so no gain is counted twice.

**Tech Stack:** Rust 1.94 (edition 2024), rayon 1 (global pool plus one new dedicated pool), salsa 0.27, mimalloc 0.1 (new), hyperfine and `/usr/bin/time -p` for measurement, the pinned PrestaShop comparison corpus.

**Spec:** `.claude/superpowers/specs/2026-08-08-cold-run-lever-implementation-design.md`

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is `forbid`. Production code returns `Result`. Test modules may locally `#[allow]` the clippy lints.
- Analysis behavior is out of bounds: `cargo xtask corpus` and `cargo xtask mixed-rate` must match their committed snapshot and baseline byte for byte on every lever. **Never run either with `--bless` in this effort.** A lever that moves them is a bug or a rejection.
- The measurement machine is the reference 10-core machine behind every published figure. The measurement base is `target/comparison-corpus-equalized` (pinned PrestaShop corpus `fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`, 6932 first-party files, root `celerrate.toml` with `include = ["."]`).
- Measurement discipline (from the diagnostic's protocol): machine otherwise idle; every A/B in one session, sides alternated; three repetitions minimum per side, medians reported; each session opens and closes with a control (three cold runs of the session's base binary, median recorded); control drift above ~10 % between open and close invalidates the session; every figure recorded with session date, commit, and spread.
- Acceptance rule per lever (spec section 5): median cold gain positive and larger than each side's own spread (max minus min); corpus gates byte-identical; `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo deny check` green. Otherwise: revert, record the measurement, move on.
- `COLD_RATIO_FLOOR` in `xtask/src/benchmark.rs` stays `4.0`. Only its doc comment's measurement record moves, and only in Task 6.
- Everything written in English, full words. Commits: gitmoji + Conventional Commits. No references to this plan, its tasks, or the spec in commit messages, PR text, changelog, or public documentation.
- Branch flow: each lever gets its own branch off `main` and its own PR. **The merge is the user's.** After each merge: `git checkout main && git pull --ff-only` before the next task's branch. Cumulative measurement in landing order is what makes the per-lever gains additive.
- The effort's measurement document is `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md`. Every A/B session, every disposition, and the mimalloc supply-chain note land there. Each lever's branch carries its own additions to it.
- `$ROOT` below means the absolute workspace root (`git rev-parse --show-toplevel`). A/B binaries live under `$ROOT/target/ab/` and are copied there immediately after each build, because the next build overwrites `target/release/celerrate`.

---

### Task 1: Measurement infrastructure and session anchor

**Files:**
- Create: `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md`
- Create (untracked, under `target/`): `target/comparison-corpus-equalized/`, `target/ab/`

**Interfaces:**
- Produces: the measurement document every later task appends to; the equalized corpus directory every A/B runs in; the `target/ab/celerrate-base` binary naming convention; the session-anchor figures Task 2's A/B is compared against for sanity.

- [ ] **Step 1: Create the branch**

```bash
git checkout main && git pull --ff-only
git checkout -b perf-mimalloc-allocator
```

- [ ] **Step 2: Prepare the comparison corpus and its equalized copy**

`cargo xtask benchmark` prepares `target/comparison-corpus` from `xtask/comparison-corpus.pin` and runs the full anchor measurement in one go (it takes on the order of ten minutes; PHPStan alone is ~35 s per cold run):

```bash
cargo xtask benchmark
test -d target/comparison-corpus-equalized || cp -R target/comparison-corpus target/comparison-corpus-equalized
printf '[project]\ninclude = ["."]\n' > target/comparison-corpus-equalized/celerrate.toml
mkdir -p target/ab
```

Expected: the benchmark prints a `scenario / median` table and a `cold ratio: <N>x` line, and the file-count cross-check passes at 6932 = 6932. If the ratio sits wildly outside the historical 6.6x to 8.01x band, the machine is not idle; find and stop the interfering process before continuing.

- [ ] **Step 3: Build and stash the base binary**

```bash
cargo build --release
cp target/release/celerrate target/ab/celerrate-base
```

- [ ] **Step 4: Create the measurement document**

Write `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md` with exactly this skeleton (fill the anchor table from Step 2's output; `<commit>` is `git rev-parse --short HEAD`):

```markdown
# Cold-Run Lever Implementation: Measurements

Machine: the reference 10-core machine behind every published figure.
Corpus: pinned PrestaShop comparison corpus
`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`, equalized file set (6932
first-party files, root `celerrate.toml` with `include = ["."]`).
Measurement base: `target/comparison-corpus-equalized`.

## Protocol

- Cold run: `rm -rf .celerrate` in the corpus directory, then
  `<binary> check .` from the corpus directory, binary named by
  absolute path.
- Every A/B is measured in one session, sides alternated
  (base, lever, base, lever, ...), machine otherwise idle, three
  repetitions minimum per side, medians reported, wall clock read
  from `/usr/bin/time -p`'s `real` line.
- Each session opens and closes with a control: three cold runs of
  the session's base binary, median recorded. Control drift above
  ~10 % invalidates the session's comparisons.
- Acceptance per lever: median gain positive and larger than each
  side's spread (max minus min); `cargo xtask corpus` and
  `cargo xtask mixed-rate` byte-identical; the full local gate suite
  green. A lever that fails is reverted and its measurement kept.

## 1. Session anchor (2026-08-08, commit <commit>)

`cargo xtask benchmark`, machine otherwise idle.

| Quantity | Value |
| --- | --- |
| PHPStan cold median | <from the table> |
| Celerrate cold median | <from the table> |
| Cold ratio | <from the "cold ratio" line> |
| File-count cross-check | 6932 reported / 6932 counted |
```

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md
git commit -m "📝 docs(perf): open the lever measurement record with the session anchor"
```

---

### Task 2: Lever 1, mimalloc as the global allocator

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/celerrate_cli/Cargo.toml`
- Modify: `crates/celerrate_cli/src/main.rs`
- Modify: `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md`

**Interfaces:**
- Consumes: Task 1's base binary, corpus, and measurement document.
- Produces: the merged allocator (or a recorded rejection); the post-merge `main` every later lever measures against.

The diagnostic bounds this lever at up to −0.47 s of the ten-thread wall clock, the only gain in the campaign measured outright (its section 6). The spec's precondition: a `cargo deny` licence pass and a supply-chain note before the code.

- [ ] **Step 1: Write the supply-chain note (the spec's precondition)**

Append to the measurement document a `## 2. mimalloc supply-chain note` section recording, from `https://crates.io/crates/mimalloc` and `cargo tree --invert --package mimalloc` after Step 2: the crate version resolved, its licence (MIT), its transitive dependency (`libmimalloc-sys`, which vendors and builds Microsoft's mimalloc C sources), the maintenance signal (last release date, download count), and the reversibility statement: the swap is one attribute at the composition root and its removal restores the system allocator with no other change.

- [ ] **Step 2: Add the dependency**

In the root `Cargo.toml`, `[workspace.dependencies]` (alphabetical position, near `notify = "8"`):

```toml
mimalloc = "0.1"
```

In `crates/celerrate_cli/Cargo.toml`, `[dependencies]` (alphabetical position, after the `celerrate_*` block, near `clap`):

```toml
mimalloc = { workspace = true }
```

- [ ] **Step 3: Run the licence gate before any code (the precondition is ordered)**

```bash
cargo fetch && cargo deny check
```

Expected: PASS. `deny.toml` already allows MIT, and mimalloc is MIT. If `cargo deny check` fails on an advisory or licence for `mimalloc`/`libmimalloc-sys`, stop: the lever is rejected on its precondition, the failure is recorded in the measurement document, and Steps 4 onward are skipped (revert the two Cargo.toml edits, keep the note).

- [ ] **Step 4: Set the allocator at the composition root**

In `crates/celerrate_cli/src/main.rs`, after the existing `use` lines:

```rust
use mimalloc::MiMalloc;

/// Set here, at the binary's composition root, so every library crate
/// stays allocator-neutral.
#[global_allocator]
static GLOBAL_ALLOCATOR: MiMalloc = MiMalloc;
```

Note: `#[global_allocator]` is a safe attribute; it coexists with the workspace's `unsafe_code = "forbid"`. If the build disagrees, stop and reassess rather than weakening the lint.

- [ ] **Step 5: Run the full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: all green, `corpus` and `mixed-rate` byte-identical (an allocator cannot change analysis results; if either moves, something else is wrong; stop).

- [ ] **Step 6: Build the lever binary**

```bash
cargo build --release
cp target/release/celerrate target/ab/celerrate-mimalloc
```

- [ ] **Step 7: A/B session**

From the corpus directory, session-open control, then three alternated pairs, then session-close control:

```bash
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT/target/comparison-corpus-equalized"
rm -f "$ROOT/target/ab/"*.times
# Session-open control: three cold runs of the base binary.
for i in 1 2 3; do
  rm -rf .celerrate
  { /usr/bin/time -p "$ROOT/target/ab/celerrate-base" check . > /dev/null; } 2>> "$ROOT/target/ab/control-open.times"
done
# Alternated A/B.
for i in 1 2 3; do
  for side in base mimalloc; do
    rm -rf .celerrate
    { /usr/bin/time -p "$ROOT/target/ab/celerrate-$side" check . > /dev/null; } 2>> "$ROOT/target/ab/$side.times"
  done
done
# Session-close control.
for i in 1 2 3; do
  rm -rf .celerrate
  { /usr/bin/time -p "$ROOT/target/ab/celerrate-base" check . > /dev/null; } 2>> "$ROOT/target/ab/control-close.times"
done
grep real "$ROOT/target/ab/"*.times
```

- [ ] **Step 8: Disposition**

Compute the medians of the three `real` values per side and the two control medians. Session valid if the controls differ by less than ~10 %. **Accept** if the mimalloc median is below the base median by more than each side's spread (max minus min). Record the whole session (date, commit, all raw values, medians, spreads, verdict) in the measurement document as `## 3. Lever 1 A/B: mimalloc`.

If **rejected**: `git revert` the code commit (or drop the changes if not yet committed), record the verdict, and continue to Step 10 committing only the measurement document; the PR then carries only documentation and the effort moves to Task 3.

- [ ] **Step 9: Commit the lever**

```bash
git add Cargo.toml Cargo.lock crates/celerrate_cli/Cargo.toml crates/celerrate_cli/src/main.rs
git commit -m "⚡️ perf(cli): adopt mimalloc as the global allocator"
```

- [ ] **Step 10: Commit the measurements, push, open the PR**

```bash
git add .claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md
git commit -m "📝 docs(perf): record the mimalloc A/B session"
git push -u origin perf-mimalloc-allocator
gh pr create --title "⚡️ perf(cli): adopt mimalloc as the global allocator" --body "Adopts mimalloc as the global allocator at the binary's composition root. Measured on the pinned comparison corpus in a same-session alternated A/B: <medians and gain from the session>. Licence pass (cargo deny) clean; supply-chain note in the measurement record. Analysis behavior unchanged: corpus snapshot and mixed-rate baseline byte-identical."
```

**Execution pauses here. The merge is the user's.**

---

### Task 3: Lever 2, cap the file-read fan-out

**Files:**
- Modify: `crates/celerrate_cli/src/session.rs:411-441` (`Session::load`)
- Modify: `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md`

**Interfaces:**
- Consumes: post-merge `main`; the corpus and conventions from Task 1.
- Produces: `READ_THREAD_CAP` constant and `read_bytes` helper in `session.rs`; the merged cap (or a recorded rejection).

The diagnostic measured the read phase at 540 ms on the full pool against 240 ms at four threads (−300 ms, both measured; up to −437 ms if it reached the fan-out's efficiency), with `open` time growing +42 % from eight threads to ten. Today the read fans out on rayon's global pool (`session.rs:437-441`); the codebase has no custom pool anywhere yet.

- [ ] **Step 1: Branch**

```bash
git checkout main && git pull --ff-only
git checkout -b perf-read-fan-out-cap
cargo build --release && cp target/release/celerrate target/ab/celerrate-base
```

- [ ] **Step 2: Re-confirm the optimum with a phase sweep (three repetitions per point)**

The `--verbose` channel prints per-phase timings to stderr; the read phase's line is labeled `file read + input set`:

```bash
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT/target/comparison-corpus-equalized"
for n in 2 4 6; do
  for i in 1 2 3; do
    rm -rf .celerrate
    env RAYON_NUM_THREADS=$n "$ROOT/target/ab/celerrate-base" check . --verbose 2>&1 >/dev/null | grep 'file read + input set'
  done
done
```

Take the median per point. Expected: four threads at or near the minimum, consistent with the diagnostic. If a different point is clearly fastest (outside the spread), use that value as the cap in Step 3 and record the sweep either way in the measurement document.

- [ ] **Step 3: Implement the cap**

In `crates/celerrate_cli/src/session.rs`, above `impl Session` (near the other file-level items), add:

```rust
/// The file read fans out over its own pool, capped, not over the
/// global one: the read phase scales negatively past this width on
/// the reference machine (`open` time grows +42 % from eight threads
/// to ten), and this is where its measured curve is fastest.
const READ_THREAD_CAP: usize = 4;

/// One file read, rendered for the walk-order error report.
fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| error.to_string())
}
```

Replace the read fan-out (currently `session.rs:437-441`):

```rust
        let read_outcomes: Vec<Result<Vec<u8>, String>> = walk
            .files
            .par_iter()
            .map(|path| std::fs::read(path).map_err(|error| error.to_string()))
            .collect();
```

with:

```rust
        // A pool that fails to build (resource exhaustion) is not a
        // reason to fail the load; the global pool is the fallback.
        let read_outcomes: Vec<Result<Vec<u8>, String>> =
            match rayon::ThreadPoolBuilder::new()
                .num_threads(READ_THREAD_CAP)
                .build()
            {
                Ok(pool) => pool.install(|| {
                    walk.files
                        .par_iter()
                        .map(|path| read_bytes(path))
                        .collect()
                }),
                Err(_) => walk
                    .files
                    .par_iter()
                    .map(|path| read_bytes(path))
                    .collect(),
            };
```

The indexed `collect` preserves input order inside the dedicated pool exactly as it did on the global one, so the zip below it and the serial mutation tail are untouched.

- [ ] **Step 4: Run the full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: all green, gates byte-identical.

- [ ] **Step 5: Build the lever binary**

```bash
cargo build --release
cp target/release/celerrate target/ab/celerrate-read-cap
```

- [ ] **Step 6: A/B session**

Same session structure as Task 2 Step 7 (open control, three alternated pairs, close control), with `side in base read-cap`. Also capture one `--verbose` run per side and record the `file read + input set` line: the phase's own movement is the mechanism check (expected: roughly 540 ms to roughly 240 ms).

- [ ] **Step 7: Disposition**

Same rule as Task 2 Step 8. Record as `## 4. Lever 2 A/B: read fan-out cap` with the sweep, the wall-clock A/B, and the phase lines. On rejection, revert the code and carry only the measurement.

- [ ] **Step 8: Commit, push, PR**

```bash
git add crates/celerrate_cli/src/session.rs
git commit -m "⚡️ perf(cli): cap the file-read fan-out at its measured optimum"
git add .claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md
git commit -m "📝 docs(perf): record the read-cap sweep and A/B session"
git push -u origin perf-read-fan-out-cap
gh pr create --title "⚡️ perf(cli): cap the file-read fan-out at its measured optimum" --body "Runs the cold-load file read on a dedicated capped pool instead of the full global pool: the phase scales negatively past four threads on the reference machine. Same-session alternated A/B on the pinned corpus: <medians and gain>, phase timing <before> to <after>. Analysis behavior unchanged: corpus snapshot and mixed-rate baseline byte-identical."
```

**Execution pauses here. The merge is the user's.**

---

### Task 4: Lever 3, parallelize the filesystem walk

**Files:**
- Modify: `crates/celerrate_vfs/Cargo.toml`
- Modify: `crates/celerrate_vfs/src/walk.rs`
- Modify: `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md`

**Interfaces:**
- Consumes: post-merge `main`; the corpus and conventions from Task 1.
- Produces: `enumerate_php_files` with an unchanged public signature (`(&[PathBuf], &[PathBuf]) -> Walk`) and unchanged observable contract; internal `Inspection` enum and `inspect_directory` function.

The diagnostic bounds this lever at up to −337 ms (374 ms at ten threads today against 37 ms at ideal scaling); the named mechanism is `BTreeSet` accumulation on the calling thread. The hard constraint from the spec: the walk's output contract must not change — sorted order, deduplication, exclusion pruning, refusal semantics, symlink-cycle protection. The design is a level-synchronous walk: each round inspects the whole frontier of directories in parallel (pure filesystem work, no shared state), then merges the outcomes serially in deterministic order on the calling thread. The spec names this lever the most likely of the four to fail its measurement (the one filesystem phase measured under threads scales negatively); the A/B exists to let it fail cheaply.

- [ ] **Step 1: Branch**

```bash
git checkout main && git pull --ff-only
git checkout -b perf-parallel-walk
cargo build --release && cp target/release/celerrate target/ab/celerrate-base
```

- [ ] **Step 2: Add the determinism regression test (green before the rewrite, the guard during it)**

In the `tests` module of `crates/celerrate_vfs/src/walk.rs`:

```rust
    #[test]
    fn repeated_walks_return_identical_results() {
        // The walk feeds the analyzed set, so its output must be a pure
        // function of the filesystem: any run-to-run difference would
        // ripple into the corpus snapshot.
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        for directory in ["src", "src/a", "src/a/deep", "src/b", "src/c"] {
            for file in ["One.php", "Two.php", "Three.php"] {
                write(&root.join(directory).join(file), "<?php");
            }
        }
        let roots = vec![root.join("src")];
        let first = enumerate_php_files(&roots, &[]);
        assert_eq!(first.files.len(), 15);
        for _ in 0..10 {
            assert_eq!(enumerate_php_files(&roots, &[]), first);
        }
    }
```

- [ ] **Step 3: Run the walk tests to confirm the new test passes on the serial walk**

```bash
cargo test --package celerrate_vfs walk
```

Expected: PASS (all existing tests plus the new one). This is a behavior-preserving restructure, so the suite stays green throughout; the existing tests are the contract, not scaffolding.

- [ ] **Step 4: Add rayon to `celerrate_vfs`**

In `crates/celerrate_vfs/Cargo.toml`, `[dependencies]`:

```toml
celerrate_source = { path = "../celerrate_source" }
rayon = { workspace = true }
```

- [ ] **Step 5: Rewrite the walk as level-synchronous**

In `crates/celerrate_vfs/src/walk.rs`, add `use rayon::prelude::*;` to the imports, then replace `enumerate_php_files` and `walk_directory` (keeping `UnreadableDirectory`, `Walk`, `is_excluded`, `has_php_extension`, and the `Vfs` impl untouched) with:

```rust
/// What inspecting one frontier directory produced. The filesystem
/// work (canonicalize, read_dir, entry classification) happens in
/// parallel with no shared state; every decision that needs order
/// (the visited set, the result sets) happens on the calling thread,
/// in frontier order, which keeps the walk a pure function of the
/// filesystem.
enum Inspection {
    /// `fs::canonicalize` refused. Recorded unconditionally, before
    /// any visited check, exactly as the serial walk did.
    CanonicalizeFailed(UnreadableDirectory),
    /// The directory resolved but `read_dir` refused. Recorded only
    /// if this canonical directory was not already visited, because
    /// the serial walk checked the cycle guard before reading.
    ReadFailed {
        canonical: PathBuf,
        refusal: UnreadableDirectory,
    },
    Read {
        canonical: PathBuf,
        php_files: Vec<PathBuf>,
        subdirectories: Vec<PathBuf>,
    },
}

pub fn enumerate_php_files(roots: &[PathBuf], excluded: &[PathBuf]) -> Walk {
    let mut files = BTreeSet::new();
    let mut unreadable = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut frontier: Vec<PathBuf> = Vec::new();
    for root in roots {
        if is_excluded(root, excluded) {
            continue;
        }
        if root.is_file() {
            files.insert(root.clone());
        } else if root.is_dir() {
            frontier.push(root.clone());
        }
    }
    while !frontier.is_empty() {
        // The sort makes the round's merge order independent of
        // `read_dir`'s enumeration order; the dedup spares inspecting
        // one literal path twice when overlapping roots meet.
        frontier.sort_unstable();
        frontier.dedup();
        let inspections: Vec<Inspection> = frontier
            .par_iter()
            .map(|directory| inspect_directory(directory, excluded))
            .collect();
        frontier.clear();
        for inspection in inspections {
            match inspection {
                Inspection::CanonicalizeFailed(refusal) => {
                    unreadable.insert(refusal);
                }
                Inspection::ReadFailed { canonical, refusal } => {
                    if visited.insert(canonical) {
                        unreadable.insert(refusal);
                    }
                }
                Inspection::Read {
                    canonical,
                    php_files,
                    subdirectories,
                } => {
                    if visited.insert(canonical) {
                        files.extend(php_files);
                        frontier.extend(subdirectories);
                    }
                }
            }
        }
    }
    Walk {
        files: files.into_iter().collect(),
        unreadable_directories: unreadable.into_iter().collect(),
    }
}

/// The parallel half of one walk round: inspect one directory with no
/// shared state. A directory that turns out to be already visited is
/// inspected for nothing; that waste is the price of keeping the
/// visited set serial, and a cycle still terminates because the
/// serial merge never re-enqueues a visited canonical path.
fn inspect_directory(directory: &Path, excluded: &[PathBuf]) -> Inspection {
    let canonical = match fs::canonicalize(directory) {
        Ok(canonical) => canonical,
        Err(reason) => {
            return Inspection::CanonicalizeFailed(UnreadableDirectory {
                path: directory.to_path_buf(),
                reason: reason.to_string(),
            });
        }
    };
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(reason) => {
            return Inspection::ReadFailed {
                canonical,
                refusal: UnreadableDirectory {
                    path: directory.to_path_buf(),
                    reason: reason.to_string(),
                },
            };
        }
    };
    let mut php_files = Vec::new();
    let mut subdirectories = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if is_excluded(&path, excluded) {
            continue;
        }
        if path.is_dir() {
            subdirectories.push(path);
        } else if has_php_extension(&path) && path.is_file() {
            php_files.push(path);
        }
    }
    Inspection::Read {
        canonical,
        php_files,
        subdirectories,
    }
}
```

- [ ] **Step 6: Run the walk tests**

```bash
cargo test --package celerrate_vfs walk
```

Expected: PASS, every existing test unmodified. Pay attention to the three Unix permission and symlink tests: they encode the refusal and cycle semantics the rewrite must preserve.

- [ ] **Step 7: Run the full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo xtask dependency-shape
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: all green (`dependency-shape` is included because `celerrate_vfs` gains a dependency; rayon is an external crate, so the layering DAG over `celerrate_*` crates is untouched), gates byte-identical. The corpus gate is the real-tree proof that the parallel walk enumerates exactly the serial walk's file set.

- [ ] **Step 8: Build, A/B session, disposition**

```bash
cargo build --release
cp target/release/celerrate target/ab/celerrate-walk
```

Same session structure as Task 2 Step 7, `side in base walk`. Capture one `--verbose` run per side and record the `filesystem walk` phase line (expected direction: 374 ms down, ideally toward double digits). Same disposition rule; record as `## 5. Lever 3 A/B: parallel walk`. This lever carries the spec's explicit warning: if the gain does not clear the spread, reject without ceremony, revert, and record.

- [ ] **Step 9: Commit, push, PR**

```bash
git add crates/celerrate_vfs/Cargo.toml Cargo.lock crates/celerrate_vfs/src/walk.rs
git commit -m "⚡️ perf(vfs): parallelize the filesystem walk"
git add .claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md
git commit -m "📝 docs(perf): record the parallel-walk A/B session"
git push -u origin perf-parallel-walk
gh pr create --title "⚡️ perf(vfs): parallelize the filesystem walk" --body "Restructures the walk as level-synchronous: each round inspects the whole directory frontier in parallel and merges serially in deterministic order, preserving the sorted, deduplicated output and the refusal and cycle semantics exactly (all existing walk tests unmodified, corpus snapshot byte-identical). Same-session alternated A/B on the pinned corpus: <medians and gain>, walk phase <before> to <after>."
```

**Execution pauses here. The merge is the user's.**

---

### Task 5: Lever 4, reduce salsa interning traffic

**Files:**
- Modify: to be determined by the profile (expected: one or two of `crates/celerrate_cli/src/analysis.rs`, `crates/celerrate_semantics/src/`, `crates/celerrate_types/src/`)
- Modify: `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md`

**Interfaces:**
- Consumes: post-merge `main`; the corpus and conventions from Task 1.
- Produces: a merged traffic reduction, or a recorded rejection. No public signature changes.

This lever is measure-first by design: the diagnostic bounds it at −91 ms (removing the contention entirely moves the fan-out from about 3.46 to about 3.70 effective cores) and names the lock's entry points, 1598 of 1763 contended samples entering through `salsa::interned::IngredientImpl<C>::intern_id` (the others: `Table::fetch_or_push_page` 83, `ClaimGuard::drop_impl` 39, `SyncTable::try_claim` 37, `Table::record_unfilled_page` 6). What it does not name is which Celerrate query drives that interning, so the first step is a profile, and the change is whatever smallest edit the profile justifies. The bound is small; this lever is the most likely to be rejected as inside noise, and that is an acceptable outcome recorded like any other.

- [ ] **Step 1: Branch**

```bash
git checkout main && git pull --ff-only
git checkout -b perf-interning-traffic
cargo build --release && cp target/release/celerrate target/ab/celerrate-base
```

- [ ] **Step 2: Profile the fan-out and attribute the interning traffic**

Three captures during the analysis fan-out of a cold run:

```bash
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT/target/comparison-corpus-equalized"
for i in 1 2 3; do
  rm -rf .celerrate
  "$ROOT/target/ab/celerrate-base" check . > /dev/null 2>&1 &
  sleep 1.5
  sample celerrate 2 -file "$ROOT/target/ab/interning-profile-$i.txt"
  wait
done
grep -B 5 'intern_id' "$ROOT/target/ab/"interning-profile-*.txt | head -100
```

From the stacks entering `intern_id`, list the Celerrate frames above them and rank the drivers. Record the attribution table in the measurement document as `## 6. Lever 4: interning attribution`.

- [ ] **Step 3: Decide the smallest edit, by decision gate**

In order of preference, take the first that the profile supports:

1. **Pre-intern on the calling thread.** If the dominant interned keys are enumerable before the fan-out (as the existing `item_tree` prewarm at `crates/celerrate_cli/src/analysis.rs:145` already enumerates its tasks), extend that prewarm (or add a sibling loop beside it) to intern them serially before the workers start, so the fan-out finds them present and takes the read path instead of the write lock.
2. **Hoist repeated interning.** If one query interns the same key repeatedly per call, hoist the interned value to a single lookup per call site.
3. **Neither is visible in the profile.** Skip to Step 7 and reject the lever's cheap form: record that the traffic has no single attributable driver, then evaluate the dependency variant: check whether a salsa release later than the pinned 0.27 shards or otherwise relieves the interning lock (read its changelog; `cargo update --package salsa --dry-run` shows what the workspace would take). Attempt the bump only if it is drop-in (no API churn beyond imports, full test suite green); otherwise reject the lever outright.

- [ ] **Step 4: Implement the chosen edit**

The edit must change no query results: interning earlier or fewer times is invisible to analysis output by construction. Keep it to the one or two call sites the profile ranked.

- [ ] **Step 5: Run the full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: all green, gates byte-identical.

- [ ] **Step 6: Build, A/B session with five pairs**

```bash
cargo build --release
cp target/release/celerrate target/ab/celerrate-interning
```

Same session structure as Task 2 Step 7 but with **five** alternated pairs instead of three (`for i in 1 2 3 4 5`), because the bound (−91 ms) is small against the historical run-to-run spread; the extra repetitions tighten the medians. `side in base interning`.

- [ ] **Step 7: Disposition**

Same rule. Record as `## 7. Lever 4 A/B: interning traffic` (or the rejection rationale from Step 3's gate 3). A gain inside the spread is a rejection; say so plainly and revert.

- [ ] **Step 8: Commit, push, PR (only if accepted)**

```bash
git add <the files the profile-guided edit touched>
git commit -m "⚡️ perf: reduce salsa interning traffic in the analysis fan-out"
git add .claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md
git commit -m "📝 docs(perf): record the interning attribution and A/B session"
git push -u origin perf-interning-traffic
gh pr create --title "⚡️ perf: reduce salsa interning traffic in the analysis fan-out" --body "<one sentence on the profile-attributed driver and the edit>. Same-session alternated A/B (five pairs) on the pinned corpus: <medians and gain>. Analysis behavior unchanged: corpus snapshot and mixed-rate baseline byte-identical."
```

If rejected, push a documentation-only branch with the measurement record and open the PR titled `📝 docs(perf): record the interning lever's measured rejection`.

**Execution pauses here. The merge is the user's.**

---

### Task 6: Republish the reference comparison

**Files:**
- Modify: `xtask/src/benchmark.rs:28-65` (the `COLD_RATIO_FLOOR` doc comment's measurement record; **not** the value)
- Modify: `.claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md`

**Interfaces:**
- Consumes: post-merge `main` with every dispositioned lever in.
- Produces: the recorded reference variables every figure in Tasks 7 and 8 copies verbatim: `MEASURED_COMMIT`, `MEASURED_PHPSTAN_MEDIAN`, `MEASURED_CELERRATE_MEDIAN`, `MEASURED_RATIO`, `MEASURED_PHPSTAN_CPU`, `MEASURED_CELERRATE_CPU`, `MEASURED_CPU_RATIO`, `PER_RUN_RATIOS`, `REPORTED_FILE_COUNT`.

This mirrors the v0.1.0 republication: three full `cargo xtask benchmark` runs on the reference machine, hyperfine exports preserved between runs, pooled medians. It runs on the reference machine directly (not in a subagent sandbox), machine otherwise idle.

- [ ] **Step 1: Branch**

```bash
git checkout main && git pull --ff-only
git checkout -b feat-republish-comparison-v0-1-1
```

- [ ] **Step 2: Three full measurement runs, exports preserved**

```bash
for i in 1 2 3; do
  cargo xtask benchmark
  cp target/benchmark/phpstan-cold.json "target/ab/phpstan-cold-$i.json"
  cp target/benchmark/celerrate-cold.json "target/ab/celerrate-cold-$i.json"
done
```

Expected: three `cold ratio` lines, file-count cross-check 6932 = 6932 every time. Acceptance: the ratio's span across the three runs under 10 %. If a run is contaminated (a leaked process, Spotlight indexing; both happened during the v0.1.0 session), discard it, fix the cause, and measure a replacement run.

- [ ] **Step 3: Compute and record the reference variables**

From the six JSON exports: the pooled wall-clock median per tool over all its timed runs (9 PHPStan, 15 Celerrate), the ratio to two decimals, the per-run ratios, and the CPU medians (hyperfine reports one CPU total per invocation; take the median of the three runs' totals per tool). Record all raw runs and the variables in the measurement document as `## 8. Republished reference measurement`. Also compute the effort's summary sentence: the old 8.01x reference, the new ratio, and which levers landed.

- [ ] **Step 4: Update the floor's doc comment record**

In `xtask/src/benchmark.rs`, rewrite the measurement record inside the `COLD_RATIO_FLOOR` doc comment (lines 28-65): the new session date, commit, pooled medians, ratio, per-run ratios, and CPU figures, keeping the explanation of hyperfine's CPU pooling and of the variance the floor's margin absorbs. **The constant stays `4.0`**; state in the comment that the floor holds its value from the 2026-08-06 derivation and moves independently of the ambition, as the existing comment already documents.

- [ ] **Step 5: Verify and commit**

```bash
cargo xtask benchmark --gate
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
git add xtask/src/benchmark.rs .claude/superpowers/plans/2026-08-08-cold-run-lever-implementation-measurements.md
git commit -m "📝 docs(xtask): record the republished reference measurement"
```

The `--gate` run is a fourth full measurement; record it in the measurement document as corroboration.

---

### Task 7: Published documents and the criterion resolution

**Files:**
- Modify: `benchmarks/PROTOCOL.md` (measurement paragraph and commit around line 66, results table around lines 77-82, per-run spread paragraph around lines 88-95)
- Modify: `README.md` (comparison sentences around lines 126-130)
- Modify: `CHANGELOG.md` (`## [Unreleased]` section)
- Modify: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (section 7 position paragraph; the ambition figure itself only in the below-9x branch)
- Modify: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` (status header and closure verdict)

**Interfaces:**
- Consumes: Task 6's reference variables, copied verbatim; nothing invents a number.
- Produces: the changelog body Task 8 dates; the resolved criterion.

The living-versus-dated rule from the v0.1.0 republication holds: `README.md`, `benchmarks/PROTOCOL.md`, `CHANGELOG.md`, and the two specs named above are living documents; everything else under `.claude/superpowers/` is a dated record and is not rewritten.

- [ ] **Step 1: Read both public documents end to end**

Read `benchmarks/PROTOCOL.md` and `README.md` fully, not only the line ranges above: any sentence carrying the old ratio, medians, CPU figures, effective-core figures, or the measured commit is in scope.

- [ ] **Step 2: Update the published figures**

Replace the old reference figures (8.01x, 39.058 s, 4.874 s, 242.5 s, 22.0 s, 11.0x, the per-run range 7.52x-8.04x, commit `4bc0156`) with Task 6's variables wherever they appear in the two documents. Keep the documents' own structure and voice; this is a figure refresh, not a rewrite.

- [ ] **Step 3: Write the changelog entries**

Under `## [Unreleased]` in `CHANGELOG.md`, a `### Changed` section listing each accepted lever in one line each (allocator, read cap, walk, interning; drop the rejected ones), and one line stating the republished cold comparison figure with its predecessor for contrast.

- [ ] **Step 4: Resolve the criterion (user checkpoint)**

Compare `MEASURED_RATIO` against ~9x and prepare the wording for the matching branch; **present it to the user and get approval before writing it**, as the diagnostic's ambition amendment did:

- **Held (`MEASURED_RATIO` ≥ ~9x):** in `2026-07-24-cli-product-v0.1-design.md`, extend the status header: `Status: Closed (v0.1.0, 2026-08-06), closure criterion met as of v0.1.1 (<MEASURED_RATIO>, <date>)`, and update the closure-verdict paragraph (around lines 753-756) to record that the measured figure now sits above ~9x. In `2026-07-09-celerrate-design.md` section 7, update the position paragraph beneath the ambition to the new figure; the ambition wording itself does not move.
- **Not held (`MEASURED_RATIO` < ~9x):** propose replacing "~9x" with the measured figure rounded conservatively (the diagnostic's own reading: ~8x is what the evidence supports with room to spare) in both documents' ambition and criterion wording, annotating each with the date and the pointer to the measurement record, and record in the v0.1 design that v0.1 closes on this amendment. The exact sentences go to the user for approval first.

- [ ] **Step 5: Verify no stale figure survives**

```bash
grep -rn "8\.01\|39\.058\|4\.874\|242\.5\|22\.0 s\|11\.0x\|7\.52\|4bc0156" \
  README.md benchmarks/ CHANGELOG.md .github/workflows/ xtask/src/benchmark.rs
```

Expected: no hits outside dated records (the workflow comments cite the floor's margin, not the ratio; update them only if a hit shows otherwise).

- [ ] **Step 6: Commit**

```bash
git add benchmarks/PROTOCOL.md README.md CHANGELOG.md .claude/superpowers/specs/2026-07-09-celerrate-design.md .claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md
git commit -m "📝 docs(benchmarks): republish the comparison with the landed levers"
```

---

### Task 8: Release v0.1.1

**Files:**
- Modify: `Cargo.toml:7` (workspace version), `Cargo.lock`
- Modify: `CHANGELOG.md` (date the entry, refresh the link references)

**Interfaces:**
- Consumes: Task 7's changelog body.
- Produces: the release PR. The merge, the tag, and the release are the user's.

- [ ] **Step 1: Bump the workspace version**

In the root `Cargo.toml`, line 7: `version = "0.1.1"`. Then refresh the lockfile:

```bash
cargo check --workspace
```

The release workflow's publish job greps `^version = "0.1.1"$` in `Cargo.toml` against the tag, so this line is load-bearing.

- [ ] **Step 2: Date the changelog entry**

In `CHANGELOG.md`: rename `## [Unreleased]` to `## [0.1.1] - <today's date>`, insert a fresh empty `## [Unreleased]` above it, and update the trailing link-reference block:

```markdown
[Unreleased]: https://github.com/celerrate/celerrate/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/celerrate/celerrate/compare/v0.1.0...v0.1.1
```

- [ ] **Step 3: Verify the release notes extract**

```bash
cargo xtask release-notes 0.1.1
```

Expected: prints the 0.1.1 body, exit 0.

- [ ] **Step 4: Run the full local gate suite, recording each verdict**

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

Expected: every command green. `bench --ceilings` guards the warm path the spec keeps as a non-regression guard.

- [ ] **Step 5: Commit, push, PR**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "🔖 chore(release): date the 0.1.1 entry"
git push -u origin feat-republish-comparison-v0-1-1
gh pr create --title "📈 feat(benchmarks): republish the comparison and date the 0.1.1 release" --body "Republishes the cold comparison against the landed allocator and pipeline levers (<old ratio> to <new ratio> on the pinned corpus), resolves the v0.1 closure criterion accordingly, and dates the 0.1.1 changelog entry. Full local gate suite green, including the benchmark ratio gate."
```

**Execution stops here. The merge, the tag (`git tag v0.1.1 && git push origin v0.1.1`), and the release watch (`gh run watch`, `gh release view v0.1.1`) are the user's, as they were for v0.1.0.**
