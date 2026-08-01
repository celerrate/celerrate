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
- PHP: 8.5.0 (cli, NTS, Homebrew build) - the interpreter the
  comparison below runs PHPStan under
- OPcache: off on the command line. `opcache.enable_cli` reads `0`,
  which is PHP's own default there, so PHPStan is measured the way a
  user gets it out of the box rather than under a tuned interpreter.

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
| Cold full | 702 MiB |
| Warm no-change | 351 MiB |

Within budget, no eviction configured.

## The comparison

`cargo xtask benchmark` measures PHPStan and Celerrate cold, in the
same run, on the same corpus working tree
(`target/benchmark/corpus`), and divides the two medians. Everything
the ratio depends on is pinned in the repository, so a third party can
rerun the harness and land on the same setup.

- **PHPStan version**: 2.2.7, installed from the committed
  `benchmarks/phpstan/composer.lock` and run under the PHP named
  above.
- **Rule level**: 5. It is the closest match to the families the
  measured Celerrate binary enables: unknown symbols, unknown members,
  and argument checks. The match is not exact, and the residual is
  disclosed rather than hidden. Celerrate additionally runs its
  nullability family (CEL0034) and its version-availability family
  (CEL0021 to CEL0024); PHPStan at level 5 runs rules Celerrate has no
  equivalent for. Neither tool's rule set is a subset of the other's,
  and no level makes it one.
- **Result cache**: off, on both sides. PHPStan's `tmpDir` is
  `target/benchmark/phpstan-tmp`, outside the analyzed tree, and it is
  removed before every timed run; Celerrate's `.celerrate` is removed
  before every timed run the same way. Both tools start from nothing
  on every measured run, and nothing either tool writes enters the
  tree the other analyzes.
- **Parallelism**: both defaults, neither touched. PHPStan
  auto-detects its worker count and the generated configuration
  overrides no `parallel` parameter; Celerrate uses its default thread
  pool, one worker per logical core (10 on this machine).
- **Analyzed paths**: matched scope. The generated configuration sets
  three parameters and nothing else - `level`, `paths`, `tmpDir` - and
  `paths` is the corpus working tree root, which is exactly the file
  set `celerrate check .` walks: 9447 PHP files, 1302218 lines, vendor
  tree included. An earlier version of this harness pointed PHPStan at
  the corpus's `src/` alone while Celerrate walked the whole tree.
  That is not a comparison, and it has been corrected: the two tools
  are given the same work.
- **Memory limit**: `4G`, passed to PHPStan on the command line. The
  matched scope needs the headroom, and passing it explicitly keeps
  the run from depending on the machine's `php.ini`.
- **Invocation**: `php benchmarks/phpstan/vendor/bin/phpstan analyse
  --configuration <generated> --no-progress --memory-limit 4G` against
  `celerrate check .`, both timed by hyperfine, both in the same
  working tree. `--ignore-failure` here too: both tools exit 1 when
  they report findings, which is a completed analysis.
- **Runs and aggregate**: 3 cold PHPStan runs, 5 cold Celerrate runs,
  the median of each, and the ratio of the two medians.

### Where the ratio narrows

The ratio is a property of the input, not a constant. On a small tree
the wall clock is dominated by PHP's interpreter startup and
bootstrapping rather than by analysis, and the gap closes. Measured on
this machine with the same two binaries, the corpus's `src/` alone (34
files, 3424 lines) takes PHPStan about 2.1 s against Celerrate's about
0.17 s: roughly 12x, against the 35.9x the full tree gives below. The
published ratio is the full-tree one, because the full tree is what
`celerrate check .` analyzes and what a user waits on, but a user
checking a handful of files should expect the smaller number.

## Substance

