# Cold-Run Performance Diagnostic: Measurements

Spec: `.claude/superpowers/specs/2026-08-07-cold-run-performance-diagnostic-design.md`
Machine: the reference 10-core machine used by every published figure
Corpus: pinned PrestaShop comparison corpus
(`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`), equalized file set
Binary: `target/release/celerrate`, built at commit `2621b81`

## Protocol

- Measurement base: the equalized corpus copy at
  `target/comparison-corpus-equalized`, cloned from the pinned corpus
  (`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`), with a `celerrate.toml`
  at its root containing `[project]` and `include = ["."]`. Without that
  configuration, discovery falls back to the Composer manifest's autoload
  walk roots, which for this PrestaShop corpus hide 1010 of its 6932
  first-party files from Celerrate while PHPStan still sees all of them;
  the pinned corpus directory alone, with no configuration written into
  it, therefore walks a smaller, non-comparable set. `include = ["."]`
  restores the equal reported file count (6932) that section 1's `cargo
  xtask benchmark` protocol already produces, by writing the same
  configuration into its own working copy. Every section in this document
  from here measures this equalized set.
- Cold run: `rm -rf .celerrate` in the corpus directory, then
  `target/release/celerrate check .` from the corpus directory, invoked
  by an absolute or repository-root-relative path to the binary. (The
  equalized corpus directory sits one path level shallower than the
  originally pinned directory, so the shortcut relative path
  `../../release/celerrate` used against the original directory does not
  resolve to the binary from here; use an unambiguous path instead.)
- Three repetitions minimum per measurement point; medians reported.
- Session discipline: machine otherwise idle; every A/B measured back
  to back in one session; each session opens and closes with a control
  (three cold `check` runs, median recorded); a control drift above
  ~10 % between open and close invalidates the session's comparisons.
- Every figure carries its session date, commit, and spread.

## 1. Session anchor (Task 1)

Date: 2026-08-07
Commit: `2621b81`
Command: `cargo xtask benchmark`
Machine: otherwise idle for the whole run

| Scenario  | Cold median | Standard deviation | Timed runs |
| --------- | ----------- | ------------------- | ---------- |
| PHPStan   | 32.652 s    | ± 1.633 s            | 3          |
| Celerrate | 4.945 s     | ± 0.379 s            | 5          |

Cold ratio (PHPStan median divided by Celerrate median): 6.6x.

Both medians land inside their historical bands (PHPStan 31 s to 39 s;
Celerrate 4.8 s to 5.5 s), which is consistent with an idle machine at
measurement time. The file-count cross-check also matched exactly
(6932 reported, 6932 counted independently), confirming the equal-file-set
invariant held for this run.

This measurement is the campaign's opening reference: the session anchor
against which later sessions are compared. Any later task in this
campaign that cites a figure measured in a different session must say so
explicitly; a cross-session figure is not directly comparable to a
same-session figure without that disclosure.

## 2. Fixed process cost (Task 2)

Date: 2026-08-07
Commit: `2621b81`
Command: `hyperfine --warmup 1 --runs 10 --prepare 'rm -rf .celerrate' 'target/release/celerrate check .'` and `hyperfine --warmup 1 --runs 10 'target/release/celerrate --version'`
Machine: otherwise idle for the whole run
Probe project: an empty project at `/tmp/celerrate-empty-probe`, containing
only an empty `composer.json` (`{}`); discovery succeeds, the walk finds
zero PHP files, and the run exercises startup (including embedded-stub
loading), configuration, discovery, and teardown with no per-file work.

| Scenario                   | Mean    | Standard deviation | Timed runs |
| --------------------------- | ------- | ------------------- | ---------- |
| Empty-project cold `check`  | 19.6 ms | ± 1.4 ms             | 10         |
| `--version`                  | 5.3 ms  | ± 2.3 ms             | 10         |

Derived split: the bare process cost, measured by `--version` and isolating
process start and teardown below discovery and stub loading, is
5.3 ms ± 2.3 ms. The increment from that bare process cost to an empty
cold `check` is 14.3 ms (19.6 ms minus 5.3 ms): startup work that scales
with the binary rather than with the corpus, covering embedded-stub
loading, discovery, and cache directory handling.

The number that matters downstream: 19.6 ms ± 1.4 ms is the fixed cost
floor that any cold run on any corpus pays, before a single file is
analyzed. Against the session anchor's Celerrate cold median of 4.945 s
(section 1, same commit), this floor accounts for under half a percent of
the total cold run time on the pinned corpus, so it is not a material
contributor to the wall clock measured there.

Hyperfine flagged the `--version` measurement's mean as below its 5
millisecond calibration threshold, meaning the shell startup overhead
cannot be isolated with full precision at that timescale. The bare
process cost and the derived increment therefore carry wider uncertainty
than the reported standard deviation alone suggests; the empty-project
cold `check` figure itself, at nearly four times that threshold, is not
affected by this caveat.

## 3. Wall clock versus phase-sum reconciliation (Task 3)

Date: 2026-08-07
Commit: `2621b81`
Corpus measured: the equalized corpus copy at
`target/comparison-corpus-equalized` (see Protocol above: cloned from the
pinned corpus, `celerrate.toml` with `[project]` / `include = ["."]`
written at its root). Verified immediately before measuring, with one
`--verbose` cold run discarded from the timed set: 6932 project files
reported, matching section 1's file count exactly.
Command: `rm -rf .celerrate` then
`target/release/celerrate check . --verbose > /dev/null`, three
repetitions, from the corpus directory, binary invoked by an unambiguous
(non-`../../`) path
Machine: otherwise idle for the whole run
Timing mechanism: `/usr/bin/time -p` wrapping each invocation; wall clock
read from its `real` line (resolution: hundredths of a second)

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 402 | 396 | 425 | 402 |
| file read + input set | 435 | 467 | 517 | 467 |
| analysis fan-out | 1454 | 1634 | 1976 | 1634 |
| suggest enrich | 234 | 234 | 244 | 234 |
| render report | 178 | 188 | 177 | 178 |
| persist: collect entries | 934 | 1041 | 1054 | 1041 |
| persist: collect signatures | 41 | 44 | 48 | 44 |
| persist: pack writes | 135 | 131 | 138 | 135 |
| **Sum of the eight phases** | 3813 | 4135 | 4579 | 4135 |

Wall-clock total per run, as reported by `/usr/bin/time -p`'s `real` line
around the command (includes process startup/teardown and stderr
formatting outside the eight measured phases, same caveat as section 2
above):

| Run | Wall clock |
| --- | ---: |
| Run 1 | 4.68 s |
| Run 2 | 4.94 s |
| Run 3 | 5.41 s |
| **Median** | **4.94 s** |

Reconciliation, using the phase-sum median above and the fixed process
cost floor from section 2:

| Component | Median (ms) | Source |
| --- | ---: | --- |
| Eight-phase sum | 4135 | phase table above |
| Fixed process cost | 19.6 | section 2 |
| Unaccounted residue | 785.4 | wall (4940) minus sum (4135) minus fixed (19.6) |

The residue is 785 ms, about 15.9 % of the 4940 ms wall-clock median,
which is above the roughly 300 ms threshold that calls for a chase before
moving on.

Two candidates were checked. First, `--verbose` stderr formatting: three
further cold runs on the same equalized corpus, same commit, with the
flag dropped (`rm -rf .celerrate` then
`target/release/celerrate check . > /dev/null`), gave wall clocks of
4.96 s, 4.84 s, 5.03 s (median 4.96 s), essentially the same as the
4.94 s median measured with `--verbose` (a 20 ms difference, with the
no-verbose median again the higher of the two). Dropping the flag does
not shrink the wall clock, so stderr formatting from `--verbose` is not
the source of the residue; a 20 ms gap running in the wrong direction is
ordinary run-to-run variance, not a formatting cost. Second,
cache-directory deletion: `rm -rf .celerrate` runs before
`/usr/bin/time -p` starts timing in every repetition of both sets above,
which the protocol already places outside the timed region by
construction; it is confirmed not to be a candidate for this residue.

Neither candidate explains the gap. The eight phases are instrumentation
points inside `check`; they do not cover process startup before the first
phase begins or teardown after the last phase ends. Across the six cold
runs in this section (three `--verbose`, three without), real time ran
4.68 s to 5.41 s against user time of 16.56 s to 17.30 s, a per-run ratio
of real to user time ranging from about 27.6 % to 32.6 % (not the roughly
one-third figure an earlier draft of this section stated), confirming the
binary is heavily multi-threaded during the cold run. Thread-pool setup,
teardown, and scheduling variance are plausible contributors that the
eight phases, as currently instrumented, cannot isolate. The residue is
recorded as unattributed beyond ruling out the two candidates above:
785 ms, about 15.9 % of the wall-clock median.

## 4. Thread-count scaling curve (Task 4)

Date: 2026-08-07
Commit: `2621b81`
Corpus measured: the equalized corpus copy at
`target/comparison-corpus-equalized` (the same corpus as sections 1 and
3). Every run below reported 6932 project files, matching the equal-file-
set invariant.
Command: `rm -rf .celerrate` then, for the curve,
`env RAYON_NUM_THREADS=<N> /usr/bin/time -p target/release/celerrate check . --verbose > /dev/null`;
the session-open and session-close controls repeat the same command
without setting `RAYON_NUM_THREADS` (the binary's default thread count).
Both from the corpus directory, binary invoked by an unambiguous absolute
path.
Machine: otherwise idle for the whole session (10 physical cores, so
N = 10 is the full-width case).
Timing mechanism: `/usr/bin/time -p` wrapping each invocation; wall clock
read from its `real` line (resolution: hundredths of a second), the same
mechanism section 3 used, so wall clocks are directly comparable across
the two sections.

Deviation from the brief: the brief's Step 1 and Step 2 command snippets
run from the pinned corpus directory with the relative binary path
`../../release/celerrate`. That directory carries no `celerrate.toml`, so
discovery falls back to the Composer autoload roots and walks only 5922
of the corpus's 6932 files, which is not comparable to this campaign's
other sections. Per the correction already recorded in the Protocol
section above, every run in this section instead uses the equalized
corpus directory and the binary by absolute path; the first run of the
session-open control confirmed a 6932-file walk before any timed
measurement was taken, and every subsequent run reported the same count.

### Session-open control

Three cold runs at the binary's default thread count:

| Run | Wall clock |
| --- | ---: |
| Run 1 | 5.16 s |
| Run 2 | 4.67 s |
| Run 3 | 4.46 s |
| **Median** | **4.67 s** |

### The curve

Phase timings in milliseconds, three cold runs per thread count. "Sum of
the eight phases" is each run's own total, medianed across the three
runs (matching section 3's method: the median of the three per-run
totals, not the sum of the per-phase medians).

#### N = 1

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 369 | 373 | 372 | 372 |
| file read + input set | 405 | 394 | 393 | 394 |
| analysis fan-out | 5354 | 5381 | 5355 | 5355 |
| suggest enrich | 231 | 234 | 234 | 234 |
| render report | 186 | 187 | 189 | 187 |
| persist: collect entries | 4053 | 4074 | 4045 | 4053 |
| persist: collect signatures | 34 | 34 | 34 | 34 |
| persist: pack writes | 120 | 118 | 117 | 118 |
| **Sum of the eight phases** | 10752 | 10795 | 10739 | 10752 |

Wall clock: Run 1 11.41 s, Run 2 11.45 s, Run 3 11.39 s. Median 11.41 s.

#### N = 2

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 370 | 367 | 369 | 369 |
| file read + input set | 286 | 288 | 287 | 287 |
| analysis fan-out | 2918 | 3066 | 2917 | 2918 |
| suggest enrich | 232 | 239 | 236 | 236 |
| render report | 187 | 191 | 188 | 188 |
| persist: collect entries | 2281 | 2377 | 2307 | 2307 |
| persist: collect signatures | 35 | 37 | 39 | 37 |
| persist: pack writes | 118 | 122 | 117 | 118 |
| **Sum of the eight phases** | 6427 | 6687 | 6460 | 6460 |

Wall clock: Run 1 7.10 s, Run 2 7.37 s, Run 3 7.13 s. Median 7.13 s.

#### N = 4

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 375 | 370 | 371 | 371 |
| file read + input set | 240 | 236 | 242 | 240 |
| analysis fan-out | 1913 | 2223 | 2069 | 2069 |
| suggest enrich | 231 | 232 | 243 | 232 |
| render report | 185 | 189 | 189 | 189 |
| persist: collect entries | 1251 | 1296 | 1265 | 1265 |
| persist: collect signatures | 37 | 40 | 38 | 38 |
| persist: pack writes | 120 | 123 | 114 | 120 |
| **Sum of the eight phases** | 4352 | 4709 | 4531 | 4531 |

Wall clock: Run 1 5.05 s, Run 2 5.41 s, Run 3 5.23 s. Median 5.23 s.

#### N = 8

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 368 | 373 | 370 | 370 |
| file read + input set | 420 | 471 | 467 | 467 |
| analysis fan-out | 1485 | 1652 | 1460 | 1485 |
| suggest enrich | 230 | 251 | 232 | 232 |
| render report | 191 | 197 | 185 | 191 |
| persist: collect entries | 946 | 961 | 944 | 946 |
| persist: collect signatures | 40 | 40 | 40 | 40 |
| persist: pack writes | 115 | 121 | 119 | 119 |
| **Sum of the eight phases** | 3795 | 4066 | 3817 | 3817 |

Wall clock: Run 1 4.53 s, Run 2 4.80 s, Run 3 4.56 s. Median 4.56 s.

#### N = 10

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 368 | 374 | 379 | 374 |
| file read + input set | 540 | 550 | 515 | 540 |
| analysis fan-out | 1535 | 1399 | 1368 | 1399 |
| suggest enrich | 236 | 236 | 239 | 236 |
| render report | 187 | 187 | 189 | 187 |
| persist: collect entries | 994 | 875 | 874 | 875 |
| persist: collect signatures | 40 | 43 | 42 | 42 |
| persist: pack writes | 115 | 128 | 122 | 122 |
| **Sum of the eight phases** | 4015 | 3792 | 3728 | 3792 |

Wall clock: Run 1 4.83 s, Run 2 4.57 s, Run 3 4.49 s. Median 4.57 s.

### Session-close control

Three cold runs at the binary's default thread count:

| Run | Wall clock |
| --- | ---: |
| Run 1 | 4.51 s |
| Run 2 | 4.77 s |
| Run 3 | 4.59 s |
| **Median** | **4.59 s** |

Drift from open to close control: absolute difference 0.08 s over the
open median of 4.67 s, about 1.7 %, well inside the ~10 % threshold that
would invalidate the session. The session stayed valid throughout, and
the machine remained idle for its full length. The two default-thread
controls (4.67 s and 4.59 s) also bracket the N = 10 explicit-thread-
count median measured mid-session (4.57 s), consistent with the default
thread count equalling the full ten-core width on this machine.

The genuinely like-for-like comparison is against section 3, which ran
the identical command (`rm -rf .celerrate` then `check . --verbose`) on
the identical corpus, binary, and day, and recorded a three-run median
of 4.94 s. Section 3 and this section were measured in separate tool
sessions, so this is a cross-session comparison under the Protocol's
session discipline, disclosed here as such. Against that 4.94 s median,
this section's open control (4.67 s) is lower by 0.27 s, about 5.5 %,
and the close control (4.59 s) is lower by 0.35 s, about 7.1 %;
averaging the two controls (4.63 s) gives a gap of about 6.3 %. That
gap is comfortably inside the ~10 % drift threshold the Protocol sets,
even though this is a cross-session comparison the threshold was not
written to judge; it is also smaller than the spread each session's own
runs already show.
Section 3's three wall clocks ranged from 4.68 s to 5.41 s; this
section's six control runs (three open, three close) ranged from 4.46 s
to 5.16 s; the two ranges overlap between 4.68 s and 5.16 s, and
section 3's lowest run (4.68 s) sits below this section's highest
control run (5.16 s). A session-median difference of this size is
therefore consistent with ordinary run-to-run variation rather than a
systematic effect. No cause distinguishing the two sessions was
measured, and none is asserted; in particular, the earlier explanation
citing a leaner `--verbose`-only workload does not hold, since section 3
also ran with `--verbose`.

Against section 1's historical band (4.8 s to 5.5 s), a cross-harness
and cross-protocol comparison, since that band comes from
`cargo xtask benchmark` rather than the `/usr/bin/time -p` and
`--verbose` protocol sections 3 and 4 share, this section's controls
(4.67 s and 4.59 s) land at or slightly below the low end. Given the
closer, like-for-like divergence already found against section 3, this
is read as the same ordinary variation and not treated as a separate
finding.

### Reading the curve

Wall-clock medians by thread count:

| N | Wall clock median |
| --- | ---: |
| 1 | 11.41 s |
| 2 | 7.13 s |
| 4 | 5.23 s |
| 8 | 4.56 s |
| 10 | 4.57 s |

**Per-phase speedup from 1 to 10 threads** (single-thread median divided
by ten-thread median), for the three parallel phases the brief names:

- Analysis fan-out: 5355 ms to 1399 ms, a 3.83x speedup.
- Persist: collect entries: 4053 ms to 875 ms, a 4.63x speedup.
- File read + input set: 394 ms to 540 ms, which is not a speedup: the
  phase runs 1.37x slower at ten threads than at one.

**Effective cores at 10 threads** (single-thread fan-out median divided
by ten-thread fan-out median): 5355 / 1399, about 3.83 effective cores
out of the 10 physical cores available.

**Stagnation point** (the smallest N beyond which the fan-out improves by
less than ~10 % going to the next measured N):

| Step | Fan-out median before (ms) | Fan-out median after (ms) | Improvement |
| --- | ---: | ---: | ---: |
| N = 1 to N = 2 | 5355 | 2918 | 45.5 % |
| N = 2 to N = 4 | 2918 | 2069 | 29.1 % |
| N = 4 to N = 8 | 2069 | 1485 | 28.2 % |
| N = 8 to N = 10 | 1485 | 1399 | 5.8 % |

The stagnation point is N = 8: the step from 8 to 10 threads gains only
5.8 %, the first step under the ~10 % threshold. Persist: collect entries
follows the same schedule (43.1 %, 45.2 %, 25.2 %, 7.5 % across the same
four steps) and stagnates at the same N = 8.

**Shape verdict per phase** (the spec's two named shapes: early plateau,
meaning contention, versus a constant but shallow slope, meaning serial
residue inside the phase):

- Analysis fan-out: early plateau. Gains shrink monotonically and fall
  under the 10 % threshold by N = 8, well short of the ten-core width;
  the phase is contention-bound rather than carrying a flat serial
  residue.
- Persist: collect entries: early plateau, on the same schedule as
  fan-out, stagnating at N = 8. Same reading: contention, not serial
  residue.
- File read + input set: neither named shape. It improves from N = 1 to
  N = 4 (394 ms to 240 ms), then reverses: 467 ms at N = 8 and 540 ms at
  N = 10, ending slower than the single-thread run. This is sharper than
  an early plateau: the phase does not merely stop gaining beyond a
  point, it loses ground beyond N = 4. The pattern is consistent with
  contention that outweighs its own parallel benefit at higher thread
  counts (plausibly filesystem or page-cache contention among concurrent
  file reads), and is recorded here as a deviation from the spec's
  two-shape taxonomy rather than forced into either label.


## 5. Sampling profiles at the stagnation point (Task 5)

Date: 2026-08-07
Commit: `2621b81`, the same commit as every section above, with no source
file changed.
Corpus measured: the equalized corpus copy at
`target/comparison-corpus-equalized` (the same corpus as sections 1, 3
and 4). Every run below reported 6932 project files.
Machine: otherwise idle for the whole session; one capture at a time,
nothing else running, no build during any capture.
Profiler: `sample`, the macOS built-in sampling profiler, nominal
interval 1 millisecond, attaching 25 to 35 milliseconds after process
launch.
Profiles written to `/tmp/celerrate-profiles/sym/` (not committed).

Thread counts profiled: N = 10 (full width), N = 8 (the stagnation point
section 4 identified) and N = 1 (the serial baseline, without which the
missing cores cannot be accounted for).

### The profiling artefact, and why it differs from sections 1 to 4

The workspace release profile sets `strip = "symbols"`, so the artefact
sections 1 to 4 measure resolves no Celerrate function name and no
profile taken from it can name a single frame of the program under
study. Every profile in this section therefore comes from a
symbol-carrying and line-table-carrying build of the same commit:

```
cargo build --release --config 'profile.release.strip="none"' --config 'profile.release.debug=1'
```

Both settings are passed on the command line. No file in the repository
was modified to obtain them, and nothing about the build configuration is
committed. The canonical stripped artefact was copied aside before each
rebuild and restored byte for byte afterwards (identical SHA-256), so the
artefact sections 1 to 4 cite remains exactly reproducible. `strip` and
`debug` change only what is emitted alongside the machine code, not the
machine code itself, and `debug=1` was included because `stub_frontier`
is a plain function with a single caller that can be inlined out of the
symbol table without line tables.

### Equivalence control

Three cold runs at each thread count on the symbol-carrying artefact,
using section 4's exact protocol (`rm -rf .celerrate`, then
`env RAYON_NUM_THREADS=<N> /usr/bin/time -p <binary> check . --verbose > /dev/null`),
so the wall clocks are directly comparable:

| N | Run 1 | Run 2 | Run 3 | Median | Section 4 median | Difference |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 10 | 5.69 s | 4.48 s | 4.75 s | 4.75 s | 4.57 s | +3.9 % |
| 8 | 5.06 s | 4.65 s | 4.68 s | 4.68 s | 4.56 s | +2.6 % |
| 1 | 13.61 s | 11.88 s | 11.76 s | 11.88 s | 11.41 s | +4.1 % |

All three sit inside the Protocol's ~10 % drift threshold, so the
symbol-carrying artefact is treated as the same performance object and
this section's figures are read against section 4's. The first run at
each thread count is the slowest of its three in all three cases, which
is the expected shape for a freshly linked binary that no run has yet
paged in; it is reported rather than discarded.

The analysis fan-out's own wall time, from the same control runs, is the
quantity the reconciliation below depends on, so it is reported with its
full spread rather than as a single median:

| N | Run 1 | Run 2 | Run 3 | Mean | Spread about the mean | Section 4 median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 10 | 1652 ms | 1361 ms | 1648 ms | 1554 ms | -12.4 % to +6.3 % | 1399 ms |
| 8 | 1625 ms | 1590 ms | 1577 ms | 1597 ms | -1.3 % to +1.8 % | 1485 ms |
| 1 | 5291 ms | 5314 ms | 5378 ms | 5328 ms | -0.7 % to +0.9 % | 5355 ms |

The single-thread fan-out is reproducible to better than one percent and
matches section 4 to within 0.8 %. The ten-thread fan-out is not: its
three readings span 291 milliseconds, about 19 % of their mean. That
spread is the dominant uncertainty in everything derived from it below,
and it is propagated rather than hidden.

### Capture protocol

Nine captures: three at each thread count. Each capture is one cold run.

```
rm -rf .celerrate
env RAYON_NUM_THREADS=<N> /absolute/path/to/target/release/celerrate check . > /dev/null 2>&1 &
sample $! <W> -file /tmp/celerrate-profiles/sym/cold-<N>t-run<K>.txt
wait
```

The window `<W>` is 4 seconds at eight and ten threads, against a cold
run of about 4.7 seconds, and 11 seconds at one thread, against a cold
run of about 11.9 seconds. Run from the equalized corpus directory,
binary by absolute path, with `rm -rf .celerrate` outside the sampled
region. The deviation section 4 records applies here too: the brief's
snippet runs from the pinned corpus directory, which walks a smaller,
non-comparable file set.

| Capture | Worker threads | Worker samples (total) | Samples per worker | Main-thread samples |
| --- | ---: | ---: | ---: | ---: |
| 10 threads, run 1 | 10 | 22390 | 2239 | 2546 |
| 10 threads, run 2 | 10 | 22480 | 2248 | 2563 |
| 10 threads, run 3 | 10 | 22360 | 2236 | 2570 |
| 8 threads, run 1 | 8 | 18864 | 2358 | 2671 |
| 8 threads, run 2 | 8 | 18568 | 2321 | 2636 |
| 8 threads, run 3 | 8 | 19056 | 2382 | 2703 |
| 1 thread, run 1 | 1 | 8833 | 8833 | 9126 |
| 1 thread, run 2 | 1 | 8853 | 8853 | 9157 |
| 1 thread, run 3 | 1 | 8446 | 8446 | 8724 |

The effective interval is longer than the nominal 1 millisecond, because
`sample` sleeps one millisecond of run time between samples and pays its
own collection cost on top, and it depends on how many threads it must
walk. Taking the main thread, which is alive for the whole window, as the
reference: 1.571, 1.561 and 1.556 milliseconds at ten threads; 1.498,
1.517 and 1.480 at eight; 1.205, 1.201 and 1.261 at one. Every
core-second figure below uses its own capture's interval, never a shared
one.

Sampling-window coverage, stated for all three thread counts because the
worker pool is created after the run starts, during the filesystem walk:
the worker threads are alive for 3.52, 3.51 and 3.48 seconds of the
4.00-second window at ten threads (87 % to 88 %), 3.53, 3.52 and 3.52
seconds at eight (88 %), and 10.65, 10.63 and 10.65 seconds of the
11.00-second window at one thread (97 %). The analysis fan-out lies
wholly inside the worker lifetime in all nine captures, which is what
makes the phase comparable across thread counts. Every worker percentage
below is a share of worker lifetime inside the window, not of the whole
process lifetime.

### Method: how stacks were bucketed

Each capture was parsed into its per-thread call tree, and each node's
self samples (its count minus the sum of its children's counts) were
assigned to exactly one bucket by reading the whole stack from thread
start to leaf, not by matching the leaf name. Per-capture bucket counts
sum exactly to the capture's worker sample total, which is the arithmetic
check that no sample was double-counted or dropped. The main thread is
summed separately and excluded from every worker percentage.

The rules, in the order they are tested, so that a lock owns the
allocation performed beneath it:

**Memo wait.** Any stack containing `salsa::runtime::Running::block_on`
or `salsa::runtime::dependency_graph::DependencyGraph::block_on`. This is
the block-on-an-in-progress-memo path, the pattern the previous effort's
diagnosis reported. Example stack, 129 samples in the ten-thread run 1:

    salsa::function::fetch::...::fetch_cold
      <- salsa::runtime::Running::block_on
      <- salsa::runtime::dependency_graph::DependencyGraph::block_on
      <- parking_lot::condvar::Condvar::wait_until_internal
      <- _pthread_cond_wait <- __psynch_cvwait

**Contended lock.** Any stack containing a `parking_lot` slow path
(`RawMutex::lock_slow`, `RawRwLock::lock_*`, `parking_lot_core::park`).
Example stack, 119 samples:

    celerrate_types::flow::Environment::join_any
      <- celerrate_types::construction::...
      <- salsa::interned::IngredientImpl<C>::intern_id
      <- parking_lot::raw_mutex::RawMutex::lock_slow
      <- cthread_yield <- swtch_pri

**Parked.** Any stack containing `rayon_core::sleep::Sleep::sleep`, split
in two by whether the worker holds a job: with
`rayon_core::join::join_context` or `rayon_core::registry::in_worker`
present it is parked at a split point of a parallel iterator, without
them it is parked with no job at all. Example of the second, 7149 samples
at ten threads, run 1:

    rayon_core::registry::ThreadBuilder::run
      <- rayon_core::registry::WorkerThread::wait_until_cold
      <- rayon_core::sleep::Sleep::sleep
      <- _pthread_cond_wait <- __psynch_cvwait

**Otherwise, the nearest attributable frame from the leaf upwards.**
Frames that carry no attribution of their own (`core::`, `alloc::`,
`std::`, `hashbrown::`, `indexmap::`, the `libsystem_platform` memory
helpers, and the like) are skipped, and the first frame that does carry
one decides:

- a leaf in `libsystem_malloc` is the **allocator**;
- a file operation in `libsystem_kernel` (`open`, `read`, `close`,
  `fstat` and their family) is **file input and output**;
- a `salsa::` frame is **salsa memo access**;
- a `celerrate_*` or `rowan::` frame is **productive work**, except that a
  salsa trait implementation generated inside a Celerrate module is
  counted as memo access wearing a Celerrate name (see the ambiguity note
  below);
- a `rayon_core::` frame outside the parking paths is scheduler overhead.

Example of the allocator rule, 42 samples:

    celerrate_semantics::items::collect_defines
      <- ... <- _xzm_free  (libsystem_malloc)

Example of the file input and output rule, showing that the Celerrate
closure is inlined away entirely, which matters for the phase attribution
below:

    rayon::iter::plumbing::bridge_producer_consumer::helper
      <- rayon::iter::map::MapFolder::consume
      <- std::fs::read::inner <- std::sys::fs::unix::File::open_c
      <- open <- __open

**The ambiguity, quantified.** The concern that salsa's generic memo
lookup inlines into its caller and makes the memo-access boundary
unmeasurable does not materialise at the entry point:
`salsa::function::fetch` is present as a real frame in 41.98 % of worker
samples at ten threads and `salsa::function::execute` in 41.95 %, so the
lookup path is not inlined away and the boundary between framework and
query body is a frame boundary the profile can see. What does blur is
narrower and is measured: salsa trait implementations generated inside
Celerrate modules, which carry a Celerrate symbol name while executing
memo machinery. They hold 0.25 % of worker samples at ten threads, 0.33 %
at eight and 0.19 % at one, dominated by a single
`salsa::interned::HashEqLike` implementation for a Celerrate string type.
That share is reported as its own line rather than folded silently into
either side. A residue that no method here can size remains: memo code
fully inlined into a query body with no distinguishing symbol leaves no
trace at all, and the profile cannot bound it.

Samples that reach no attributable frame at all are 2 or 3 per capture at
ten threads, 0 or 1 at eight and 1 to 3 at one thread, that is under
0.03 % everywhere.

### Worker-thread buckets, ten threads

Raw self-sample counts, with each capture's share of that capture's total
worker samples in parentheses.

| Bucket | Run 1 | Run 2 | Run 3 | Median share |
| --- | ---: | ---: | ---: | ---: |
| Parked, no job at all | 7149 (31.93 %) | 6305 (28.05 %) | 5529 (24.73 %) | 28.05 % |
| Productive work | 5239 (23.40 %) | 5527 (24.59 %) | 5357 (23.96 %) | 23.96 % |
| File input and output syscalls | 4275 (19.09 %) | 4355 (19.37 %) | 3971 (17.76 %) | 19.09 % |
| Allocator | 2217 (9.90 %) | 2268 (10.09 %) | 2374 (10.62 %) | 10.09 % |
| Parked at a split | 1497 (6.69 %) | 1843 (8.20 %) | 3122 (13.96 %) | 8.20 % |
| Salsa memo access | 1265 (5.65 %) | 1285 (5.72 %) | 1288 (5.76 %) | 5.72 % |
| Salsa lock contention | 541 (2.42 %) | 675 (3.00 %) | 547 (2.45 %) | 2.45 % |
| Memo wait | 129 (0.58 %) | 126 (0.56 %) | 107 (0.48 %) | 0.56 % |
| Salsa glue in Celerrate symbols | 57 (0.25 %) | 74 (0.33 %) | 53 (0.24 %) | 0.25 % |
| Rayon scheduler | 19 (0.08 %) | 20 (0.09 %) | 9 (0.04 %) | 0.08 % |
| Unattributable | 2 (0.01 %) | 2 (0.01 %) | 3 (0.01 %) | 0.01 % |
| **Total worker samples** | **22390** | **22480** | **22360** | |

### Worker-thread buckets, eight threads (the stagnation point)

| Bucket | Run 1 | Run 2 | Run 3 | Median share |
| --- | ---: | ---: | ---: | ---: |
| Productive work | 5306 (28.13 %) | 5319 (28.65 %) | 5044 (26.47 %) | 28.13 % |
| Parked, no job at all | 4810 (25.50 %) | 4387 (23.63 %) | 5770 (30.28 %) | 25.50 % |
| File input and output syscalls | 3278 (17.38 %) | 2962 (15.95 %) | 3187 (16.72 %) | 16.72 % |
| Allocator | 2221 (11.77 %) | 2496 (13.44 %) | 2052 (10.77 %) | 11.77 % |
| Salsa memo access | 1503 (7.97 %) | 1686 (9.08 %) | 1217 (6.39 %) | 7.97 % |
| Parked at a split | 1342 (7.11 %) | 1220 (6.57 %) | 1449 (7.60 %) | 7.11 % |
| Salsa lock contention | 222 (1.18 %) | 249 (1.34 %) | 183 (0.96 %) | 1.18 % |
| Memo wait | 95 (0.50 %) | 150 (0.81 %) | 66 (0.35 %) | 0.50 % |
| Salsa glue in Celerrate symbols | 63 (0.33 %) | 84 (0.45 %) | 53 (0.28 %) | 0.33 % |
| Rayon scheduler | 23 (0.12 %) | 15 (0.08 %) | 35 (0.18 %) | 0.12 % |
| Unattributable | 1 (0.01 %) | 0 | 0 | 0.00 % |
| **Total worker samples** | **18864** | **18568** | **19056** | |

### Worker-thread buckets, one thread (the serial baseline)

| Bucket | Run 1 | Run 2 | Run 3 | Median share |
| --- | ---: | ---: | ---: | ---: |
| Productive work | 4326 (48.98 %) | 4569 (51.61 %) | 3931 (46.54 %) | 48.98 % |
| Allocator | 1790 (20.26 %) | 1872 (21.15 %) | 1534 (18.16 %) | 20.26 % |
| Salsa memo access | 1307 (14.80 %) | 1338 (15.11 %) | 1231 (14.57 %) | 14.80 % |
| File input and output syscalls | 990 (11.21 %) | 646 (7.30 %) | 1314 (15.56 %) | 11.21 % |
| Parked, no job at all | 397 (4.49 %) | 400 (4.52 %) | 409 (4.84 %) | 4.52 % |
| Salsa glue in Celerrate symbols | 12 (0.14 %) | 19 (0.21 %) | 16 (0.19 %) | 0.19 % |
| Rayon scheduler | 8 (0.09 %) | 8 (0.09 %) | 9 (0.11 %) | 0.09 % |
| Unattributable | 3 (0.03 %) | 1 (0.01 %) | 2 (0.02 %) | 0.02 % |
| Salsa lock contention | 0 | 0 | 0 | 0.00 % |
| Memo wait | 0 | 0 | 0 | 0.00 % |
| **Total worker samples** | **8833** | **8853** | **8446** | |

Salsa lock contention and memo wait are exactly zero in all three
single-thread captures, and parked-at-a-split does not occur either.
That is the expected shape with one worker and no one to contend with,
and it is a check on the classifier: the two buckets that should vanish
without concurrency do vanish, rather than picking up stray samples.

Each capture's counts sum exactly to its total. Medians are taken per
bucket across the three captures, so they do not compose: a median of a
sum is not the sum of the medians, and no total below is built by adding
the medians above.

### The parked total, from per-capture totals

The parked share is computed per capture and only then medianed:

| | Run 1 | Run 2 | Run 3 | Median | Range |
| --- | ---: | ---: | ---: | ---: | ---: |
| Parked, 10 threads | 38.62 % | 36.25 % | 38.69 % | 38.62 % | 36.25 to 38.69 |
| Parked, 8 threads | 32.61 % | 30.20 % | 37.88 % | 32.61 % | 30.20 to 37.88 |
| Parked, 1 thread | 4.49 % | 4.52 % | 4.84 % | 4.52 % | 4.49 to 4.84 |

Between eight and ten threads the medians differ by 6.01 points, but the
two distributions overlap: the eight-thread run 3 (37.88 %) is parked
more than the ten-thread run 2 (36.25 %). At three captures per thread
count, that rise is a direction, not an established quantity, and nothing
below rests on its size. Against one thread the separation is not in
doubt, but whole-run parked share is in any case the wrong instrument for
the fan-out question, for the reason the next section gives.

### Call sites, and what belongs to no call site

The parallel call sites resolve by name. Reading the source establishes
which phase timer wraps each one: `session.rs` line 439 is the file read
and input set phase; `analysis.rs` lines 145 and 152 are both inside the
analysis fan-out timer, the first a prewarm of `item_tree` over every
file and the second the per-file `analyze_one`; `cache/mod.rs` lines 276
and 302 are both inside the persist collect-entries timer.

Median shares of total worker samples:

| Call site | 10 threads | 8 threads | 1 thread | Phase |
| --- | ---: | ---: | ---: | --- |
| `analysis::served_typed_diagnostics` | 21.17 % | 21.38 % | 23.22 % | analysis fan-out |
| `semantics::queries::item_tree` (prewarm) | 18.66 % | 19.42 % | 27.32 % | analysis fan-out |
| `std::fs::read` (closure inlined) | 19.21 % | 16.79 % | 11.24 % | file read and input set |
| `analysis::persistable_diagnostics` | 2.60 % | 2.81 % | 3.41 % | persist collect entries |
| `semantics::queries::member_tree` | 0.00 % | 2.08 % | 3.06 % | persist collect entries |
| `types::records::class_surface_digest` | 0.00 % | 3.60 % | 26.96 % | not mapped by rule |
| Parked, no job at all | 28.05 % | 25.50 % | 4.52 % | **no call site** |
| Parked at a split | 8.20 % | 7.11 % | 0.00 % | **no call site** |

**The share that belongs to no call site, stated plainly.** A worker
parked with no job carries no job frame, and a worker parked at a split
carries no job body either. Together they are 36.25 % to 38.69 % of
worker samples at ten threads, 30.20 % to 37.88 % at eight, and 4.49 % to
4.84 % at one. That share cannot be assigned to any phase by
construction, and no reading below assigns it to one.

The unmapped call-site residue is a separate matter and it is not small
at every thread count: 0.16 % median at ten threads, 3.94 % at eight, and
27.20 % at one (19.35 %, 27.20 % and 30.78 % across the three captures).
It is almost entirely one call site, `class_surface_digest`, a types
query reached through a parallel construct whose enclosing frame is
inlined away, so the rule that names a call site by its outermost
Celerrate frame stops at the query instead of at the phase that asked
for it.

Two independent measurements place that residue in the persist
collect-entries phase and not in the analysis fan-out. First, the
fan-out's own reconciliation at one thread returns 101 % of the phase's
available processor time from the two named fan-out call sites alone,
which leaves no room inside the fan-out for a third call site worth
2.87 core-seconds. Second, persist collect entries costs 4016, 4005 and
4084 milliseconds at one thread, that is about 4.03 core-seconds, against
0.70 core-seconds that the rule does map to persist plus the 2.87 of this
residue, totalling 3.57, which is the right size with the phase's
main-thread serialisation left over. The residue is therefore left
unmapped by rule but is understood, and it does not enter any fan-out
figure below. Its near-disappearance at eight and ten threads is the same
effect the persist row shows: at higher thread counts the persist phase's
workers are mostly parked rather than running these queries.

The `item_tree` call site was checked for whether it is the fan-out's
prewarm or the persist phase's tree pass, since both call the same query:
99.95 % of its samples at ten threads have `salsa::function::execute` on
the stack, meaning a cold miss that is computing, not a memoized fetch
being serialised. It is the prewarm, inside the fan-out.

The persist phase holds only 2.68 % of worker samples at ten threads and
5.18 % at eight, far below the share its wall time would suggest. Its
parallel work is memoized fetches and conversions, so the workers are
largely parked during it, which is part of what the parked buckets
contain.

### Reconciling the fan-out against its wall clock

The reconciliation the parked share cannot give directly: the fan-out's
wall time on this artefact, times the thread count, is the processor time
the phase has available if every worker is busy for its whole duration.
Comparing that with the processor time the fan-out call sites actually
consume gives the phase's own utilisation, with no appeal to the
unattributable parked share.

Both terms carry spread, and both are propagated. The observed side is
the fan-out's measured cost in core-seconds across the three captures;
the available side is the fan-out's measured wall time across the three
control runs, times the thread count. The utilisation range below is the
full cross of the two, that is every observed value against every
available value, which is the honest bound when the two sets cannot be
paired (the profiled runs and the control runs are different runs).

| N | Observed cost (core-seconds) | Available (core-seconds) | Utilisation, median observed over mean available | Full propagated range |
| --- | --- | --- | ---: | ---: |
| 10 | 13.84, 14.08, 14.16 | 16.52, 13.61, 16.48 | 90.6 % | 83.8 % to 104.1 % |
| 8 | 11.53, 11.85, 11.17 | 13.00, 12.72, 12.62 | 90.2 % | 85.9 % to 93.9 % |
| 1 | 5.38, 5.38, 5.61 | 5.29, 5.31, 5.38 | 101.0 % | 100.0 % to 106.0 % |

**The utilisation difference between eight and ten threads does not
survive its own uncertainty.** By this measure the two are 90.6 % and
90.2 %, a difference of 0.4 points, and the propagated ranges overlap
almost completely. An earlier reading of this section claimed the two
ranges did not overlap; that claim was built from a single median fan-out
wall time applied to all three captures, so the only spread it displayed
was the sampling interval's, which is the smaller term by an order of
magnitude. It is withdrawn. What the profiles establish is that the
fan-out is roughly 90 % utilised at both eight and ten threads, and
nothing about how that figure moves between them.

The single-thread row calibrates the method. One worker inside a phase it
occupies alone must be essentially 100 % utilised, and the reconciliation
returns 101.0 % by this measure, with an upper excursion to 106.0 %. The
method therefore reads about 1 % high, with a systematic band of roughly
±6 % coming from window edges, phase boundaries and the interval
estimate. Any utilisation difference smaller than that band is not
measurable by this method, which is a second, independent reason the
0.4-point difference above carries no weight.

### What the fan-out costs, one thread against ten

This is the comparison that accounts for the missing cores, and unlike
the utilisation comparison it does not depend on the fan-out's wall time
at all: it is measured processor time against measured processor time.
Fan-out cost in core-seconds inside the window, per capture:

| Bucket | 1 thread | Median | 10 threads | Median | Growth |
| --- | --- | ---: | --- | ---: | ---: |
| Productive work | 3.674, 3.776, 3.741 | 3.741 | 7.877, 7.953, 8.044 | 7.953 | x2.13 |
| Allocator | 1.074, 1.014, 1.136 | 1.074 | 3.199, 3.093, 3.371 | 3.199 | x2.98 |
| Salsa memo access | 0.620, 0.574, 0.716 | 0.620 | 1.778, 1.787, 1.788 | 1.787 | x2.88 |
| Salsa lock contention | 0, 0, 0 | 0 | 0.834, 1.036, 0.837 | 0.837 | new |
| Salsa glue | 0.013, 0.019, 0.015 | 0.015 | 0.090, 0.115, 0.082 | 0.090 | x6.0 |
| Memo wait | 0, 0, 0 | 0 | 0.063, 0.098, 0.042 | 0.063 | new |
| **Total** | **5.38, 5.38, 5.61** | **5.38** | **13.84, 14.08, 14.16** | **14.08** | **x2.62** |

Every line separates cleanly: no bucket's one-thread range touches its
ten-thread range, and the totals are 5.38 to 5.61 against 13.84 to 14.16.

**The fan-out does 2.62 times more processor work at ten threads than at
one, to analyse the same 6932 files.** That is the missing cores. The
identity closes: ten threads, divided by the 2.62 work expansion, times
the 90.6 % utilisation, gives 3.46 effective cores, against 3.43 measured
directly from this artefact's own wall clocks (5328 milliseconds at one
thread over 1554 at ten) and against the 3.83 section 4 measured on the
stripped artefact. Of the roughly 6.5 cores lost out of ten, work
expansion accounts for a factor of 2.62 and idleness for a factor of
1.10: expansion is by far the larger term, and the eight-versus-ten
comparison alone could never have shown it.

Where the extra processor time goes, as differences of the medians above:
productive work +4.21, allocator +2.13, salsa memo access +1.17, salsa
lock contention +0.84, salsa glue +0.08, memo wait +0.06. Those parts sum
to 8.49 core-seconds. The difference of the median totals is 8.70. The
two do not agree because a median of sums is not a sum of medians; the
0.21 core-second gap is that artefact of composition and not an
unattributed residue. Either figure is a fair statement of the growth,
and they are quoted here together rather than one being presented as the
decomposition of the other.

By share of the expansion, taking the summed parts as the whole:
productive work is 50 %, the allocator 25 %, salsa memo access 14 %,
salsa lock contention 10 %, and the two remaining buckets 2 % together.

**The largest term is the one the profile cannot explain.** Productive
work is the same lexing, parsing, lowering and inference code, over the
same corpus, producing the same result, and it consumes 3.741
core-seconds at one thread and 7.953 at ten. The profile establishes that
this is where the time goes and that it is not lock contention, not memo
waiting, and not the allocator, because those are separate buckets that
grow separately. It does not establish why the same instructions cost
more. Contention for shared cache and memory bandwidth across more active
cores is the standard candidate and is consistent with the allocator, the
other memory-bound bucket, growing even faster (x2.98). But nothing in
this campaign measures the memory system, no hardware counter was read,
and no cause is asserted. That 4.21 core-seconds, roughly half the
expansion and roughly a third of the fan-out's cost at ten threads, is
the residue that resists attribution.

For completeness, the eight-thread column of the same table: total 11.53
core-seconds (range 11.17 to 11.85), productive 6.933, allocator 2.556,
salsa memo access 1.569, salsa lock contention 0.276. Between eight and
ten threads the totals do separate (11.17 to 11.85 against 13.84 to
14.16), so the expansion continues past the stagnation point even though
the utilisation difference does not resolve.

Salsa lock contention is named down to its entry points, and it is
concentrated: across the three ten-thread captures, 1598 of 1763 samples
enter through `salsa::interned::IngredientImpl<C>::intern_id`, with
`salsa::table::Table::fetch_or_push_page` (83),
`salsa::function::sync::ClaimGuard::drop_impl` (39),
`salsa::function::sync::SyncTable::try_claim` (37) and
`salsa::table::Table::record_unfilled_page` (6) making up the rest. Five
distinct entry sites, not two. It is absent entirely at one thread.

One term outside the fan-out is worth recording because section 4 asked
about it. The file input and output bucket, which is the read phase, costs
6.72 core-seconds at ten threads against 4.72 at eight (ranges 6.18 to
6.80 and 4.49 to 4.91, non-overlapping), a 42 % increase in time spent
inside the `open` syscall for the same 6932 files. That is the mechanism
behind section 4's finding that the file read and input set phase runs
slower at ten threads than at four.

### The `stub_*` self-time check

Performed, and answered: no measurable self time, confirming the previous
record.

| Function | 10 threads, self samples | 8 threads, self samples | Presence anywhere in a stack |
| --- | ---: | ---: | ---: |
| `stub_symbol_table` | 0, 0, 0 | 0, 0, 0 | 7 to 15 samples per capture |
| `stub_frontier` | 0, 0, 1 | 0, 1, 0 | 0 to 4 samples per capture |
| `stub_signature_table` | 0, 0, 0 | 0, 0, 0 | 2 to 3 samples per capture |

Against worker totals of 18568 to 22480 samples per capture, the largest
self-time reading is a single sample, under 0.005 %, and the largest
in-stack presence is 15 samples, 0.08 %. The three functions
(`crates/celerrate_semantics/src/index.rs`) are present in the symbol
table of the profiled build, so their absence from the profile is a
measurement and not a symbol-resolution failure; `debug=1` was passed
specifically so that `stub_frontier`, a plain function with one caller,
could not be inlined out of reach.

### Reading: which bucket owns the missing cores

The four buckets the brief names, as shares of total worker samples, all
measurements rather than bounds:

| Bucket | 1 thread | 8 threads | 10 threads |
| --- | ---: | ---: | ---: |
| Allocator | 20.26 % | 11.77 % | 10.09 % |
| Salsa memo access | 14.80 % | 7.97 % | 5.72 % |
| Memo wait | 0.00 % | 0.50 % | 0.56 % |
| Productive work | 48.98 % | 28.13 % | 23.96 % |

Salsa lock contention (0.00 %, 1.18 %, 2.45 %) is reported separately from
memo access rather than folded into it, because the two behave
differently with thread count. File input and output (11.21 %, 16.72 %,
19.09 %) is its own bucket rather than being counted inside productive
work, which matters because it is almost entirely the `open` syscall.
These shares fall with thread count only because the parked buckets rise
to fill the denominator; the processor-time table above is the figure to
read for how the work itself changes.

**The missing cores are work expansion, not idleness, and the largest
single term in the expansion is not attributable from these profiles.**

The accounting, for the analysis fan-out at ten threads against one:

- The phase is about 90 % utilised at ten threads. That costs a factor of
  1.10 out of the ten.
- The phase does 2.62 times more processor work at ten threads than at
  one, for the same corpus and the same result. That costs a factor of
  2.62.
- Together: 10 divided by 2.62, times 0.906, is 3.46 effective cores,
  against 3.43 measured directly from this artefact's wall clocks and
  3.83 from section 4's. Expansion is the dominant term by a wide margin.

Within the expansion, by difference of medians: productive work +4.21
core-seconds (50 % of the summed parts), allocator +2.13 (25 %), salsa
memo access +1.17 (14 %), salsa lock contention +0.84 (10 %), memo wait
and salsa glue +0.14 together (2 %).

Confidence, per claim:

- **High** that the fan-out does about 2.6 times more processor work at
  ten threads than at one, and that every bucket listed grows. The
  one-thread and ten-thread ranges do not touch for any bucket or for the
  total, the comparison uses measured processor time on both sides with
  no wall-clock term, and the single-thread reconciliation returning
  101 % confirms the fan-out call sites capture the whole phase.
- **High** that salsa lock contention grows sharply with thread count,
  that it is absent at one thread, and that it enters overwhelmingly
  through interning. The entry points are named frames.
- **High** that memo wait is not the problem: 0.56 % of worker samples at
  ten threads, 0.50 % at eight, zero at one. The pattern the previous
  effort's diagnosis reported, many workers blocked behind one worker's
  in-progress work, is absent. Those samples are split between
  `persistable_diagnostics` and `served_typed_diagnostics` at ten
  threads, with `class_surface_digest` a third site at eight.
- **Medium** on the fan-out being about 90 % utilised. The figure is
  90.6 % at ten threads and 90.2 % at eight by this measure, but the
  propagated ranges are 83.8 % to 104.1 % and 85.9 % to 93.9 %, and the
  single-thread calibration shows the method carries a systematic band of
  roughly ±6 %. The order of magnitude is established; the second digit
  is not.
- **Not claimed**: any difference in utilisation between eight and ten
  threads, and any figure for the change in whole-run parked share
  between them. Both are smaller than their own uncertainty at three
  captures per thread count.
- **Not established**: why productive work itself costs 2.13 times more
  processor time at ten threads. Cache and memory-bandwidth contention
  across more active cores is the standard candidate and is consistent
  with the allocator, the other memory-bound bucket, growing faster
  still, but no hardware counter was read and no cause is asserted.

**Scope of this reading.** It rests on three thread counts, one, eight
and ten, three captures each, on one corpus, one machine and one commit.
It accounts for the fan-out's effective-core figure through the identity
above. It says nothing measured about four threads, where section 4's
curve still had most of its gain, so the shape of the expansion between
one and eight threads is interpolated by nothing here; the campaign
should not read the 2.62 factor as linear in thread count. The residue
that resists attribution is the 4.21 core-seconds of productive-work
growth, roughly half the expansion and roughly 30 % of the fan-out's
whole cost at ten threads.

For the campaign's decision between local optimization and architectural
rework, the profile now supports a sharper statement than the earlier
draft of this section did. Salsa's interning lock is a real, named, local
target: its cost is unmeasurable at one thread, where a single worker
never contends for it, and rises to about 0.84 core-seconds at ten; it is
0.84 of the 8.49 core-seconds of expansion, so removing it entirely would
move the effective-core figure from about 3.5 to about 3.7. The decision the
campaign is actually facing is what to do about the other 7.6
core-seconds, half of which is the same analysis code getting slower per
unit of work as threads are added. That is not a lock-level question, and
answering it needs an instrument this campaign does not have: hardware
performance counters, or an experiment that varies memory pressure
independently of thread count.

## 6. The mimalloc A/B scaling curve (Task 6)

Date: 2026-08-07
Commit: `18f55ff6da548936fc29365b00f78651faa8bf96` (this branch's HEAD at
measurement time). The source tree is identical to `2621b81` for every
crate this campaign measures: `git diff 2621b81 HEAD --stat` touches only
this measurement document, so the default-allocator binary built here is
the same performance object every earlier section cites.
Corpus measured: the equalized corpus copy at
`target/comparison-corpus-equalized` (the same corpus as sections 1, 3, 4
and 5). Every run below reported 6932 project files.
Machine: otherwise idle for the whole session; both binaries were built
before any timed run, and nothing was built while a run was in progress.

Binaries:

- Default allocator: `target/release/celerrate`, built from this
  branch's HEAD with the workspace's default release profile, unchanged
  (no `--config` override).
- mimalloc probe: built on scratch branch `scratch-mimalloc-probe`,
  local commit `b6e253d894bd4661397d19dcbfd149dabdd82178`. That commit
  adds `mimalloc = "0.1.52"` to `celerrate_cli`'s dependencies (via
  `cargo add mimalloc --package celerrate_cli`) and sets
  `#[global_allocator] static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;`
  in `crates/celerrate_cli/src/main.rs`, with no other source or build
  configuration change, so the two binaries differ only by the
  allocator. The branch is local only: never merged, never pushed, and
  never the base of another branch; its commit hash is recorded here for
  provenance only. `cargo deny` was not run on the scratch branch, since
  the licence pass belongs to a future landing effort, not this probe.

