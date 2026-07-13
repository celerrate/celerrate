# Persistent Artifact Cache Audit (part 8a)

Audit of `276d4df`; the Critical finding (C1) this audit identified was
fixed on this branch (see the 8-closure spec amendment history).

Read-only audit of the content-addressed derived-artifact cache against the
design contract in `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md`
section 2 and section 6. Paths are relative to the worktree root
`/Users/jh3ady/Businesses/JDevelop/OpenSource/celerrate/.claude/worktrees/semantic-core-8b-corpus-benchmark`.

Out of scope (known, not re-derived): defines missing from the persisted
boundary artifact, the deferred symbol-index pack, and warm/cold parity being
dominated by the defines gap.

Severity legend: Critical = wrong results or panic possible; Important =
contract violated or unprotected; Minor = polish/debt.

Tally: **1 Critical, 8 Important, 8 Minor.**

---

## Critical

### C1. A crafted verdict entry with `start > end` panics through `TextRange::new`

- Evidence: `crates/celerrate_cli/src/cache/stored.rs:211`
  (`range: TextRange::new(TextSize::from(self.start), TextSize::from(self.end))`);
  `text-size-1.1.1/src/range.rs:48` (`assert!(start.raw <= end.raw)`).
- `StoredDiagnostic::to_diagnostic` performs no ordering check on the two
  stored `u32` offsets. The blake3 checksum (`pack.rs:81`) is integrity, not
  authenticity: anyone who can write `.celerrate/cache/diagnostics.bin` can
  compute a valid checksum over a payload whose entry has `start > end` and
  zero revalidation records (an empty `records` list vacuously passes
  `validated_verdict`, `crates/celerrate_cli/src/cache/verdict.rs:32-47`).
- Failure scenarios, both contract violations:
  1. Hit path: `analyze_one` calls `to_diagnostic` inside `guarded`
     (`crates/celerrate_cli/src/analysis.rs:138-144`), so the panic becomes
     `InternalError::FilePanicked`, a user-visible internal-error report and
     exit 2 — the spec says corruption of any kind is answered by regeneration,
     never by a user-visible error, and that hostile-cache deserialization
     never panics.
  2. Persist path: `collect_entries` calls the same conversion inside
     `analysis::isolated` (`crates/celerrate_cli/src/cache/mod.rs:131-141`),
     so the whole persist is silently dropped on every cycle — a hostile
     entry permanently disables persistence with no signal.
- The 8a plan's fuzz waiver (`.claude/superpowers/plans/2026-07-13-semantic-core-8a-cache.md:687`)
  rests on "checksum-gated postcard, structurally panic-free"; this panic is
  after postcard, in the post-decode conversion the waiver did not consider.
- Fix direction: make `to_diagnostic` return `None` when `start > end`
  (discard-and-recompute, like an unknown identifier), and add a
  crafted-checksum-valid-entry test.

---

## Important

### I1. The header's binary identity is `CARGO_PKG_VERSION`: development builds accept each other's packs

