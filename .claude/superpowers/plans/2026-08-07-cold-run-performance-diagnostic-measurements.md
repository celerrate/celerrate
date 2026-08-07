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
Commit: `2621b81`
Corpus measured: the equalized corpus copy at
`target/comparison-corpus-equalized` (the same corpus as sections 1, 3
and 4).
Machine: otherwise idle for the whole session; one capture at a time,
nothing else running, no build during the session.
Profiler: `sample`, the macOS built-in sampling profiler, nominal
interval 1 millisecond, 4-second window. It attached 25 to 35
milliseconds after process launch in every capture, so the window opens
at the very start of the run and covers roughly the first 4 seconds of a
cold run whose wall clock is about 4.6 seconds at ten threads
(section 4).
Profiles written to `/tmp/celerrate-profiles/` (not committed).

### Capture protocol

Six captures, three at ten threads and three at the stagnation point
N = 8 that section 4 identified. Each capture is one cold run:

```
rm -rf .celerrate
env RAYON_NUM_THREADS=<N> /absolute/path/to/target/release/celerrate check . > /dev/null 2>&1 &
sample $! 4 -file /tmp/celerrate-profiles/cold-<N>t-run<K>.txt
wait
```

Run from the equalized corpus directory, binary by absolute path, with
`rm -rf .celerrate` outside the sampled region, as the Protocol section
requires. The same deviation section 4 records applies here: the brief's
snippet runs from the pinned corpus directory with a relative binary
path, which walks a smaller, non-comparable file set.

Sample counts per capture, which set the resolution of every percentage
below:

| Capture | Worker threads | Worker samples (total) | Samples per worker | Main-thread samples |
| --- | ---: | ---: | ---: | ---: |
| 10 threads, run 1 | 10 | 22120 | 2212 | 2512 |
| 10 threads, run 2 | 10 | 22220 | 2222 | 2566 |
| 10 threads, run 3 | 10 | 22380 | 2238 | 2551 |
| 8 threads, run 1 | 8 | 18528 | 2316 | 2630 |
| 8 threads, run 2 | 8 | 18896 | 2362 | 2695 |
| 8 threads, run 3 | 8 | 18240 | 2280 | 2623 |

The effective interval is about 1.5 milliseconds, not the nominal 1
millisecond: `sample` sleeps one millisecond of run time between samples
and pays its own collection cost on top, so a 4-second window yields
roughly 2500 main-thread samples rather than 4000. The worker threads
carry 250 to 400 fewer samples than the main thread in every capture,
because the pool's threads are created after the run starts, during the
filesystem walk. Percentages below are therefore shares of worker
lifetime inside the window, not shares of the whole process lifetime.

### Deviation: the released binary carries no symbols

The workspace release profile sets `strip = "symbols"`, so
`target/release/celerrate` and every copy of it under `target/release/deps`
contain only the 137 undefined symbols they import from the system
libraries. `sample` therefore resolves no frame belonging to Celerrate
itself: every in-binary frame is reported as `???  (in celerrate)  load
address 0x... + 0x<offset>`. Only frames in the system libraries
(`libsystem_kernel`, `libsystem_pthread`, `libsystem_malloc`,
`libsystem_platform`, `dyld`) carry names.

The campaign forbids rebuilding the binary, and a rebuild is the only way
to recover the names, so this section reports what a stripped profile can
establish and states plainly what it cannot. The consequence for the four
buckets the brief asks for:

- The allocator bucket is fully measurable: the allocator lives in
  `libsystem_malloc`, which is symbolicated.
- The wait buckets are measurable by wait site, because every blocking
  primitive bottoms out in a symbolicated `libsystem_kernel` or
  `libsystem_pthread` frame, and the distinct wait sites are
  distinguishable by the in-binary offsets immediately above them.
- The boundary between "salsa memo access" and "productive work" is not
  measurable. Both are pure in-binary execution with no system-library
  frame to name them. Only the part of memo access that is contended
  hard enough to yield the processor or park a thread becomes visible,
  through the lock implementation's calls into `libsystem_pthread`.

The unresolved share is quantified below as its own bucket rather than
being split by guesswork.

### Method: how stacks were bucketed