Because the build directory is shared across branches, the probe binary
was copied aside to a scratch path immediately after building it and
before switching back; the working tree then returned to
`docs-cold-run-performance-diagnostic` and rebuilt, restoring
`target/release/celerrate` to the default-allocator artefact before any
run below was measured. (Deviation from the brief's literal path: the
probe binary was copied to a session-scoped scratch directory rather than
`/tmp/celerrate-mimalloc`; this is a path substitution only, made to
follow this session's own working-file convention, and does not change
anything the brief specifies about the binaries, the build, or the
protocol.)

**Verification that mimalloc is actually linked.** The workspace release
profile sets `strip = "symbols"`, so a symbol-table check is not
available on either artefact (the same limitation section 5 records for
naming frames). Two checks were used instead. First, the two binaries
are byte-for-byte distinct after the rebuild step separated them
(different SHA-256 digests; they had been identical immediately after
the probe build, because that build ran while the working tree was still
on the scratch branch and overwrote the same shared `target/release/`
path the default binary also occupies). Second, and decisively: mimalloc
recognizes the `MIMALLOC_SHOW_STATS` environment variable and prints an
allocator statistics block (page, arena and heap counters) on exit.
Running `MIMALLOC_SHOW_STATS=1 <binary> --version` against the probe
binary prints that block; the same invocation against the default binary
prints only the version line, with no statistics block. This is a
runtime behavioural check, not a static one, and it is conclusive: the
statistics block cannot appear unless mimalloc's global allocator is
actually installed and actually serving the process's allocations.