- Evidence: `crates/celerrate_cli/src/cache/pack.rs:42`
  (`binary: env!("CARGO_PKG_VERSION")`), and the constant's own comment at
  `pack.rs:17-19` ("this constant is what protects development builds within
  one version") — protection that exists only if a human remembers to bump.
- Any rebuild that changes a rule message, a severity, a lowering detail, or
  adds a new diagnostic, without bumping `CACHE_SCHEMA_VERSION`, leaves the
  old pack fully acceptable: stale messages and severities are served
  byte-for-byte from `StoredDiagnostic` (severity is trusted from the entry,
  never re-derived from the registry: `stored.rs:206-210`), and a *newly
  added* rule's findings are silently missing from validated verdicts
  (revalidation records cannot notice a rule that did not exist when they
  were recorded). Same-version rebuilds are the *default* state of
  development and of anyone building from source. This directly breaks the
  "a hit must return byte-for-byte what the computation would produce"
  contract for every dev iteration between schema bumps. A forgotten bump
  after a `Stored*` shape change is worse: postcard is non-self-describing,
  so old bytes decode under new shapes into garbage that may error (fine) or
  may type-confuse silently.
- Fix direction: fold a build fingerprint into the header for non-release
  builds (git commit + dirty flag via a build script, or a compile-time hash
  of the binary's own identity), keeping `CARGO_PKG_VERSION` for releases.

### I2. Revalidation sufficiency holds by convention, not by construction

- Evidence: `crates/celerrate_semantics/src/revalidation.rs:20-41`
  (`ResolutionAnswer` deliberately drops the resolved declaration's kind and
  source location: "A `Source` answer produces no diagnostic whatever its
  declaration kind"); `crates/celerrate_cli/src/cache/verdict.rs:19-49`
  (acceptance = every record's answer unchanged, nothing else);
  `crates/celerrate_cli/src/cache/mod.rs:149-177` (`composed_verdict`
  "composed exactly as `analyze_one` composes" — a mirror maintained by
  hand); `revalidation.rs:53-55` ("The same traversal and resolution path as
  `reference_diagnostics`" — a second hand-maintained mirror).
- Today the reduction is sound: the four reference diagnostics (unknown
  symbol, introduced/removed gating, deprecation —
  `crates/celerrate_semantics/src/reference_checks.rs:121-175`) are all pure
  functions of the answer plus the header-pinned range, and
  `syntax_version_diagnostics` plus `file_diagnostics` are pure functions of
  content plus range. But nothing mechanical (type, test, or shared code
  path) enforces that every future check reads only what the answer
  captures. The first rule that reads the resolved declaration's kind
  ("cannot instantiate an interface"), its defining file, or index-global
  state (duplicate-definition, did-you-mean suggestions in messages) makes
  `validated_verdict` accept entries whose diagnostics are wrong, silently,
  on every warm run. The two mirrors (`composed_verdict`/`analyze_one`,
  `resolution_records`/`reference_diagnostics`) can drift the same way.
- Fix direction: a property/harness test that, for every corpus file whose
  records all revalidate, asserts stored diagnostics equal recomputed ones
  (would also have caught C1); longer term, derive the records from the
  checks themselves (record at the resolution call site the checks share).

### I3. The stub-blob header field is load-bearing but has no test

- Evidence: `pack.rs:32` carries `stub_blob`; the header-mismatch tests
  (`pack.rs:167-182`) cover range, schema, and binary — never `stub_blob` —
  and no seeding test writes a pack under a different stub hash.
- Spec section 6 item 2 requires header tests; the field whose whole purpose
  is "a new snapshot changes availability answers" is the one field no test
  proves discards the pack. A regression (for example hashing the format
  version instead of the blob) would ship unseen.
- Fix direction: one decode test flipping a `stub_blob` byte, and one
  seeding test planting a probe under a wrong stub hash.

### I4. "Only analyzed project files get diagnostics entries" has no pack-level test

- Evidence: the enforcement is `collect_entries` iterating `inputs.reported`
  (`mod.rs:122-123`), which excludes vendor files by construction
  (`session.rs:188-198`). The composer test
  (`crates/celerrate_cli/tests/cache_consistency.rs:172-198`) checks
  rendering equality only, and
  `the_written_packs_validate_and_carry_the_analyzed_files`
  (`tests/cache_seeding.rs:247-267`) uses a project with no vendor tree.
- The spec names this boundary explicitly (section 2, "Two boundaries the
  entries respect"). No test opens `diagnostics.bin` on a composer project
  and asserts the vendor file's content hash is absent while its item-tree
  entry is present.
- Fix direction: extend the composer fixture to decode both packs and assert
  the vendor hash appears in `item_trees.bin` and not in `diagnostics.bin`.

### I5. The atomicity test is weaker than the atomicity clause

- Evidence: `the_atomic_write_replaces_the_file_whole` (`pack.rs:184-192`)
  writes twice sequentially and reads back. The clause it protects
  (`pack.rs:88-90`: "a reader never sees a torn file and a concurrent
  writer's last rename wins whole"; spec: "rewritten atomically") is about
  concurrency, and nothing exercises a reader racing a writer or two
  concurrent `celerrate check` processes in one project.
- The implementation is right on POSIX (`NamedTempFile::new_in` the same
  directory, `persist` = `rename`, `pack.rs:91-99`), and cross-pack
  interleaving between two processes is benign *today* because both packs'
  entries are independently content-keyed and revalidated — but that
  cross-pack independence is exactly what the deferred symbol-index pack
  (keyed over the *set* of tree hashes) will break, and no test pins the
  property that currently makes the race safe.
- Fix direction: a loop test with a writer thread and a reader thread
  asserting every observed read decodes whole (or is absent), and a comment
  or test naming the cross-pack-independence invariant the symbol-index
  pack must not silently violate.

### I6. Persist-after-every-watch-iteration is untested

- Evidence: the call exists (`crates/celerrate_cli/src/watch.rs:100`), and
  the header-moved and write-failure unit tests drive `persist` directly
  (`mod.rs:280-393`), but no test drives a watch cycle (or the
  `cycle`-level seam) and asserts the packs on disk were rewritten with the
  cycle's results — the spec's "including every `--watch` iteration" clause
  is proven only by reading the source.
- Fix direction: a watch-level test (the existing watch test harness in
  `watch.rs` tests can host it) asserting pack mtime/content moves after an
  absorbed edit's cycle.

### I7. Hostile-cache coverage stops at accidental corruption

- Evidence: every decode-path test uses truncation, bit flips, garbage, or
  header mismatches (`pack.rs:132-182`, `tests/cache_seeding.rs:96-110`,
  `tests/cache_consistency.rs:203-233`) — all rejected by the checksum or
  the header before any entry is interpreted. No test feeds
  checksum-*valid* adversarial entries (out-of-order spans, spans past the
  file's length, absurd `ast_index` values, duplicate keys, empty record
  lists on files with references), which is the only hostile class that
  actually reaches `stored.rs` conversion code — and the one that fails
  today (C1). The plan waived fuzzing for this format on the grounds the
  post-checksum path is panic-free; that ground is currently false.
- Fix direction: a small adversarial-entry test matrix over `Stored*`
  values (cheaper than fuzzing and targeted at the actual gap); reconsider
  the fuzz waiver for `decode` + `to_diagnostic`/`to_item_tree` composed.

### I8. The economics contract is not observable in-tree

- Evidence: no hit/miss counters, no statistics, no verbose/debug output
  anywhere in the cache (`grep` over `crates/celerrate_cli/src/cache/` and
  `celerrate_semantics/src/cache.rs` finds nothing); the only signals are
  the render timing line and manually deleting a pack between runs.
- The spec makes economics decisions measured ("if measurement says the
  class loses, the pack is deleted") and requires "deserialization plus
  revalidation must beat recomputation". Wall-clock A/B (delete a pack,
  re-run) can measure a class end-to-end, but hit rate, revalidation
  acceptance rate, and revalidation cost per entry — the numbers the
  diagnostics-class decision actually needs — cannot be observed today
  without a debugger or profiler.
- Fix direction: cheap counters (hits, misses, revalidation accepts /
  discards, persist skipped/written) surfaced behind an environment
  variable or a `--cache-stats` flag, read by the 8b benchmark.

---

## Minor

### M1. Entries are fully decoded before the header is compared

- `pack.rs:84-85`: `postcard::from_bytes::<Pack<Entries>>` decodes header
  *and* entries in one call; the header equality check runs after. A
  version-mismatched pack pays a full entry decode before rejection, and a
  future header-shape change relies on postcard failing loudly on old
  bytes. Fix: encode header and entries as two length-delimited payloads;
  check the header before touching entries.

### M2. Crash-mid-write temp files accumulate forever

- `pack.rs:95-97`: `NamedTempFile` debris survives SIGKILL/power loss in
  `.celerrate/cache/` and nothing ever sweeps it. Fix: best-effort removal
  of stale `.tmp*` files in `prepare_directory`.

### M3. Every watch cycle clones the whole project's stored trees to decide "unchanged"

- `mod.rs:110-120`: `collect_entries` builds a full `StoredItemTree` (all
  declaration/import strings cloned) for every source file on every cycle,
  even when nothing changed, purely to feed the equality fast path at
  `mod.rs:212-216`. O(project) allocation per keystroke. Fix: compare
  per-entry content hashes against the snapshot first and only materialize
  entries when the set differs (or cache the stored form).

### M4. Stored spans are never bounds-checked against the file

- `stored.rs:203-214`: a (crafted) span past the file's end is accepted and
  rendered; `LineIndex::line_column` tolerates it by design
  (`crates/celerrate_source/src/line_index.rs:44-45`) and prints an
  oversized column. Not a panic, but a hit that is not byte-for-byte
  anything the computation could produce. Fix: discard entries whose spans
  exceed the file length (the content the key hashes is available).

### M5. Persist failure is completely silent, even under `--watch`

- By documented design (`mod.rs:29-46`), every encode/I-O failure is
  swallowed and retried next pass. A cache directory that is permanently
  unwritable (read-only checkout, quota) means paying full recomputation
  plus a doomed serialization on every cycle, with zero indication
  anywhere. Consistent with "never a user-visible error", but
  indistinguishable from a working cache. Fix: fold into I8's counters (a
  "persist failed" statistic), not a user-facing error.

### M6. The empty-project fast path can leave a stale-header pack on disk

- `mod.rs:212-218`: with zero entries and an empty loaded snapshot,
  `unchanged && path.is_file()` returns true even when the file on disk
  carries a foreign header (it was discarded at load, so the snapshot is
  empty either way). Harmless — every later run discards it again — but
  the pack is never healed. Fix: fold header validity into the `is_file`
  fast-path check, or ignore (cost is nil).

### M7. `load_pack` reads the whole file before the 8-byte magic check

- `crates/celerrate_cli/src/cache/snapshot.rs:42`: `std::fs::read`
  allocates the full file size before any validation; a planted
  multi-gigabyte `.bin` is swallowed whole before rejection. Within the
  plan's accepted threat model (writer controls the project anyway), so
  minor. Fix: stat and cap at a sane ceiling, or read magic + checksum
  through a bounded reader first.

### M8. `.gitignore` creation is non-atomic and unconditional

- `mod.rs:186-194`: plain `fs::write` (torn on crash → half-written
  `.gitignore` that never gets repaired, since only existence is checked),
  and two concurrent first runs race benignly. Fix: reuse
  `write_atomically`.

---

## Audit-area notes (no defect, recorded answers)

- **Header completeness (area 1).** All four spec fields are present and
  load-bearing: schema (`pack.rs:19,41`), binary (`pack.rs:42`), stub blob
  content hash (`pack.rs:43`), PHP range as two `(major, minor)` pairs
  (`pack.rs:44-45` — lossless, `PhpVersion` has no patch component,
  `crates/celerrate_project/src/version.rs:7-10`). Mismatch discards whole
  (`pack.rs:85`), proven for schema/binary/range (`pack.rs:167-182`), not
  stub blob (I3). A composer.json range edit *between* runs is keyed:
  `Session::start` builds the expected header from the freshly discovered
  range (`session.rs:115-120`). Mid-watch range moves are handled twice:
  `inputs()` swaps in an empty verdict snapshot until persist confirms the
  new header (`session.rs:162-167`), and `persist` force-rewrites on
  `header_moved` even when entries are byte-equal (`mod.rs:63-78`, tested
  at `mod.rs:334-393`). `ProjectConfiguration` is only the range
  (`crates/celerrate_project/src/input.rs:13-16`), so no configuration
  state escapes the header today; the diagnostic registry is not in the
  header, mitigated per-entry for *removed* ids by `find_identifier`
  re-interning (`stored.rs:203-205`) but not for added/changed rules within
  one version string (I1).
- **Checksum coverage (area 2).** The blake3 checksum covers the entire
  postcard payload, header included (`pack.rs:60-66`); only the magic sits
  outside it, harmlessly. Truncation anywhere, including between entries,
  fails the checksum. Postcard's decode is allocation-safe on hostile
  lengths (serde's cautious size hints; element counts bounded by payload
  bytes), string decode is UTF-8-checked error-not-panic, and all slicing
  in `decode` uses `get` (`pack.rs:75-80`). The surviving hostile surface
  is post-decode conversion (C1, M4).
- **Item-tree hit exactness (area 4).** Sound today: `ItemTree` is exactly
  `{declarations, imports}` (`crates/celerrate_semantics/src/items.rs:63-66`),
  `StoredItemTree` round-trips all of it with `FileId` stamped back into
  every `AstId` (`stored.rs:106-168`, round-trip test onto another file
  identity at `stored.rs:331-342`); the exhaustive struct literal in
  `to_item_tree` makes a future `ItemTree` field (defines) a compile error,
  not a silent drop. The salsa hook is consulted pre-parse and is
  deterministic (`queries.rs:31-42`); the registered `CacheHandle` is fixed
  for the process lifetime by design (`celerrate_semantics/src/cache.rs:38-45`)
  — item trees are range-independent, so never re-registering it under a
  range move is correct.
- **Concurrency (area 3).** Temp file in the destination directory
  guarantees same-filesystem rename (`pack.rs:95`). Two concurrent checks:
  each pack is last-writer-wins whole; a reader either sees the old or the
  new file, never torn (POSIX); safety of cross-pack mixing rests on the
  unstated per-entry-independence invariant flagged in I5. Crash mid-write
  leaves the destination untouched plus temp debris (M2).
