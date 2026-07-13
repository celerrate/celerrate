# Cache Audit Debt Settlement (Design)

Date: 2026-07-14
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md`
(section 2, section 6, and the amendment entries recording the audit debt)
Audit settled: `.claude/superpowers/audits/2026-07-13-persistent-cache-audit.md`

## 1. Goal and scope

Settle the persistent-cache audit debt the 8-closure spec recorded as
open before the type-engine sub-project begins. The audit tallied
1 Critical (fixed before v0.0.1), 8 Important, and 8 Minor findings;
the 8-closure amendment history names the Important findings as open
priority debt.

In scope: all eight Important findings (I1 through I8), plus the four
Minor findings that fall inside the same work (M2 crash-debris sweep,
M4 out-of-bounds stored spans, M5 silent persist failure, M8
non-atomic `.gitignore` creation).

Out of scope, explicitly:

- M1 (header decoded together with entries), M3 (O(project) stored-tree
  clone per watch cycle), M6 (stale-header pack never healed on the
  empty fast path), M7 (whole-file read before the magic check): they
  remain recorded in the audit as accepted polish, not debt.
- The symbol-index pack: the 8-closure amendment closed that decision
  (not built; `define()` names ride the item-tree pack instead).
- The architecture audit's note on `source_symbol_table` assembly being
  the one global serial loop: separate debt, revisited at the next
  scale-up, not cache work.
- Any new public CLI surface. The statistics of section 4 live behind
  an environment variable; an official `--cache-stats` flag is CLI
  product sub-project territory.
- No re-tag and no re-publication of the v0.0.1 protocol number: the
  published number belongs to the v0.0.1 binary, which does not change.

The work is one part, one branch, one implementation plan, ordered so
that the risky production changes land first and the coverage added
right behind them protects them immediately: behavior changes
(section 2), then coverage debt (section 3), then instrumentation
(section 4), then the equivalence harness (section 5).

## 2. Behavior changes

### I1. Binary identity becomes a self-hash of the executable

Today `PackHeader.binary` is `env!("CARGO_PKG_VERSION")`, so every
development rebuild within one version accepts the previous build's
packs: stale messages, severities, and silently missing findings from
newly added rules — the default state of anyone building from source.

The fix: a new module in `celerrate_cli` computes the binary identity
once per process as the blake3 hash of the executable's own bytes
(`std::env::current_exe`, then read and hash the file), rendered as hex
into the existing `binary: String` header field. The rule is uniform —
development and release builds alike — so there is no two-path
dev/release distinction and no human bump to forget: two different
binaries never speak to each other's packs, mechanically.

- **Header shape is unchanged**, so `CACHE_SCHEMA_VERSION` does not
  bump. Packs written by v0.0.2 carry `"0.0.2"` in the field, which
  matches no hash: discarded wholesale at load, the designed mechanism.
- **Fallback**: if `current_exe` or reading it fails, the identity
  falls back silently to `CARGO_PKG_VERSION`. Zero panic, never fatal,
  no user-visible error.
- **Seam for tests**: the hash-to-identity function is pure over
  provided bytes and unit-tested on them; only the process wiring reads
  the real executable.
- **`CACHE_SCHEMA_VERSION` remains**, re-documented: no longer the
  protection for development builds (the self-hash carries that), it
  documents deliberate format breaks.
- **Cost acceptance**: one local protocol run (per
  `benchmarks/PROTOCOL.md`) confirms warm one-edit stays comfortably
  sub-second with the startup hash (~5-10 ms expected against the
  0.29 s baseline).

### M4. Out-of-bounds stored spans are discarded

`StoredDiagnostic::to_diagnostic` receives the file's content length
and answers `None` for any entry whose span exceeds it — the same
answer as the C1 fix: discard and recompute honestly, never render a
diagnostic the computation could not have produced.

### M2. Crash debris is swept

`prepare_directory` best-effort removes orphaned temporary files (the
`NamedTempFile` pattern, `.tmp*`) left in `.celerrate/cache/` by a
crash mid-write. Failures to remove are ignored.

### M8. `.gitignore` creation becomes atomic

The `.celerrate/.gitignore` write goes through the existing
`write_atomically` instead of plain `fs::write`, so a crash can no
longer leave a torn file that is never repaired.

## 3. Coverage debt

Tests only; no production change except where a seam is needed.

- **I3, the stub-blob header field**: one decode test flipping a
  `stub_blob` byte proves the whole pack is discarded; one seeding test
  plants a pack under a wrong stub hash and proves a cold recompute.
- **I4, the vendor boundary at pack level**: the existing Composer
  fixture test decodes both packs and asserts the vendor file's content
  hash is present in `item_trees.bin` and absent from
  `diagnostics.bin` — the spec's "installed dependencies are indexed,
  never reported" made checkable on disk.
- **I5, concurrent readers and writers**: a loop test with one writer
  thread and one reader thread asserts every observed read either
  decodes whole or is absent, never torn. Alongside it, the per-entry
  cross-pack-independence invariant — the property that currently makes
  two concurrent `celerrate check` processes safe, and exactly what a
  future set-keyed pack would break — is pinned by a named test or, at
  minimum, a named invariant comment where the packs are loaded.
- **I6, persist per watch cycle**: a watch-level test (hosted by the
  existing watch test harness) asserts that after a cycle absorbs an
  edit, the packs on disk were rewritten with the cycle's results.
- **I7, checksum-valid adversarial entries**: a test matrix of packs
  that pass the checksum but carry hostile entries — reversed spans
  (pins the C1 fix), spans past the file's length (pins M4), absurd
  `ast_index` values, duplicate keys, empty record lists on files with
  references — each answered by discard-and-recompute, never a panic,
  never a user-visible error. Whether the fuzz waiver is lifted with a
  composed `decode`-plus-conversion fuzz target is a plan decision,
  taken against the surface that remains after the matrix.

## 4. Instrumentation (I8 and M5)

A `CacheStatistics` structure with atomic counters (the item-tree hook
is reached under the rayon fan-out), carried by the session, counting:

- item-tree hits and misses;
- verdicts accepted, verdicts discarded at revalidation, verdicts
  absent;
- per pack: persist written, persist skipped (unchanged), and persist
  **failed** — M5's silent failure becomes observable.

When `CELERRATE_CACHE_STATS=1`, one statistics line prints to stderr at
the end of the run, and after every `--watch` cycle. The counters never
feed analysis (salsa determinism is untouched), and stderr is not a
contractual surface: the format may move freely until the CLI product
sub-project decides an official flag. This is what makes the parent
spec's economics rule ("if measurement says the class loses, the pack
is deleted") measurable without a profiler: hit rate, revalidation
acceptance rate, and persist health, per class.

## 5. The equivalence harness (I2)

Revalidation sufficiency currently holds by convention: the
`ResolutionAnswer` reduction and two hand-maintained mirrors
(`composed_verdict` mirroring `analyze_one`, `resolution_records`
mirroring `reference_diagnostics`) are sound today, but nothing
mechanical fails when a future check reads what the answer does not
capture.

The mechanical net: an integration test in `celerrate_cli/tests`
extending the cross-process harness — analyze, persist, restart a fresh
session, and for **every file whose records all revalidate**, assert
the diagnostics served from the pack equal, byte for byte, a full
recomputation that bypasses the cache. It runs over the existing
fixtures plus fixtures covering each case the reduction must carry:
source resolution, stub resolution with an availability window, unknown
symbol, version gating, and deprecation. This is the test that would
have caught C1, and that catches the first future check whose
diagnostics depend on more than the recorded answers.

In addition, the plan examines whether unifying `resolution_records`
with the `reference_diagnostics` traversal (one shared code path
instead of the mirror) is cheap. If it is, it lands here; if not, the
constructive derivation is explicitly deferred to the type-engine
sub-project — which reshapes these check paths anyway — and this spec
records the deferral as the remaining, smaller convention.

## 6. Acceptance

- The four local commands green: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all` (check), `cargo deny check`.
- Zero-panic lints on every new module, no exception outside test
  modules; nothing in this part may panic on hostile cache input.
- One local protocol run confirms warm one-edit remains sub-second with
  the self-hash startup cost.
- At close: an amendment entry in the 8-closure spec records the debt
  as settled, and the audit document marks I1 through I8, M2, M4, M5,
  and M8 as settled, with M1, M3, M6, and M7 remaining as recorded,
  accepted polish.
