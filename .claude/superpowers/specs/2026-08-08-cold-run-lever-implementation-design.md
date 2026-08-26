# Celerrate: Cold-Run Lever Implementation Design

Date: 2026-08-08
Status: Approved
Related:
`.claude/superpowers/specs/2026-08-07-cold-run-performance-diagnostic-design.md`
(the diagnostic whose prioritized lever list this effort implements),
`.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`
(the measurement record every bound below cites; section 10 carries the
lever list, section 11 the amended ambition),
`.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`
(the pinned comparison corpus and the published-figure protocol),
`.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`
(the v0.1 closure criterion this effort exists to close),
`.claude/superpowers/specs/2026-07-09-celerrate-design.md`
(the parent design carrying the ~9x ambition).

## 1. Problem

The v0.1 closure criterion asks for "at least ~9x PHPStan on a cold
run". The published figure is 8.01x (2026-08-06, `v0.1.0` tag), and the
tag was taken by decision with the criterion unmet. The 2026-08-07
diagnostic measured where the remaining time lives and produced a
prioritized lever list. Four levers have both a measured bound and a
named mechanism; composed, they reach 9.19x if each pays out in full.
That composition is a ceiling, not a floor: only 95 ms of its 959.6 ms
saving was measured outright, the rest are upper bounds under
assumptions the diagnostic did not verify.

This effort lands those four levers, one at a time, each accepted or
rejected by its own measurement, then republishes the comparison and
lets the measured figure decide the criterion.

## 2. Decisions taken with the user (2026-08-08)

- **The measure decides the closure.** The levers land, the comparison
  is republished under the pinned protocol. If the published ratio
  reaches ~9x, the criterion is held. If it does not, the v0.1 closure
  criterion and the parent design's ambition are re-amended to the
  measured figure, in line with the diagnostic's own reading that ~8x
  is what the evidence supports with room to spare, and v0.1 closes on
  that amendment. No second diagnostic is chained into this effort; the
  unpriced leads stay named for a later one.
- **One lever at a time, verify-then-accept.** Each lever gets its own
  same-session A/B measurement. A lever is kept only on a measured
  gain; a lever that does not pay is reverted and its measurement
  recorded. A measured rejection is a result, not a failure.
- **The effort ships as v0.1.1.** A patch release closes it: gains are
  user-visible, the published figures move, the changelog says why. The
  release follows the v0.1.0 process.

## 3. Exit criterion

The effort is complete when:

1. All four levers are dispositioned: landed with a measured gain, or
   reverted with a measured rejection, each recorded in the effort's
   measurement document.
2. The comparison is republished under `benchmarks/PROTOCOL.md` on the
   reference machine, and the published figures (`README.md`,
   `benchmarks/PROTOCOL.md`) carry the new ratio.
3. The v0.1 closure criterion is resolved: held at ~9x, or re-amended
   to the measured figure with the user's approval of the wording, as
   the diagnostic's ambition amendment was.
4. `v0.1.1` is tagged and released.

## 4. The levers, in the diagnostic's order

Every bound below is from the diagnostic's measurement record,
section 10. Bounds are not additive; they are per-lever ceilings. The
numbering below is this effort's landing order; the diagnostic's lever
list knows these four as rows 1, 2, 4, and 5, and section 10 of this
document cites the excluded rows by the diagnostic's numbers.

1. **mimalloc as the global allocator** (dependency class, up to
   −0.47 s, 10.3 % of the ten-thread wall clock; the one gain the
   diagnostic measured outright). The allocator is set in
   `celerrate_cli` only, at the binary's composition root, so libraries
   stay allocator-neutral. Precondition, stated by the diagnostic
   before any code: a `cargo deny` licence pass and a supply-chain note
   recording the crate's provenance, maintenance, and licence in the
   effort's measurement document.
2. **Cap the file read and input set phase's parallelism** (local
   class, up to −437 ms; the conservative form, running the read at the
   four threads where its own measured curve is fastest, is −300 ms,
   both figures measured). The read in
   `crates/celerrate_cli/src/session.rs` (`Session::load`) currently
   fans out over the full rayon pool and scales negatively past four
   threads (+42 % inside `open` from eight threads to ten). The lever
   caps the read's parallelism at its measured optimum. A short
   thread-count sweep of the read phase re-confirms the optimum in this
   effort's own session before the cap is pinned; the cap is a
   compile-time constant, not an environment read.
3. **Parallelize the filesystem walk** (local class, up to −337 ms:
   374 ms at ten threads today against 37 ms at ideal scaling). The
   mechanism is named in the source: `enumerate_php_files` in
   `crates/celerrate_vfs/src/walk.rs` accumulates into a `BTreeSet` on
   the calling thread. The hard constraint is byte-identical output:
   the walk's file order, its deduplication, and its refusal semantics
   (unreadable directories, dangling symlinks) are observable and must
   not change. The diagnostic's standing warning applies: the one
   filesystem phase it measured under threads scales negatively, so
   this lever's bound is the least trustworthy of the four and the
   lever is a candidate for measured rejection.
4. **Reduce salsa interning traffic and contention** (local class with
   a dependency variant, up to −91 ms). The lock is named down to five
   entry points, 1598 of 1763 contended samples entering through
   `salsa::interned::IngredientImpl<C>::intern_id`. The cheap form goes
   first: reduce redundant interning traffic at those call sites. The
   dependency variant (a salsa upgrade, if a newer salsa shards the
   interning lock) is attempted only if the cheap form measures near
   zero and the upgrade cost is small; otherwise the lever is rejected
   on its measurement.

