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