Every capture was parsed into its per-thread call tree, and each tree
node's self samples (its own count minus the sum of its children's
counts) were assigned to exactly one bucket by reading the whole stack
from thread start to leaf, not by matching the leaf name. The
per-capture bucket counts sum exactly to the capture's worker sample
total, which is the arithmetic check that no sample was double-counted
or dropped. Offsets are stable across captures because every capture ran
the same binary, which is how the same wait sites are recognisable in
all six profiles.

Every worker thread in every capture shares the same six-frame prefix
(`thread_start`, `_pthread_start`, then four in-binary frames ending at
`celerrate+0x3af4e8`), the thread pool's worker loop. The seventh frame
splits into two values: `celerrate+0x484664`, which goes on to park the
thread, and `celerrate+0x4846cc`, which goes on to execute a job. The
eighth frame, present only under the job-executing branch, takes one of a
small set of values, each of which is one parallel call site, that is,
one phase. This is what makes a per-phase attribution possible without
symbols.

The buckets, in the order they are tested:

**Contended lock.** Any stack containing a frame in the
`celerrate+0x486xxx` region. That region is a lock implementation:
its frames call `cthread_yield`, `_pthread_cond_wait` and
`pthread_cond_signal`, it is entered from exactly two in-binary call
sites (`celerrate+0x167ba8` and `celerrate+0x1610d0`), and it carries
self samples of its own, the spin before the yield. Testing this rule
first is what implements the brief's instruction that an allocation
performed beneath a lock acquisition belongs to the lock, not to the
allocator. Example stack, 82 samples in the ten-thread run 1:

    ... celerrate+0x230d50 <- celerrate+0x20c614 <- celerrate+0x167ba8
        <- celerrate+0x486724 <- cthread_yield <- swtch_pri

**Deep condition-variable wait.** Any stack containing
`celerrate+0x3b70b8`, a second, distinct caller of `_pthread_cond_wait`,
always reached through `celerrate+0x39e200` and `celerrate+0x3aef0c` and
always at a stack depth above 50, that is, from deep inside application
work rather than from the worker loop. This is the site that would carry
the "workers blocked behind one worker's in-progress work" pattern.
Example stack, 7 samples, depth 54:

    ... 40 in-binary frames ... <- celerrate+0x39e200
        <- celerrate+0x3aef0c <- celerrate+0x3b70b8
        <- _pthread_cond_wait <- __psynch_cvwait

**Idle park.** Leaf `__psynch_cvwait`, reached through
`celerrate+0x484664` then `celerrate+0x484b5c` directly from the worker
loop, at depth 10 with no job frame in between. The worker is parked
because there is no job to run. Example stack, 784 samples:

    thread_start <- _pthread_start <- celerrate+0x3ee360
        <- celerrate+0x3b4d78 <- celerrate+0x3b3c9c <- celerrate+0x3af4e8
        <- celerrate+0x484664 <- celerrate+0x484b5c
        <- _pthread_cond_wait <- __psynch_cvwait

**Latch wait.** Leaf `__psynch_cvwait` through the same
`celerrate+0x484b5c` wait site, but with the job-executing frame
`celerrate+0x4846cc` present, so the worker is inside a job and blocked
at a split point of the parallel iterator waiting for its other half.
These stacks show the recursive splitting triple
(`celerrate+0x38188`, `celerrate+0x6a66c`, `celerrate+0x69ce4`) repeated
once per split level, at depths 15, 18, 20, 22 and 25. Example stack,
333 samples, depth 15:

    ... celerrate+0x4846cc <- celerrate+0x7313c <- celerrate+0x38188
        <- celerrate+0x6a66c <- celerrate+0x69ce4 <- celerrate+0x484664
        <- celerrate+0x484b5c <- _pthread_cond_wait <- __psynch_cvwait

**Allocator.** Leaf in `libsystem_malloc`, with no lock frame anywhere in
the stack. On this operating system version that is the `xzm` allocator
family. Example stack, 42 samples:

    ... celerrate+0x2e0560 <- celerrate+0x2e016c <- celerrate+0x3deb68
        <- celerrate+0x3dd950 <- celerrate+0x3de39c <- _xzm_free