Precision gates alone cannot distinguish a precise engine from a
silent one. The published substance number is the residual `mixed`
rate on the corpus's expressions: 1046 of 4233 expressions
(24.7 %) infer to `mixed`, measured by the committed baseline behind
`cargo xtask mixed-rate`. At element level, 56 of
758 structural element positions (7.4 %) are
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
cargo xtask benchmark   # installs the pinned PHPStan itself
```

## The gate in CI

The earlier position - no tool comparison published, on the ground
that a cross-scope timing would be misleading - is superseded by the
protocol above: the comparison is now measured at matched scope and
published with its asymmetries stated.

What CI can hold is narrower than what this document reports. A shared
runner cannot hold an absolute wall clock: its cores, its neighbours,
and its storage vary from job to job. So the gate is the same-machine
ratio, `cargo xtask benchmark --gate`, with a floor of 20x. Both tools
run on whatever runner the job landed on, inside that one job, and the
ratio survives a slow runner because a slow runner slows both.

The sub-second incremental target is not gated there at all. It is
held on the reference machine by this protocol run. In CI it is
guarded only structurally, by `cargo xtask bench --ceilings`: generous
per-scenario ceilings that catch the cache silently ceasing to work
and claim nothing about speed.

## Results

Protocol run of 2026-08-01, at commit `9c24879`, binary version
`celerrate 0.0.3` (a single run of the harnesses; the raw exports live
under `target/bench/` and `target/benchmark/` and are not committed):

| Scenario | Median |
| --- | --- |
| Cold full | 1.496 s |
| Warm no-change | 0.452 s |
| Warm one-edit | 0.467 s |
| Warm body-edit | 0.444 s |
| Warm signature-edit | 0.446 s |

The warm body-edit number is the published flagship; the README links
here. The four warm medians sit within 25 ms of one another, which is
about the run-to-run spread hyperfine reports for them on this
machine: their ordering carries no signal, and no edit scenario is
measurably more expensive than the no-change floor.

The comparison, same corpus, same run:

| Tool | Cold median |
| --- | --- |
| PHPStan 2.2.7, rule level 5 | 53.898 s |
| Celerrate 0.0.3 | 1.502 s |

Cold ratio: **35.9x**. The Celerrate median here is measured in the
comparison harness's own working tree, and it agrees with the cold
full scenario above (1.496 s) within the run-to-run spread.

Per-scenario cache statistics (one manual run each with
`CELERRATE_CACHE_STATS=1`, recorded verbatim):

```
cold full:            cache: trees 0 hit / 9341 miss; members 0 hit / 9341 miss; verdicts 0 served / 0 discarded / 46 absent; typed 217 bodies, edges 794 declared / 25 inferred / 7 provider, verdicts 0 served / 46 recomputed; persist 4 written / 0 skipped / 0 failed, 292ms
warm no-change:       cache: trees 9341 hit / 0 miss; members 9341 hit / 0 miss; verdicts 46 served / 0 discarded / 0 absent; typed 0 bodies, edges 0 declared / 0 inferred / 0 provider, verdicts 46 served / 0 recomputed; persist 0 written / 4 skipped / 0 failed, 35ms
warm one-edit:        cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 5 bodies, edges 23 declared / 0 inferred / 0 provider, verdicts 45 served / 1 recomputed; persist 4 written / 0 skipped / 0 failed, 69ms
warm body-edit:       cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 5 bodies, edges 24 declared / 0 inferred / 0 provider, verdicts 45 served / 1 recomputed; persist 4 written / 0 skipped / 0 failed, 66ms
warm signature-edit:  cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 65 bodies, edges 367 declared / 3 inferred / 1 provider, verdicts 37 served / 9 recomputed; persist 4 written / 0 skipped / 0 failed, 62ms
```

### Trajectory

The 2026-07-13 run (semantic core, commit `24b6950`) recorded cold
full 1.11 s, warm no-change 0.28 s, warm one-edit 0.29 s with two
diagnostic families and no inference. The 2026-07-18 run (commit
`a8382be`) recorded cold full 1.533 s, warm no-change 0.434 s, warm
body-edit 0.521 s, with the full type engine enabled: a 1.4x change
against the previous cold number, the price of inference.

This run records cold full 1.496 s, warm no-change 0.452 s, warm
body-edit 0.444 s. Cold full is flat against the previous run, inside
its spread, and warm body-edit came down from 0.521 s: the type
engine's cost stopped growing between the two runs. The comparison the
earlier runs pointed forward to is no longer withheld - it is measured
and published above, at 35.9x against PHPStan on this corpus, against
an ambition of roughly 20x.
