# Check Pipeline Performance: Equalized Baseline Measurements

Date: 2026-08-03
Machine: reference machine used for this work, 10 cores
Corpus: pinned PrestaShop comparison corpus
(`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`), equalized file set
(`celerrate.toml` with `[project] include = ["."]`, 6932 files reported
by both Celerrate and an independent filesystem count)

All figures below were measured on this run of `cargo xtask benchmark`
and three immediately following instrumented `check` invocations. None
of the numbers are estimated, extrapolated, or carried over from the
design document or issue #124.

## 1. Official protocol medians (`cargo xtask benchmark`)

| Scenario | Median |
| --- | --- |
| PHPStan cold | 44.165 s |
| Celerrate cold | 14.087 s |
| Cold ratio | 3.1x |

Method, read from `xtask/src/benchmark.rs`: hyperfine runs one untimed
warmup plus 5 timed runs per scenario, with `--prepare "rm -rf
.celerrate"` executed before the warmup and before every timed run (so
the warmup is not a loophole in coldness). The timed command is plain
`celerrate check .`, without `--verbose`. Full hyperfine detail:

- PHPStan: mean 44.630 s ± 5.880 s, range 38.996 s – 50.729 s (3 runs).
- Celerrate: mean 14.060 s ± 1.407 s, range 12.589 s – 16.205 s (5 runs).

Both processes exited non-zero (expected: the corpus has diagnostics to
report); hyperfine's non-zero exit warning was ignored as it is for
every corpus run.

## 2. Fresh per-phase profile (three instrumented cold runs)

Protocol: from `target/benchmark/corpus`, `rm -rf .celerrate` then
`../../release/celerrate check . --verbose > /dev/null`, stderr captured
separately per run. Each run printed exactly the eight expected phase
lines. Values in milliseconds, as printed.

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 408 | 398 | 437 | 408 |
| file read + input set | 456 | 2377 | 2121 | 2121 |
| analysis fan-out | 5265 | 6199 | 4801 | 5265 |
| suggest enrich | 2834 | 3319 | 2495 | 2834 |
| render report | 198 | 317 | 207 | 207 |
| persist: collect entries | 6265 | 5978 | 4563 | 5978 |
| persist: collect signatures | 49 | 43 | 56 | 49 |
| persist: pack writes | 188 | 186 | 151 | 186 |
| **Sum of the eight phases** | 15663 | 18817 | 14831 | 17048 |

Note on the totals row: these three instrumented runs (with `--verbose`,
one untimed sample each) ran 15.663 s, 18.817 s, and 14.831 s
respectively, somewhat higher and more variable than the hyperfine
median of 14.087 s over 5 timed runs. The instrumented runs did not have
a discarded warmup before them the way the hyperfine protocol does, and
`--verbose` adds stderr formatting work outside the timed phases
themselves; the hyperfine figure in section 1 remains the authoritative
wall-clock number, this section exists to locate where the time goes.

Ranked by median, largest first:

1. `persist: collect entries` — 5978 ms
2. `analysis fan-out` — 5265 ms
3. `suggest enrich` — 2834 ms
4. `file read + input set` — 2121 ms
5. `filesystem walk` — 408 ms
6. `render report` — 207 ms
7. `persist: pack writes` — 186 ms
8. `persist: collect signatures` — 49 ms

## 3. What the three planned levers attack, and the equalized budget

The three planned levers, in their stated order, and the phase each
attacks:

1. Remove allocation churn in `suggest::enrich` → `suggest enrich`
   (measured median 2834 ms, 16.6 % of the phase sum, about 20.1 % of
   the hyperfine cold median).
2. Parallelize persist entry collection → `persist: collect entries`
   (measured median 5978 ms, 35.1 % of the phase sum, about 42.4 % of
   the hyperfine cold median).
3. Parallelize the walk's file reads → `file read + input set`
   (measured median 2121 ms, 12.4 % of the phase sum, about 15.1 % of
   the hyperfine cold median).

Combined, the three targeted phases account for 10933 ms of the 17048 ms
phase sum (64.1 %). `analysis fan-out` (5265 ms, 30.9 % of the phase
sum) is not attacked by any of the three planned levers; it is carried
in the design as a reserve lever, built only if the 6 s target is not
met by the other three.

This table is the working budget from here forward. It replaces the
issue #124 table (61.7 % `suggest::enrich`, 18.5 % persist collect
entries, 5.9 % file read + input set, measured before file-set
equalization, 20.89 s total) as the reference every later
re-measurement is compared against.

## 4. Sanity check against the design's assumption

The design document (`.claude/superpowers/specs/2026-08-03-check-pipeline-performance-design.md`,
section 1) assumed, from the pre-equalization issue #124 profile, that
`suggest::enrich` was the dominant cost at 61.7 % of the wall clock,
which is why it is the first lever in the stated order.

On this fresh, equalized profile that assumption no longer holds:

- `suggest::enrich` is still one of the phases with a measurable cost
  (2834 ms), so it has not disappeared as a target. But it has dropped
  from being the dominant phase (61.7 % pre-equalization) to the
  **third**-largest of the eight phases, at roughly 20 % of the cold
  median, behind both `persist: collect entries` (5978 ms, now the
  largest phase) and `analysis fan-out` (5265 ms, now the second-
  largest phase and one that no planned lever touches).
- The stated lever order (enrich, then persist, then read) attacks the
  now-third-largest phase first and the now-largest phase second, while
  leaving the second-largest phase (`analysis fan-out`) as a reserve
  that is only built if the other three fall short.

This is the kind of contradiction the plan anticipated needing a stop
for: the lever most worth landing first, by measured cost on the
equalized corpus, is persist entry collection, not `suggest::enrich`.
Whether the eventual order changes is a decision for the next
implementation step to make explicitly, informed by this data; this
document does not itself reorder the levers or write any production
code.

**Conclusion: STOP before implementing further levers. Report status
DONE_WITH_CONCERNS.** The fresh measurement contradicts the design's
premise that `suggest::enrich` is the dominant cost driving the lever
order. The next step should decide, with this table in hand, whether to
reorder the levers (persist entry collection first, since it is now the
largest of the three attacked phases) and whether `analysis fan-out`
(now essentially tied with persist entry collection and untouched by
any planned lever) should be promoted from a reserve lever to a primary
one before further work proceeds.
