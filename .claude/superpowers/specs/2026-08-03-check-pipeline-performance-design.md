# Celerrate: Check Pipeline Performance Design

Date: 2026-08-03
Status: Approved
Resolves: issue #124
Related: `.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`
(section 11, the scouting measurement this design acts on)

## 1. Problem

The `v0.1.0` release is held back by cold-run performance on the pinned
comparison corpus (PrestaShop 9.0.3,
`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`). The measured cold median is
13.41 s against PHPStan's 38.92 s, a 2.90x wall-clock ratio, while the
CPU ratio is 14.9x: Celerrate runs at roughly 1.1 effective cores of 10
because its heaviest phases are serial.

The phase breakdown recorded on issue #124 (measured before the file-set
equalization, 20.89 s total) locates the cost:

- `suggest::enrich`, the presentation-time did-you-mean pass, at 61.7 %
  of the wall clock. Profiling attributes 68 % of that phase to
  allocation churn: the candidate pool is re-folded and re-cloned per
  diagnostic, and the edit-distance matrix is reallocated per candidate.
- Cache persist entry collection at 18.5 %, serial though it is
  embarrassingly parallel per file.
- File read plus salsa input set at 5.9 %, serial.
- Roughly 1.7 s of the 2.1 s analysis fan-out, serial.

## 2. Decisions taken with the user (2026-08-03)

- **Success criterion**: the cold median over three full protocol runs
  on the pinned corpus is at or under 6 s. The work iterates until that
  holds. This is the gate that unblocks the `v0.1.0` release.
- **Scope**: performance work and the re-measurement only. The release
  itself (tag, changelog, publishing the new ratio in
  `benchmarks/PROTOCOL.md`) is a separate effort that starts once the
  figure is validated.
- **Approach**: incremental and measurement-driven. Re-profile the
  equalized run first, then land one lever at a time (enrich, persist,
  read), re-measuring after each and stopping as soon as the criterion
  holds. Reserve levers are not built ahead of need.
- **Behavior constraint** (not negotiable, mechanically enforced): every
  optimization is behavior-preserving. The corpus diagnostic snapshot
  and the mixed-rate baseline must stay byte-identical at every step.

## 3. Instrumentation and measurement

The phase profile behind issue #124 was produced with throwaway
instrumentation in a scratch directory. Because this design re-measures
after every lever, the instrumentation becomes permanent instead:
per-phase wall-clock timings (filesystem walk, file read + input set,
analysis fan-out, `suggest::enrich`, render, persist collect entries,
persist collect signatures, persist pack writes) emitted on the existing
`--verbose` channel (`verbose.rs`, stderr).

That channel already has exactly the discipline this needs: it is
meta-reporting outside every salsa query, the machine formats stay
byte-identical with or without the flag, and a failed stderr write never
changes the run's outcome. The persist phase already records its elapsed
time (`persist_milliseconds` in `CacheStatistics`); the same pattern
generalizes to the other phases.

The target itself is measured by the published protocol, unchanged: the
harness of `benchmarks/PROTOCOL.md`, cold runs with warmup on the pinned
corpus, median over three full runs.

The first implementation step is a fresh per-phase profile of the
equalized configuration: the issue's breakdown predates the file-set
equalization (5836 diagnostics then, far fewer now, 17 % more files), so
the real budget of each lever is confirmed before any lever lands.

## 4. `suggest::enrich` without the churn

Three changes inside `crates/celerrate_cli/src/suggest.rs`, all with
strictly identical results:

1. **Precomputed pools.** `CandidatePools` stores, per name and at pool
   construction (already at most once per pass): the original name, its
   folded key (`folded_symbol_key` / `folded_member_key`, today
   recomputed for the whole pool on every diagnostic), and its
   lowercased `Vec<char>` with its character length. The member pools
   keyed by (class key, member kind) get the same treatment.
2. **No per-diagnostic clones.** `did_you_mean_across_keys` and
   `member_did_you_mean` stop materializing a filtered `Vec<String>`
   per diagnostic. They iterate the shared pool and skip fold-equal
   entries inline, comparing precomputed keys.
3. **Reused matrix and length filter.** `bounded_distance` receives the
   written name already lowercased (once per attempted key, not once
   per candidate) and works on row buffers owned by a small scratch
   value reused across candidates (filled in place, not reallocated).
   The `abs_diff(lengths) > bound` rejection runs on precomputed
   lengths before any other work, eliminating most of the pool without
   touching memory.

The existing unit tests (distance semantics, ties, guards, pinned
message formats) remain the specification of behavior; the module's
public contract (`enrich`) does not change.

`enrich` is deliberately not parallelized: once the churn is gone the
expected residual cost is too small to justify working around salsa's
non-`Sync` storage. That call is revisited only if the re-measurement
demands it.

## 5. Parallelizing the serial phases

**Persist entry collection** (`collect_entries`,
`crates/celerrate_cli/src/cache/mod.rs`). The three collections (item
trees, member trees, verdicts) are independent per file. The design
applies the pattern already proven in the analysis fan-out
(`analysis.rs`): the salsa storage is `Send` but not `Sync`, so database
handles are cloned up front on the calling thread, then rayon's
`par_iter` runs per file. The existing `sort_entries` calls guarantee a
deterministic pack order whatever the scheduling. The
`analysis::isolated` panic guard stays around the whole collection. The
underlying queries are already memoized by the analysis pass, so the
parallelized work is mostly `Stored*::of` serialization, without salsa
contention.

**File read + salsa input set.** Disk reads and UTF-8 decoding move to a
rayon-parallel step producing an index-ordered buffer; the salsa input
insertion stays serial on the main thread (the setters require `&mut`
on the database), in the current sorted order. The dominant cost (I/O
and decoding) leaves the serial path; determinism is untouched.

**Reserve levers, built only if the re-measurement requires them:**

- The filesystem walk (411 ms in the recorded profile).
- The residual serial share of the analysis fan-out (~1.7 s of 2.1 s
  per the issue). This one starts with a diagnosis, not a fix: whether
  the serialization comes from lock contention or from the up-front
  per-file handle clones is not known.

## 6. Testing

- TDD on the new units: the distance scratch (same outcomes as the
  current implementation on the existing test cases), the precomputed
  pools (folded keys and lengths correct), and the parallel collection
  (byte-for-byte pack equality with the serial collection on a fixture
  project — the key non-regression test).
- The existing gates serve as the integration surface, run at every
  lever: `cargo xtask corpus` (snapshot unchanged), `cargo xtask
  mixed-rate` (baseline unchanged), `cargo test --workspace`, `cargo
  clippy --workspace --all-targets -- -D warnings` (no panic paths in
  the new rayon code).
- Main risk: a subtle ordering or exclusion divergence in enrich that
  changes a suggestion. Covered by the corpus snapshot, which contains
  the rendered suggestions.
- Secondary risk: a warm-run regression (the parallel persist must not
  pay handle clones when nothing changed). Covered by the protocol
  re-measurement, which times warm runs too.

## 7. Out of scope

- The `v0.1.0` release itself (tag, changelog, `PROTOCOL.md` update
  with the new ratio).
- Any change to diagnostic content, suggestion content, or rendered
  output.
- Parallelizing `suggest::enrich` and the reserve levers of section 5,
  unless the measured cold median stays above 6 s without them.
