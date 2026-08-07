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
