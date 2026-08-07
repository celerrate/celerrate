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
gap is larger than the ~10 % drift threshold would tolerate if it were
being judged as a same-session drift, but it is not being judged as one:
it is smaller than the spread each session's own runs already show.
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
interval 1 millisecond, 4-second window, attaching 25 to 35 milliseconds
after process launch.
Profiles written to `/tmp/celerrate-profiles/sym/` (not committed).

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
committed. The canonical stripped artefact was copied aside before the
rebuild and restored byte for byte afterwards (identical SHA-256), so the
artefact sections 1 to 4 cite remains exactly reproducible. `strip` and
`debug` change only what is emitted alongside the machine code, not the
machine code itself, and `debug=1` was included because `stub_frontier`
is a plain function with a single caller that can be inlined out of the
symbol table without line tables.

The equivalence control below establishes that the symbol-carrying
artefact is the same performance object, which is what licenses comparing
this section's percentages to section 4's timings.

### Equivalence control

Three cold runs at each thread count on the symbol-carrying artefact,
using section 4's exact protocol (`rm -rf .celerrate`, then
`env RAYON_NUM_THREADS=<N> /usr/bin/time -p <binary> check . --verbose > /dev/null`),
so the wall clocks are directly comparable:

| N | Run 1 | Run 2 | Run 3 | Median | Section 4 median | Difference |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 10 | 5.69 s | 4.48 s | 4.75 s | 4.75 s | 4.57 s | +3.9 % |
| 8 | 5.06 s | 4.65 s | 4.68 s | 4.68 s | 4.56 s | +2.6 % |

Both sit inside the Protocol's ~10 % drift threshold, so the
symbol-carrying artefact is treated as the same performance object and
this section's figures are read against section 4's. The first run at
each thread count is the slowest of its three in both cases, which is the
expected shape for a freshly linked binary that no run has yet paged in;
it is reported rather than discarded.

Disclosure of a divergence the wall clock hides: the per-phase medians
from these same control runs do not all match section 4 as closely as the
wall clock does. The analysis fan-out reads 1648 milliseconds at ten
threads against section 4's 1399, and 1590 against 1485 at eight. The
per-run values at ten threads are 1652, 1361 and 1648 against section 4's
1535, 1399 and 1368, so the two sets overlap and the difference is inside
the spread each set already shows, but the median gap at ten threads is
17.8 %, larger than the wall-clock gap. Every derived figure in this
section that needs a fan-out wall time uses this artefact's own control
medians, never section 4's, so the derivation stays internally
consistent.

### Capture protocol

Six captures, three at ten threads and three at the stagnation point
N = 8 that section 4 identified. Each capture is one cold run:

```
rm -rf .celerrate
env RAYON_NUM_THREADS=<N> /absolute/path/to/target/release/celerrate check . > /dev/null 2>&1 &
sample $! 4 -file /tmp/celerrate-profiles/sym/cold-<N>t-run<K>.txt
wait
```

Run from the equalized corpus directory, binary by absolute path, with
`rm -rf .celerrate` outside the sampled region. The deviation section 4
records applies here too: the brief's snippet runs from the pinned corpus
directory, which walks a smaller, non-comparable file set.

| Capture | Worker threads | Worker samples (total) | Samples per worker | Main-thread samples |
| --- | ---: | ---: | ---: | ---: |
| 10 threads, run 1 | 10 | 22390 | 2239 | 2546 |
| 10 threads, run 2 | 10 | 22480 | 2248 | 2563 |
| 10 threads, run 3 | 10 | 22360 | 2236 | 2570 |
| 8 threads, run 1 | 8 | 18864 | 2358 | 2671 |
| 8 threads, run 2 | 8 | 18568 | 2321 | 2636 |
| 8 threads, run 3 | 8 | 19056 | 2382 | 2703 |

The effective interval is about 1.5 milliseconds, not the nominal 1:
`sample` sleeps one millisecond of run time between samples and pays its
own collection cost on top. Taking the main thread, which is alive for
the whole window, as the reference gives 1.571, 1.561 and 1.556
milliseconds for the three ten-thread captures and 1.498, 1.517 and 1.480
for the three at eight.