Command: `rm -rf .celerrate` then
`env RAYON_NUM_THREADS=<N> /usr/bin/time -p <binary> check . --verbose 2>&1 >/dev/null`,
three repetitions per binary per thread count, alternating binaries at
each N in the order default, mimalloc, default, mimalloc, default,
mimalloc, so machine drift cannot masquerade as an allocator effect. The
session-open and session-close controls repeat the same command on the
default binary only, without setting `RAYON_NUM_THREADS`. Both from the
corpus directory, binary invoked by absolute path. Eighteen cold curve
runs plus six control runs, twenty-four in total.
Timing mechanism: `/usr/bin/time -p`, the same mechanism sections 3, 4
and 5 used, so wall clocks are directly comparable across all four
sections.

Deviation carried over from sections 4 and 5: the brief's snippets run
from the pinned corpus directory with the relative path
`../../release/celerrate`, which walks only 5922 of the corpus's 6932
files. Every run below instead used the equalized corpus directory and
an absolute binary path, per the correction already recorded in the
Protocol section above.

### Session-open control

Three cold runs, default binary, default thread count:

| Run | Wall clock |
| --- | ---: |
| Run 1 | 5.15 s |
| Run 2 | 4.62 s |
| Run 3 | 4.72 s |
| **Median** | **4.72 s** |

### The curve

Both binaries were re-measured in this session rather than citing
section 4's default-allocator curve directly, per the brief's re-anchor
discipline. The re-measured default curve is close to section 4's: 11.41
s (identical) at N = 1, 5.18 s against 5.23 s at N = 4, 4.56 s against
4.57 s at N = 10, all within ordinary run-to-run variance.

#### N = 1

Wall clock:

| Binary | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Default | 11.27 s | 11.56 s | 11.41 s | 11.41 s |
| mimalloc | 9.89 s | 10.14 s | 9.71 s | 9.89 s |

Analysis fan-out phase (ms):

| Binary | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Default | 5316 | 5376 | 5347 | 5347 |
| mimalloc | 4734 | 4734 | 4709 | 4734 |

#### N = 4

Wall clock:

| Binary | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Default | 5.18 s | 5.05 s | 5.29 s | 5.18 s |
| mimalloc | 4.21 s | 4.40 s | 4.20 s | 4.21 s |

Analysis fan-out phase (ms):

| Binary | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Default | 2015 | 1811 | 2115 | 2015 |
| mimalloc | 1556 | 1682 | 1544 | 1556 |

#### N = 10

Wall clock:

| Binary | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Default | 5.16 s | 4.56 s | 4.53 s | 4.56 s |
| mimalloc | 4.36 s | 3.93 s | 4.09 s | 4.09 s |

Analysis fan-out phase (ms):

| Binary | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Default | 1916 | 1463 | 1438 | 1463 |
| mimalloc | 1421 | 1213 | 1368 | 1368 |

### Session-close control

Three cold runs, default binary, default thread count:

| Run | Wall clock |
| --- | ---: |
| Run 1 | 4.59 s |
| Run 2 | 4.86 s |
| Run 3 | 4.67 s |
| **Median** | **4.67 s** |

Drift from open to close control: absolute difference 0.05 s over the
open median of 4.72 s, about 1.1 %, well inside the ~10 % threshold that
would invalidate the session. The session stayed valid throughout.

### Reading the curve

Wall-clock medians by thread count, both binaries:

| N | Default median | mimalloc median | Difference | mimalloc faster by |
| --- | ---: | ---: | ---: | ---: |
| 1 | 11.41 s | 9.89 s | 1.52 s | 13.3 % |
| 4 | 5.18 s | 4.21 s | 0.97 s | 18.7 % |
| 10 | 4.56 s | 4.09 s | 0.47 s | 10.3 % |

**Level.** At ten threads, mimalloc's median (4.09 s) sits 0.47 s below
default's (4.56 s), about 10.3 %. The brief's prior estimate from the
previous effort was about −0.9 s; this session confirms the direction
(mimalloc is faster) but at roughly half the previously reported
magnitude. The estimate is revised down to −0.47 s, measured in this
session against this session's own re-anchored default curve, not
against section 4's figure directly.

