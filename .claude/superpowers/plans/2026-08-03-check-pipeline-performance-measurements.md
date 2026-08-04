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

# Persist Entry Collection Lever: Cold and Warm Measurements

Date: 2026-08-04
Machine: reference machine used for this work, 10 cores
Corpus: pinned PrestaShop comparison corpus, equalized file set (see
section 1 above for method)

Lever measured: commit `d9841e4` (`⚡️ perf(cli): parallelise persist
entry collection`), which rewrites `collect_entries` in
`crates/celerrate_cli/src/cache/mod.rs` with rayon using the salsa
handle-clone pattern.

Sequencing note: the written plan (section 3 above) lists the
`suggest::enrich` lever first and the persist entry collection lever
second. The order was revisited by the measured cost recorded in
section 4 above: persist entry collection was the largest of the three
attacked phases on the equalized profile, so it was implemented first.
**This is the only optimisation landed on the branch at the time of
this measurement. The `suggest::enrich` lever has not been implemented
yet**; the `suggest enrich` figures below are unchanged by any lever
work and are reported only for context.

## 5. Gate results

- `cargo test --workspace`: exit 0, 111 test binaries, 2423 passed, 0
  failed.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0, no
  warnings.
- `cargo xtask fetch-corpus`: snapshot already present at
  `target/corpus/03fe25671b720b15103a2ff26934e94c87bd4d82`.
- `cargo xtask corpus`: exit 0, "the corpus report matches the committed
  snapshot".
- `cargo xtask mixed-rate`: exit 0, "the mixed-rate report matches the
  committed baseline".

Nothing was blessed. All gates pass unmodified, consistent with a
behavior-identical change.

## 6. Cold per-phase profile (three instrumented cold runs)

Protocol: same as section 2 above, `rm -rf .celerrate` then
`../../release/celerrate check . --verbose > /dev/null`, wall clock
captured from the shell's own timing report around the command. Values
in milliseconds, as printed.

| Phase | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| filesystem walk | 377 | 358 | 373 | 373 |
| file read + input set | 860 | 855 | 867 | 860 |
| analysis fan-out | 3424 | 3419 | 3473 | 3424 |
| suggest enrich | 2084 | 2165 | 2210 | 2165 |
| render report | 165 | 169 | 181 | 169 |
| persist: collect entries | 964 | 875 | 855 | 875 |
| persist: collect signatures | 39 | 42 | 41 | 41 |
| persist: pack writes | 145 | 137 | 154 | 145 |
| **Sum of the eight phases** | 8058 | 8020 | 8154 | 8052 |

Wall-clock total per run, as reported by the shell around the command
(includes process startup/teardown and stderr formatting outside the
eight measured phases, same caveat as section 2 above):

| Run | Wall clock |
| --- | ---: |
| Run 1 | 8.858 s |
| Run 2 | 8.825 s |
| Run 3 | 8.863 s |
| **Median** | **8.858 s** |

## 7. `persist: collect entries`: before and after

| | Baseline (section 2, three cold runs) | This measurement (three cold runs) |
| --- | ---: | ---: |
| Run values (ms) | 6265, 5978, 4563 | 964, 875, 855 |
| Median (ms) | 5978 | 875 |

The lever cuts the median of its own phase from 5978 ms to 875 ms, a
reduction of 5103 ms (about 85.4 %).

## 8. Reading the cold total: what this lever demonstrably did, and what it did not

Compared against the baseline's phase-sum median of 17048 ms (section
2), the new cold phase-sum median is 8052 ms. Two other phases this
lever does not touch also dropped sharply between the baseline and this
measurement:

- `analysis fan-out`: baseline median 5265 ms → now 3424 ms.
- `file read + input set`: baseline median 2121 ms → now 860 ms.

The persist lever changes only `collect_entries`; it has no code path
through analysis or file reading, so it cannot explain those drops. The
baseline runs in section 2 were taken immediately after a full `cargo
xtask benchmark` invocation (which itself runs PHPStan and a Composer
install) and already showed wide run-to-run variance in exactly these
two phases — `file read + input set` ran 456 / 2377 / 2121 ms across its
three baseline runs, a more than 5x spread on a phase that does the same
work every time. The honest reading is that the baseline was captured on
a machine that was not idle, and it overstates the whole-pipeline
improvement measured here. What this lever demonstrably did, isolated
from that noise, is move its own phase: `persist: collect entries` from
a median of 5978 ms to 875 ms. The non-persist phases reported in this
section are included for completeness but are not a trustworthy
comparison point against the baseline; the authoritative before/after
for the total will be the hyperfine protocol runs at the end of this
effort, run under equally idle conditions on both sides.

## 9. Is the cold total at or under 6 seconds?

No. The cold wall-clock median measured here is 8.858 s, above the 6 s
target. Only one of the three planned levers has landed; `suggest
enrich` (2165 ms median) and `analysis fan-out` (3424 ms median, not
attacked by any planned lever) remain unaddressed on this branch. The
target is not yet met.

## 10. Warm run: guarding against a no-change persist regression

The plan's stated purpose for a warm run is to guard the risk that the
per-file salsa handle clones introduced by this lever make a no-change
persist slower.

### 10.1 Discarded outlier

The first warm run recorded in `/tmp/instrumented.log` reports:

```
../../release/celerrate check . --verbose > /dev/null  13.53s user 1.12s system 1% cpu 16:08.55 total
```

16 minutes 8.55 seconds of wall clock at 1 % CPU, while its eight phases
sum to 7224 ms (about 7.2 s). That leaves roughly 15 minutes of wall
clock unaccounted for by any measured phase, with the process reporting
essentially no CPU activity during it — consistent with the process
being blocked outside computation, not the lever doing extra work. This
run started immediately after a cold run had just written the entire
`.celerrate` cache, which on this platform triggers filesystem indexing;
that is the most plausible external cause. It did not reproduce: the two
warm runs in `/tmp/warm-repeat.log`, run under the same protocol
immediately after, completed in 6.899 s and 6.798 s with normal CPU
utilisation (204 % and 211 %). This outlier is recorded here and
excluded from the warm figures below; it is not treated as a finding
against the lever.

### 10.2 Reproducing warm runs

| Phase | Warm A | Warm B |
| --- | ---: | ---: |
| filesystem walk | 649 | 877 |
| file read + input set | 876 | 402 |
| analysis fan-out | 1070 | 1138 |
| suggest enrich | 2094 | 2172 |
| render report | 168 | 177 |
| persist: collect entries | 24 | 23 |
| persist: collect signatures | 1267 | 1276 |
| persist: pack writes | 25 | 25 |
| Wall clock | 6.899 s | 6.798 s |

`persist: collect entries` on a no-change warm run is 24 ms and 23 ms.
That is far below the cold figures (875 ms median) and shows no sign of
the per-file salsa handle clones costing extra time on a no-change
persist; the guard the warm run was designed for holds.

As an observation, not a finding against this lever: `persist: collect
signatures`, a phase this change does not touch, is markedly higher warm
(1267 ms, 1276 ms) than cold (41 ms median, section 6 above). Whatever
causes that difference is unrelated to the entry-collection rewrite
measured here.