Sampling-window coverage, stated for both thread counts because the
worker pool is created after the run starts, during the filesystem walk:
the worker threads are alive for 3.52, 3.51 and 3.48 seconds of the
4.00-second window at ten threads (87 % to 88 %), and 3.53, 3.52 and 3.52
seconds at eight threads (88 %). Every worker percentage below is a share
of worker lifetime inside the window, not of the whole process lifetime.

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
memo machinery. They hold 0.25 % of worker samples at ten threads and
0.33 % at eight, dominated by a single `salsa::interned::HashEqLike`
implementation for a Celerrate string type. That share is reported as its
own line rather than folded silently into either side. A residue that no
method here can size remains: memo code fully inlined into a query body
with no distinguishing symbol leaves no trace at all, and the profile
cannot bound it.

Samples that reach no attributable frame at all are 2 or 3 per capture at
ten threads and 0 or 1 at eight, that is under 0.01 %.

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

Each capture's counts sum exactly to its total. Medians are taken per
bucket across the three captures, so they do not compose: a median of a
sum is not the sum of the medians, and no total below is built by adding
the medians above.

### The parked total, from per-capture totals

The parked share is the figure the reading turns on, so it is computed
per capture and only then medianed:

| | Run 1 | Run 2 | Run 3 | Median | Range |
| --- | ---: | ---: | ---: | ---: | ---: |
| Parked, 10 threads | 38.62 % | 36.25 % | 38.69 % | 38.62 % | 36.25 to 38.69 |
| Parked, 8 threads | 32.61 % | 30.20 % | 37.88 % | 32.61 % | 30.20 to 37.88 |
| Running, 10 threads | 61.38 % | 63.75 % | 61.31 % | 61.38 % | 61.31 to 63.75 |
| Running, 8 threads | 67.39 % | 69.80 % | 62.12 % | 67.39 % | 62.12 to 69.80 |

The medians differ by 6.01 points, but the two distributions overlap:
the eight-thread run 3 (37.88 %) is parked more than the ten-thread run 2
(36.25 %). At three captures per thread count, the rise in parked time
from eight to ten threads is a direction, not an established quantity,
and nothing below rests on its size. Whole-run parked share is in any
case the wrong instrument for the fan-out question, for the reason the
next section gives.

### Call sites, and what belongs to no call site

The parallel call sites resolve by name. Reading the source establishes
which phase timer wraps each one: `session.rs` line 439 is the file read
and input set phase; `analysis.rs` lines 145 and 152 are both inside the
analysis fan-out timer, the first a prewarm of `item_tree` over every
file and the second the per-file `analyze_one`; `cache/mod.rs` lines 276
and 302 are both inside the persist collect-entries timer.

Median shares of total worker samples:

| Call site | 10 threads | 8 threads | Phase |
| --- | ---: | ---: | --- |
| `analysis::served_typed_diagnostics` | 21.17 % | 21.38 % | analysis fan-out |
| `semantics::queries::item_tree` (prewarm) | 18.66 % | 19.42 % | analysis fan-out |
| `std::fs::read` (closure inlined) | 19.21 % | 16.79 % | file read and input set |
| `analysis::persistable_diagnostics` | 2.60 % | 2.81 % | persist collect entries |
| `semantics::queries::member_tree` | 0.00 % | 2.08 % | persist collect entries |
| `types::records::class_surface_digest` | 0.00 % | 3.60 % | not mapped |
| Parked, no job at all | 28.05 % | 25.50 % | **no call site** |
| Parked at a split | 8.20 % | 7.11 % | **no call site** |

**The share that belongs to no call site, stated plainly.** A worker
parked with no job carries no job frame, and a worker parked at a split
carries no job body either. Together they are 36.25 % to 38.69 % of
worker samples at ten threads and 30.20 % to 37.88 % at eight. That share
cannot be assigned to any phase by construction, and no reading below
assigns it to one. The unmapped call-site residue is separate and small:
0.16 % median at ten threads, 3.94 % at eight, the latter almost entirely
`class_surface_digest`, a types query reached through a parallel
construct whose enclosing frame is inlined away. It most plausibly sits
inside the analysis phase, but that is not established here and it is
left unmapped rather than folded in.

