# Celerrate: Cold-Run Performance Diagnostic Design

Date: 2026-08-07
Status: Approved
Related:
`.claude/superpowers/specs/2026-08-03-check-pipeline-performance-design.md`
(the previous performance effort, whose levers this diagnostic builds on),
`.claude/superpowers/plans/2026-08-03-check-pipeline-performance-measurements.md`
(the measurement record this diagnostic inherits its discipline and its
open leads from),
`.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`
(the pinned comparison corpus and protocol),
`.claude/superpowers/specs/2026-07-09-celerrate-design.md`
(the parent design carrying the "at least ~20x" ambition this diagnostic
exists to revise on evidence).

## 1. Problem

The cold run on the pinned comparison corpus (PrestaShop 9.0.3,
`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`, 6932 first-party PHP files,
equalized file set) sits between 4.8 s and 5.5 s depending on the
measurement session, a PHPStan ratio between 6.6x and 8.01x. The parent
design's ambition names "at least ~20x PHPStan on a cold run", which
would require roughly 1.6 s on this corpus and machine. Nothing measured
so far says whether that figure sits above or below the physical floor
of the corpus, and nothing explains why the engine claims only 4.51
effective cores of 10 while PHPStan claims 6.21.

The previous performance effort was lever-driven: profile, attack the
largest phase, re-measure, repeat. It took the cold median from 14.087 s
to under 6 s and it stopped, by design, when its target held. That
method finds local maxima; it never answers the standing question of
whether the remaining gap is a matter of more local levers or of the
architecture itself (the shared salsa database against PHPStan's
isolated worker processes).

This diagnostic answers that question. It lands no levers.

## 2. Decisions taken with the user (2026-08-07)

- **Diagnostic first.** The effort produces a measured explanation of
  the gap and a defensible ceiling before any target figure is fixed.
  The next performance target is set on those facts, the same way the
  previous effort's 6 s target was set after its first profile.
- **Cold only.** The diagnostic covers the cold run on the pinned
  corpus, the path behind the published ratio and the ~20x ambition. The
  warm path is measured only as a non-regression guard if any
  instrumentation lands.
- **Every lever class is admissible downstream.** Local optimizations,
  architectural rework, and new dependencies (mimalloc after a
  cargo-deny licence pass and a supply-chain note) are all acceptable
  outcomes if the diagnostic justifies them. The diagnostic itself
  builds none of them.
- **The ambition is revised on facts.** The parent design's "at least
  ~20x" was never derived from a measurement. This diagnostic produces
  a measured ceiling, and the parent design is amended with a defensible
  figure, above or below 20x, whatever the evidence says. The amendment
  wording is approved by the user before it is written.

## 3. Exit criterion

The diagnostic is complete when three measured answers exist:

1. **The corpus floor**: the incompressible cost of the corpus (read,
   lex, parse at 10 cores) and the ratio ceiling it implies.
2. **The missing cores explained**: a quantified attribution of the
   fan-out's non-scaling (allocator contention, salsa contention, memo
   waits, serial residue, other), precise enough to say whether the
   answer is local optimization or architectural rework.
3. **The Amdahl accounting**: every millisecond of the cold wall clock
   attributed to a phase or to process overhead, with the best
   achievable total if everything parallelizable were parallelized at
   the measured fan-out efficiency.

From those answers, two deliverables close the effort: the prioritized
lever list (section 8) and the ambition amendment proposal (section 2,
last decision).

## 4. Measurement discipline

The previous effort's record shows session-to-session variance of about
14 % on a reference workload that received no code change. Every
comparative claim in this diagnostic therefore follows these rules:

- **Same-session A/B.** Any two figures presented as a comparison are
  measured in the same session, machine idle, back to back.
- **In-session load control.** Each session re-measures a control
  workload: the reference Celerrate binary at the session's start and
  end (cheaper than re-running PHPStan). A control drift above the
  run-to-run noise invalidates the session's comparisons.
- **Three repetitions minimum** per measurement point; medians reported.
- **Provenance on every figure.** Every number in the measurement
  document carries its session date, its commit, and its spread.

The machine is the reference 10-core machine used by every published
figure so far. The corpus is the pinned, equalized PrestaShop file set,
unchanged.

## 5. Question 1: the corpus floor

A throwaway probe (an uncommitted xtask subcommand or a scratch branch)
performs the strict minimum any analyzer of this corpus must do: walk
the equalized file set, read and UTF-8-decode every file in parallel,
lex and parse each file with `celerrate_syntax` under rayon at 10
threads, discard the results. Three cold runs, median.

A second probe measures the fixed process cost: `celerrate check` on an
empty project, separating startup and teardown (including compiled-stub
loading) from per-file work.