**File input and output.** Leaf in `libsystem_kernel` naming a file
operation (`open`, `read`, `close`, `fstat`, `stat`, `getdirentries`,
and the rest of that family). Example stack, 99 samples, depth 25:

    ... celerrate+0x4846cc <- celerrate+0x72f24 <- (five split levels)
        <- celerrate+0x99acc <- celerrate+0xc8c44 <- celerrate+0x3e8818
        <- celerrate+0x3eb7f0 <- open <- __open

**Running in the binary (unresolved).** Everything else: self samples in
unnamed Celerrate frames, plus the `libsystem_platform` memory helpers
(`_platform_memcmp`, `_platform_memmove`, `_platform_memset`) that
in-binary code calls directly. This is the bucket that mixes productive
analysis work with uncontended memo access, and that the stripped binary
cannot split.

The check that each wait bucket is pure: in the ten-thread run 1, every
sample in idle park, latch wait and deep condition-variable wait has
`__psynch_cvwait` as its leaf, and the contended-lock bucket's leaves are
`swtch_pri` (468), `__psynch_cvwait` (80), in-binary spin frames (25) and
`__psynch_cvsignal` (4).

### Worker-thread buckets, ten threads

Raw self-sample counts, with each capture's share of that capture's total
worker samples in parentheses.

| Bucket | Run 1 | Run 2 | Run 3 | Median share |
| --- | ---: | ---: | ---: | ---: |
| Idle park (no job available) | 4664 (21.08 %) | 5720 (25.74 %) | 5632 (25.17 %) | 25.17 % |
| Running in the binary (unresolved) | 7704 (34.83 %) | 7179 (32.31 %) | 6830 (30.52 %) | 32.31 % |
| File input and output syscalls | 4258 (19.25 %) | 4537 (20.42 %) | 4437 (19.83 %) | 19.83 % |
| Latch wait (blocked inside a job) | 2621 (11.85 %) | 2166 (9.75 %) | 2962 (13.24 %) | 11.85 % |
| Allocator | 2198 (9.94 %) | 1854 (8.34 %) | 1826 (8.16 %) | 8.34 % |
| Contended lock | 577 (2.61 %) | 639 (2.88 %) | 617 (2.76 %) | 2.76 % |
| Deep condition-variable wait | 98 (0.44 %) | 125 (0.56 %) | 76 (0.34 %) | 0.44 % |
| **Total worker samples** | **22120** | **22220** | **22380** | |

### Worker-thread buckets, eight threads (the stagnation point)

| Bucket | Run 1 | Run 2 | Run 3 | Median share |
| --- | ---: | ---: | ---: | ---: |
| Idle park (no job available) | 3598 (19.42 %) | 5051 (26.73 %) | 3848 (21.10 %) | 21.10 % |
| Running in the binary (unresolved) | 6979 (37.67 %) | 6736 (35.65 %) | 7147 (39.18 %) | 37.67 % |
| File input and output syscalls | 3300 (17.81 %) | 3017 (15.97 %) | 3301 (18.10 %) | 17.81 % |
| Latch wait (blocked inside a job) | 2316 (12.50 %) | 1938 (10.26 %) | 1606 (8.80 %) | 10.26 % |
| Allocator | 1950 (10.52 %) | 1847 (9.77 %) | 1995 (10.94 %) | 10.52 % |
| Contended lock | 263 (1.42 %) | 240 (1.27 %) | 248 (1.36 %) | 1.36 % |
| Deep condition-variable wait | 122 (0.66 %) | 67 (0.35 %) | 95 (0.52 %) | 0.52 % |
| **Total worker samples** | **18528** | **18896** | **18240** | |

Each capture's counts sum exactly to its total. The medians are taken
per bucket across the three captures, so they do not sum to exactly
100 % (100.70 % at ten threads, 99.24 % at eight); the per-capture
columns are the exact accounting.

### The main thread, counted separately

| Bucket | Median share, 10 threads | Median share, 8 threads |
| --- | ---: | ---: |
| Waiting on the thread pool | 73.97 % | 72.02 % |
| Running in the binary (unresolved) | 14.22 % | 16.58 % |
| File input and output syscalls | 10.35 % | 10.65 % |
| Allocator | 0.64 % | 0.88 % |

The main thread blocks at its own wait site (`celerrate+0x3b4540`
calling `_pthread_cond_wait`, distinct from all three worker wait sites)
for roughly three quarters of the window, which is the expected shape:
it hands each phase to the pool and waits. Its file input and output
share is the filesystem walk, which runs before the pool starts. It is
excluded from every worker percentage above, as the brief requires.