The `item_tree` call site was checked for whether it is the fan-out's
prewarm or the persist phase's tree pass, since both call the same query:
99.95 % of its samples at ten threads have `salsa::function::execute` on
the stack, meaning a cold miss that is computing, not a memoized fetch
being serialised. It is the prewarm, inside the fan-out.

The persist phase holds only 2.68 % of worker samples at ten threads and
5.18 % at eight, far below the share its 883-millisecond wall time would
suggest. Its parallel work is memoized fetches and conversions, so the
workers are largely parked during it, which is part of what the parked
buckets contain.

### Reconciling the fan-out against its wall clock

The reconciliation the parked share cannot give directly: the fan-out's
wall time on this artefact, times the thread count, divided by the
effective sampled interval, is the number of worker samples the phase
would hold if every worker were busy for its whole duration. Comparing
that with the samples the fan-out call sites actually hold gives the
phase's own utilisation, with no appeal to the unattributable parked
share.

| | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| 10 threads, expected if fully busy | 10490 | 10560 | 10588 | |
| 10 threads, observed | 8810 | 9024 | 9101 | |
| 10 threads, utilisation | 84.0 % | 85.5 % | 86.0 % | 85.5 % |
| 8 threads, expected if fully busy | 8494 | 8382 | 8596 | |
| 8 threads, observed | 7697 | 7806 | 7547 | |
| 8 threads, utilisation | 90.6 % | 93.1 % | 87.8 % | 90.6 % |

The two ranges do not overlap: 84.0 to 86.0 against 87.8 to 93.1. The
fan-out loses about five points of utilisation between eight and ten
threads, and it is between 85 % and 91 % busy in both cases. Idleness
inside the fan-out is therefore real but small, and it is nowhere near
large enough to explain a phase that reaches 3.83 effective cores out of
ten.

### What the fan-out actually costs

The same arithmetic converts samples into processor time, which is what
exposes the rest of the loss. Fan-out cost in core-seconds inside the
window:

| Bucket | 10 threads (3 captures) | Median | 8 threads (3 captures) | Median |
| --- | --- | ---: | --- | ---: |
| Productive work | 7.88, 7.95, 8.04 | 7.95 | 7.03, 6.93, 6.73 | 6.93 |
| Allocator | 3.20, 3.09, 3.37 | 3.20 | 2.56, 2.82, 2.54 | 2.56 |
| Salsa memo access | 1.78, 1.79, 1.79 | 1.79 | 1.57, 1.59, 1.56 | 1.57 |
| Salsa lock contention | 0.83, 1.04, 0.84 | 0.84 | 0.28, 0.31, 0.25 | 0.28 |
| Memo wait | 0.06, 0.10, 0.04 | 0.06 | 0.01, 0.07, 0.02 | 0.02 |
| Salsa glue | 0.09, 0.12, 0.08 | 0.09 | 0.08, 0.11, 0.08 | 0.08 |
| **Total** | **13.84, 14.08, 14.16** | **14.08** | **11.53, 11.85, 11.17** | **11.53** |

Every line here has non-overlapping ranges between the two thread counts
except salsa glue, and the totals are 13.84 to 14.16 against 11.17 to
11.85. The fan-out consumes about 2.5 core-seconds more processor time at
ten threads than at eight, roughly 22 % more, to do identical work on an
identical corpus, while its wall time does not improve at all (1648
milliseconds against 1590 on this artefact). That is the loss: not
idleness, and not any one bucket, but the whole phase costing more per
unit of work as threads are added.

Where the extra 2.5 core-seconds go, by difference of medians: productive
work +1.02, allocator +0.64, salsa lock contention +0.56, salsa memo
access +0.22, memo wait +0.04. In relative terms the ordering reverses:
salsa lock contention triples (+200 %), the allocator grows 25 %,
productive work grows 15 %.