The ratio ceiling follows: the same session's PHPStan cold median
divided by the floor. This figure is the central input to the ambition
revision. If the floor is, say, 2.5 s, then ~20x is physically out of
reach on this machine and corpus, and the parent design must say so; if
the floor is 0.8 s, the ambition survives as a stretch figure with a
measured distance to it.

## 6. Question 2: the missing cores

Three instruments, in order:

1. **Scaling curve.** Full `celerrate check` at 1, 2, 4, 8, and 10
   rayon threads (`RAYON_NUM_THREADS`, read by rayon at pool
   initialization, outside every salsa query; determinism is untouched),
   with per-phase timings at each point. The curve's shape already
   separates contention (early plateau) from serial residue (constant
   but shallow slope).
2. **Sampling profiles at the stagnation point,** taken during the
   fan-out. Worker time is categorized: allocator locks, salsa memo
   table access, waiting on a memo another worker is computing (the
   `__psynch_cvwait` pattern the previous effort's diagnosis observed),
   or productive work. This is the measurement that attributes the lost
   cores to a cause. The three `stub_*` whole-set queries flagged as a
   latent trap in the previous effort's record
   (`stub_symbol_table`, `stub_frontier`, `stub_signature_table` in
   `crates/celerrate_semantics/src/index.rs`) are checked for self time
   in the same profiles.
3. **mimalloc A/B on a scratch branch.** The same scaling curve with
   the global allocator swapped. The previous effort quantified the
   swap at about −0.9 s cold and −6 s CPU but did not land it; here it
   serves as an instrument, not a lever. If the curve's slope improves,
   the contention was allocation-bound and a cheap local lever exists;
   if the slope does not move, the cause is structural and the
   architectural question is posed on evidence.

**Conditional probe, built only if instruments 1 to 3 leave the salsa
share ambiguous:** a scratch prototype of shared-nothing analysis, one
salsa database per worker over a partition of the file set, results
merged, with no identical-output requirement. It is an instrument, not
a product: it bounds from above what a PHPStan-style isolated-worker
architecture could gain, and the report must state that the bound
excludes the merge and determinism costs a real implementation would
pay.

## 7. Question 3: the Amdahl accounting

Reconcile the cold wall clock with the sum of the eight instrumented
phases: measure what lives outside them (process startup, compiled-stub
loading, teardown, stderr formatting). The previous record shows
roughly 0.5 s to 1 s of wall clock unaccounted for by the phase sum.

Then a table: every phase, its median cost, serial or parallel today,
parallelizable or not, and the best achievable cold total if every
parallelizable phase ran at the fan-out's measured efficiency. This
table is the bridge from the diagnosis to the lever list: it says what
the local-optimization path can reach at best, which is exactly the
number the architectural decision needs.

## 8. Deliverables

1. **The measurement document**, kept as the diagnostic proceeds, in
   the discipline of the previous effort's record: protocol stated,
   dated tables, honest readings of every unexplained movement. Lives
   under `.claude/superpowers/plans/`.
2. **The core-second map**: the closing synthesis answering the three
   questions, each cause of loss carrying its quantified attribution.
3. **The prioritized lever list**: each lever with its estimated gain
   (bounded by measurement, not intuition), its class (local,
   architectural, dependency), and a recommended order. This is the
   input to the next implementation effort, which gets its own plan.
4. **The ambition amendment proposal**: the replacement wording for the
   parent design's "at least ~20x", derived from the measured ceiling,
   submitted for user approval before the parent design is touched.

## 9. Constraints

- **The diagnostic changes no behavior.** Probes live on scratch
  branches or in throwaway instrumentation and are not merged. If a
  piece of instrumentation deserves to stay (new `--verbose` phase
  timings, for example), it follows the existing verbose-channel
  discipline (meta-reporting outside every salsa query, byte-identical
  machine formats, a failed stderr write never changes the outcome) and
  passes the usual gates.
- The corpus snapshot and the mixed-rate baseline stay untouched;
  nothing is blessed, since nothing changes.
- Project rules apply to any code that lands (zero panic, clippy
  gates, TDD for durable units). Throwaway probes on scratch branches
  are exempt because they are never merged.

## 10. Out of scope

- Landing any performance lever, including mimalloc as a lever; it
  appears here only as a diagnostic instrument on a scratch branch.
- The warm path and watch mode, except as a non-regression guard if
  instrumentation lands.
- The CPU ratio as a target.
- The architectural rework itself: if the diagnostic justifies it, it
  becomes a sub-project with its own spec and plan cycle.

## 11. Risks

- **Machine variance** producing false attributions: covered by the
  session discipline of section 4.
- **A measured floor that invalidates ~20x**: not a failure of the
  diagnostic but its purpose; the parent design is amended with the
  defensible figure, whatever it is.
- **The shared-nothing probe reading too well**: it measures an upper
  bound without the merge and determinism costs of a real
  implementation; the report states this limitation explicitly wherever
  the bound is cited.
