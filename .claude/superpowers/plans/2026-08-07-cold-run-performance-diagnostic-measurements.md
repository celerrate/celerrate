# Cold-Run Performance Diagnostic: Measurements

Spec: `.claude/superpowers/specs/2026-08-07-cold-run-performance-diagnostic-design.md`
Machine: the reference 10-core machine used by every published figure
Corpus: pinned PrestaShop comparison corpus
(`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`), equalized file set
Binary: `target/release/celerrate`, built at commit `2621b81`

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
Command: `rm -rf .celerrate` then
`../../release/celerrate check . --verbose > /dev/null`, three repetitions,
from the corpus directory
(`target/comparison-corpus/fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`)
Machine: otherwise idle for the whole run
Timing mechanism: `/usr/bin/time -p` wrapping each invocation; wall clock
read from its `real` line (resolution: hundredths of a second)

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 281 | 245 | 494 | 281 |
| file read + input set | 279 | 400 | 482 | 400 |
| analysis fan-out | 718 | 674 | 731 | 718 |
| suggest enrich | 364 | 356 | 365 | 364 |
| render report | 77 | 84 | 77 | 77 |
| persist: collect entries | 801 | 768 | 878 | 801 |
| persist: collect signatures | 33 | 30 | 31 | 31 |
| persist: pack writes | 95 | 87 | 89 | 89 |
| **Sum of the eight phases** | 2648 | 2644 | 3147 | 2761 |

Wall-clock total per run, as reported by `/usr/bin/time -p`'s `real` line
around the command (includes process startup/teardown and stderr
formatting outside the eight measured phases, same caveat as section 2
above):

| Run | Wall clock |
| --- | ---: |
| Run 1 | 3.26 s |
| Run 2 | 3.23 s |
| Run 3 | 3.77 s |
| **Median** | **3.26 s** |

Reconciliation, using the phase-sum median above and the fixed process
cost floor from section 2:

| Component | Median (ms) | Source |
| --- | ---: | --- |
| Eight-phase sum | 2761 | phase table above |
| Fixed process cost | 19.6 | section 2 |
| Unaccounted residue | 479.4 | wall (3260) minus sum (2761) minus fixed (19.6) |

The residue is 479 ms, about 14.7 % of the 3260 ms wall-clock median,
which is above the roughly 300 ms threshold that calls for a chase before
moving on.

Two candidates were checked. First, `--verbose` stderr formatting: three
further cold runs on the same corpus, same commit, with the flag dropped
(`rm -rf .celerrate` then `../../release/celerrate check . > /dev/null`),
gave wall clocks of 3.71 s, 3.62 s, 3.55 s (median 3.62 s), higher than the
3.26 s median measured with `--verbose`, not lower. Dropping the flag does
not shrink the wall clock, so stderr formatting from `--verbose` is not the
source of the residue; the 360 ms gap between the two medians runs the
wrong direction to be explained by formatting cost, and is consistent with
ordinary run-to-run variance instead. Second, cache-directory deletion:
`rm -rf .celerrate` runs before `/usr/bin/time -p` starts timing in every
repetition of both sets above, which the protocol already places outside
the timed region by construction; it is confirmed not to be a candidate
for this residue.

Neither candidate explains the gap. The eight phases are instrumentation
points inside `check`; they do not cover process startup before the first
phase begins or teardown after the last phase ends. Across the six cold
runs in this section, user time ran 13.36 s to 13.81 s and sys time ran
1.75 s to 4.95 s, both measured against a real time near a third of the
user figure, confirming the binary is heavily multi-threaded during the
cold run. Thread-pool setup, teardown, and scheduling variance are
plausible contributors that the eight phases, as currently instrumented,
cannot isolate. The residue is recorded as unattributed beyond ruling out
the two candidates above: 479 ms, about 14.7 % of the wall-clock median.
