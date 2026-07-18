# Benchmark protocol

Every published Celerrate performance number comes from this protocol,
run on the hardware named below, and is reproducible by a third party
following this document. A number that would not survive third-party
scrutiny is not published.

## Corpus

- Repository: https://github.com/symfony/demo
- Commit: `03fe25671b720b15103a2ff26934e94c87bd4d82` (committed in `xtask/corpus.pin`)
- Vendor tree: installed from the corpus's own `composer.lock` via
  `composer install --no-interaction --no-progress --no-scripts
  --no-plugins --ignore-platform-reqs`
- Size: 9447 PHP files, 1302218 lines of PHP, vendor tree included -
  that is the tree `celerrate check` analyzes.

symfony/demo is the corpus because it has the exact shape
`celerrate check` is aimed at: a real user project, with application
code, a real `composer.json`, and the full Symfony vendor tree
installed from its lock file.

## Hardware and toolchain

- Machine: Apple M5, 10 cores (4 performance, 6 efficiency), 32 GiB memory, 1 TB NVMe SSD
- Operating system: macOS 26.5 (build 25F71)
- Rust toolchain: 1.94 (pinned in `rust-toolchain.toml`)
- Binary: `celerrate` built with `cargo build --release`, version
  `celerrate --version` reports at the commit the results name

## What is enabled

The measured binary is the default `celerrate check`, nothing disabled
and nothing added: the parse-level syntax diagnostics (CEL0002 to
CEL0017), the unknown-symbol families of the semantic core, the
version-availability family (CEL0021 to CEL0024), the three typed
families of the type engine (unknown members CEL0030 to CEL0033,
nullability CEL0034, argument types CEL0035 to CEL0038),
interprocedural type inference, the phpdoc bridge, the stdlib type
provider, inline suppressions, and the persistent cache including the
typed artifact classes. The numbers describe what a user runs.

## Method