The recommended order is the diagnostic's: certainty descending. The
one measured-outright gain goes first, the two mechanism-named phase
levers next, the smallest bound last.

## 5. Verify-then-accept, per lever

Each lever follows the same cycle:

- **A/B in one session.** The lever's branch against its base commit,
  cold runs on the pinned corpus, three repetitions per side minimum,
  alternated within the session, medians compared, machine idle, with
  the diagnostic's in-session control discipline. Every figure lands in
  the effort's measurement document with its session date, commit, and
  spread.
- **Accept** when all hold: the median cold gain is positive and
  outside the session's run-to-run spread; `cargo xtask corpus` matches
  the committed snapshot byte for byte; `cargo xtask mixed-rate`
  matches the committed baseline; `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`, and `cargo deny check` are green. Nothing is
  blessed: a lever that moves the snapshot or the baseline has changed
  analysis behavior, which no lever may do, and is a bug or a
  rejection.
- **Reject** otherwise: the branch is not merged, and the measurement
  is recorded so the lever is not re-proposed without new evidence.

## 6. Tests and determinism

- The walk parallelization is the only lever that restructures
  observable internal behavior. It is developed under TDD against the
  walk's contract: sorted order, deduplication, exclusion handling,
  unreadable-directory refusals, and dangling-symlink tolerance, plus
  a determinism test asserting identical output across repeated runs
  on a fixture tree. The existing walk tests stay green unmodified.
- mimalloc, the read cap, and the interning reduction change no
  observable behavior. Their guard is the full test suite and the two
  corpus gates, which must pass unchanged; no new tests are demanded
  beyond what TDD requires of any incidental refactoring.
- Determinism rules are untouched: no wall-clock time, randomness, or
  environment reads inside salsa queries. The read cap and the walk's
  thread use live outside every query.

## 7. Publication and release

After the last lever is dispositioned:

- The comparison is republished under `benchmarks/PROTOCOL.md` on the
  reference machine, same corpus pin, same equalized file set.
- `README.md` and `benchmarks/PROTOCOL.md` take the new measured
  figures. Public documents carry measured figures only, as they do
  today.
- The CI regression gate floor (`COLD_RATIO_FLOOR = 4.0` in
  `xtask/src/benchmark.rs`) does not move. The floor and the ambition
  move independently, as documented when they were separated.
- The criterion is resolved per section 2's first decision. A
  re-amendment, if needed, touches the parent design's ambition wording
  and the v0.1 design's closure criterion, with the user approving the
  wording first.
- `CHANGELOG.md` records the landed levers and the new published ratio;
  `v0.1.1` is tagged and released following the v0.1.0 process.

## 8. Deliverables

1. The landed levers, each merged through its own reviewed branch.
2. **The measurement document**, in the diagnostic's discipline, under
   `.claude/superpowers/plans/`: every A/B session, every acceptance or
   rejection, the supply-chain note for mimalloc, and the final
   republished comparison.
3. The updated public figures, the changelog entry, and the `v0.1.1`
   release.
4. The criterion resolution: either the held ~9x or the approved
   re-amendment in the parent design and the v0.1 design.

## 9. Constraints

- Project rules hold for everything that lands: zero panic
  mechanically enforced, no `unsafe`, error resilience, determinism,
  strict layering. The walk lever stays inside `celerrate_vfs`; the
  read cap stays inside `celerrate_cli`; the allocator is set only in
  the binary.
- The corpus snapshot and the mixed-rate baseline are never blessed by
  this effort. Analysis behavior is out of bounds by definition.
- New dependencies are limited to mimalloc (and a salsa version bump
  only under section 4's fourth lever, dependency variant). Each new or
  bumped dependency passes `cargo deny check` before its lever is
  measured.

## 10. Out of scope

- **Lever 3, persist: collect entries** (bound contradicted by its own
  scaling curve, which stagnates at eight threads).
- **Lever 6, suggest enrich and render report** (a bound with no
  measured removable fraction inside it).
- **Lever 7, the fixed process cost** (19.6 ms, immaterial, on the
  record so it is not revisited).
- **Lever 8, the shared-nothing worker architecture** (a measured
  negative result: every configuration slower than the single
  ten-thread process; not to be pursued in the form measured).
- **The unpriced leads**: the fan-out's productive-work growth
  (+4.21 core-seconds, no measured cause), the 785 ms unattributed
  residue, and the `tests`/`classes` cost concentration. They remain
  the named input to a possible later diagnostic, not to this effort.
- The warm path and watch mode, except as non-regression guards
  through the existing test suite.
- The CPU ratio as a target.

## 11. Risks

- **The ceiling is not a floor.** 9.19x requires all four levers to
  pay in full, and only mimalloc's gain was measured outright. The
  effort may land measurably faster and still sit under 9x; section 2's
  first decision makes that outcome a defined closure, not a stall.
- **The walk lever's bound may not survive contact.** The measured
  negative scaling of the read phase is the standing warning; the
  verify-then-accept cycle exists so this lever can fail cheaply.
- **mimalloc is a supply-chain decision**, mitigated by the licence
  pass and the recorded note, and reversible: the allocator swap is
  one line at the composition root.
- **Session variance** (17.5 % measured on PHPStan between two
  sessions) can fake or hide a gain: covered by same-session A/B,
  alternation, controls, and medians of three or more.
- **A lever interacting with another** (mimalloc's unassigned 375 ms
  overlaps the phase levers): accepted, since levers are measured
  cumulatively in landing order and each A/B compares against the
  current base, so no gain is counted twice.