Salsa lock contention is named down to its entry points, and it is
concentrated: across the three ten-thread captures, 1598 of 1763 samples
enter through `salsa::interned::IngredientImpl<C>::intern_id`, with
`salsa::table::Table::fetch_or_push_page` (83),
`salsa::function::sync::ClaimGuard::drop_impl` (39),
`salsa::function::sync::SyncTable::try_claim` (37) and
`salsa::table::Table::record_unfilled_page` (6) making up the rest. Five
distinct entry sites, not two.

The whole-run view agrees and adds one term the fan-out table does not
contain. Total processor time in the window, all phases, median of three:
21.59 core-seconds at ten threads against 19.04 at eight. The file input
and output bucket alone accounts for 6.72 against 4.72 core-seconds
(ranges 6.18 to 6.80 and 4.49 to 4.91, non-overlapping), a 42 % increase
in time spent inside the `open` syscall for the same 6932 files. This is
the mechanism behind section 4's finding that the file read and input set
phase runs slower at ten threads than at four.

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

The four buckets the brief names, as shares of total worker samples, now
all measurements rather than bounds:

| Bucket | 10 threads | 8 threads |
| --- | ---: | ---: |
| Allocator | 10.09 % | 11.77 % |
| Salsa memo access | 5.72 % | 7.97 % |
| Memo wait | 0.56 % | 0.50 % |
| Productive work | 23.96 % | 28.13 % |

Salsa lock contention (2.45 % and 1.18 %) is reported separately from
memo access rather than folded into it, because the two behave
differently with thread count. Productive work is a measurement, not the
upper bound the earlier draft of this section gave: file input and output
(19.09 % and 16.72 %) is now its own bucket rather than being counted
inside it, which matters because that bucket is almost entirely the
`open` syscall.

**No single bucket owns the missing cores.** The loss splits into two
mechanisms, both measured, neither dominant:

- The fan-out is 85.5 % utilised at ten threads against 90.6 % at eight
  (non-overlapping ranges). About 14 % of the phase's capacity is
  workers parked with no work, and that share grows by roughly five
  points between the two thread counts.
- The fan-out costs about 22 % more processor time at ten threads than at
  eight for identical work, 14.08 against 11.53 core-seconds
  (non-overlapping ranges), while its wall time does not improve.

The second mechanism is the larger of the two, and within it the largest
absolute term is productive work itself costing more (+1.02 core-seconds
of the +2.55), followed by the allocator (+0.64) and salsa's interning
lock (+0.56). The sharpest relative growth is salsa lock contention,
which triples.

Confidence, per claim:

- **High** that the fan-out utilisation figures and the fan-out cost
  figures are what they are measured to be. Both rest on non-overlapping
  ranges across three captures per thread count, on symbolicated frames,
  and on a reconciliation that never touches the unattributable parked
  share.
- **High** that salsa lock contention grows sharply with thread count and
  that it enters overwhelmingly through interning. The entry points are
  named frames and the ranges do not overlap.
- **High** that memo wait is not the problem: 0.56 % and 0.50 % of worker
  samples. The pattern the previous effort's diagnosis reported, many
  workers blocked behind one worker's in-progress work, is absent. Those
  samples are split between `persistable_diagnostics` and
  `served_typed_diagnostics` at ten threads, and at eight threads
  `class_surface_digest` appears as a third site; the earlier claim that
  they sat entirely inside one phase does not hold.
- **Medium** on why productive work itself costs 15 % more per unit of
  work at ten threads. Contention for shared cache and memory bandwidth
  across more active cores is the standard candidate and is consistent
  with the allocator growing alongside it, but nothing here measures the
  memory system, and no cause is asserted.
- **Low**, and therefore not claimed, on any statement about the size of
  the change in whole-run parked share between the two thread counts.
  The per-capture distributions overlap at three captures each.

For the campaign's decision between local optimization and architectural
rework, the profile points at neither cleanly. Salsa's interning lock is
a local target with a clear name and a contention curve that worsens with
width. But it is 0.84 of 14.08 core-seconds in the fan-out, so removing
it entirely would not recover the missing cores; the larger term is that
the same analysis work simply costs more when more threads run it, which
is not a target a lock-level fix reaches.