The harness is `cargo xtask bench`. It fetches the corpus at the
pinned commit, installs the vendor tree, builds the release binary,
copies the corpus into a disposable working tree
(`target/bench/corpus`), and measures with [hyperfine](https://github.com/sharkdp/hyperfine),
which times the full process: startup, cache loading, analysis,
rendering. The in-place scripted edits are computed in Rust from the
pinned file contents and applied by copying variant files, so the
edits are pure functions of the corpus pin; a moved pin fails the
harness loudly.

- Aggregate: the median.
- hyperfine runs with `--ignore-failure`, because `celerrate check`
  exits 1 when it reports diagnostics - a completed analysis.
- "Cold" means no Celerrate cache (`.celerrate/` removed before every
  timed run); operating-system file caches are warm after the first
  run, and the protocol does not pretend otherwise.

## Scenarios

1. **Cold full** - 5 runs. Before each timed run: `rm -rf .celerrate`.
   Timed: `celerrate check .`. The complete analysis with nothing to
   reuse.
2. **Warm no-change** - 10 runs. The cache is primed once by a full
   run; nothing changes between runs. Timed: `celerrate check .`. The
   floor of the one-shot run.
3. **Warm one-edit** - 10 runs. Before each timed run: the edit target
   (`src/Controller/BlogController.php`) is restored, a full run
   primes the cache, then one comment line is appended. Timed:
   `celerrate check .`. A comment is trivia no annotation reader
   consumes: the body representation is unchanged, and this scenario
   demonstrates that cutoff - the floor of the edit path, not its
   cost.
4. **Warm body-edit** - 10 runs. Same restore-and-prime, then one
   statement inside `BlogController::search` changes (its expression
   is wrapped in `trim(...)`): the body changes, the signature does
   not. **This is the flagship number**: a full CLI run, wall clock,
   process startup and cache loading included, on the edit class a
   save-and-rerun user actually produces. Target: sub-second.
5. **Warm signature-edit** - 10 runs. Same restore-and-prime on
   `src/Entity/Post.php`, then `Post::getSlug(): ?string` becomes
   `: string`: one member signature changes, and its dependents
   (call sites in three other files) re-check. Target: sub-second.

On this corpus every application-code return is declared or annotated,
so a body edit cannot change a return type callers consume: the
declared-return firewall the type engine's design leans on is active
in scenario 4, and the per-scenario cache statistics under Results
document the residual (how many verdicts depend on inferred returns).

## Peak memory

`cargo xtask memory` analyzes the corpus cold, then warm, under
`/usr/bin/time`, and reports the peak resident set size. The cold
number is gated (`--ceiling`) against a budget of 1536 MiB,
reconducted from the semantic core's closure budget.

| Run | Peak RSS |
| --- | --- |
| Cold full | 709 MiB |
| Warm no-change | 342 MiB |

Within budget, no eviction configured.

## Substance

Precision gates alone cannot distinguish a precise engine from a
silent one. The published substance number is the residual `mixed`
rate on the corpus's expressions: 1059 of 4233 expressions
(25.0 %) infer to `mixed`, measured by the committed baseline behind
`cargo xtask mixed-rate`. At element level, 56 of
754 structural element positions (7.4 %) are
`mixed`: the whole-expression rate is blind to element-type sharpening
(`array<K, mixed>` to `array<K, Tag>`), so both rates are published
(issue #45). The seeded-defect suite
(`cargo test --package celerrate_cli --test seeded_defects`) is the
per-family recall gate: nine known defects, each reported.

## Reproduction

```sh
# prerequisites: rust 1.94, git, composer, php, hyperfine,
# and GNU time on Linux (macOS ships /usr/bin/time)
cargo xtask bench
cargo xtask memory
```

## What is not compared

No PHPStan (or other tool) comparison is published at v0.0.x: the
preview runs a handful of diagnostic families while PHPStan runs
hundreds of rules, and a cross-scope timing comparison would be
meaningless at best and misleading at worst. The matched-scope
comparison is the v0.1 claim.

## Results

Protocol run of 2026-07-18, at commit `a8382be` (recorded as the
median of three protocol runs; the raw hyperfine exports live under
`target/bench/` and are not committed):

| Scenario | Median |
| --- | --- |
| Cold full | 1.533 s |
| Warm no-change | 0.434 s |
| Warm one-edit | 0.460 s |
| Warm body-edit | 0.521 s |
| Warm signature-edit | 0.471 s |

The warm body-edit number is the published flagship; the README links
here.

Per-scenario cache statistics (one manual run each with
`CELERRATE_CACHE_STATS=1`, recorded verbatim):

```
cold full:            cache: trees 0 hit / 9341 miss; members 0 hit / 9341 miss; verdicts 0 served / 0 discarded / 46 absent; typed 217 bodies, edges 794 declared / 25 inferred / 7 provider, verdicts 0 served / 46 recomputed; persist 4 written / 0 skipped / 0 failed, 323ms
warm no-change:       cache: trees 9341 hit / 0 miss; members 9341 hit / 0 miss; verdicts 46 served / 0 discarded / 0 absent; typed 0 bodies, edges 0 declared / 0 inferred / 0 provider, verdicts 46 served / 0 recomputed; persist 0 written / 4 skipped / 0 failed, 37ms
warm one-edit:        cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 5 bodies, edges 23 declared / 0 inferred / 0 provider, verdicts 45 served / 1 recomputed; persist 4 written / 0 skipped / 0 failed, 63ms
warm body-edit:       cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 5 bodies, edges 24 declared / 0 inferred / 0 provider, verdicts 45 served / 1 recomputed; persist 4 written / 0 skipped / 0 failed, 62ms
warm signature-edit:  cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 65 bodies, edges 367 declared / 3 inferred / 1 provider, verdicts 37 served / 9 recomputed; persist 4 written / 0 skipped / 0 failed, 65ms
```

### Trajectory

The previous protocol run (2026-07-13, semantic core, commit
`24b6950`) recorded cold full 1.11 s, warm no-change 0.28 s, warm
one-edit 0.29 s with two diagnostic families and no inference. This
run records cold full 1.533 s with the full type engine enabled - a
1.4x change against the previous cold number. The parent
design's v0.1 ambition is roughly 20x faster than PHPStan at matched
scope; that comparison is deliberately not published here (see What is
not compared), and this cold number is the trajectory data point the
v0.1 measurement will be judged against.
