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

| N | Observed cost (core-seconds) | Available (core-seconds) | Utilisation at the means | Full propagated range |
| --- | --- | --- | ---: | ---: |
| 10 | 13.84, 14.08, 14.16 | 16.52, 13.61, 16.48 | 90.6 % | 83.8 % to 104.1 % |
| 8 | 11.53, 11.85, 11.17 | 13.00, 12.72, 12.62 | 90.2 % | 85.9 % to 93.9 % |
| 1 | 5.38, 5.38, 5.61 | 5.29, 5.31, 5.38 | 101.0 % | 100.0 % to 106.0 % |

**The utilisation difference between eight and ten threads does not
survive its own uncertainty.** At the means the two are 90.6 % and
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
returns 101.0 % at the means with an upper excursion to 106.0 %. The
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
  90.6 % at ten threads and 90.2 % at eight at the means, but the
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
target whose cost triples between one and ten threads, but it is 0.84 of
the 8.49 core-seconds of expansion, so removing it entirely would move
the effective-core figure from about 3.5 to about 3.7. The decision the
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
component of every cold run, and section 3's measured walk phase
(366 ms) agrees with it.

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
configuration, discovery and teardown):

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
observed extremes in the least favourable direction (PHPStan's slowest
run, 33.741 s, over the floor's worst envelope, 1.526 s) still gives
22.1x; the most favourable combination (39.242 s over 1.223 s) gives
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
  read median (311 ms) sit alongside the real run's measured walk phase
  (366 ms) and read-and-set-inputs phase (591 ms, which additionally
  interns paths and creates salsa inputs).
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
