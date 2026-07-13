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

## Method

The harness is `cargo xtask bench`. It fetches the corpus at the
pinned commit, installs the vendor tree, builds the release binary,
copies the corpus into a disposable working tree
(`target/bench/corpus`), and measures with [hyperfine](https://github.com/sharkdp/hyperfine),
which times the full process: startup, cache loading, analysis,
rendering. The umbrella design named criterion; criterion measures
in-process, and the flagship number is defined end to end, process
included, so hyperfine is the honest tool for this document. criterion
remains available for in-process query benchmarks when a later part
needs them.

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
   is restored to its pristine content, a full run primes the cache,
   then the scripted edit is applied. Timed: `celerrate check .`.
   **This is the flagship number**: a full CLI run, wall clock,
   process startup and cache loading included, exactly the execution
   mode a save-and-rerun user experiences. Target: sub-second.

The scripted edit appends one comment line
(`// celerrate benchmark edit`) to
`src/Controller/BlogController.php`: one file's content changes,
every other input is byte-identical.

## Reproduction

```sh
# prerequisites: rust 1.94, git, composer, php, hyperfine
cargo xtask bench
```

## What is not compared

No PHPStan (or other tool) comparison is published at v0.0.x: the
preview runs a handful of diagnostic families while PHPStan runs
hundreds of rules, and a cross-scope timing comparison would be
meaningless at best and misleading at worst. The matched-scope
comparison is the v0.1 claim.

## Results

Protocol run of 2026-07-13, at commit `ff6b406`:

| Scenario | Median |
| --- | --- |
| Cold full | 1.15 s |
| Warm no-change | 1.10 s |
| Warm one-edit | 1.10 s |

The warm one-edit number is the published number; the README will
link here when the number is published with v0.0.1. (Recorded as the
median of three protocol runs; the raw hyperfine exports live under
`target/bench/` and are not committed.)

Both measured decisions from this run escalated to the human partner
rather than resolving automatically: the warm one-edit median is at
or over one second, and warm no-change is not at or under half of
cold full. See the 2026-07-13 protocol-run entry in the amendment
history of `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md`
for the measured values and the escalation.
