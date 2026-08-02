# Celerrate: Benchmark Comparison Corpus Design

Date: 2026-08-02
Status: Draft
Parent: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`
(section 8 and amendment 3)
Resolves: issue #118

## 1. Problem

The pinned analysis corpus, symfony/demo, cannot carry a comparison with
PHPStan: 51 first-party files against 9396 vendor files means both tools
are dominated by fixed costs (PHP interpreter startup on one side, the
whole-tree index on the other), and the measured cold ratio swings
between 1.4x and 2.0x across runs. A figure that moves 40 % between runs
is not a claim. The comparison side of closure gate 7 is therefore not
held, and the `v0.1.0` tag waits on it.

What is needed is a second corpus whose first-party code is large enough
that rule-checking dominates both wall clocks, pinned separately so the
analysis corpus, its diagnostic snapshot, and the type-precision baseline
are untouched.

## 2. Decisions taken with the user (2026-08-02)

- **Corpus shape**: a large real open-source application, matching the
  shape `celerrate check` is aimed at, over a framework monorepo or a
  dual-corpus setup.
- **Candidate**: Shopware 6 (`shopware/shopware`), scouted empirically
  before the pin is committed. Fallback order if scouting rejects it:
  PrestaShop, then phpMyAdmin. Triggering a fallback is reported to the
  user before the pin is committed.
- **CI wiring**: a weekly scheduled run plus a required gate in the
  release workflow. No per-pull-request job.
- **The published claim**: the measured ratio is published whatever it
  is, the parent design's "~20x" ambition is amended to the measured
  figure, and `v0.1.0` tags on it. No minimum threshold conditions the
  release.
- **Mechanics**: extend `cargo xtask benchmark` to read a dedicated
  comparison pin; no new xtask command.

## 3. The comparison corpus and its pin

A second pin file, `xtask/comparison-corpus.pin`, committed next to
`xtask/corpus.pin` and read by the same pin machinery. It names the
repository and commit of the comparison corpus. The vendor tree installs
from the corpus's own `composer.lock`, exactly as the analysis corpus
does today (`composer install --no-interaction --no-progress
--no-scripts --no-plugins --ignore-platform-reqs`).

The analysis corpus does not move: `xtask/corpus.pin`, the diagnostic
snapshot, and the mixed-rate baseline are untouched by this design. The
comparison corpus serves the ratio only; the five absolute scenario
medians and the memory ceiling keep coming from the analysis corpus.

## 4. Scouting before the pin

The first implementation task is empirical, and the pin is only
committed once the candidate passes it:

1. Fetch Shopware 6 at the candidate commit, install its vendor tree
   from its lock file.
2. Count the first-party PHP files and identify the first-party source
   directories (the paths handed to PHPStan).
3. Trial-run both tools once: `celerrate check .` must complete without
   crashing, and pinned PHPStan at rule level 5 must complete on the
   first-party paths with the vendor tree loaded for reflection only and
   the result cache off - the configuration the harness already
   generates.
4. Measure three consecutive cold runs of each tool on one machine.

Acceptance criteria:

- At least ~3000 first-party PHP files, so rule-checking dominates both
  wall clocks.
- Both tools complete.
- The cold ratio is stable across the three runs: the spread between the
  smallest and largest ratio stays well under the 40 % that disqualified
  symfony/demo (target: under 10 %).

If Shopware 6 fails any criterion, the same scouting runs on PrestaShop,
then phpMyAdmin, and the user is told which fallback fired before the
pin is committed.

## 5. The harness

`cargo xtask benchmark` keeps its single code path and reads the
comparison pin instead of the analysis pin. Concretely:

- Corpus preparation goes through the comparison pin (fetch snapshot,
  install vendor, copy to a disposable working tree under
  `target/benchmark/`).
- The generated PHPStan configuration lists the corpus's first-party
  source directories as `paths`, determined during scouting. Equal
  reported work holds by construction: Celerrate rule-checks only the
  files the project owns, and PHPStan is given exactly those.
- The `--gate` floor is set after the reference run, conservatively - on
  the order of half the measured median ratio - so shared-runner
  variance does not fail a healthy build. The floor is a named constant
  with the reference measurement recorded next to it.

## 6. CI wiring

Two workflow locations, no per-pull-request job:

- **Weekly**: a scheduled workflow (weekly cron) runs `cargo xtask
  benchmark --gate` on the pinned comparison corpus. A failure is a red
  run and its notification; it blocks nothing. The job's runtime is
  bounded, as the removed benchmark job's was.
- **Release**: the release workflow gains a required `benchmark-gate`
  job running the same command. No tag ships if the ratio has fallen
  under the floor.

## 7. Publication

After the full protocol run on the reference machine:

- `benchmarks/PROTOCOL.md` gains a comparison-corpus section: repository,
  commit, first-party and vendor sizes, the generated PHPStan
  configuration, the method, and the measured cold medians of both tools
  with their ratio.
- The README states the measured ratio as is.
- The parent design (2026-07-09, section 7) is amended: the "at least
  ~20x faster than PHPStan" ambition is replaced by the measured figure,
  met or not, without defensive rewording.

## 8. Closure hand-off

This design does not own the `v0.1.0` tag; it removes the last blocker
of `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`.
Once the ratio is published and the gate is wired:

- That spec's amendment 3 is updated: the comparison is no longer
  withheld.
- That spec's status moves to Closed and its closure section records
  gate 7 as fully held.
- `v0.1.0` is tagged with the already-prepared CHANGELOG entry.
- Issue #118 closes.

## 9. Testing

The new mechanics are thin and tested where they live:

- Reading `xtask/comparison-corpus.pin`: a unit test beside the existing
  pin-reading tests.
- The generated PHPStan configuration with the corpus's first-party
  paths: unit tests on the existing configuration function.
- The updated `--gate` floor: the existing `under_ratio_floor` unit
  tests, updated to the new constant.

The rest is protocol execution, verified by the reference run itself and
by the weekly workflow thereafter.

## 10. Out of scope

- Any change to the analysis corpus, its snapshot, or the mixed-rate
  baseline.
- The sub-second incremental target: it stays held by the protocol run
  on the reference machine and `cargo xtask bench --ceilings`, as
  amendment 1 of the parent spec records.
- A per-pull-request benchmark job.
- Publishing absolute wall-clock numbers from CI runners.