**Slope.** Fan-out speedup from one thread to ten threads (single-thread
median divided by ten-thread median):

- Default: 5347 ms to 1463 ms, a 3.65x speedup.
- mimalloc: 4734 ms to 1368 ms, a 3.46x speedup.

mimalloc's speedup is not materially higher than default's; if anything
it is marginally lower, a difference well inside what three runs per
point can resolve. Per the brief's own decision rule, matching slopes
mean the missing cores are not in the allocator: the contention is not
allocation-bound, and the structural question section 5 raised stands.

Effective cores at ten threads, computed the same way sections 4 and 5
did (single-thread fan-out median divided by ten-thread fan-out median):
default 5347 / 1463, about 3.65 effective cores of 10, close to section
4's cross-session 3.83 (about 4.7 % apart, inside the variance sections 4
and 5 already documented for this same comparison); mimalloc 4734 / 1368,
about 3.46 effective cores of 10 — lower, not higher, than the default
figure, confirming the slope verdict above rather than the level verdict.

**Stated against section 5's allocator share.** Section 5 measured the
allocator bucket growing 2.98x between one and ten threads (1.074 to
3.199 core-seconds), accounting for +2.13 of the 8.49 core-seconds of
work expansion the fan-out undergoes over that range, about 25 % of the
summed parts. If that growth were the dominant driver of the missing
cores, swapping to a faster or less-contended allocator should have
disproportionately helped at ten threads relative to one, raising
mimalloc's fan-out speedup above default's. It did not: mimalloc's
speedup (3.46x) is statistically indistinguishable from default's
(3.65x), and mimalloc's wall-clock benefit is present at every thread
count measured, including N = 1 (13.3 % faster serially, where there is
no cross-thread contention to relieve). That pattern, a roughly constant
percentage benefit independent of thread count, is the signature of a
uniformly cheaper allocator (a lower per-call cost) rather than of
relieved multi-threaded contention. Section 5's 25 % allocator share is a
real cost, and mimalloc recovers part of it, but as a flat per-run
improvement rather than a change in scaling shape: the effective-core
loss between one and ten threads remains, at essentially the same size,
on the faster allocator. **The allocator is not where the missing cores
are hiding.** The productive-work growth (50 % of the expansion, section
5's largest and least-attributed term) and the salsa lock contention
(10 %) that section 5 could not resolve with a global allocator swap
remain the open structural question for the campaign's next task.

Confidence, per claim:

- **High** that mimalloc lowers wall clock at every thread count
  measured, by a consistent 10 % to 19 %, including at N = 1 where there
  is no multi-threaded contention for it to relieve.
- **High** that the fan-out's speedup from one to ten threads does not
  improve under mimalloc (3.46x against default's 3.65x): the level
  effect and the slope effect are cleanly separable in this data, and
  they point in different directions relative to what an
  allocation-contention hypothesis would predict.
- **Medium** on the exact size of the level effect at N = 10 (−0.47 s):
  three runs per point on a machine whose own control drifted 1.1 %
  between session-open and session-close bound this figure loosely, and
  N = 1 and N = 4 show different percentage benefits (13.3 % and 18.7 %)
  that a larger sample would be needed to reconcile with the 10.3 % seen
  at N = 10.
- **Not established**: why mimalloc is faster at all (its own internal
  design was not profiled in this session), only that its benefit does
  not scale with thread count in the way that would indict the allocator
  for the missing cores.


## 7. The corpus parse floor and the ratio ceiling (Task 7)

Date: 2026-08-07
Commit: `aa80613e` (this branch's HEAD at measurement time). The source
tree is identical to `2621b81` for every crate this campaign measures:
`git diff 2621b81 HEAD --stat` touches only this measurement document, so
the binary built here is the same performance object every earlier
section cites.
Corpus measured: the equalized corpus copy at
`target/comparison-corpus-equalized` (the same corpus as sections 1, 3,
4, 5 and 6).
Machine: otherwise idle for the whole session; both binaries were built
before any timed run, and nothing was built while a run was in progress.

This section measures the cost no optimization of Celerrate can remove:
walking the project, reading every file the walk finds, and lexing and
parsing all of them. Everything Celerrate does beyond that (name
resolution, type inference, rule evaluation, rendering, cache writes)
sits on top of this figure. Dividing PHPStan's cold median by it gives a
hard upper bound on the speed ratio this corpus can ever show on this
machine.

### The probe

Built on scratch branch `scratch-parse-floor`, local commit
`28e9c36d20089705f1c5e8f72d94cc8cb07db288`. The branch is local only:
never merged, never pushed, and never the base of another branch; its
commit hash is recorded here for provenance only. `cargo deny` was not
run on the scratch branch, since the licence pass belongs to a future
landing effort, not this probe.

The probe adds `crates/celerrate_cli/src/parse_floor.rs` behind a
`__parse-floor` argument checked before the normal dispatch in `main.rs`.
It reproduces the discovery-and-walk slice of `Session::start` exactly:
it normalizes the root the same way, loads `celerrate.toml` through
`crate::configuration::load`, derives the same configuration model,
calls `celerrate_project::discover` with it, and then calls
`celerrate_vfs::enumerate_php_files(&discovery.walk_roots(),
&discovery.excluded_roots)` with those same arguments. It then reads
every walked file through rayon and calls `celerrate_syntax::parse` on
each, discarding both results. `parse` is fully eager (it lexes, runs the
parser, and builds the green tree before returning), so nothing measured
here is deferred to a later access.

Because the build directory is shared across branches, the probe binary
was copied aside to a session-scoped scratch path immediately after
building it and before switching back; the working tree then returned to
`docs-cold-run-performance-diagnostic` and rebuilt, restoring
`target/release/celerrate` to the ordinary artefact before any run below
was measured.

### The file-count check, and what it corrected

The brief for this task expected the probe to print `floor: files: 6932`
and to treat any other value as proof that the probe walks a different
set from a real `check`. That expectation conflated two different
quantities, and holding the probe to it would have produced a floor that
was wrong in both directions at once.

A real cold `check` on this corpus reports `6932 project files`. That
number is not the walked set: it is the walked set filtered by
`Session::reported_files`, which keeps only the files
`ProjectDiscovery::classify` calls `FileOrigin::Project`. An installed
dependency's files stay in the analyzed set, because their symbols are
what make a `use` statement resolve; what they do not do is report
diagnostics. The walked, read, loaded and parsed set is therefore
strictly larger than the reported set. On this corpus the independent
filesystem counts are exact: 24033 PHP files in total, 17101 of them
under `vendor`, and 6932 outside it, matching the reported count to the
file.

That the whole 24033 is parsed on a real run, and not merely loaded, is
settled by `analysis::analyze`: before the reporting fan-out it builds
`prewarm_tasks` over `inputs.files`, the entire analyzed file set rather
than the reported subset, and demands `item_tree` for every one of them, so
every walked file is parsed on every cold run. The floor must therefore
cover all 24033 files, and it does.

The probe prints both numbers so the equality is proved rather than
assumed, and the cross-check is computed after every timer has stopped so
it cannot enter a measured figure:

    floor: files: 24033
    floor: project files: 6932

The project-classified subset matches the `6932 project files` a real
`check --verbose` reports on the same corpus, exactly. This is the check
the brief intended, in the form that actually tests the claim.

Finding that equality also caught a genuine bug in the probe. The brief
wired it as `parse_floor_probe(std::path::Path::new("."))`, but
`Session::start` never receives a relative root: `check .` resolves its
argument through `absolute_root` first. A bare `"."` normalizes to the
empty path, which is neither a file nor a directory, so the project walk
root was silently dropped and only the vendor autoload roots were walked.
The first probe build reported `files: 12617` with `project files: 0`,
that is, it walked barely half the corpus and none of the project's own
code, while still printing a plausible-looking file count. Had the count
alone been trusted, the floor would have been measured over the wrong
set. The probe now resolves the current directory to an absolute path
before calling `Session::start`'s slice, exactly as the CLI does.

### Raw probe output

Command: `env RAYON_NUM_THREADS=<N> <probe> __parse-floor`, run from the
equalized corpus directory, three repetitions per thread count. The probe
prints its own instrumented millisecond figures; `/usr/bin/time -p` is
not used here, because the interesting quantity is the per-phase split
and not the process wall clock. The probe reads only: no `.celerrate` is
created, read, or removed, so there is no cold-versus-warm cache
distinction to control for. The page cache was warm throughout, which is
the same condition every other section in this document measured under
(a cold run in this campaign clears `.celerrate`, never the page cache).

Ten threads (the machine has 10 logical cores, so this is also the
default):

| Phase | Run 1 | Run 2 | Run 3 | Median | Range |
| --- | ---: | ---: | ---: | ---: | ---: |
| Walk | 372 ms | 362 ms | 364 ms | **364 ms** | 362 to 372 ms |
| Read | 460 ms | 294 ms | 311 ms | **311 ms** | 294 to 460 ms |
| Parse | 579 ms | 549 ms | 673 ms | **579 ms** | 549 to 673 ms |
| Sum of medians | | | | **1254 ms** | |

One thread:

| Phase | Run 1 | Run 2 | Run 3 | Median | Range |
| --- | ---: | ---: | ---: | ---: | ---: |
| Walk | 406 ms | 424 ms | 377 ms | **406 ms** | 377 to 424 ms |
| Read | 440 ms | 450 ms | 407 ms | **440 ms** | 407 to 450 ms |
| Parse | 2464 ms | 2250 ms | 2250 ms | **2250 ms** | 2250 to 2464 ms |
| Sum of medians | | | | **3096 ms** | |

Three things in that pair are worth naming before the floor is computed.

The walk is serial and thread-count-independent: 364 ms at ten threads
against 406 ms at one, a difference within the run-to-run range of
either. `enumerate_php_files` accumulates into a `BTreeSet` on the
calling thread, so no thread count changes it. It is a hard serial
component of every cold run, and section 3's measured walk phase agrees
with it: a median of 402 ms over runs of 402, 396 and 425 ms, against
this section's 364 ms.

The 38 ms difference between those two medians is not explained here.
It is not a difference in what the two timers cover: `Session::start`
(`session.rs:267-274`) starts its clock, calls `enumerate_php_files` with
the walk roots and excluded roots, and records `Phase::Walk` immediately,
stopping before `session.load(&walk)` on the following line. The probe
wraps exactly that same call with the same arguments and nothing else, so
both figures time the same work. Nor is the gap absorbed by within-set
variation: the two ranges (362 to 372 ms here, 396 to 425 ms in
section 3) do not overlap. It is a between-session difference this
campaign does not isolate, and it is load-bearing for nothing, since the
floor below uses this section's own median and not section 3's.

The read barely parallelizes: 311 ms at ten threads against 440 ms at
one, a 1.41x speedup for a tenfold thread increase. It is bound by the
filesystem, not by cores.

The parse scales 3.89x from one thread to ten (2250 ms to 579 ms).
Expressed in core-seconds, it costs 2.250 core-seconds at one thread and
5.790 core-seconds at ten, a growth factor of **2.57x**. Section 5
measured the analysis fan-out's core-seconds growing by 2.62x over the
same thread range. Those two numbers are close enough to matter: the
floor measured at ten threads already carries the same magnitude of
parallel inefficiency the real pipeline suffers, so it is not an
artificially clean bound flattering the comparison. Whatever costs
Celerrate roughly 2.6x in core-seconds when it fans out is present in
this floor too, and reading, lexing and parsing is about as simple as a
parallel workload gets.

### The same-session anchor

Command: `cargo xtask benchmark`, run in this session, machine otherwise
idle.

| Scenario  | Cold median | Mean       | Standard deviation | Range               | Timed runs |
| --------- | ----------- | ---------- | ------------------- | ------------------- | ---------- |
| PHPStan   | 38.361 s    | 37.115 s   | ± 2.955 s            | 33.741 to 39.242 s   | 3          |
| Celerrate | 5.278 s     | 5.268 s    | ± 0.112 s            | 5.106 to 5.405 s     | 5          |

Cold ratio this session: 7.3x. The file-count cross-check matched exactly
(6932 reported, 6932 counted independently).

Both medians sit inside the historical bands section 1 records (PHPStan
31 s to 39 s; Celerrate 4.8 s to 5.5 s), but both sit higher than
section 1's own figures from earlier the same day: PHPStan 38.361 s
against 32.652 s (17.5 % higher) and Celerrate 5.278 s against 4.945 s
(6.7 % higher). PHPStan's own spread this session is wide (± 2.955 s over
three runs, a 5.5 s range), which is most of the gap. The ceiling below
is computed against this session's PHPStan median, per the same-session
discipline the brief requires; the effect of choosing section 1's median
instead is quantified afterwards.

### The corpus floor

**Corpus floor** = walk + read + parse medians at ten threads, plus the
fixed process cost section 2 measured for an empty-project cold `check`
(19.6 ms ± 1.4 ms, which covers startup, embedded-stub loading,
configuration, discovery and teardown, but only as an *empty* project
exercises them; this corpus's own Composer and autoload discovery is
timed nowhere in the sum below, an omission the caveat at the end of this
section states in full):

    364 ms + 311 ms + 579 ms + 19.6 ms = 1273.6 ms

**Corpus floor: 1.274 s** (2026-08-07, probe at scratch commit
`28e9c36d`, working-branch source at `2621b81`). Summing the per-phase
extremes rather than the medians, and carrying section 2's own spread,
puts the run-to-run envelope at 1.223 s to 1.526 s.

The same computation at one thread gives 406 + 440 + 2250 + 19.6 =
3115.6 ms, a **one-thread floor of 3.116 s**.

For scale: the floor is 24.1 % of this session's cold Celerrate median
(1.274 s of 5.278 s). Roughly three quarters of a cold run, 4.00 s, is
spent on work above reading, lexing and parsing.

### The ratio ceiling

**Ratio ceiling** = this session's PHPStan cold median divided by the
corpus floor:

    38.361 s / 1.274 s = 30.1x

**Ratio ceiling: 30.1x.** This is a bound, not a target. It is the ratio
Celerrate would show if it did nothing at all beyond walking the project,
reading every file, lexing and parsing them, and paying its fixed process
cost, while PHPStan continued to do its whole job.

The bound is robust to the spreads in its two inputs. Combining the
observed extremes in the least favourable direction (PHPStan's fastest
run, 33.741 s, over the floor's worst envelope, 1.526 s) still gives
22.1x; the most favourable combination (PHPStan's slowest run, 39.242 s,
over the floor's best envelope, 1.223 s) gives
32.1x. Dividing by section 1's lower, cross-session PHPStan median
(32.652 s) instead of this session's gives 25.6x. Every one of those
values is above 20x.

At one thread the ceiling is 38.361 / 3.116 = **12.3x**, which is below
20x. The 20x ambition is therefore not merely an optimization target but
a claim that depends on multi-core execution: no single-threaded
Celerrate can reach it on this corpus and machine, whatever else is
optimized away, because parsing alone costs more than a twentieth of
PHPStan's run.

### Where roughly 20x sits, and the floor's own caveat

**Roughly 20x sits below the ceiling, with a factor of 1.51x to spare**
(30.1 / 20). Stated as wall clock: a 20x ratio against this session's
PHPStan median means a cold run of 38.361 / 20 = 1.918 s. The floor
consumes 1.274 s of that, leaving **644 ms** for everything Celerrate
does above parsing: name resolution, the symbol index, type inference,
rule evaluation, suggestion enrichment, rendering, and every cache write.
Today that same work takes about 4.00 s. Reaching 20x therefore requires
compressing the above-parse work by roughly **6.2x**, while the cold run
as a whole gets 2.75x faster (5.278 s to 1.918 s).

So the answer the spec asks for is that 20x is not arithmetically
impossible on this corpus and machine, but the margin is much thinner
than the headline factor suggests. The ceiling being 30.1x does not mean
there is 30.1x of room; it means that the 1.274 s of incompressible work
already spends about two thirds of the entire time budget a 20x run
would have.

**The floor's caveat, stated explicitly.** A real analyzer does more than
read, lex and parse. Name resolution, the symbol index, type inference,
rule evaluation, rendering and cache persistence are not optional
extras: they are the product. The true reachable ceiling is therefore
**below** 30.1x, and by an unknown margin this section does not measure:
the bound is what no optimization can beat, not what any optimization can
approach. Any figure derived from this section must be read as an upper
bound on a bound.

**The floor also omits project discovery, and that omission is not
above-parse work but below-walk work.** The probe runs
`configuration::load` and `celerrate_project::discover` before starting
its first timer, so reading `composer.json`, reading
`vendor/composer/installed.json`, parsing both, and deriving this
corpus's autoload mappings, walk roots and PHP version range are timed
nowhere. Section 2's 19.6 ms cannot stand in for them: it was measured on
an empty project whose `composer.json` is `{}`, with no installed
packages to enumerate and no autoload map to build, so it captures
startup and stub loading but essentially none of this corpus's discovery
work. Discovery is as incompressible as the walk, and nothing in this
document isolates its cost. It falls inside section 3's 785 ms
unattributed residue, which explicitly covers everything before the first
instrumented phase begins; but that residue also covers process startup,
teardown and thread-pool scheduling variance, so it bounds discovery
loosely from above rather than measuring it, and no honest point estimate
can be extracted from it without a measurement this section did not make.
The direction is nonetheless unambiguous: including discovery would make
the true floor **higher** than 1.274 s and therefore the true ceiling
**lower** than 30.1x. That moves the bound the same way the caveat above
does, so the conclusion about roughly 20x is unaffected in direction,
only in margin: the room between 20x and the ceiling is narrower than
30.1x states, never wider.

Two further caveats bound the bound's own precision. First, the floor is
measured with a warm page cache; a genuinely cold filesystem would raise
the read median and lower the ceiling. Second, the floor inherits the
parallel inefficiency quantified above: if parse scaled ideally from its
one-thread cost (2250 / 10 = 225 ms instead of the observed 579 ms), the
floor would fall to 920 ms and the ceiling would rise to 41.7x. That
hypothetical is not claimed as achievable; it is recorded because it
shows the ceiling is itself sensitive to the same fan-out inefficiency
sections 4 and 5 investigated, and that improving that inefficiency
raises the bound rather than merely moving Celerrate toward it.

### Confidence

- **High** that the probe walks and parses the same file set a real cold
  `check` does: it calls the same discovery and walk functions with the
  same arguments, and its project-classified subset (6932) matches the
  real run's reported count exactly, while its walk median (364 ms) and
  read median (311 ms) sit alongside section 3's measured walk phase
  (402 ms) and read-and-set-inputs phase (467 ms; that phase times
  `Session::load`, which reads the bytes under rayon as the probe does
  and then, on the calling thread, interns every path in the VFS and
  creates or updates each file's `SourceFile` salsa input
  (`session.rs:437-465`), so its being the larger of the two is
  expected). The walk figures are the two that differ without an
  explanation in the source; the paragraph above says so.
- **High** that the corpus floor exceeds one second on this corpus and
  machine, and therefore that the ratio ceiling is well under 40x.
- **Medium** on the floor's exact value of 1.274 s: three runs per thread
  count, one read median disturbed by a 460 ms outlier against 294 ms and
  311 ms, and a 1.223 s to 1.526 s envelope over the observed extremes.
- **Medium** on the ceiling's exact value of 30.1x, chiefly because
  PHPStan's own cold median moved 17.5 % between section 1's measurement
  and this one on the same day and the same machine. The qualitative
  conclusion survives that movement: every combination of observed
  extremes leaves the ceiling between 22.1x and 32.1x.
- **Not established**: how far below 30.1x the true reachable ceiling
  lies. That depends on the irreducible cost of name resolution,
  inference and rendering, which this section deliberately does not
  measure.

## 8. The shared-nothing process-level bound (Task 8)

**Decision gate.** This section's brief runs conditionally: only if
sections 5 and 6 left the salsa share of the missing cores ambiguous
after a clear owner failed to emerge. That is this campaign's situation.
Section 5's salsa buckets at ten threads sum to 6.28 % of worker time
(5.72 % memo access plus 0.56 % memo wait), under the roughly 15 %
threshold the gate names. Section 6's mimalloc slope verdict was
negative: the fan-out speedup from one to ten threads is 3.65x on the
default allocator against 3.46x on mimalloc, so the allocator moves the
level, not the slope. Section 4's fan-out still stagnates at 3.83
effective cores of ten, and no single bucket in section 5 owns the
shortfall, its largest measured component being productive-work growth
whose cause section 5 states it did not measure. The gate is met, so
this probe was built.

Date: 2026-08-07
Commit: `2621b81` (source tree; the working branch's HEAD at measurement
time is a later docs-only commit, and `git diff 2621b81 HEAD --stat`
touches only this measurement document, so the binary this section runs
is the same performance object every earlier section cites). Binary:
`target/release/celerrate`, not rebuilt.
Corpus measured: four clones of the equalized corpus copy at
`target/comparison-corpus-equalized` (the same corpus as sections 1, 3,
4, 5, 6 and 7), made with `cp -Rc` into `/tmp/celerrate-partitions/corpus-1`
through `corpus-4`. Each clone carries its own `celerrate.toml`
replacing the source corpus's `include = ["."]` with that partition's
own directory and file list (below), which also keeps the four
`.celerrate` cache directories separate.
Machine: otherwise idle for the whole session, except that the four
partition processes are deliberately concurrent with each other, which
is the measurement itself.
Timing mechanism: `/usr/bin/time -p`, the same mechanism sections 3 and
4 use; for the concurrent measurement it wraps the whole four-process
`wait` block, so its `real` line is the wall clock until the last
partition process exits, matching the brief's "last one exits"
definition.

### Partitioning the corpus

The brief's Step 1 asks for a hand-packed, roughly-equal split by
top-level directory. Two corrections established earlier in this
campaign changed what "roughly equal" has to mean here. First, this
corpus's `vendor/` directory alone holds about 17101 of its 24033 walked
files, so a naive one-directory-per-partition split puts most of the
corpus's cost into whichever partition draws `vendor/` and is badly
imbalanced by construction; splitting within `vendor/` at its next level
down (one entry per vendor organization, `vendor/symfony`,
`vendor/rector`, and so on) is necessary to reach balance at all. Second,
the balancing variable is the *walked* file count (24033 across the
corpus), not the *project-reported* count (6932): those are different
sets (`vendor/` is walked and parsed but not project-classified), and
the brief's own file-count expectation was written against the wrong
one before Task 7 corrected it.

The split was computed, not hand-packed: every top-level entry in the
corpus was counted for its own `*.php` file total (each non-vendor
top-level directory, each `vendor/<organization>` directory, and each
standalone `*.php` file sitting directly at the corpus root or directly
under `vendor/`, since `enumerate_php_files` accepts a file as its own
include root and inserts it as-is (`celerrate_vfs/src/walk.rs:64-65`,
confirmed against the test `an_explicit_file_root_is_included_regardless_of_extension`),
so a root-level file like `index.php` or `vendor/autoload.php` can be
named directly in `include` without pulling in a whole directory around
it). That produced 97 items summing to exactly 24033. The 97 items were
then packed into four partitions by longest-processing-time greedy
bin-packing (sort descending by file count, repeatedly add the next item
to whichever partition currently holds the smallest sum): the standard
approximation for balanced multiway partitioning, and a mechanical,
reproducible stand-in for the brief's "greedy bin-packing by hand is
fine." Two zero-file entries (`vendor/bin`, the corpus root's `bin`)
carried no `*.php` files and were left out of every partition's
`include` list, since naming them would add nothing to any walk.

Recursion needs no explicit subdirectory listing: `walk_directory`
descends into every subdirectory of a directory root automatically
(`celerrate_vfs/src/walk.rs:87-133`), so naming `vendor/symfony` in
`include` is sufficient to pull in everything beneath it.

### The partition table

| Partition | Walked files (`*.php` under its `include` list) | Project files reported (`--verbose`) | Largest entries |
| --- | ---: | ---: | --- |
| 1 | 6009 | 5049 | `src` (4965), `vendor/nikic` (250), `vendor/smarty` (220) |
| 2 | 6008 | 60 | `vendor/symfony` (4730), `vendor/greenlion` (368), `vendor/twig` (238) |
| 3 | 6008 | 1474 | `vendor/rector` (3117), `tests` (952), `vendor/friendsofphp` (689) |
| 4 | 6008 | 349 | `vendor/prestashop` (1370), `vendor/doctrine` (1032), `vendor/phpunit` (976) |
| **Sum** | **24033** | **6932** | |

Both sums check out exactly: the four walked counts (6009, 6008, 6008,
6008) sum to 24033, the full walked set Task 7 established for this
corpus, and the four project counts (5049, 60, 1474, 349) sum to 6932,
the equal-file-set invariant every earlier section in this document
carries. The walked balance is close to perfect (6008.25 is the exact
quarter; the actual split is 6009/6008/6008/6008, an imbalance of one
file). The project-count balance is not attempted and is not close:
partition 1 alone holds 5049 of the corpus's 6932 project files, because
`src` is both the corpus's largest single directory and entirely
first-party, and the bin-packer balances on walked count only, exactly
as instructed. Project-file count turns out not to predict wall-clock
cost either, in either direction: partition 1, with 5049 project files,
finishes fastest among the four when run concurrently (below); partition
3, with only 1474, finishes slowest. Neither the walked count this
section balances on, nor the project count it does not, tracks the
quantity that actually matters, which a later subsection of this section
measures directly.

Each partition's full `include` list (23, 22, 25 and 27 entries
respectively) is recorded in this session's scratch directory,
`/tmp/celerrate-partitions/`, not reproduced here in full; the table
above names each partition's largest entries.

### Verification that each partition's configuration works

For each partition: `cd /tmp/celerrate-partitions/corpus-<K> && rm -rf
.celerrate && target/release/celerrate check . --verbose`, absolute
binary path, captured stdout and stderr separately (`--verbose` writes
its summary line to stderr).

| Partition | `--verbose` project-file line | Configuration diagnostics (CEL0043 to CEL0049) |
| --- | --- | --- |
| 1 | `5049 project files reported; verdicts 0 served / 0 discarded / 5049 absent from the cache` | 0 |
| 2 | `60 project files reported; verdicts 0 served / 0 discarded / 60 absent from the cache` | 0 |
| 3 | `1474 project files reported; verdicts 0 served / 0 discarded / 1474 absent from the cache` | 0 |
| 4 | `349 project files reported; verdicts 0 served / 0 discarded / 349 absent from the cache` | 0 |

Every partition reported exactly the project count its `include` list
was built to produce, and none raised a configuration diagnostic. Each
run exited 1, the ordinary exit code for a `check` that finds
diagnostics in the code it analyzed, not a configuration failure.

The walked count has no equivalent `--verbose` line (that channel
reports only the project-classified subset; see `verbose.rs:89-92`), so
it was cross-checked the same way section 1 cross-checks its own project
count: independently, by walking each partition's own `include` entries
with `find` (a directory entry counted via `find <entry> -name '*.php'
-type f | wc -l`, a file entry counted as 1) inside that partition's
corpus copy. That independent count reproduced the partition table's
walked column exactly (6009, 6008, 6008, 6008), which is expected since
the table was built from the same counts, but confirms no entry was
dropped or misspelled between the plan and the four `celerrate.toml`
files actually written to disk.

### Session-open control

Three cold runs, default binary, default thread count, on the
(unpartitioned) equalized corpus. Core-seconds is `user` plus `sys` from
the same `/usr/bin/time -p` invocation as `real`.

| Run | Wall clock | User | Sys | Core-seconds |
| --- | ---: | ---: | ---: | ---: |
| Run 1 | 4.49 s | 17.48 s | 2.90 s | 20.38 |
| Run 2 | 4.62 s | 17.39 s | 4.32 s | 21.71 |
| Run 3 | 4.71 s | 17.46 s | 4.65 s | 22.11 |
| **Median (by wall clock)** | **4.62 s** | | | **21.71** |

The run matching the median wall clock (Run 2) is this section's
single-process reference for every core-seconds comparison below:
**21.71 core-seconds**, on ten physical cores, for the whole corpus.

### The shared-nothing cold run

Three repetitions, default binary, default thread count per process (the
binary auto-detects the machine's ten cores, so each of the four
concurrent processes independently sizes its own rayon pool to ten,
forty worker threads in total across the group). Before each, `.celerrate`
was removed from all four partition copies (cold, outside the timed
region); each repetition then launched all four partitions as concurrent
background processes from their own directories, with the whole group
wrapped in one outer `/usr/bin/time -p`, so its `real`, `user` and `sys`
cover all four processes combined and its `real` is the wall clock until
the last one exits:

| Repetition | Wall clock | User | Sys | Core-seconds | Effective cores (core-seconds / wall clock) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Repetition 1 | 10.41 s | 31.61 s | 9.96 s | 41.57 | 3.99 |
| Repetition 2 | 9.96 s | 30.65 s | 12.45 s | 43.10 | 4.33 |
| Repetition 3 | 10.08 s | 31.01 s | 12.52 s | 43.53 | 4.32 |
| **Median (by wall clock)** | **10.08 s** | | | **43.53** | **4.32** |

Wall clock: 10.08 s against this session's control (4.62 s), a factor of
2.18x, and against section 4's ten-thread cold median from an earlier
session (4.57 s), a factor of 2.21x. This alone already answers the
brief's question in the negative direction: run as specified, the
shared-nothing configuration is slower, not faster.

**The core-seconds column is the more informative one, and an earlier
draft of this section left it uncollected even though the raw
`/usr/bin/time -p` output already contained it.** Across the three
repetitions, the shared-nothing group burns 41.57 to 43.53 core-seconds
against the single process's 21.71, a factor of **1.91x to 2.00x**
(2.00x at the median-wall-clock repetition). The four processes are not
merely slower in wall clock than the single process; they perform
roughly twice its total processor work to analyze the same corpus.

**A supplementary, uncontrolled measurement locates most of that
processor-work excess.** Outside the three counted repetitions, one
further cold run recorded each partition's own `/usr/bin/time -p`
figures while all four ran concurrently (the same launch, with each
process wrapped individually instead of only the outer `wait` block):

| Partition | Wall clock | User | Sys | Core-seconds | Effective cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 4.58 s | 6.91 s | 3.05 s | 9.96 | 2.17 |
| 2 | 3.63 s | 4.04 s | 3.19 s | 7.23 | 1.99 |
| 3 | 9.79 s | 16.59 s | 2.33 s | 18.92 | 1.93 |
| 4 | 2.96 s | 3.13 s | 2.58 s | 5.71 | 1.93 |
| **Sum** | | | | **41.82** | |

The per-partition sum (41.82 core-seconds) reproduces the group figure
from the same run (41.57 to 43.53 across the three counted repetitions)
independently, confirming the processor-work excess is not an artefact
of how the outer `time` wrapper accounts for its child processes: it is
the sum of what each partition process actually did.

Partition 3 alone accounts for essentially the whole shared-nothing wall
clock (9.79 s of the 10.08 s median) despite a walked file count
(6008) indistinguishable from the other three. But its *effective core
count* while doing so is only **1.93**, the lowest of the four, not the
highest: it was not competing harder for cores than the others, it had
less parallel work available to fill them with. Every partition in this
supplementary run sits between 1.93 and 2.17 effective cores out of ten
available. At the group level, across the three counted repetitions (the
table above), effective cores are 3.99, 4.33 and 4.32, a mean of **4.21**
(the group's median-wall-clock repetition happens to read 4.32 as well,
which is a coincidence of these particular three numbers, not the same
statistic as the mean; both are reported here so neither is mistaken for
the other). **Cores were never the contended resource here.** A
process given the whole ten-core machine to itself and running at fewer
than two effective cores is parallelism-starved, not core-starved;
oversubscription (forty rayon worker threads across four processes
contending for ten physical cores) is not what produced this result,
because the processes never collectively demanded anywhere near ten
cores' worth of concurrent work in the first place. What earlier
inflated 40 worker threads into a story about contention was a category
error: worker thread count is not worker demand.

That leaves the imbalance and the processor-work excess to explain on
their own terms, which a further diagnostic investigation, below, does
directly rather than by elimination.

### What actually drives the cost

Equal walked-file-count balance does not produce equal wall-clock
balance, and the processor-work total is nearly double the single
process's. Both point at the same underlying fact: cost is concentrated
in specific directories, and file count is a poor proxy for it. A series
of solo, single-repetition, cold diagnostic runs (one item's `include`
list at a time, on an otherwise-idle machine, each item's own corpus
copy reused for the next probe) isolated where:

| Item | Files | Wall clock | User | Sys | Core-seconds |
| --- | ---: | ---: | ---: | ---: | ---: |
| `src` | 4965 | 2.11 s | 6.08 s | 3.10 s | 9.18 |
| `vendor/symfony` | 4730 | 0.98 s | 2.67 s | 2.31 s | 4.98 |
| `vendor/rector` | 3117 | 1.25 s | 3.18 s | 2.92 s | 6.10 |
| `admin-dev` | 181 | 0.99 s | 2.72 s | 2.12 s | 4.84 |
| `classes` | 326 | 2.55 s | 5.14 s | 2.52 s | 7.66 |
| `tests` (whole) | 952 | 4.73 s | 12.44 s | 2.58 s | 15.02 |
| `tests/Unit` | 480 | 3.41 s | 9.38 s | 1.98 s | 11.36 |
| `tests/Integration` | 325 | 2.03 s | 5.31 s | 2.32 s | 7.63 |
| Partition 3's small vendor remainder (`vendor/friendsofphp` plus 18 smaller organizations, cache, translations and four root files) | 1191 | 0.99 s | 2.68 s | 2.22 s | 4.90 |
| `tests` + `classes` + `admin-dev` together | 1459 | 6.02 s | 14.91 s | 2.86 s | 17.77 |

This corrects a claim in an earlier draft of this section, which
attributed partition 3's dominance to `vendor/rector` on the strength of
its file count (3117, the largest single item in partition 3) without
measuring it. `vendor/rector` alone costs 1.25 s, barely above the
roughly 0.95 to 1.0 s floor every one of these solo runs shares
regardless of content (that floor is this corpus's own process startup,
stub loading and Composer or autoload discovery, the same fixed cost
sections 2 and 7 describe, just not separately isolated here). So does
`vendor/symfony` at 4730 files. Vendor content, however large, walks and
parses cheaply and does not drive the wall clock. What does: `tests`
(4.73 s alone, more than the entire single-process control's 4.62 s
median), and within it specifically `tests/Unit` (3.41 s of that 4.73 s).
`classes` (326 files, 2.55 s) and, more modestly, `src` itself (4965
files, only 2.11 s, so its cost is not simply "large first-party
directory" either) also carry more marginal cost than their vendor
counterparts at similar or larger file counts. `admin-dev`, despite
being project-classified like `tests` and `classes`, is cheap (0.99 s):
being a project file is not sufficient by itself, since it does not
predict cost either (`src`'s 5049 project files cost little per file;
`tests`'s 952 cost a great deal). This document does not determine why
`tests` and, to a lesser extent, `classes` are disproportionately
expensive to walk, read, parse and analyze; only that they are, and by
how much.

### Rebalancing by measured cost

The partitions were rebuilt using the weights above instead of file
count. `tests` was split at its own next directory level (`tests/Unit`,
`tests/Integration`, `tests/Resources`, `tests/bin`, `tests/TestCase`,
plus the standalone file `tests/index.php`; `tests/UI` held no `*.php`
files and was dropped, the same treatment zero-file entries received in
the original split), the same technique the original split used on
`vendor/`. The four heaviest items, one per partition, seeded a fresh
longest-processing-time packing: partition 1 took `tests/Unit` (and,
since they cost near nothing on their own, `tests/Resources`, `tests/bin`,
`tests/TestCase` and `tests/index.php` alongside it); partition 2 took
`classes` and `admin-dev`; partition 3 took `src`; partition 4 took
`tests/Integration` and `vendor/rector`. Every other item (all of
`vendor/`'s remaining organizations and every other small project
directory, all measured or reasonably inferred from the table above to
carry near-zero marginal cost) was then packed by walked file count
across the four partitions' remaining budget, exactly as the original
split packed everything, since balancing cost among items already known
to cost almost nothing has no effect worth computing precisely.

| Partition | Walked files | Project files reported | Rebalanced around |
| --- | ---: | ---: | --- |
| 1 | 6009 | 735 | `tests/Unit` and the rest of `tests` |
| 2 | 6008 | 533 | `classes`, `admin-dev` |
| 3 | 6008 | 5312 | `src` |
| 4 | 6008 | 352 | `tests/Integration`, `vendor/rector` |
| **Sum** | **24033** | **6932** | |

Both sums check out again: walked 6009 + 6008 + 6008 + 6008 = 24033;
project 735 + 533 + 5312 + 352 = 6932. Each partition's configuration
was verified the same way as the original split (`rm -rf .celerrate`,
`check . --verbose`, absolute binary path):

| Partition | `--verbose` project-file line | Configuration diagnostics |
| --- | --- | ---: |
| 1 | `735 project files reported; verdicts 0 served / 0 discarded / 735 absent from the cache` | 0 |
| 2 | `533 project files reported; verdicts 0 served / 0 discarded / 533 absent from the cache` | 0 |
| 3 | `5312 project files reported; verdicts 0 served / 0 discarded / 5312 absent from the cache` | 0 |
| 4 | `352 project files reported; verdicts 0 served / 0 discarded / 352 absent from the cache` | 0 |

The walked counts were independently cross-checked the same way as the
original split (`find` over each partition's own `include` entries
inside its own corpus copy) and reproduced the table exactly (6009,
6008, 6008, 6008).

This rebalance is by measured cost at unrestricted thread counts; it is
not, and does not claim to be, balanced at every thread budget. The
sweep below shows it is not: `src` (partition 3) parallelizes well and
finishes quickly regardless of its thread budget, but partition 1's
`tests/Unit`-plus-vendor mixture includes several large, parallel-
friendly vendor directories (`vendor/prestashop`, `vendor/doctrine`,
`vendor/phpunit`, `vendor/friendsofphp`, `vendor/phpoffice`) that were
packed onto it as near-zero-marginal-cost filler at ten threads; at a
restricted thread budget they are no longer free, and partition 1
becomes the new straggler. This is recorded, not hidden: a rebalance
computed at one thread count does not transfer cleanly to another, and
this document does not attempt a second rebalance to chase it.

### The thread-budget sweep

The oversubscription hypothesis is refuted above by the effective-core
data, but the reason to expect an architectural gain in the first place
survives it: section 5 measured the analysis fan-out's own core-seconds
growing 2.62x from one thread to ten inside a single process. A
PHPStan-style architecture of isolated workers is supposed to avoid
exactly that growth, by never letting any one worker claim the whole
machine. Running four processes at the default (auto-detected ten)
thread count each reproduces that intra-process growth four times over
instead of avoiding it, which is the more likely source of the 2.00x
processor-work figure above than anything about partition balance. This
was tested directly: the rebalanced partitions were run concurrently,
three cold repetitions each, at `RAYON_NUM_THREADS=2` (eight worker
threads across the group, fewer than the single process's own default of
ten) and at `RAYON_NUM_THREADS=3` (twelve across the group). Group
figures wrap the whole four-process launch in one outer `/usr/bin/time
-p`; per-partition figures wrap each process individually, in the same
run.

#### `RAYON_NUM_THREADS=2` (eight threads total)

| Repetition | Wall clock | User | Sys | Core-seconds |
| --- | ---: | ---: | ---: | ---: |
| Repetition 1 | 7.13 s | 24.83 s | 7.46 s | 32.29 |
| Repetition 2 | 6.87 s | 24.58 s | 6.43 s | 31.01 |
| Repetition 3 | 6.93 s | 24.79 s | 6.98 s | 31.77 |
| **Median (by wall clock)** | **6.93 s** | | | **31.77** |

Per-partition breakdown, the median-wall-clock repetition (Repetition 3):

| Partition | Wall clock | User | Sys | Core-seconds |
| --- | ---: | ---: | ---: | ---: |
| 1 | 6.92 s | 8.24 s | 1.77 s | 10.01 |
| 2 | 5.10 s | 5.82 s | 1.52 s | 7.34 |
| 3 | 4.40 s | 5.03 s | 1.81 s | 6.84 |
| 4 | 5.25 s | 5.68 s | 1.86 s | 7.54 |

#### `RAYON_NUM_THREADS=3` (twelve threads total)

| Repetition | Wall clock | User | Sys | Core-seconds |
| --- | ---: | ---: | ---: | ---: |
| Repetition 1 | 6.80 s | 26.68 s | 10.15 s | 36.83 |
| Repetition 2 | 6.94 s | 26.93 s | 11.33 s | 38.26 |
| Repetition 3 | 7.04 s | 27.34 s | 10.19 s | 37.53 |
| **Median (by wall clock)** | **6.94 s** | | | **38.26** |

Per-partition breakdown, the median-wall-clock repetition (Repetition 2):

| Partition | Wall clock | User | Sys | Core-seconds |
| --- | ---: | ---: | ---: | ---: |
| 1 | 6.93 s | 8.76 s | 2.93 s | 11.69 |
| 2 | 5.36 s | 6.37 s | 2.40 s | 8.77 |
| 3 | 4.60 s | 5.62 s | 2.93 s | 8.55 |
| 4 | 5.24 s | 6.16 s | 3.05 s | 9.21 |

In both sweeps, partition 1 (`tests/Unit` plus the large, parallel-
friendly vendor directories described above) is the straggler and sets
the group's wall clock almost by itself, exactly as the rebalancing
section anticipated: giving each process fewer threads makes
partition 1's own mixed content take longer, not shorter, because the
vendor content it carries needed those threads to stay cheap.

### The core-seconds comparison, and the bound

The first three data rows below share one partitioning (the rebalanced
split) and vary only the thread budget, so the thread budget is the
sole variable across them. The fourth row is the original, file-count-
balanced split at the same unrestricted thread budget as the third; it
is not a fourth point on the same sweep, because it changes which files
sit in which process as well as reporting on the same thread budget, and
is kept separate for that reason. Three further cold repetitions of the
unrestricted default configuration on the rebalanced partitions were run
specifically to give the sweep a same-partitioning fourth point:

```
rep1: real 6.66  user 30.30  sys 17.09  (47.39 core-s)
rep2: real 6.27  user 29.73  sys 13.83  (43.56 core-s)
rep3: real 6.43  user 30.16  sys 14.50  (44.66 core-s)   <- median wall clock
```

| Configuration | Partitioning | Threads (total) | Wall clock (median) | Core-seconds (median-wall-clock repetition) | Core-seconds range | Ratio to the single-process control |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Single process (control) | n/a | 10 | 4.62 s | 21.71 | 20.38 to 22.11 | 1.00x |
| Shared-nothing, `RAYON_NUM_THREADS=2` | rebalanced | 8 | 6.93 s | 31.77 | 31.01 to 32.29 | 1.46x |
| Shared-nothing, `RAYON_NUM_THREADS=3` | rebalanced | 12 | 6.94 s | 38.26 | 36.83 to 38.26 | 1.76x |
| Shared-nothing, default | rebalanced | ~40 | 6.43 s | 44.66 | 43.56 to 47.39 | 2.06x |
| Shared-nothing, default | original (file-count) split | ~40 | 10.08 s | 43.53 | 41.57 to 43.53 | 2.00x |

Across the three same-partitioning rows, the excess grows monotonically
with the thread budget given to each worker: 1.46x at two threads per
process (fewer total threads across the whole group than the single
process uses by itself), 1.76x at three, 2.06x at the unrestricted
default, with the thread budget as the only variable that changed
between them. That progression is consistent with the mechanism named
above: each process pays a share of section 5's intra-process fan-out
growth scaled by its own thread count, and no thread budget tested
removes it, only shrinks it.

The fourth row shows something the three-row sweep alone could not: the
original split's default-configuration core-seconds (43.53) and the
rebalanced split's (44.66) are close enough to overlap at their nearer
edges (43.53 against 43.56), despite the two partitionings sending
different files to different processes. **Total processor-seconds is
largely independent of how the corpus is divided into four processes;
it tracks the thread budget each process is given, not which files it
was given.** Wall clock is the opposite: at the same unrestricted
thread budget, the original split's median wall clock (10.08 s) is 57 %
higher than the rebalanced split's (6.43 s), because wall clock is set
by whichever single partition finishes last, which is exactly the
quantity the rebalance targeted and the file-count split did not. The
core-seconds leg of this section's conclusion is therefore largely
independent of how well these four partitions happen to be balanced;
the wall-clock leg is not, and is the weaker of the two for that reason.

Every configuration is also slower in wall clock than the control: 6.43
to 6.94 s at the rebalanced partitions across all three thread budgets,
10.08 s at the original split's default budget, against the control's
4.62 s (this session's open control) to 4.79 s (this session's close
control, below) and section 4's 4.57 s cross-session anchor. On the
rebalanced partitions specifically, restricting the thread budget shrinks
the processor-work excess, as intended, but actually makes the wall
clock slightly worse, not merely fails to improve it: 6.93 to 6.94 s at
eight and twelve threads against 6.43 s at the unrestricted default,
because it does not fix the partition imbalance the rebalancing section
already flagged as unresolved at low thread counts, and giving each
process fewer threads makes that dominating partition's own content take
longer to finish: partition 1 alone takes 6.92 to 6.93 s at both
restricted budgets, essentially the whole group's wall clock, against
6.42 s for the same partition at the unrestricted default.

**An idealized wall-clock floor, computed the same way section 7 derives
a floor from measured phase costs**, divides the rebalanced default
configuration's median core-seconds (44.66) by the machine's ten
physical cores: 44.66 / 10 = **4.47 s**. Against the control's 4.62 to
4.79 s range, that is an upper bound on the achievable gain of only a
few percent, not the multiple the raw wall-clock comparison for the
original split alone would suggest. But this floor is idealized in a way
no configuration actually measured here achieves: it assumes the default
configuration's total processor work could be redistributed perfectly
across all ten cores with no partition boundary and no scheduling
overhead, which is not what a fixed-per-process, shared-nothing
architecture does. The `RAYON_NUM_THREADS=2` configuration, built
specifically to test whether a lower thread budget could approach or
beat this floor, produced a *lower* idealized floor (31.77 / 10 = 3.18 s)
but a *higher* actual wall clock (6.93 s), because restricting threads
did not fix which partition dominates.

**The two caveats the brief requires, corrected for direction.** An
earlier draft of this section stated both caveats below inflate the
shared-nothing figures and therefore make the "no gain" verdict
optimistic. That direction is backwards for the second caveat, and this
draft corrects it. First, this measurement excludes the merge and
determinism costs a real shared-nothing implementation would have to
pay: reconciling four independently produced diagnostic sets into one
coherent report, and guaranteeing the merged result is deterministic and
independent of which partition finishes first, are real engineering
costs this measurement does not include. That omission understates what
a real implementation would cost, which is conservative in the
direction of "no gain": if anything, a real implementation would show a
larger deficit than measured here, not a smaller one. Second, each of
the four processes redundantly pays its own stub loading and process
startup: the fixed process cost section 2 measured (19.6 ms on an empty
project) is far smaller than what this corpus's own Composer and
autoload discovery costs on every partition; the diagnostic table above
puts the solo floor for cheap content at roughly 4.8 to 5.0
core-seconds per process (`vendor/symfony` 4.98, `admin-dev` 4.84,
partition 3's small-vendor remainder 4.90). A single process pays that
floor once; four shared-nothing processes pay it four times, three of
those payments being pure redundancy rather than architecture-intrinsic
work. Three redundant payments at roughly 4.9 core-seconds each are
about **14.7 core-seconds**, against the original split's total
processor-work excess of 43.53 − 21.71 = **21.82 core-seconds**: on this
estimate, **roughly two thirds of the measured excess is redundant
startup paid four times over, not work genuinely expanded by running
under isolated workers.** The solo-probe floor this estimate is built
from is the total processor cost of a solo run against real content, not
a startup-only measurement, so it is an upper bound on per-process
startup rather than a value isolated to it; the 14.7 core-seconds is
therefore an upper bound on redundant startup, and every ratio derived
from it below is a lower bound on the intrinsic ratio, not a point
estimate. The residual, after subtracting that estimate, is about
21.82 − 14.7 ≈ 7.1 core-seconds, or a ratio of at least about **1.33x**
rather than 2.00x. Applying the same method to the rebalanced split's
excess (44.66 − 21.71 = 22.95 core-seconds, minus the same 14.7
core-seconds) gives a residual ratio of about **1.38x**, so the two
partitionings together put the lower bound on the intrinsic ratio
somewhere in the 1.33x-to-1.38x range rather than at a single figure.
This second caveat therefore makes the measured result *pessimistic*
about the architecture's intrinsic prospects, the opposite of the first
caveat and the opposite of what the earlier draft claimed: a real
implementation that amortizes startup across many runs (a persistent
worker pool rather than a fresh process per run, for instance) would
likely show a substantially smaller processor-work deficit than the
roughly 2x this section's raw numbers report, though this section did
not measure such a configuration, and the 1.33x-to-1.38x residual range
is itself an estimate built from solo-probe floors, not a fourth
measured data point in the sweep above, and remains a lower bound on the
intrinsic ratio rather than the ratio itself.

**A third caveat this measurement warrants.** Partitioning breaks
cross-partition name resolution: each partition's process only ever
sees its own slice of the corpus, so a class in one partition that
extends a class physically walked into another is, from the first
process, an unresolved symbol exactly as if that class did not exist in
the project at all. The four processes are therefore not doing the same
analysis the single process does; they are doing four smaller, mutually
blind analyses whose diagnostics this section never attempted to
reconcile or compare against the single-process run's output. Unlike the
second caveat above, this one pulls in the same direction as the first:
part of what makes the single-process run slower and costlier in
processor-seconds is work (cross-partition resolution, and everything
downstream of it in type inference and rule evaluation) that the
four-process run simply never does, so the comparison in this section
understates how much more the four-process architecture would cost if it
did the same job the single process does.

**The bound, stated once, plainly, and scoped to what was actually
measured: on this corpus, this machine, these four partitions (both the
original file-count split and the cost-rebalanced split), and every
thread budget from two per worker up to the unrestricted default, the
shared-nothing configurations measured here show no net gain over the
single ten-thread process, in either wall clock or raw processor-
seconds.** That scope matters in two different ways for the two legs of
the claim. The core-seconds leg is the sturdier one: it is largely
independent of how these four partitions happen to be balanced (the
rebalanced and original splits produced near-identical default-
configuration core-seconds, 44.66 against 43.53), so a different,
better-balanced or differently-sized partitioning of this same corpus
would likely show a similar processor-work ratio at a given thread
budget. The wall-clock leg is not sturdy in the same way: it is highly
sensitive to partition balance (10.08 s against 6.43 s for the same
thread budget, depending only on which split was used), so a wall-clock
verdict at a partitioning this section did not try remains genuinely
open, in a way the core-seconds verdict does not. And per the caveat
above, roughly two thirds of the measured core-seconds excess at the
unrestricted default is estimated to be redundant per-process startup
rather than intrinsic work expansion, which is not architecture-
intrinsic in the way section 5's fan-out growth is; a shared-nothing
design that amortizes startup would likely land closer to the estimated
1.33x-to-1.38x lower bound than the raw 2.00x to 2.06x this section
measured. The
direction of the verdict, no gain at any thread budget or partitioning
tested, is high confidence. The magnitude is not: it depends on how much
of the measured excess is avoidable startup overhead, which this section
estimates but does not measure directly.

### Session-close control

Three cold runs, default binary, default thread count, on the
(unpartitioned) equalized corpus:

| Run | Wall clock | User | Sys | Core-seconds |
| --- | ---: | ---: | ---: | ---: |
| Run 1 | 4.39 s | 17.54 s | 2.25 s | 19.79 |
| Run 2 | 4.79 s | 17.66 s | 4.60 s | 22.26 |
| Run 3 | 4.84 s | 17.73 s | 5.17 s | 22.90 |
| **Median (by wall clock)** | **4.79 s** | | | **22.26** |

Drift from open to close control: absolute difference 0.17 s over the
open median of 4.62 s, about 3.7 %, well inside the ~10 % threshold the
Protocol section sets. The session's comparisons stand.

A second control, bracketing only the rebalancing and thread-budget
work above (the diagnostic solo probes and the two `RAYON_NUM_THREADS`
sweeps), was run afterward on the same otherwise-idle machine: three
further cold runs gave 4.71 s, 4.49 s and 4.78 s, median 4.71 s. Against
the session-close control immediately preceding it (4.79 s, the last
formal control recorded before this further work began), the drift is
0.08 s, about 1.7 %, again well inside the ~10 % threshold.

A third control bracketed the rebalanced partitions' unrestricted
default-configuration repetitions (the fourth row of the core-seconds
table above), the last measurement added to this section: three cold
runs opened it, 4.63 s, 4.66 s and 4.40 s, median 4.63 s; three more
closed it, 4.13 s, 4.86 s and 4.88 s, median 4.86 s. Drift: 0.23 s over
the 4.63 s open median, about 5.0 %, inside the ~10 % threshold. Every
comparison in this section, across its full measurement history, rests
on control medians between 4.62 s and 4.86 s, a 0.24 s band.

The two caveats the brief requires, and the third this measurement
warrants, are stated together with the core-seconds comparison above,
next to the numbers that make their direction and size checkable, rather
than repeated here.

### Confidence

- **High** that the four original partitions' walked file counts sum to
  24033 and project-reported counts sum to 6932, and that the rebalanced
  partitions do too, both independently verified against the counts
  each `celerrate.toml` was built from.
- **High** that every shared-nothing configuration measured in this
  section, at every thread budget from two per process to the
  unrestricted default, and under both partitionings tried, is both
  slower in wall clock and costlier in total processor-seconds than the
  single ten-thread process on this corpus and machine: the pattern
  holds across twelve repetitions (three each at the original split's
  default configuration, the rebalanced split's `RAYON_NUM_THREADS=2`,
  its `RAYON_NUM_THREADS=3`, and its own default configuration) and
  three independently bracketed controls whose medians span 4.62 s to
  4.86 s.
- **High** that the direction of the bound (no net gain) holds; **medium**
  on its magnitude. The raw processor-work ratio at the unrestricted
  default is 2.00x to 2.06x depending on partitioning, but roughly two
  thirds of that excess is estimated, from the solo diagnostic floors
  above, to be redundant per-process startup rather than work
  genuinely expanded by isolated execution; because the solo-probe floor
  behind that estimate is the total cost of a solo run rather than a
  startup-only measurement, it is an upper bound on startup, and the
  residual, intrinsic ratio it implies is itself a lower bound, estimated
  at about 1.33x for the original split and about 1.38x for the
  rebalanced split. That estimate was not itself measured as a fourth
  sweep point (no configuration here amortizes startup across runs), so
  it is a reasoned bound on the bound, not a measured one.
- **High** that thread oversubscription does not explain the original
  default-configuration result: the group averaged (mean) 4.21 effective
  cores of ten across the three counted repetitions, and the single
  dominating partition ran at 1.93 effective cores over its own 9.79 s,
  both far below contention.
- **High** that the processor-work excess is concentrated in a small,
  specific set of directories (`tests`, `classes`, and to a lesser
  degree `src`) rather than spread evenly or tied to project-file status
  in general: nine solo diagnostic measurements, not merely partition-
  level aggregates, isolate `tests/Unit` in particular as the single
  largest contributor found.
- **High** that the core-seconds leg of the bound is largely independent
  of these four partitions' balance, and the wall-clock leg is not: the
  same unrestricted thread budget produced near-identical core-seconds
  under the two different partitionings tried (43.53 against 44.66,
  a 2.6 % difference) but wall clocks 57 % apart (10.08 s against
  6.43 s). A partitioning this section did not try could plausibly
  change the wall-clock verdict; it is far less likely to change the
  core-seconds verdict.
- **Medium** on whether a differently rebalanced or further-subdivided
  partitioning (splitting `tests/Unit` itself, for instance, or
  thread-limiting each worker to a value chosen per its own measured
  content rather than uniformly across the group) could close the
  remaining gap between the idealized 4.47 s floor and the 6.43 to
  10.08 s wall clocks this section actually measured. This section's
  own rebalance already shows a cost-balanced split at one thread
  budget does not stay balanced at another, which is a specific,
  measured obstacle to closing that gap, not merely an unexplored
  possibility.
- **Not established**: why `tests` and `classes` specifically cost so
  much more per file to walk, read, parse and analyze than `src` or any
  measured vendor directory. This section identifies the concentration
  and its magnitude; it does not open the files to find the cause.
  Also not established: the actual processor-work ratio a
  startup-amortized shared-nothing implementation would show; 1.33x is
  this section's estimate from solo-probe floors, not a measurement of
  such an implementation.

## 9. The core-second map (Task 9)

Date: 2026-08-07
Commit: `2621b81` (the source tree every section above measures)
Corpus: the equalized corpus copy at `target/comparison-corpus-equalized`

This section attributes the cold run's wall clock and its idle cores. It
introduces no new measurement: every figure below is carried from a
numbered section above and carries that section as its evidence pointer.

**The two file counts, stated once more so no arithmetic below is read
against the wrong one.** A cold run walks, reads and parses 24033 PHP
files; 6932 of them are project-classified and are the count
`check --verbose` reports (section 7). Every per-file figure in this
section divides by 24033. Section 5 was written before that correction
landed and describes the fan-out as analysing "the same 6932 files"; that
phrase is restated as 24033 wherever this section quotes it, because the
fan-out's prewarm demands `item_tree` for every walked file, not only the
project-classified subset (section 7 settles this from `analysis::analyze`).

### The map

Median cost is section 3's reference cold run (`check . --verbose`,
three repetitions, `/usr/bin/time -p`). Measured efficiency is section
4's single-thread median divided by its ten-thread median for the same
phase, expressed as effective cores out of the ten the machine has; it
comes from a different session than the median-cost column, and is
reported as a ratio for that reason rather than as an absolute time.
Section 10 does not mix the two: it builds entirely on section 4.

| Phase | Median cost | Parallel today | Measured efficiency (effective cores of 10) | Loss owner | Evidence |
| --- | ---: | --- | ---: | --- | --- |
| Filesystem walk | 402 ms | No | 0.99 (372 / 374) | Serial by construction: `enumerate_php_files` accumulates into a `BTreeSet` on the calling thread, so no thread count changes it | sections 3, 4, 7 |
| File read and input set | 467 ms | Yes | 0.73 (394 / 540), negative | Measured: the file input and output bucket grows from 4.72 core-seconds at eight threads to 6.72 at ten, a 42 % increase inside the `open` syscall for the same files. The mechanism behind that growth (filesystem or page-cache contention is the stated candidate) is **not measured** | sections 3, 4, 5 |
| Analysis fan-out | 1634 ms | Yes | 3.83 (5355 / 1399) | Work expansion 2.62x, plus about 10 % idleness. The expansion decomposes as productive work +4.21 core-seconds (50 % of the summed parts), allocator +2.13 (25 %), salsa memo access +1.17 (14 %), salsa interning lock +0.84 (10 %), memo wait and salsa glue +0.14 together (2 %). The largest term, productive-work growth, is **unattributed as to cause**: no hardware counter was read | sections 3, 4, 5, 6 |
| Suggest enrich | 234 ms | No | 0.99 (234 / 236) | Serial. No mechanism measured: **unattributed** | sections 3, 4 |
| Render report | 178 ms | No | 1.00 (187 / 187) | Serial. No mechanism measured: **unattributed** | sections 3, 4 |
| Persist: collect entries | 1041 ms | Yes | 4.63 (4053 / 875) | The best-scaling phase measured, and it stagnates at the same N = 8 the fan-out does (43.1 %, 45.2 %, 25.2 %, 7.5 % across the four steps). The 5.37 cores it does not claim are **unattributed**: the phase holds only 2.68 % of worker samples at ten threads, its parallel work being memoized fetches and conversions with the workers largely parked, and section 5 does not size its main-thread serialisation | sections 3, 4, 5 |
| Persist: collect signatures | 44 ms | No | 0.81 (34 / 42), negative | Serial, and slightly slower at ten threads than at one. **Unattributed** | sections 3, 4 |
| Persist: pack writes | 135 ms | No | 0.97 (118 / 122) | Serial. **Unattributed** | sections 3, 4 |
| Fixed process cost | 19.6 ms | No | not applicable | Process start and teardown 5.3 ms, plus a 14.3 ms increment covering embedded-stub loading, configuration, discovery and cache-directory handling as an *empty* project exercises them | section 2 |
| Unattributed residue | 785 ms | not applicable | not applicable | **Unattributed.** Two candidates were tested and ruled out by measurement (`--verbose` stderr formatting, cache-directory deletion). Thread-pool setup, teardown and scheduling variance are named as plausible and untested. This corpus's own Composer and autoload discovery falls inside this residue and is timed nowhere in this document | sections 3, 7 |
| **Wall-clock total** | **4940 ms** | | | | section 3 |

The eight phases sum to 4135 ms; adding the fixed process cost (19.6 ms)
and the residue (785.4 ms) closes on the 4940 ms wall-clock median
exactly, by construction of the residue.

### The idle cores

Whole run, from the only measurement in this document that reports
processor time for the entire process: 21.71 core-seconds over a 4.62 s
wall clock, that is **4.70 effective cores out of ten** (section 8's
session-open control, its median-wall-clock run). The session-close
control gives 22.26 over 4.79 s, 4.65 effective cores. Roughly 5.3 of the
machine's ten cores are idle across a cold run, averaged over its length.

Only one phase has a measured decomposition of where its own cores go,
and the campaign's central number lives there. For the analysis fan-out
at ten threads against one (section 5):

- The phase is about 90 % utilised. That costs a factor of 1.10 out of
  the ten.
- The phase does 2.62 times more processor work at ten threads than at
  one, for the same 24033 walked files and the same result. That costs a
  factor of 2.62.
- Together: 10 divided by 2.62, times 0.906, is 3.46 effective cores,
  against 3.43 measured directly from the profiled artefact's own wall
  clocks and 3.83 from section 4's stripped artefact.

**Expansion, not idleness, owns the missing cores**, and the single
largest component of the expansion (productive work, +4.21 core-seconds,
roughly a third of the fan-out's whole cost at ten threads) has a
measured size and no measured cause. Section 6 tested the one structural
hypothesis the campaign could price cheaply and eliminated it: the
allocator moves the level, not the slope, so the expansion is not
allocation-bound.

**The scaling has already stopped, and section 4 measured where.** The
stagnation point is N = 8: the fan-out gains 45.5 %, 29.1 % and 28.2 %
across the steps to two, four and eight threads, then only 5.8 % from
eight to ten, the first step under the 10 % threshold. Persist: collect
entries stagnates on the same schedule (7.5 % over the same last step).
That measurement is what disciplines every hypothetical in section 10:
the two phases that carry most of the parallel work were already flat
before the machine ran out of cores, so any construction that assumes
ideal ten-core scaling is describing a machine this campaign did not
observe.

No other phase carries a bucket-level attribution. For the file read and
input set phase the direction and size of the loss are measured (a 42 %
growth in `open` time between eight and ten threads) but the mechanism is
not. For persist: collect entries, for the five serial phases and for the
785 ms residue, nothing in this campaign attributes the loss at all;
those rows say unattributed and mean it.

### Per-file scale, for calibration

Over the 24033 files a cold run walks, reads and parses:

- Whole cold run: 4940 ms, about **206 microseconds per file**.
- Corpus floor (walk, read, parse, fixed cost): 1273.6 ms, about
  **53 microseconds per file**, that is 24.1 % of the run (section 7).
- Everything above parsing: about 4.00 s, about **166 microseconds per
  file** (section 7).

## 10. The Amdahl accounting and the lever list

### The basis, stated before any number

Every phase figure in this section comes from **section 4's curve**, and
every ratio is derived from it in exactly two steps. A reader who mixes
bases will get different numbers; this subsection exists so that cannot
happen silently.

**Step one: model the improvement as a relative one, against section 4's
own ten-thread configuration.** That baseline is the section's measured
phases plus two constants imported from elsewhere:

    parallel phases at ten threads   2814.0 ms   (540 + 1399 + 875, section 4)
    serial phases at ten threads      961.0 ms   (374 + 236 + 187 + 42 + 122, section 4)
    fixed process cost                 19.6 ms   (imported from section 2)
    unattributed residue              785.0 ms   (imported from section 3)
    modelled baseline                4579.6 ms

**The two imports, and the small gap they open.** The basis is therefore
not section 4 and nothing else. The fixed process cost is section 2's
empty-project measurement, and the residue is section 3's by-construction
remainder of a 4.94 s run, not of section 4's. Computing the residue the
same way from section 4's own ten-thread numbers gives 4570 minus 3775
minus 19.6, that is 775.4 ms, so the baseline above sits **9.6 ms high,
0.21 %**, against the 4570 ms section 4 actually measured. Rebuilding
every construction below on 775.4 ms moves each factor by about two parts
in a thousand and changes no figure at the precision reported. The gap is
stated rather than absorbed, because a subsection about basis discipline
should not describe a 0.21 % mismatch as a reconciliation.

**Step two: transfer the modelled improvement onto section 7's paired
ratio.** Section 7 measured PHPStan and Celerrate in one session:
38.361 s and 5.278 s, a ratio of **7.27x**. That pair is the only
same-session ratio in this document measured alongside a PHPStan figure
this campaign also uses for the ceiling.

**The transfer rule is itself an assumption, and it is worth about as
much as the basis error it replaces.** Section 7's session ran 15.5 %
slower than section 4's (5.278 s against 4.57 s). Carrying a modelled
saving across that gap can be done two ways, and this document has no
measurement that decides between them:

- **Proportional transfer** (the one used throughout): the model's
  improvement is a *factor*, and a slower session pays it in proportion.
  A modelled total `M` reports as `7.27 x (4579.6 / M)`.
- **Additive transfer**: the model's improvement is a fixed number of
  *milliseconds* of work removed, and a slower session removes the same
  milliseconds. A modelled total `M` reports as
  `38.361 / (5.278 - (4579.6 - M) / 1000)`.

**Both transfers are computed on the unrounded ratio**, 38.361 / 5.278 =
7.26810, not on the 7.27x quoted for readability. The difference is
visible in the second decimal: the mechanism-backed composition is 9.19x
unrounded and 9.20x if the literal 7.27 is used, and construction C1 is
8.36x against 8.37x. Every figure below is the unrounded one.

The two transfer rules disagree by a material amount, and the
disagreement grows with the size of the modelled saving:

| Construction | Proportional | Additive |
| --- | ---: | ---: |
| B, parallel phases at the fan-out's efficiency | 8.04x | 7.93x |
| C1, the mimalloc branch's upper figure | 8.36x | 8.20x |
| The mechanism-backed lever composition | 9.19x | 8.88x |
| The full priced lever list | 12.29x | 11.26x |
| D-prime, all eight phases scaled ideally | 17.71x | 14.88x |

**Proportional is the rule chosen here**, for two reasons. First, most of
what the model removes is parallel work whose cost scales with how fast
the machine is running that day, so a session that runs everything 15.5 %
slower plausibly runs the removed work 15.5 % slower too. Second, the
additive rule has an absurd limit: it would let a modelled saving larger
than the faster session's whole wall clock drive a slower session's total
below zero. Neither reason is a measurement. **The additive figures are
reported beside the proportional ones wherever a conclusion depends on
the difference**, and section 11 checks its proposal under both.

**Why neither rule simply divides the modelled total by 38.361 s.**
Because the model's phases come from section 4's session and PHPStan's
median comes from section 7's, and those two sessions differ measurably:
section 4's Celerrate control medians are 4.67 s and 4.59 s, section 3's
is 4.94 s, section 7's is 5.278 s. Section 4 itself measured a 6.3 % gap
against section 3 and warned it is ordinary run-to-run variation, not a
systematic effect to be spent. Dividing a section 4 model by a section 7
PHPStan median banks that gap as if it were an optimization. **An earlier
draft of this section did exactly that**, quoting a saving of 0.80 s
against section 3's 4.94 s and ratios of 9.3x, 9.6x to 10.4x and 14.0x.
Those figures are withdrawn and replaced below. The absolute-basis value
is still shown for one construction, where the gap between the bases is
itself the point.

### The inputs

Three of the eight phases run under rayon today: file read and input set
(`session.rs` line 439), analysis fan-out (`analysis.rs` lines 145 and
152) and persist: collect entries (`cache/mod.rs` lines 276 and 302), as
section 5 establishes by reading the source against the profiled call
sites. The other five are serial today; three constructions below relax
that.

| Phase | Single-thread median | Ten-thread median |
| --- | ---: | ---: |
| File read and input set (parallel today) | 394 | 540 |
| Analysis fan-out (parallel today) | 5355 | 1399 |
| Persist: collect entries (parallel today) | 4053 | 875 |
| **Parallelizable sum** | **9802** | **2814** |
| Filesystem walk (serial) | 372 | 374 |
| Suggest enrich (serial) | 234 | 236 |
| Render report (serial) | 187 | 187 |
| Persist: collect signatures (serial) | 34 | 42 |
| Persist: pack writes (serial) | 118 | 122 |
| **Serial sum** | **945** | **961** |
| **All eight phases** | **10747** | **3775** |

The all-eight single-thread figure is the sum of the per-phase medians,
10747 ms. Section 4's own "sum of the eight phases" row reports 10752 ms,
because it medians the three per-run totals rather than summing the
per-phase medians; the two differ by 5 ms and nothing below turns on it.

Fixed process cost (19.6 ms) and residue (785 ms) are carried unchanged
in every construction. **Carrying the residue unchanged is an assumption,
not a measurement**: nothing in this campaign establishes that process
startup, teardown and thread-pool scheduling cost the same on a faster
run.

### Construction A, the literal reading

Every parallelizable phase at the fan-out's measured efficiency, 3.83
effective cores of ten:

    9802 / 3.83                      = 2559.3 ms
    2559.3 + 961 + 19.6 + 785        = 4324.9 ms
    factor 4579.6 / 4324.9 = 1.059   -> 7.27 x 1.059 = 7.70x

Reported because the brief asks for the literal figure, and immediately
superseded: it drags persist: collect entries down from its measured 4.63
effective cores to 3.83, which is not an improvement anyone would build.

### Construction B, the local path's parallelism-only best case

The fan-out's measured efficiency as a floor, with no phase made worse
than it is today:

    File read and input set:  394 / 3.83  =  102.9 ms   (measured today: 540)
    Analysis fan-out:        5355 / 3.83  = 1398.2 ms   (measured today: 1399)
    Persist: collect entries: measured     =  875.0 ms   (already 4.63 cores)
    Parallel sum                           = 2376.1 ms
    2376.1 + 961 + 19.6 + 785              = 4141.6 ms

    saving against the 4579.6 ms baseline: 438.0 ms, 9.56 %
    factor 4579.6 / 4141.6 = 1.106   ->  7.27 x 1.106 = 8.04x

**Cold total: 4.14 s modelled, a ratio of about 8.0x.**

The saving decomposes to a single line: 437.1 ms of it is the file read
phase (540 ms down to 102.9 ms) and the remaining 0.8 ms is rounding on
the fan-out, which is already at 3.83 cores by definition. **Construction
B is the read-phase lever and nothing else**, because the fan-out cannot
improve on its own efficiency and persist: collect entries is already
above it. That is the honest content of "bring every parallel phase up to
the fan-out's efficiency": it is worth one phase.

For scale against section 7, transferred onto its session: a 1.106
improvement on its 5.278 s Celerrate median gives 4.773 s, of which the
1.274 s corpus floor is 26.7 %, leaving 3.50 s above parsing against the
644 ms a 20x run allows, a compression of **5.4x** still required. That
holds the floor constant while construction B in fact shortens the read
phase, part of which is inside the floor; correcting for that would lower
both the floor and the required compression somewhat, and this document
does not attempt the correction.

### Construction C, the mimalloc branch

Section 6's verdict has two halves that must not be conflated. Its
*slope* verdict is negative: the fan-out's speedup from one thread to ten
is 3.65x on the default allocator against 3.46x on mimalloc, so there is
no mimalloc-improved efficiency to substitute into construction B. Its
*level* verdict is positive: mimalloc's fan-out costs 4734 ms at one
thread against the default's 5347 ms, and its whole-run wall clock is
0.47 s lower at ten threads.

**What is separable and what is not.** Of that 470 ms whole-run gain,
section 6 measures exactly 95 ms inside the analysis fan-out (its own
default and mimalloc ten-thread fan-out medians are 1463 ms and 1368 ms).
The remaining **375 ms sits in phases section 6 never timed separately**,
and it cannot be assigned. Only the fan-out is separable.

    C1, fan-out at mimalloc's measured single-thread cost, run at 3.83 cores:
      4734 / 3.83                                  = 1236.0 ms
      1236.0 + 102.9 + 875 + 961 + 19.6 + 785      = 3979.5 ms
      factor 1.151  ->  8.36x

    C2, fan-out at mimalloc's own measured efficiency (3.46 cores):
      4734 / 3.46                                  = 1368.2 ms
      1368.2 + 102.9 + 875 + 961 + 19.6 + 785      = 4111.7 ms
      factor 1.114  ->  8.10x

    C3, construction B with section 6's whole-run level gain applied:
      4141.6 - 470                                 = 3671.6 ms
      factor 1.247  ->  9.07x   [withdrawn, see below]

**C3 is withdrawn as an anchor because it plausibly double-counts.**
Construction B has already removed 437.1 ms from the file read and input
set phase. C3 then subtracts mimalloc's whole 470 ms, of which only 95 ms
is demonstrably fan-out; the unassigned 375 ms is spread across the seven
phases and the residue that section 6 did not time, and the read phase is
a prime candidate to hold some of it, since `Session::load` interns every
walked path in the VFS and creates or updates a `SourceFile` salsa input
per file on the calling thread (section 7), which is allocation-heavy
work of exactly the kind a faster allocator speeds up. Whatever part of
the 375 ms lives inside the 437.1 ms construction B already removed is
counted twice.

Bounding the overlap: it is at least 0 ms and at most 375 ms, so C3's
true value lies between 3671.6 ms (no overlap) and 4046.6 ms (the whole
unassigned gain overlapping), that is between 9.07x and **8.23x**. The
upper edge of C3 is therefore *worse* than C1, which is built only from
figures section 6 measured phase by phase and cannot double-count at all.

**The mimalloc branch is 8.1x to 8.4x** (C2 to C1). C3 is recorded for
completeness and used for nothing.

### Construction D, the parallelism bound, and what it assumes

Ideal ten-core scaling, which is the same as removing the fan-out's 2.62x
work expansion outright and claiming all ten cores.

**D, today's three parallel phases only:**

    9802 / 10                                      =  980.2 ms
    980.2 + 961 + 19.6 + 785                       = 2745.8 ms
    factor 1.668  ->  12.12x

**D-prime, all eight phases, which is what levers 4 and 6 below actually
propose:**

    10747 / 10                                     = 1074.7 ms
    1074.7 + 19.6 + 785                            = 1879.3 ms
    factor 2.437  ->  17.71x

**The difference between 12.1x and 17.7x is entirely an assumption about
which phases may be parallelized, not a measurement.** D holds the walk,
suggest enrich, render report, collect signatures and pack writes serial
because they are serial today; D-prime relaxes that, which is exactly
what levers 4 and 6 propose doing. An earlier draft of this section
stated D alone and concluded that "even perfect parallelism does not
reach 20x" and that "everything beyond 14.0x has to come from doing less
work". Both are withdrawn: they were artefacts of D's assumption, and the
second was wrong on its own terms since two of this section's own levers
attack phases D holds fixed.

Computed on the absolute basis instead (1879.3 ms divided into section
7's 38.361 s) D-prime reads **20.41x**. The spread of 2.7 ratio points
between 17.7x and 20.4x for one and the same construction is the size of
the basis error this section's opening subsection corrects, displayed
rather than hidden. On the additive transfer rule it reads 14.88x
instead, a further 2.8 ratio points down, so this one construction spans
14.9x to 20.4x depending only on how its figures are carried between
sessions. On the basis and the transfer rule this section declares,
D-prime is 17.7x.

**Neither D nor D-prime is reachable, and section 4 measured why.** Both
assume ten-core scaling of phases that were already flat at eight
threads: the fan-out gains 5.8 % from eight threads to ten and persist:
collect entries 7.5 %. A construction that assumes those two phases would
divide cleanly by ten is contradicted by the campaign's own stagnation
measurement. D and D-prime bound a class of work; they do not describe an
outcome.

The claim the evidence does support, replacing the withdrawn one:
**no path this campaign priced reaches 20x**, and the largest measured
quantity standing between the campaign and any of these bounds (the
fan-out's 2.62x work expansion, half of it unexplained) has no lever
attached to it.

### The lever list

Every lever below carries an estimated gain bounded by a measured figure
from this document, its class, and its recommended order. A lever whose
gain no measurement bounds is not on this list; the ones that had to be
left off are named after it.

**These bounds are not additive.** mimalloc's whole-run figure overlaps
every phase's, and several of the others bound the same parallel
inefficiency. The composed figures are given after the table, not by
adding the rows.

| Order | Lever | Class | Estimated gain, and the measured figure that bounds it | Evidence |
| ---: | --- | --- | --- | --- |
| 1 | Adopt mimalloc as the global allocator | Dependency | Up to **-0.47 s**, 10.3 % of the ten-thread wall clock (4.56 s default against 4.09 s mimalloc, medians of three runs each, alternated within one session). Only **95 ms** of that is separable to a named phase (the fan-out); the other 375 ms is real but unassigned, which is why it cannot be composed with the phase levers below without double-counting | section 6 |
| 2 | Stop the file read and input set phase scaling negatively (cap its thread count, or serialise its `open` calls) | Local | Up to **-437 ms**, if it reached the fan-out's 3.83 effective cores (394 ms single-thread divided by 3.83 is 103 ms, against 540 ms measured at ten threads). The conservative form, running it at the four threads where its own curve is fastest, is **-300 ms** (540 ms against 240 ms, both measured). The mechanism is measured in direction and size (file input and output core-seconds 4.72 at eight threads against 6.72 at ten, +42 % inside `open`) but not in cause | sections 4, 5 |
| 3 | Close persist: collect entries' remaining parallel gap | Local | Up to **-470 ms**: 875 ms measured at ten threads against 405 ms at ideal ten-core scaling of its 4053 ms single-thread cost. **This bound is contradicted by the phase's own curve**: it stagnates at N = 8 exactly as the fan-out does, gaining 7.5 % over the last step, so the measured trend says the remaining gap does not close by adding threads. The row stays on the list because the gap is measured; its bound should be read as arithmetic headroom, not as an expectation | sections 3, 4, 5 |
| 4 | Parallelize the filesystem walk | Local | Up to **-337 ms**: 374 ms at ten threads today, against 37 ms at ideal ten-core scaling of its 372 ms single-thread cost. The cause of its serialisation is named in the source (`BTreeSet` accumulation on the calling thread), which is why this row has a mechanism where rows 3 and 6 do not. The read phase's negative scaling is the standing warning against assuming filesystem work parallelizes freely on this machine | sections 3, 4, 7 |
| 5 | Reduce salsa interning traffic, or contend less for its lock | Local, with a dependency variant | Up to **-91 ms** on section 4's fan-out median (1399 ms), from section 5's own statement that removing this contention entirely moves the fan-out from about 3.46 to about 3.70 effective cores, that is 6.5 % off the phase. The lock is named down to five entry points, 1598 of 1763 contended samples entering through `salsa::interned::IngredientImpl<C>::intern_id` | section 5 |
| 6 | Make suggest enrich and render report cheaper or parallel | Local | Up to **-423 ms**, their entire combined measured cost at ten threads (236 ms and 187 ms), both flat with thread count. This bound is the phase cost itself: **no measurement in this campaign suggests any particular fraction of it is removable**, so the row is honest about being an upper bound with nothing inside it | sections 3, 4 |
| 7 | Reduce the fixed process cost | Local | Up to **-19.6 ms**, 0.4 % of the wall clock. Bounded, measured, and immaterial. It is listed so that its size is on the record and it is not revisited | section 2 |
| 8 | Adopt a shared-nothing, PHPStan-style worker architecture | Architectural | Bounded at **no gain**. Every configuration measured is slower in wall clock than the single ten-thread process (6.43 s to 10.08 s against a 4.62 s control) and burns 1.46x to 2.06x its processor work, across two partitionings and three thread budgets. A startup-amortizing implementation is estimated at a 1.33x residual, which is still a loss, and that estimate is reasoned from solo-probe floors rather than measured. **Recommended order: not to be pursued in the form measured** | section 8 |

Rows 1, 2, 4 and 5 have both a bound and a named mechanism. Rows 3 and 6
have a bound and no mechanism, and row 3's bound is actively contradicted
by its own phase's stagnation. Row 7 is immaterial. Row 8 is a negative
result and belongs on the list precisely so that it is not re-proposed.

### What the lever list composes to

Three compositions, all on the basis declared at the top of this section,
and all counting only the 95 ms of mimalloc that is separable to a phase
so that nothing is double-counted.

**Why lever 1's 95 ms and lever 5's 90.7 ms may be added even though both
act on the analysis fan-out.** They are measured in different buckets,
and section 5's bucketing rules make those buckets disjoint by
construction: each sample's self time is assigned to exactly one bucket
by reading the whole stack, and the rules are tested in an order that
gives a lock ownership of the allocation performed beneath it, so
allocator time spent inside a contended interning lock counts as salsa
lock contention and not as allocator. Per-capture bucket counts sum
exactly to each capture's worker total, which is the arithmetic check
that no sample is counted twice. The allocator lever and the interning
lever therefore attack different measured time.

**What is measured outright, before any lever's bound is assumed:**

    4579.6 - 95 (mimalloc's separable fan-out part, 1463 against 1368)
      = 4484.6 ms      factor 1.021  ->  7.42x   (additive: 7.40x)

**The mechanism-backed levers (1, 2, 4, 5), each landing at its full
bound:**

    4579.6 - 437.1 (read) - 336.8 (walk) - 90.7 (interning) - 95 (mimalloc, fan-out part)
      = 3619.9 ms      factor 1.265  ->  9.19x   (additive: 8.88x)

**The full priced lever list (1 to 7), every lever landing at its measured
upper bound simultaneously:**

    3619.9 - 469.7 (persist) - 423 (enrich and render) - 19.6 (fixed cost)
      = 2707.6 ms      factor 1.691  ->  12.29x  (additive: 11.26x)

**9.19x is a ceiling, not a floor, and the distinction decides what may
be built on it.** Only 95 ms of its 959.6 ms saving is measured outright.
The other three components are upper bounds under assumptions the
campaign did not verify: the read phase reaching the fan-out's 3.83
effective cores, the walk scaling ideally across ten cores, and interning
contention removed in its entirety. Lever 4's own row carries the
standing warning against the second of those, since the one filesystem
phase this campaign measured under threads scales negatively. **9.19x is
therefore what the mechanism-backed subset reaches if each of its four
levers pays out in full**, and an earlier draft of this section which
called it what those levers "defend" and "deliver" overstated it. The
figure genuinely measured outright is 7.42x, which is barely above
today's 7.27x, because only one lever in the entire campaign has a gain
that was measured rather than bounded.

The same reading applies one level up: **12.29x is the ceiling of the
whole priced list**, and reaching it additionally requires row 3 to beat
its own stagnation measurement and row 6 to be removed entirely with no
evidence that any of it is removable. All three compositions sit below
construction D-prime's 17.7x, which is itself unreachable for the reason
the stagnation measurement gives.

### Levers that had to be left off, for lack of a measured bound

Naming these is a finding of the campaign, not an omission from it.

- **The productive-work growth in the analysis fan-out.** +4.21
  core-seconds between one thread and ten, 50 % of the fan-out's work
  expansion and roughly a third of its cost at ten threads: the largest
  single quantity this campaign measured. Its size is bounded; its cause
  is not. Section 5 establishes that it is not lock contention, not memo
  waiting and not the allocator, because those are separate buckets that
  grow separately, and section 6 independently eliminates the allocator
  by showing the level and the slope move differently. No lever can be
  named against it without an instrument this campaign does not have:
  hardware performance counters, or an experiment that varies memory
  pressure independently of thread count. **The campaign's largest
  finding has no lever attached to it**, and it is the same quantity that
  makes constructions D and D-prime unreachable.
- **The 785 ms unattributed residue**, and inside it this corpus's own
  Composer and autoload discovery cost, which is timed nowhere in this
  document. The residue is 15.9 % of the cold wall clock, larger than
  five of the eight phases. Two candidates were ruled out by measurement;
  no positive attribution exists, so no gain can be estimated.
- **The `tests` and `classes` cost concentration.** `tests` costs 4.73 s
  in a solo cold run against a roughly 0.95 to 1.0 s floor every solo run
  in that table shares, and `tests/Unit` accounts for 3.41 s of it over
  480 files, while `vendor/symfony` costs 0.98 s over 4730 files. The
  concentration's size is measured and it is large. Section 8 states
  plainly that it does not open the files to find the cause, so no gain
  can be estimated and this stays a diagnostic lead rather than a lever.
  It is the most promising unexplored direction the campaign leaves
  behind.
- **Compressing the work above parsing.** 4.00 s today, 166 microseconds
  per walked file. Section 7 computes that a 20x ratio requires
  compressing it by about 6.2x; construction B's own transferred figure
  above still leaves 5.4x of it to find. Nothing in this campaign bounds
  whether any fraction of that compression is achievable, which is
  precisely why the ambition proposal below cannot be built on it.

## 11. The ambition amendment proposal

**Status: a proposal. Nothing in the parent design document was changed
by the work that produced this section.** It awaits approval before the
wording below is applied to
`.claude/superpowers/specs/2026-07-09-celerrate-design.md`.

### The three figures the proposal rests on

- **The measured ceiling: 30.1x.** PHPStan's same-session cold median of
  38.361 s divided by the 1.274 s corpus floor, which is the cost of
  walking, reading, lexing and parsing all 24033 files plus the fixed
  process cost. Every combination of the observed extremes leaves it
  between 22.1x and 32.1x; using section 1's lower cross-session PHPStan
  median gives 25.6x; at one thread it collapses to 12.3x. It is an upper
  bound on a bound: the truly reachable ceiling is lower by a margin
  section 7 deliberately does not measure, and lower again because
  project discovery is omitted from the floor. (Section 7.)
- **The local path: 7.4x measured outright, a mechanism-backed ceiling of
  about 9.2x, and a priced ceiling of about 12.3x.** Only one lever in
  the campaign has a gain that was measured rather than bounded
  (mimalloc's 95 ms inside the fan-out), and it alone is worth 7.42x.
  Bringing every parallel phase to the fan-out's own measured efficiency
  is worth 8.04x; adding the measured allocator gain takes it to 8.10x to
  8.36x; adding the walk and interning levers reaches 9.19x, **which is
  the ceiling of that subset and not its floor**, since three of its four
  components are upper bounds under assumptions the campaign did not
  verify. Letting every priced lever land at its upper bound
  simultaneously, including two whose bounds have no mechanism behind
  them and one contradicted by its own phase's stagnation, reaches
  12.29x. Ideal ten-core scaling of today's three parallel phases would
  be 12.12x, and of all eight phases 17.71x, both contradicted by the
  campaign's own measurement that scaling stagnates at eight threads. On
  the additive transfer rule every figure in this bullet falls: 7.40x,
  7.93x, 7.98x to 8.20x, 8.88x, 11.26x, 11.14x and 14.88x. (Section 10.)
- **The architectural path's bound: no gain.** A shared-nothing split
  into isolated worker processes is slower in wall clock at every
  partitioning and thread budget measured and burns 1.46x to 2.06x the
  single process's processor work; the startup-amortized residual is
  estimated at 1.33x, still a loss, and is an estimate rather than a
  measurement. (Section 8.)

### What the evidence defends

Measured today: 7.27x in section 7's paired session, 6.6x in section 1's.
The parent design document separately records 8.01x from 2026-08-06; that
figure paired a 4.874 s Celerrate wall clock against a PHPStan median
near 39.0 s, the top of PHPStan's 31 s to 39 s historical band, while
section 1 paired against 32.652 s at the bottom of it. The three
current-ratio figures are not in conflict: they span PHPStan's own
run-to-run variation, which this campaign measured directly at 17.5 %
between two sessions on the same day and the same machine.

Above that, as a ladder of ceilings rather than of expectations, on both
transfer rules:

| Reading | Proportional | Additive |
| --- | ---: | ---: |
| Measured today (section 7's paired session) | 7.27x | 7.27x |
| The one lever whose gain is measured outright | 7.42x | 7.40x |
| Ceiling of the mechanism-backed levers (1, 2, 4, 5) | 9.19x | 8.88x |
| Ceiling of the full priced list (1 to 7) | 12.29x | 11.26x |
| Every phase scaled ideally across ten cores | 17.71x | 14.88x |
| Arithmetic ceiling (an upper bound on a bound) | 30.1x | 30.1x |

The ideal-scaling row is contradicted by the stagnation at N = 8 and is
listed to bound a class of work, not to be reached. The architectural
alternative delivers nothing.

**The evidence defends about 9x, and does not defend 20x.** The proposal
is therefore **at least ~9x**, revised down from the ~10x an earlier
draft of this section proposed.

**One thing must be said plainly, because what is being approved is a
published number.** The largest whole figure that both transfer rules
place strictly inside the levers with a measured mechanism is **8x**: it
needs 43.7 % of that subset's 959.6 ms bound proportionally and 50.3 %
additively, so it is comfortably inside the subset under either reading.
**9x is a deliberate stretch above 8x, not the conservative reading of
the same evidence.** It needs 91.8 % of the subset proportionally, and
additively it needs the whole subset plus a further 6.1 % of the levers
whose bounds have no mechanism. Approving ~9x is approving a target that
requires nearly everything the mechanism-backed levers can give and, on
the less favourable transfer rule, slightly more than they hold; ~8x is
what the same evidence supports with room to spare. This document
proposes 9x because a published ambition should sit above the
comfortable reading, and it names 8x here so that choice is the owner's
rather than an artefact of how the figure was presented.

The reason for the revision is the correction one row up. That earlier
draft placed 10x "above what the mechanism-backed levers deliver (9.2x)
and below the bound of the full priced list (12.3x)", treating 9.19x as a
floor to clear. It is not a floor: only 95 ms of its 959.6 ms saving is
measured outright, and its other three components assume the read phase
reaches the fan-out's efficiency, the walk scales ideally across ten
cores, and interning contention disappears entirely. Anchoring a
published target above a ceiling built from three unverified upper bounds
is the same top-edge error this document already corrected once, in
withdrawing construction C3.

**Where 9x sits, checked under both transfer rules.** Proportionally, 9x
needs a modelled total of 3698.3 ms, that is 881.3 ms of saving, which is
91.8 % of the mechanism-backed subset's own 959.6 ms: it is reachable
inside that subset, without calling on either lever whose bound has no
mechanism. Additively, 9x needs 1015.7 ms, which exceeds the subset by
56.1 ms, so it additionally needs 6.1 % of the 912.3 ms held by levers 3,
6 and 7. **9x is therefore inside the mechanism-backed subset under the
transfer rule this document uses, and just outside it under the other**,
which is the honest description of a target set at the top of what the
levers with mechanisms can carry.

For comparison, 10x would need 32.0 % of those mechanism-free levers
proportionally and 52.9 % additively, on top of all four mechanism-backed
levers landing in full. That is a target whose path runs mostly through
bounds with nothing behind them, one of which its own phase's stagnation
contradicts. It is inside the priced band under both rules, so it is not
indefensible; it is simply less defensible than 9x, and this document
proposes the figure it can trace.

The campaign's central tension should be stated rather than dissolved.
20x is not arithmetically impossible: it is below the 30.1x ceiling with
a factor of 1.51x to spare. But the only two structural options the
campaign priced were an allocator swap, worth about ten percent of the
wall clock as a level gain and nothing as a slope gain, and a
shared-nothing architecture, worth nothing at all. What 20x actually
requires is a compression of everything above parsing of about 6.2x, or
5.4x after construction B's own modelled improvement, and the campaign
found no lever pointed at it, because its largest measured quantity (the
fan-out's unexplained productive-work growth) has no measured cause and
therefore no lever. Amending the published figure down to 9x is not a
retreat from ambition; it is refusing to publish a number that no
measurement in this campaign supports. The gap between 9x and the 30.1x
ceiling is not a claim that the remaining room does not exist; it is a
statement that this campaign found no measured path into it, and the
directions it could not price (the unexplained productive-work growth,
the `tests` and `classes` concentration, the 785 ms residue) are named
above so a later effort knows where to look.

### The exact replacement wording

Two passages in
`.claude/superpowers/specs/2026-07-09-celerrate-design.md`, section 7,
under "Published performance targets", plus one amendment-history entry.

**Passage 1, the target sentence.** Replace:

> Held in CI by benchmarks: at least ~20x faster than PHPStan on a cold full
> analysis, and sub-second incremental updates on single-file changes in a
> Symfony-sized project.

with:

> Held in CI by benchmarks: at least ~9x faster than PHPStan on a cold full
> analysis, and sub-second incremental updates on single-file changes in a
> Symfony-sized project.

**Passage 2, the position paragraph's closing reasoning.** Replace,
inside the paragraph beginning "Position at the end of the CLI product
sub-project", everything from `The "at least ~20x faster" ambition above
is still not amended down` through `isolates how much, or rules other
costs in or out.` with:

> The "at least ~20x faster" ambition this section previously held is
> amended down to "at least ~9x" on the evidence of the 2026-08-07
> cold-run performance diagnostic
> (`.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`).
> Both reasons the previous measurement gave for not testing the gap are
> gone: the quadratic did-you-mean pass in the presentation layer is
> fixed, and the whole process now runs at 4.51 effective cores of 10
> (22.0 s of CPU over 4.874 s of wall clock), up from the 1.27 the
> superseded run measured the same way (17.0 s over 13.41 s), still short
> of PHPStan's own 6.21. The 8.01x above is the highest of three
> same-session cold ratios now on record, and the diagnostic explains the
> spread rather than resolving it: it paired against a PHPStan median
> near 39.0 s, the top of PHPStan's 31 s to 39 s band, while the
> diagnostic's own two sessions paired against 32.652 s (6.6x) and
> 38.361 s (7.3x), PHPStan's median having moved 17.5 % between two
> sessions on the same day and the same machine. Against that spread, the
> diagnostic measured what fills the remaining gap, and three of its
> findings set the new number. First, the arithmetic ceiling on this
> corpus and machine is 30.1x: walking, reading, lexing and parsing all
> 24033 PHP files a cold run touches costs 1.274 s against PHPStan's
> same-session cold median of 38.361 s, so no optimization of anything
> Celerrate does above parsing can beat that ratio, and the truly
> reachable ceiling is lower than 30.1x by a margin the diagnostic
> deliberately does not measure. Second, the local path's priced levers
> reach a ceiling of about 9.2x with mechanisms behind them and about
> 12.3x in total: bringing every phase that runs under rayon today up to
> the analysis fan-out's own measured parallel efficiency is worth about
> 8.0x, the measured allocator gain takes it to between 8.1x and 8.4x,
> and the filesystem walk and the salsa interning lock take it to 9.2x,
> which is that subset's ceiling and not its floor, since only one of its
> four components (the allocator's 95 ms inside the fan-out, worth 7.4x
> on its own) is a measured gain rather than an upper bound. Reaching
> 12.3x needs every priced lever to land at its upper bound at once,
> including two whose bounds no mechanism supports and one contradicted
> by its own phase's measured curve. Ideal ten-core scaling of every
> phase would reach 17.7x, and the same diagnostic measured that scaling
> has already stagnated at eight threads, so that bound describes a
> class of work rather than an outcome. The ~9x published above is a
> deliberate stretch: the largest figure that sits comfortably inside
> the levers with a measured mechanism is about 8x, and 9x needs nearly
> everything those levers can give. Third, the architectural alternative
> was priced and rejected: a shared-nothing, PHPStan-style split into
> isolated worker processes is slower in wall clock at every
> partitioning and thread budget measured, and burns 1.46x to 2.06x the
> single process's processor work. Reaching ~20x would require
> compressing everything Celerrate does above parsing by about 6.2x, and
> no measurement in that campaign bounds whether any part of that
> compression is achievable, so ~20x stays arithmetically possible on
> this corpus while being reached by no path the campaign priced. It is
> recorded here as an unbounded aspiration, not as a held target.

The paragraph's final sentence, beginning "Section 11 of
`.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`
carries the evidence", is unchanged and stays where it is.

**Passage 3, the amendment-history entry**, appended to the list near the
top of the document in its established dated style. The list carries two
punctuation forms, `2026-07-14 —` and `2026-07-19:`; the colon form is
used below, since this repository's writing conventions exclude
em-dashes:

> - 2026-08-07: amended the published cold-run performance target down
>   from "at least ~20x faster than PHPStan" to "at least ~9x", on the
>   evidence of the cold-run performance diagnostic
>   (`.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`):
>   the arithmetic ceiling on the pinned corpus is 30.1x, the local
>   path's levers with a measured mechanism reach a ceiling of about
>   9.2x and the full priced lever list one of about 12.3x, ideal ten-core
>   scaling of every phase would reach 17.7x against a measured
>   stagnation at eight threads, and the shared-nothing architectural
>   alternative was measured to deliver no gain at any partitioning or
>   thread budget tried. Section 7's published-performance-targets
>   paragraph carries the derivation.

### What this proposal does not claim

- It does not claim 9x is easy. It is above every ratio measured on this
  corpus (6.6x, 7.27x, and the 8.01x already in the design document), it
  needs 91.8 % of the mechanism-backed subset's whole bound under the
  transfer rule this document uses and slightly more than that subset
  holds under the other, and every one of those component bounds except
  the allocator's 95 ms is an upper bound the campaign did not verify.
- It does not claim the ceiling is reachable. 30.1x is an upper bound on
  a bound, and both of its own caveats push the true ceiling down.
- It does not claim 20x is impossible. It claims that no path this
  campaign priced reaches it, that even ideal ten-core scaling of every
  phase falls short of it on the basis section 10 declares, and that
  publishing a target with no measured path behind it is what the design
  document's own anti-false-positive stance on performance claims
  forbids.
