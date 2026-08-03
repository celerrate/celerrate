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
  that is the tree `celerrate check` analyzes. Of those, 51 files are
  the project's own and 9396 are the installed vendor tree. Celerrate
  parses and indexes all of them so names resolve, and rule-checks the
  51: a dependency's finding is not the user's to fix.

symfony/demo is the corpus because it has the exact shape
`celerrate check` is aimed at: a real user project, with application
code, a real `composer.json`, and the full Symfony vendor tree
installed from its lock file.

## Comparison corpus

The published PHPStan ratio is measured on a second pinned corpus,
separate from the analysis corpus above, because a comparison needs
first-party code large enough that rule-checking dominates both wall
clocks (issue #118 records why symfony/demo cannot carry one).

- Repository: https://github.com/PrestaShop/PrestaShop
- Commit: `fc96d0d4eae383e8c6f1f54f19cf592c221a62e3` (tag 9.0.3,
  committed in `xtask/comparison-corpus.pin`)
- Vendor tree: installed from the corpus's own `composer.lock`, same
  flags as the analysis corpus
- Size: 24033 PHP files, of which 6932 are first-party and the rest are
  the installed vendor tree

Both tools are handed the same file set, and the harness enforces it
rather than assuming it. Celerrate discovers a project through Composer's
autoload roots, but PrestaShop loads its 326-file `classes/` directory
through its own runtime autoloader, which Composer never declares: left
alone, Celerrate would report on 5922 files while PHPStan analysed 6926.
The harness therefore writes a `celerrate.toml` pinning
`[project] include = ["."]` into the corpus working tree. Vendor is
indexed for reflection on both sides but analysed by neither. PHPStan is
pinned in `benchmarks/phpstan/composer.lock`, at rule level 5, with its
result cache wiped before every timed run.

Rule level 5 remains the closest match to the families the measured
Celerrate binary enables, not an exact one: Celerrate additionally runs
its nullability family (CEL0034) and its version-availability family
(CEL0021 to CEL0024); PHPStan at level 5 runs rules Celerrate has no
equivalent for. Neither tool's rule set is a subset of the other's, and
no level makes it one.

The harness is `cargo xtask benchmark`; `--gate` fails under the
committed floor (`COLD_RATIO_FLOOR` in `xtask/src/benchmark.rs`), half
the reference ratio below.

Measured on the reference machine, cold. The two columns pool
differently, and are labelled accordingly: the wall-clock medians are
taken over all twenty-four timed runs across three full runs (nine
timed PHPStan runs and fifteen timed Celerrate runs). Hyperfine reports
one CPU total per invocation, not per timed run, so the CPU column
cannot be pooled the same way; it is the median of the three full
runs' own CPU totals, three values per tool.

| | wall clock | CPU consumed |
| --- | ---: | ---: |
| PHPStan | 38.92 s | 253.4 s |
| Celerrate | 13.41 s | 17.0 s |
| ratio | **2.90x** | **14.9x** |

Both ratios are published because either one alone misleads. The wall
clock is what you wait through; the CPU column is what the engines cost.
They differ by roughly a factor of five because Celerrate is effectively
single-threaded today while PHPStan forks worker processes: Celerrate
wins the wall clock while using an order of magnitude less machine.
Parallelising it is tracked work, not a claim made here (issue #124).

The three full runs gave ratios of 2.97x, 2.95x and 2.70x. The spread is
Celerrate's, not PHPStan's: its five timed runs step from about 12.3 s to
about 13.8-14.3 s partway through and stay there, which has the shape of
frequency scaling under sustained load on this machine. PHPStan's own
spread stayed between 1.1 % and 6.1 % throughout. The published figure is
therefore the pooled median rather than any single run, and the gate
floor sits far below the worst of them: 1.4, cleared by 1.93x even at
2.70x.

The gate runs weekly (`.github/workflows/benchmark.yml`) and as a
required job before any release publishes (`.github/workflows/release.yml`).

Further methodology detail, so the ratio survives the same scrutiny as
every other number in this document:

- **PHPStan version**: 2.2.7, installed from the committed
  `benchmarks/phpstan/composer.lock` and run under the PHP named below.
- **Result cache**: off, on both sides. PHPStan's `tmpDir` is
  `target/benchmark/phpstan-tmp`, outside the analyzed tree, and it is
  removed before every timed run; Celerrate's `.celerrate` is removed
  before every timed run the same way. Both tools start from nothing on
  every measured run.
- **Memory limit**: `2G`, passed to PHPStan on the command line so the
  run does not depend on the machine's `php.ini`. The measured runs stay
  far below it.
- **Invocation**: `php benchmarks/phpstan/vendor/bin/phpstan analyse
  --configuration <generated> --no-progress --memory-limit 2G` against
  `celerrate check .`, both timed by hyperfine with a one-run warmup
  before the timed runs, both in the same working tree.
  `--ignore-failure` in both cases: each tool exits 1 when it reports
  diagnostics, which is a completed analysis, not a failed one.

## Hardware and toolchain

- Machine: Apple M5, 10 cores (4 performance, 6 efficiency), 32 GiB memory, 1 TB NVMe SSD
- Operating system: macOS 26.5 (build 25F71)
- Rust toolchain: 1.94 (pinned in `rust-toolchain.toml`)
- Binary: `celerrate` built with `cargo build --release`, version
  `celerrate --version` reports at the commit the results name
- PHP: 8.5.0 (cli, NTS, Homebrew build) - the interpreter the
  comparison harness runs PHPStan under
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

## Why the analysis corpus cannot carry the comparison

`symfony/demo` remains the corpus for every absolute number above, but
it cannot carry a PHPStan ratio: only 51 of its 9447 files are the
project's own. At that size neither wall clock is decided by rule
checking. PHPStan pays for the PHP interpreter's startup and its own
bootstrapping; Celerrate pays for walking, parsing, and indexing the
9396 vendor files it needs in order to resolve names. Measured on the
machine named above, PHPStan's cold median lands near 2.6 s and
Celerrate's near 1.6 s, and three consecutive harness runs on that
machine within one hour produced ratios of 1.4, 2.0, and 1.7. A figure
that moves that far between runs, on fixed inputs, is measuring setup
cost rather than either tool's throughput.

The two workloads also differ in kind, not only in size. On this
corpus Celerrate reports nothing at all and PHPStan reports five
diagnostics, none of them in a family Celerrate implements. Even a
stable ratio here would be timing two different pieces of work.

This is why the published ratio is measured on a second, separate
corpus instead: [Comparison corpus](#comparison-corpus), above.

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
cargo xtask benchmark          # installs the pinned PHPStan itself
cargo xtask benchmark --gate   # fails under COLD_RATIO_FLOOR
```

## The gate in CI

What CI can hold is narrower than what this document reports. A shared
runner cannot hold an absolute wall clock: its cores, its neighbours,
and its storage vary from job to job.

No comparison runs per pull request. `cargo xtask benchmark --gate`
encodes a same-machine cold-ratio floor instead of an absolute wall
clock, which is the form a shared runner can carry: a slow runner
slows both tools, and the ratio is unaffected. It runs weekly
(`.github/workflows/benchmark.yml`) and as a required job before any
release publishes (`.github/workflows/release.yml`), rather than on
every pull request, because the comparison corpus's `composer install`
and the timed runs together take longer than a per-pull-request budget
allows.

The sub-second incremental target is not gated in CI either. It is
held on the reference machine by this protocol run. In CI it is
guarded only structurally, by `cargo xtask bench --ceilings`: generous
per-scenario ceilings that catch the cache silently ceasing to work
and claim nothing about speed.

## Results

Protocol run of 2026-08-01, at commit `9c24879`, which is the code
that becomes 0.1.0; the binary still reported `celerrate 0.0.3` there,
because the run predates the version bump. A single run of the
harnesses; the raw exports live under `target/bench/` and
`target/benchmark/` and are not committed:

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

The cold full median is the aggregate of five runs that included one
2.804 s excursion, the usual first-run cold-cache behavior; the
median is what this protocol publishes precisely so that a single
excursion does not move the figure.

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
earlier runs pointed forward to is now published, on the separate
corpus this document's own methodology requires: see
[Comparison corpus](#comparison-corpus), above.
