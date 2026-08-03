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
  user before the pin is committed. (Outcome: both Shopware and
  phpMyAdmin were rejected and PrestaShop is the pinned corpus - see
  section 11.)
- **CI wiring**: a weekly scheduled run plus a required gate in the
  release workflow. No per-pull-request job.
- **The published claim**: the measured ratio is published whatever it
  is, and `v0.1.0` tags on it. No minimum threshold conditions the
  release. (Superseded in one respect by section 11: the parent design's
  "~20x" ambition is *not* amended down to the measured figure, because
  the measurement does not test what the ambition claims.)
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
  source directories as `paths`, determined during scouting.
- Equal reported work is **enforced, not assumed**. The original design
  asserted it held by construction, on the reasoning that Celerrate
  rule-checks the files the project owns and PHPStan is given exactly
  those. Measurement falsified that (section 11): Celerrate discovers
  through Composer autoload, and a real application routinely loads part
  of its own code through a runtime autoloader Composer never sees, so
  Celerrate silently reports on fewer files than PHPStan analyses. The
  harness therefore writes a `celerrate.toml` into the corpus working
  tree pinning `[project] include` to the same set PHPStan gets, and the
  reference run records both tools' analysed file counts so a future
  divergence is visible rather than silent.
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
  commit, first-party and vendor sizes, both generated configurations,
  the analysed file count of each tool, the method, and the measured cold
  medians with their ratio.
- Both ratios are published, because one without the other misleads: the
  **wall-clock** ratio is what a user waits through, and the
  **CPU-time** ratio is what the engines cost. On the pinned corpus they
  differ by a factor of six, because Celerrate is effectively
  single-threaded today and PHPStan forks workers. Publishing only the
  wall clock understates the engine; publishing only the CPU time
  overstates the experience.
- The README states the measured wall-clock ratio as is, with the
  parallelism caveat in the same breath.
- The parent design (2026-07-09, section 7) keeps its "at least ~20x
  faster than PHPStan" ambition, annotated with the measured figure and
  its date. The ambition is not amended down: the measurement is of a
  single-threaded run whose wall clock is dominated by a quadratic
  presentation pass, so it does not test the claim the ambition makes
  (section 11). Amending it down would record a conclusion the evidence
  does not support.

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
- The performance work the scouting exposed (section 11): the quadratic
  did-you-mean pass and the absent parallelism are tracked separately and
  do not gate this design or the tag.
- Instrumenting the sustained-load step in Celerrate's timings, and any
  cooldown the protocol might grow because of it (section 11).
- A per-pull-request benchmark job.
- Publishing absolute wall-clock numbers from CI runners.

## 11. Amendment (2026-08-03): what the scouting measured

Scouting rejected the intended candidate and falsified one of this
design's own assumptions. Both outcomes are recorded here because they
change what section 5 builds and what section 7 publishes.

### The corpus

Shopware 6 fails the reproducible-vendor requirement outright:
`shopware/shopware` is the `shopware/platform` library monorepo and its
`.gitignore` excludes `/composer.lock`, so no commit on it carries a
lock; the downstream `shopware/production` carries no first-party PHP.
phpMyAdmin has a lock but only 959 first-party files, under the 3000
floor. The pinned corpus is therefore **PrestaShop 9.0.3**
(`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`): 6932 first-party PHP files
of 24 033 total, with a committed `composer.lock`.

### The falsified invariant

Measured on that corpus under the configuration section 5 originally
described, PHPStan analysed 6926 first-party files and Celerrate reported
on 5922 - **85 % of the work**. PrestaShop's `composer.json` declares
only `src/` in its autoload; its 326-file `classes/` directory
(`ObjectModel`, `Address`, `PrestaShopException`) is loaded by
PrestaShop's own runtime autoloader. Celerrate never indexed it, so 3191
of its 5836 diagnostics were spurious "unknown class" reports - and each
diagnostic costs roughly 3.7 ms in the did-you-mean pass, so the benchmark
charged Celerrate for the files it had been denied.

Pinning `[project] include = ["."]` equalises the sets (6932 against
6926), drops the spurious reports to 327, and Celerrate finishes **35 %
faster while analysing 17 % more files**: 21.12 s to 13.67 s over three
cold runs. This is why section 5 now enforces the file set instead of
assuming it.

### The measured comparison

The published figure is the median of **three full runs** on the
reference machine, pooled: nine timed PHPStan runs and fifteen timed
Celerrate runs.

| | wall clock, cold median | CPU consumed |
| --- | ---: | ---: |
| PHPStan (level 5) | 38.92 s | 237.5 s |
| Celerrate | 13.41 s | 16.8 s |
| ratio | **2.90x** | **14.2x** |

The two ratios differ by roughly a factor of five for one reason:
Celerrate is effectively single-threaded where PHPStan forks workers.
Section 7 publishes both.

### The stability the acceptance criteria did not get

Section 4 requires each tool's spread to stay under 10 %. **Celerrate's
does not, on this machine**, and the design records that rather than
quietly dropping the criterion.

The harness originally passed hyperfine no warmup, so the first timed
run absorbed the cold page cache: Celerrate's spread was 22.66 % and two
consecutive full measurements disagreed by 11 %. Adding a warmup fixed
that specific defect. What remains is different in kind: Celerrate's
five timed runs step from about 12.3 s to about 13.8-14.3 s partway
through and stay there, rather than scattering. The pattern reproduces
on a rested machine with nothing measured before it, so it is not
contamination from a preceding run; it has the shape of frequency
scaling under roughly seventy seconds of sustained load, though the
mechanism was not instrumented.

The three full runs produced ratios of 2.969x, 2.950x and 2.703x, a
9.9 % span. Two of the three exceeded the 10 % spread criterion on the
Celerrate side (19.93 % and 16.42 %); PHPStan stayed within it every
time (1.12 % to 6.10 %).

What follows from that:

- The published ratio is the pooled median, not any single run, and
  `benchmarks/PROTOCOL.md` states the observed range beside it. A figure
  quoted from the best of three runs would not be honest.
- The gate is unaffected. `COLD_RATIO_FLOOR` is 1.4, and the worst ratio
  observed clears it by 1.93x, so shared-runner variance has enormous
  headroom before a healthy build fails.
- Instrumenting the step, and deciding whether the protocol should
  impose a cooldown between runs, is follow-up work. It does not gate
  this design: the floor holds and the published figure is conservative.

### Why the parent ambition stands

62 % of Celerrate's wall clock is `suggest::enrich`, a presentation pass
that re-clones an 18 000-name pool and reallocates its edit-distance
matrix per candidate. It is not the analysis engine. Removing that churn
and parallelising the persist, index and read phases is estimated to
land the run near 4.5-6 s, a wall-clock ratio of **6x-8x**. Celerrate
already consumes 14x less CPU than PHPStan for the same corpus; the
whole gap between that and the wall clock is cores left idle. Spending
the same CPU across as many cores as PHPStan actually uses would put the
wall-clock ratio in the same range again. The "~20x" ambition is gated
on parallelism and one quadratic pass, not on analysis throughput, so
the measurement does not refute it and section 7 does not amend it down.
