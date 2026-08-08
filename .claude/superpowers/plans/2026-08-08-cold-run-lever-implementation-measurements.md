# Cold-Run Lever Implementation: Measurements

Machine: the reference 10-core machine behind every published figure.
Corpus: pinned PrestaShop comparison corpus
`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`, equalized file set (6932
first-party files, root `celerrate.toml` with `include = ["."]`).
Measurement base: `target/comparison-corpus-equalized`.

## Protocol

- Cold run: `rm -rf .celerrate` in the corpus directory, then
  `<binary> check .` from the corpus directory, binary named by
  absolute path.
- Every A/B is measured in one session, sides alternated
  (base, lever, base, lever, ...), machine otherwise idle, three
  repetitions minimum per side, medians reported, wall clock read
  from `/usr/bin/time -p`'s `real` line.
- Each session opens and closes with a control: three cold runs of
  the session's base binary, median recorded. Control drift above
  ~10 % invalidates the session's comparisons.
- Acceptance per lever: median gain positive and larger than each
  side's spread (max minus min); `cargo xtask corpus` and
  `cargo xtask mixed-rate` byte-identical; the full local gate suite
  green. A lever that fails is reverted and its measurement kept.

## 1. Session anchor (2026-08-08, commit 0222695)

`cargo xtask benchmark`, machine otherwise idle.

| Quantity | Value |
| --- | --- |
| PHPStan cold median | 36.468s |
| Celerrate cold median | 4.889s |
| Cold ratio | 7.5x |
| File-count cross-check | 6932 reported / 6932 counted |

The medians above come from the benchmark's own `scenario / median`
table. For reference, the same run's hyperfine mean and range lines
report PHPStan at 35.357s mean (32.706s to 36.896s over 3 runs) and
Celerrate at 4.898s mean (4.803s to 5.042s over 5 runs).