### Which phase each bucket belongs to

The eighth frame identifies the parallel call site. Three of the call
sites are large enough to name, and one supplementary capture was taken
to name them: a single extra cold run at ten threads sampled twice in
sequence, a first 2-second window and then a second one, so that early
phases and late phases could be told apart by which call sites appear in
which window (`/tmp/celerrate-profiles/phase-split-early.txt` and
`phase-split-late.txt`). This capture is diagnostic only and contributes
no figure to the tables above.

- `celerrate+0x72f24` is the file read and input set phase. It is
  present only in the early window, and 99.7 % of its samples are the
  `open` syscall.
- `celerrate+0x7313c` is the analysis fan-out. It dominates the early
  window (40.4 %) and still holds 11.9 % of the late window, the tail of
  the phase.
- `celerrate+0x73fe0` is the persist collect-entries phase. It is absent
  from the early window and takes 48.3 % of the late window.

Median shares of total worker samples, and the bucket composition inside
each call site:

| Call site | Share of worker samples, 10 threads | Share, 8 threads |
| --- | ---: | ---: |
| Analysis fan-out | 25.43 % | 25.14 % |
| Persist: collect entries | 24.22 % | 23.13 % |
| File read and input set | 17.90 % | 15.64 % |

**Inside the analysis fan-out** (median across the three captures at each
thread count, as a share of that call site's own samples):

| Bucket | 10 threads | 8 threads |
| --- | ---: | ---: |
| Running in the binary (unresolved) | 60.40 % | 61.99 % |
| Latch wait (blocked inside a job) | 28.67 % | 27.68 % |
| Allocator | 10.88 % | 10.41 % |
| Contended lock | 0.12 % | 0.02 % |

**Inside persist: collect entries:**

| Bucket | 10 threads | 8 threads |
| --- | ---: | ---: |
| Running in the binary (unresolved) | 63.62 % | 68.49 % |
| Allocator | 21.62 % | 25.01 % |
| Contended lock | 9.31 % | 4.41 % |
| Deep condition-variable wait | 1.62 % | 1.92 % |

**Inside the file read and input set phase:** 99.70 % of its samples at
ten threads and 99.72 % at eight are the `open` syscall itself, with the
allocator and everything else below half a percent.

### The `stub_*` self-time check

Not performable on this binary, and recorded as such rather than
answered. The brief asks for self time in `stub_symbol_table`,
`stub_frontier` and `stub_signature_table`
(`crates/celerrate_semantics/src/index.rs`). Those are Celerrate
functions, and the stripped binary resolves no Celerrate function name,
so no profile in this campaign can confirm or deny their presence by
name. The previous record found no self time in them; this campaign
neither confirms nor contradicts that. Answering it requires a build
without `strip = "symbols"`, which this campaign's constraints exclude.

What can be said in its place, from the same profiles, is that the
analysis fan-out has no single dominant unresolved hot spot: in the
ten-thread run 1 the largest unnamed in-binary self-time frame inside
the fan-out holds 2.49 % of the phase's running samples, and the next
three hold 2.37 %, 1.36 % and 1.05 %. A shared table serialising the
fan-out would be expected to concentrate self time rather than spread it
this thinly, but this is an argument from shape, not a name match, and
it does not close the question.

The one named frame that is concentrated is `_platform_memcmp`, the
system byte-comparison routine, which in-binary code calls directly. It
holds a median 5.03 % of all worker samples at ten threads and 5.41 % at
eight, and a median 13.36 % and 13.77 % of the analysis fan-out's own
samples. Two thirds of it arrives through a single call chain
(`celerrate+0x3878fc` to `celerrate+0x3dcc14` to `celerrate+0x3bcdbc`),
the shape of hash-table key equality on byte-string keys. Which table
that is cannot be established without symbols.

### Reading: which bucket owns the missing cores

Mapping the measurements onto the four buckets the brief names, at ten
threads and at the stagnation point N = 8, as shares of total worker
samples:

| Bucket as the brief names it | 10 threads | 8 threads | What was actually measured |
| --- | ---: | ---: | --- |
| Allocator | 8.34 % | 10.52 % | Leaf in `libsystem_malloc`, no lock frame above it. Complete. |
| Salsa memo access | 2.76 % | 1.36 % | Only the contended part, the lock region that spins, yields and parks. The uncontended part is inside the unresolved bucket. |
| Memo wait | 0.44 % | 0.52 % | The deep condition-variable wait site. |
| Productive work | at most 52.14 % | at most 55.48 % | Unresolved in-binary running (32.31 % / 37.67 %) plus file input and output syscalls (19.83 % / 17.81 %). An upper bound: it still contains uncontended memo access. |

The unassignable share is the unresolved running bucket: 32.31 % of
worker samples at ten threads and 37.67 % at eight cannot be split
between memo access and productive analysis work. That is the honest
size of what the stripped binary hides.

**None of the four buckets owns the missing cores.** The bucket that
does is the one the brief's taxonomy does not contain: workers parked
with no work to run.

| | 10 threads | 8 threads | Change |
| --- | ---: | ---: | ---: |
| Idle park (no job available) | 25.17 % | 21.10 % | +4.07 points |
| Latch wait (blocked inside a job) | 11.85 % | 10.26 % | +1.59 points |
| **Parked, total** | **37.02 %** | **31.36 %** | **+5.66 points** |
| Running in the binary | 32.31 % | 37.67 % | -5.36 points |

At ten threads, roughly 37 % of worker capacity inside the sampling
window is parked, against roughly 31 % at eight. The two threads added
between the stagnation point and full width bought parked time, not
work: the parked share rises by 5.66 points and the running share falls
by 5.36 points, an almost exact trade. This is what a fan-out reaching
3.83 effective cores out of ten (section 4) looks like from the inside.

Confidence in that reading is high, and it rests on a discriminator
inside the fan-out rather than on the aggregate alone. Inside the
analysis fan-out call site, contended-lock samples are 0.12 % at ten
threads and 0.02 % at eight, effectively zero, while 28.67 % of the
phase's own samples are workers blocked at a split point of the parallel
iterator. A phase losing its cores to lock contention would show the
opposite. The fan-out is starved of stealable work, not blocked on a
shared structure.

The confidence attaches to the shape, not to the cause of the
starvation. The profile establishes that workers have no work; it does
not establish why, because the frames that would name the work
distribution belong to Celerrate and are stripped. Two candidates are
consistent with these profiles and neither is decided here: a parallel
iterator whose splitting granularity leaves the tail of each phase
running on too few items, and a dependency structure in which later work
cannot start until earlier work finishes. The supplementary early and
late windows favour the first, since idle park is 10.34 % of the early
window and 31.68 % of the late one, so the parking concentrates in phase
tails rather than being spread evenly.

Three secondary findings, all measured:

- **Contention does grow with threads, but in the persist phase, not the
  fan-out.** The contended-lock share inside persist collect-entries
  roughly doubles from 4.41 % at eight threads to 9.31 % at ten, and 86 %
  of all contended-lock samples in the ten-thread run 1 sit in that call
  site (499 of 577). The spec's prediction that contention grows with
  thread count holds; it just does not hold where the fan-out is.
- **The file read phase is a syscall wall.** Its workers spend 99.7 % of
  their samples in `open`. That is the mechanism behind section 4's
  finding that the file read and input set phase runs slower at ten
  threads than at four: concurrent `open` calls serialise in the kernel,
  so adding threads adds contention on a path that was never
  parallel-friendly.
- **The pattern the previous effort's diagnosis reported is absent.**
  The signature of many workers blocked behind one worker's in-progress
  sequential work is the deep condition-variable wait site, and it holds
  0.44 % of worker samples at ten threads and 0.52 % at eight, entirely
  inside the persist phase and not the fan-out. Whatever the fan-out's
  problem is, it is not that.

Confidence, stated per claim: high that the parked share is what it is
measured to be, and high that the fan-out is starvation-bound rather
than contention-bound, since both rest on symbolicated system frames and
on wait sites that reproduce identically across all six captures.
Medium that the contended-lock region is the lock implementation it
appears to be, since that rests on reading its behaviour (it spins,
yields, parks, signals) rather than on its name. Low on any claim tying
a specific bucket to a specific Celerrate component, including whether
the contended lock belongs to memo access at all; those claims need
symbols this binary does not have.
