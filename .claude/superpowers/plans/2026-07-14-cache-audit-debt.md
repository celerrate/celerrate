# Cache Audit Debt Settlement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settle the persistent-cache audit debt (findings I1–I8 plus M2,
M4, M5, M8) per the approved spec
`.claude/superpowers/specs/2026-07-14-cache-audit-debt-design.md`.

**Architecture:** All work lives in `crates/celerrate_cli` (cache module,
analysis, session, watch) plus one comment-level export in tests. Ordered
so risky production changes land first (binary self-hash identity,
span-bounds discard, debris sweep), coverage tests pin them immediately
after, then observability counters, then the stored-equals-recomputed
equivalence net.

**Tech Stack:** Rust, salsa, blake3, postcard, tempfile, rayon. No new
dependencies.

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]` these.
- TDD: failing test → minimal implementation → refactor. No production
  code without a test that demanded it.
- Determinism: no wall-clock, randomness, or environment reads inside
  salsa queries. The `CELERRATE_CACHE_STATS` environment read happens at
  the CLI orchestration layer only, never inside a query.
- No user input may ever crash the tool; hostile cache input is answered
  by discard-and-recompute, never a panic, never a user-visible error.
- Everything in English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits, repository-configured identity.
- Verify each task with: `cargo test -p celerrate_cli` (or the named
  test), and before finishing:
  `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`.
- Working branch: `cache-audit-debt` (already created, carries the spec).

---

### Task 1: Binary identity becomes a self-hash of the executable (I1)

**Files:**
- Create: `crates/celerrate_cli/src/cache/identity.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (add `pub mod identity;` to the module list at the top)
- Modify: `crates/celerrate_cli/src/cache/pack.rs` (header construction + `CACHE_SCHEMA_VERSION` comment)

**Interfaces:**
- Produces: `celerrate_cli::cache::identity::binary_identity() -> &'static str`
  — the blake3 hex hash (64 characters) of the running executable's
  bytes, computed once per process, falling back to `CARGO_PKG_VERSION`
  if the executable cannot be found or read.
- `PackHeader::current` now fills `binary` from `binary_identity()`. The
  `binary: String` field's shape does not change, so
  `CACHE_SCHEMA_VERSION` stays at 2 (old packs carry `"0.0.2"`, which
  matches no hash and is discarded wholesale — the designed mechanism).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_cli/src/cache/identity.rs`:

```rust
//! The binary identity the pack header carries: the blake3 hash of the
//! running executable's own bytes, computed once per process. Two
//! different binaries never accept each other's packs, mechanically —
//! no human-remembered version bump involved (audit finding I1: keying
//! on `CARGO_PKG_VERSION` alone let every development rebuild within
//! one version serve the previous build's stale packs). When the
//! executable cannot be found or read, the identity falls back to the
//! crate version: the pre-hash behavior, never a failure.

use std::sync::OnceLock;

/// The identity of `bytes` as a pack header carries it: blake3, hex.
fn identity_of(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// This process's binary identity, computed once and cached.
pub fn binary_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| identity_of(&bytes))
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::{binary_identity, identity_of};

    #[test]
    fn the_identity_is_the_blake3_hex_of_the_bytes() {
        assert_eq!(
            identity_of(b"payload"),
            blake3::hash(b"payload").to_hex().to_string(),
        );
        assert_eq!(identity_of(b"payload").len(), 64);
    }

    /// The fallback branch (`current_exe` failing) cannot be driven from
    /// a test: the test binary exists and is readable by construction.
    /// What can be pinned is that the fallback did NOT fire here — the
    /// identity is a 64-character hash, not a version string — and that
    /// repeated calls answer the same interned value.
    #[test]
    fn the_binary_identity_is_stable_and_hash_shaped() {
        let first = binary_identity();
        assert_eq!(first, binary_identity());
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
    }
}
```

Then register the module: in `crates/celerrate_cli/src/cache/mod.rs`,
the module list at the top becomes:

```rust
pub mod identity;
pub mod pack;
pub mod snapshot;
pub mod stored;
pub mod verdict;
```

Add a failing test to the `tests` module of
`crates/celerrate_cli/src/cache/pack.rs`:

```rust
    /// Audit finding I1: keying the header on `CARGO_PKG_VERSION` alone
    /// let development rebuilds within one version accept each other's
    /// packs — stale messages served byte-for-byte, newly added rules
    /// silently missing. The header now carries the executable's own
    /// content hash: two different binaries never speak.
    #[test]
    fn the_header_carries_the_binary_self_hash() {
        assert_eq!(header().binary, super::super::identity::binary_identity());
    }
```

- [ ] **Step 2: Run the tests to verify the new one fails**

Run: `cargo test -p celerrate_cli the_header_carries_the_binary_self_hash`
Expected: FAIL — `header().binary` is `"0.1.0"`-style (`CARGO_PKG_VERSION`), not the hash.
(The two `identity` unit tests should already pass: the module is self-contained.)

- [ ] **Step 3: Wire the header and re-document the schema constant**

In `crates/celerrate_cli/src/cache/pack.rs`, change `PackHeader::current`:

```rust
impl PackHeader {
    /// The header of this binary analyzing under `range`.
    pub fn current(range: PhpVersionRange) -> Self {
        Self {
            schema: CACHE_SCHEMA_VERSION,
            binary: super::identity::binary_identity().to_owned(),
            stub_blob: *blake3::hash(celerrate_stubs::EMBEDDED_STUB_BLOB).as_bytes(),
            php_minimum: (range.minimum.major, range.minimum.minor),
            php_maximum: (range.maximum.major, range.maximum.minor),
        }
    }
}
```

And replace the `CACHE_SCHEMA_VERSION` doc comment's first paragraph
(keep the existing `/// 2: ...` history paragraph unchanged below it):

```rust
/// Bumped on a deliberate break of the stored shapes. The header also
/// carries the binary's own content hash, so any rebuild already
/// discards packs on its own; this constant is no longer what protects
/// development builds (the self-hash carries that), it is the named,
/// reviewable record of deliberate format breaks.
```

- [ ] **Step 4: Run the full CLI test suite**

Run: `cargo test -p celerrate_cli`
Expected: PASS — every existing test builds its expected header through
`PackHeader::current` in the same process, so writes and reads agree on
the new identity automatically.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/cache/identity.rs crates/celerrate_cli/src/cache/mod.rs crates/celerrate_cli/src/cache/pack.rs
git commit -m "🐛 fix(cache): key binary identity on the executable's hash"
```

---

### Task 2: Out-of-bounds stored spans are discarded (M4)

**Files:**
- Modify: `crates/celerrate_cli/src/cache/stored.rs` (`StoredDiagnostic::to_diagnostic` signature and checks, plus its unit tests)
- Modify: `crates/celerrate_cli/src/analysis.rs` (`analyze_one` call site)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`collect_entries` call site)

**Interfaces:**
- Changes: `StoredDiagnostic::to_diagnostic(&self, file: FileId, content_length: u32) -> Option<Diagnostic>`
  — the new second parameter is the analyzed file's byte length; an
  entry whose `end` exceeds it is discarded (`None`), same as a
  reversed range or an unknown identifier. Every later task uses this
  two-argument signature.
- Call sites compute the length as
  `u32::try_from(file.bytes(database).len()).unwrap_or(0)` — a file
  over `u32::MAX` bytes cannot carry valid `TextSize` spans anyway, so
  answering 0 discards honestly.

- [ ] **Step 1: Write the failing unit test**

In the `tests` module of `crates/celerrate_cli/src/cache/stored.rs`:

```rust
    /// Audit finding M4: a crafted span past the file's end was accepted
    /// and rendered with an oversized column — a hit that is not
    /// byte-for-byte anything the computation could produce. The content
    /// the entry's key hashes is available at both call sites, so the
    /// length is checked here, like the ordering.
    #[test]
    fn a_span_past_the_files_end_is_rejected() {
        let oversized = StoredDiagnostic {
            id: "CEL0018".to_owned(),
            severity: StoredSeverity::Error,
            start: 10,
            end: 40,
            message: "crafted".to_owned(),
        };
        assert!(oversized.to_diagnostic(FileId::new(9), 20).is_none());
        assert!(
            oversized.to_diagnostic(FileId::new(9), 40).is_some(),
            "a span ending exactly at the file's end is valid",
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p celerrate_cli a_span_past_the_files_end_is_rejected`
Expected: FAIL to compile — `to_diagnostic` takes one argument.

- [ ] **Step 3: Change the signature and the check**

In `crates/celerrate_cli/src/cache/stored.rs`, replace `to_diagnostic`
(extend its doc comment's first sentence to name the third rejection):

```rust
    /// `None` when the stored identifier is unknown to the registry (the
    /// entry comes from another era), the stored range has `start > end`
    /// (the entry cannot come from any real computation: `TextRange::new`
    /// asserts the ordering and panics otherwise), or the range reaches
    /// past `content_length` (no computation over these bytes could have
    /// produced it). Either way the answer is the same: discard the entry
    /// and let the file recompute. The blake3 checksum a pack carries
    /// proves only that its bytes were not corrupted in transit, never
    /// that whoever wrote them was honest, so both bounds must be checked
    /// here rather than trusted.
    pub fn to_diagnostic(&self, file: FileId, content_length: u32) -> Option<Diagnostic> {
        if self.start > self.end || self.end > content_length {
            return None;
        }
        Some(Diagnostic {
            id: find_identifier(&self.id)?,
            severity: match self.severity {
                StoredSeverity::Warning => Severity::Warning,
                StoredSeverity::Error => Severity::Error,
            },
            file,
            range: TextRange::new(TextSize::from(self.start), TextSize::from(self.end)),
            message: self.message.clone(),
        })
    }
```

Update the two existing unit tests in the same file that call it —
`a_diagnostic_round_trips_and_an_unknown_identifier_is_rejected` (spans
end at 12; pass `100`), `a_reversed_range_is_rejected_without_panicking`
(pass `100`), and `an_empty_range_round_trips` (pass `100`):
each `to_diagnostic(FileId::new(9))` becomes `to_diagnostic(FileId::new(9), 100)`.

Update the two production call sites:

In `crates/celerrate_cli/src/analysis.rs`, `analyze_one` becomes:

```rust
fn analyze_one(inputs: &AnalysisInputs, file: SourceFile) -> Result<Vec<Diagnostic>, FileId> {
    let database = &inputs.database;
    let file_id = file.file_id(database);
    let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
    guarded(file_id, || {
        if let Some(stored) = crate::cache::verdict::validated_verdict(inputs, file)
            && let Some(diagnostics) = stored
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length))
                .collect::<Option<Vec<_>>>()
        {
            return diagnostics;
        }
        let mut diagnostics = celerrate_db::file_diagnostics(database, file).clone();
        diagnostics.extend(
            celerrate_semantics::semantic_diagnostics(
                database,
                file,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
            )
            .iter()
            .cloned(),
        );
        diagnostics
    })
}
```

In `crates/celerrate_cli/src/cache/mod.rs`, inside `collect_entries`'s
verdict loop, the mirror check becomes:

```rust
        let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
        // Mirrors `analyze_one`: a validated hit is only reused when
        // every stored diagnostic still re-interns, or `persist` would
        // re-persist an entry the pass itself refused to serve.
        let stored = match verdict::validated_verdict(inputs, file) {
            Some(stored)
                if stored
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.to_diagnostic(file_id, content_length).is_some()) =>
            {
                stored.clone()
            }
            _ => composed_verdict(inputs, file),
        };
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p celerrate_cli`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/cache/stored.rs crates/celerrate_cli/src/analysis.rs crates/celerrate_cli/src/cache/mod.rs
git commit -m "🐛 fix(cache): discard stored spans that exceed the file"
```

---

### Task 3: Crash-debris sweep and atomic `.gitignore` (M2, M8)

**Files:**
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`prepare_directory`, new `sweep_crash_debris`, new test)

**Interfaces:**
- Consumes: `pack::write_atomically(path, bytes)` from Task 0 state (already exists).
- Produces: nothing new outside the module; `prepare_directory` stays private.

- [ ] **Step 1: Write the failing test**

In the `tests` module of `crates/celerrate_cli/src/cache/mod.rs`:

```rust
    /// Audit finding M2: `write_atomically`'s temporary files (the
    /// `.tmp` prefix `tempfile` uses) survive SIGKILL and power loss in
    /// `.celerrate/cache/`, and nothing ever swept them. `persist` now
    /// sweeps them best-effort; anything not matching the prefix is
    /// someone else's file and stays.
    #[test]
    fn crash_debris_is_swept_and_other_files_are_not() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());

        let cache_directory = root.path().join(".celerrate/cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        std::fs::write(cache_directory.join(".tmpAbC123"), b"debris").unwrap();
        std::fs::write(cache_directory.join("unrelated.bin"), b"not ours").unwrap();

        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: Vec::new(),
        };
        super::persist(&mut session, &outcome);

        assert!(
            !cache_directory.join(".tmpAbC123").exists(),
            "the crash debris is gone",
        );
        assert!(
            cache_directory.join("unrelated.bin").exists(),
            "only the .tmp prefix is ours to sweep",
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p celerrate_cli crash_debris_is_swept`
Expected: FAIL — the `.tmpAbC123` file still exists.

- [ ] **Step 3: Implement the sweep and make the `.gitignore` write atomic**

In `crates/celerrate_cli/src/cache/mod.rs`, replace `prepare_directory`:

```rust
/// Creates the cache directory and its self-ignoring `.gitignore`, and
/// sweeps crash debris. The `.gitignore` goes through the atomic write
/// (audit finding M8): a plain write torn by a crash left a half-written
/// file that was never repaired, since only existence is checked.
fn prepare_directory(cache_directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_directory)?;
    sweep_crash_debris(cache_directory);
    if let Some(dot_celerrate) = cache_directory.parent() {
        let gitignore = dot_celerrate.join(".gitignore");
        if !gitignore.exists() {
            pack::write_atomically(&gitignore, b"*\n")?;
        }
    }
    Ok(())
}

/// Best-effort removal of temporary files a crash mid-write left behind
/// (audit finding M2): `write_atomically`'s temporaries carry the `.tmp`
/// prefix `tempfile` uses, survive SIGKILL and power loss, and nothing
/// else ever removes them. A concurrent process mid-persist can lose its
/// temporary to this sweep; its rename then fails, that persist is
/// skipped, and its next pass rewrites — the same best-effort answer as
/// any other write failure.
fn sweep_crash_debris(cache_directory: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(".tmp") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p celerrate_cli`
Expected: PASS — including the existing
`a_completed_run_writes_both_packs_and_the_gitignore` integration test,
which pins the `.gitignore` content `"*\n"` across the M8 change.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/cache/mod.rs
git commit -m "🐛 fix(cache): sweep crash debris, write .gitignore atomically"
```

---

### Task 4: The stub-blob header field gets its tests (I3)

**Files:**
- Modify: `crates/celerrate_cli/src/cache/pack.rs` (one decode test)
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs` (one seeding test)

**Interfaces:**
- Consumes: `PackHeader` (public fields), `encode`/`decode`,
  `write_item_trees_pack` helper already in `cache_seeding.rs`.
- Produces: tests only, no production change.

- [ ] **Step 1: Write both tests**

In `crates/celerrate_cli/src/cache/pack.rs`, extend the existing
`a_header_mismatch_discards_the_whole_pack` test with a fourth case
(after the `other_binary` block):

```rust
        let mut other_stub = header();
        other_stub.stub_blob[0] ^= 0xFF;
        assert!(
            decode::<Vec<(u32, String)>>(&bytes, &other_stub).is_none(),
            "the stub-blob field is load-bearing: a new snapshot changes availability answers",
        );
```

In `crates/celerrate_cli/tests/cache_seeding.rs`, after
`a_range_mismatch_ignores_the_pack`:

```rust
/// A pack written under another stub snapshot is ignored whole (audit
/// finding I3): the field's whole purpose is "a new snapshot changes
/// availability answers", and it was the one header field no test
/// proved discards the pack.
#[test]
fn a_stub_blob_mismatch_ignores_the_pack() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let probe = StoredItemTree::of(&ItemTree::default());
    let mut foreign_stub = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    foreign_stub.stub_blob[0] ^= 0xFF;
    write_item_trees_pack(root.path(), &foreign_stub, vec![(hash, probe)]);

    let session = Session::start(root.path());
    let (_, &file) = session.sources.iter().next().unwrap();
    assert_eq!(
        item_tree(&session.database, file).declarations.len(),
        1,
        "the mismatched pack is ignored and the file is lowered",
    );
}
```

- [ ] **Step 2: Run them to verify they pass (coverage tests pin existing behavior)**

Run: `cargo test -p celerrate_cli stub_blob`
Expected: PASS — the header comparison at `decode` already covers every
field; these tests exist so a regression (for example hashing the wrong
thing into `stub_blob`) ships seen. If either FAILS, that is a real
defect: stop and investigate before continuing.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_cli/src/cache/pack.rs crates/celerrate_cli/tests/cache_seeding.rs
git commit -m "✅ test(cache): pin the stub-blob header field"
```

---

### Task 5: The vendor boundary, checked at pack level (I4)

**Files:**
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs` (one test)

**Interfaces:**
- Consumes: `run_check` helper, `Pack`/`decode`, `Session::start`,
  `PackHeader::current`, `session.configuration.php_version_range(&session.database)`.
- Produces: tests only.

- [ ] **Step 1: Write the test**

In `crates/celerrate_cli/tests/cache_seeding.rs` (the Composer package
shape mirrors `tests/cache_consistency.rs`'s
`a_composer_project_replays_consistently`, which documents why
`installed.json` must declare the package):

```rust
/// The spec's boundary, checkable on disk (audit finding I4): an
/// installed dependency is indexed — its item tree is in the pack,
/// which is what makes its symbols resolve on a warm start — but never
/// reported: no diagnostics entry may exist under its content hash.
#[test]
fn a_vendor_file_has_a_tree_entry_and_no_diagnostics_entry() {
    let vendor_source = "<?php namespace Lib; class Helper {}";
    let project_source = "<?php namespace App; use Lib\\Helper; new Helper();";
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("vendor/lib/src/Helper.php", vendor_source),
        (
            "vendor/composer/installed.json",
            r#"{"packages": [{"name": "acme/lib", "install-path": "../lib",
               "autoload": {"psr-4": {"Lib\\": "src/"}}}]}"#,
        ),
        ("src/App.php", project_source),
    ]);
    let (_, _) = run_check(root.path());

    // The expected header is derived from the project's own discovered
    // range, not hard-coded: `^8.2` maps to whatever maximum the binary
    // supports, and this test must not re-derive that rule.
    let session = Session::start(root.path());
    let header = PackHeader::current(session.configuration.php_version_range(&session.database));

    let vendor_hash = *blake3::hash(vendor_source.as_bytes()).as_bytes();
    let project_hash = *blake3::hash(project_source.as_bytes()).as_bytes();

    let bytes = std::fs::read(root.path().join(".celerrate/cache/").join(ITEM_TREES_PACK)).unwrap();
    let trees: Pack<Vec<([u8; 32], StoredItemTree)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let tree_keys: Vec<[u8; 32]> = trees.entries.iter().map(|(key, _)| *key).collect();
    assert!(tree_keys.contains(&vendor_hash), "the vendor file is indexed");
    assert!(tree_keys.contains(&project_hash));

    let bytes =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();
    let verdicts: Pack<Vec<([u8; 32], StoredVerdict)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let verdict_keys: Vec<[u8; 32]> = verdicts.entries.iter().map(|(key, _)| *key).collect();
    assert!(
        !verdict_keys.contains(&vendor_hash),
        "an installed dependency never gets a diagnostics entry",
    );
    assert!(verdict_keys.contains(&project_hash), "the project file does");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p celerrate_cli a_vendor_file_has_a_tree_entry`
Expected: PASS (pins existing behavior). A FAIL is a real defect: stop
and investigate.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_cli/tests/cache_seeding.rs
git commit -m "✅ test(cache): pin the vendor boundary at pack level"
```

---

### Task 6: Concurrency — torn reads and cross-pack independence (I5)

**Files:**
- Modify: `crates/celerrate_cli/src/cache/pack.rs` (reader-racing-writer test)
- Modify: `crates/celerrate_cli/tests/cache_consistency.rs` (mixed-generation test)

**Interfaces:**
- Consumes: `encode`, `decode`, `write_atomically`, the `run_check` and
  `normalized` helpers already in `cache_consistency.rs`.
- Produces: tests only.

- [ ] **Step 1: Write the reader-racing-writer test**

In the `tests` module of `crates/celerrate_cli/src/cache/pack.rs`:

```rust
    /// The atomicity clause is about concurrency (audit finding I5):
    /// "a reader never sees a torn file and a concurrent writer's last
    /// rename wins whole". One writer alternates two payloads while
    /// this thread reads; every observed read must be byte-for-byte one
    /// of the two payloads and must decode whole.
    #[test]
    fn a_reader_racing_a_writer_never_sees_a_torn_pack() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pack.bin");
        let first = encode(&sample()).unwrap();
        let second = encode(&Pack {
            header: header(),
            entries: vec![(9, "nine".to_owned())],
        })
        .unwrap();
        write_atomically(&path, &first).unwrap();

        let writer_path = path.clone();
        let writer_first = first.clone();
        let writer_second = second.clone();
        let writer = std::thread::spawn(move || {
            for round in 0..200 {
                let bytes = if round % 2 == 0 {
                    &writer_second
                } else {
                    &writer_first
                };
                write_atomically(&writer_path, bytes).unwrap();
            }
        });

        for _ in 0..200 {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                bytes == first || bytes == second,
                "a read observed bytes that are neither payload: torn",
            );
            let decoded: Option<Pack<Vec<(u32, String)>>> = decode(&bytes, &header());
            assert!(decoded.is_some(), "every observed read decodes whole");
        }
        writer.join().unwrap();
    }
```

- [ ] **Step 2: Write the cross-pack independence test**

In `crates/celerrate_cli/tests/cache_consistency.rs`:

```rust
/// The invariant that makes two concurrent `celerrate check` processes
/// safe today, named and pinned (audit finding I5): both packs' entries
/// are independently content-keyed and revalidated, so packs from two
/// different generations of the project may be mixed freely — a stale
/// pack beside a fresh one must render exactly what a fresh run
/// renders. A future pack keyed over the *set* of tree hashes would
/// break exactly this; this test is the tripwire that says so.
#[test]
fn packs_from_different_generations_mix_safely() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.php"), "<?php new Missing();").unwrap();
    let _ = run_check(root.path());
    let cache = root.path().join(".celerrate/cache");
    let stale_verdicts = std::fs::read(cache.join("diagnostics.bin")).unwrap();
    let stale_trees = std::fs::read(cache.join("item_trees.bin")).unwrap();

    // Second generation: a defining file appears, and a full run
    // refreshes both packs and the expected rendering.
    std::fs::write(root.path().join("b.php"), "<?php class Missing {}").unwrap();
    let baseline = normalized(&run_check(root.path()), root.path());

    // Stale verdicts beside fresh trees: the stale entry's recorded
    // `Unknown` answer no longer holds, revalidation discards it.
    std::fs::write(cache.join("diagnostics.bin"), &stale_verdicts).unwrap();
    assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

    // Stale trees beside fresh verdicts: the new file's tree is simply
    // absent from the stale pack, a miss, recomputed.
    std::fs::write(cache.join("item_trees.bin"), &stale_trees).unwrap();
    assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
}
```

- [ ] **Step 3: Run both**

Run: `cargo test -p celerrate_cli a_reader_racing_a_writer && cargo test -p celerrate_cli packs_from_different_generations`
Expected: PASS (pins existing behavior). A FAIL is a real defect.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_cli/src/cache/pack.rs crates/celerrate_cli/tests/cache_consistency.rs
git commit -m "✅ test(cache): pin torn-read safety and cross-pack independence"
```

---

### Task 7: Persist per watch cycle, proven (I6)

**Files:**
- Modify: `crates/celerrate_cli/src/watch.rs` (extract `completed_cycle` from the `watch` loop; one test)

**Interfaces:**
- Produces (private to `watch.rs`, reachable from its test module):
  `fn completed_cycle(session: &mut Session, watcher: &mut Watch, output: &mut dyn Write, reanalyzed: usize) -> Result<AnalysisOutcome, Outcome>`
  — one complete iteration: forget prior pass errors, analyze
  (restarting as edits land), absorb, render, **persist**. `Err` carries
  the `Outcome` the watch loop must return with.

- [ ] **Step 1: Extract the iteration**

In `crates/celerrate_cli/src/watch.rs`, add above `watch` (moving the
loop-body comments with the code they describe):

```rust
/// One complete watch iteration up to and including the persist.
/// Extracted from the loop so a test can drive exactly what an
/// iteration does to the packs on disk — the cache spec's "rewritten
/// after every completed analysis, including every `--watch` iteration"
/// clause (audit finding I6) — without needing a channel event to stop
/// the loop.
fn completed_cycle(
    session: &mut Session,
    watcher: &mut Watch,
    output: &mut dyn Write,
    reanalyzed: usize,
) -> Result<AnalysisOutcome, Outcome> {
    let started = Instant::now();
    // Every cycle re-analyzes, so every cycle also recomputes what the
    // analysis can go wrong about. Last cycle's panics are dropped
    // before this one speaks: the picture is always complete, never a
    // stale log of past edits, and that has to hold for the
    // internal-error block too.
    session.forget_analysis_errors();
    let outcome = match cycle(session, watcher) {
        Ok(outcome) => outcome,
        Err(error) => return Err(unwatchable(output, &error)),
    };
    session.absorb_outcome(&outcome);
    // What the watch is not observing is part of the picture, and it is
    // read from the watch that is in place now: `cycle` may have
    // respawned it, and the picture must describe the watch the next
    // burst will come from, not the one this cycle started with.
    watcher.report_unwatchable_paths(session);
    if render::render_cycle(output, session, &outcome, reanalyzed, started.elapsed()).is_err() {
        return Err(Outcome::InternalError);
    }
    crate::cache::persist(session, &outcome);
    Ok(outcome)
}
```

And replace the body of `watch`'s loop head (from `let started = ...`
through the `crate::cache::persist(...)` line) with:

```rust
    let mut reanalyzed = session.sources.len();
    loop {
        let outcome = match completed_cycle(session, &mut watcher, output, reanalyzed) {
            Ok(outcome) => outcome,
            Err(ended) => return ended,
        };

        let changed = wait_for_a_burst(watcher.events());
```

(the rest of the loop — the empty-burst return, `session.absorb`,
`watcher.resynchronize`, `reanalyzed = changed.len()` — is unchanged).

- [ ] **Step 2: Run the suite to verify the extraction is behavior-neutral**

Run: `cargo test -p celerrate_cli`
Expected: PASS.

- [ ] **Step 3: Write the persist-per-cycle test**

In the `tests` module of `crates/celerrate_cli/src/watch.rs`:

```rust
    /// The cache spec's persist clause at the watch level (audit finding
    /// I6): after a cycle absorbs an edit, the packs on disk carry the
    /// cycle's results — proven by decoding the diagnostics pack and
    /// finding the edited content's hash keyed in it, not by reading the
    /// source of `watch`.
    #[test]
    fn a_cycle_rewrites_the_packs_with_its_results() {
        use crate::cache::pack::{Pack, PackHeader, decode};
        use crate::cache::stored::StoredVerdict;

        let root = tempfile::tempdir().unwrap();
        let edited = root.path().join("a.php");
        std::fs::write(&edited, "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();
        let mut output = Vec::new();

        let first = super::completed_cycle(&mut session, &mut watcher, &mut output, 1).unwrap();
        assert!(first.diagnostics.is_empty(), "sanity: the initial state is clean");
        let diagnostics_pack = root.path().join(".celerrate/cache/diagnostics.bin");
        let after_first = std::fs::read(&diagnostics_pack).unwrap();

        let edited_source = "<?php class A {} new Missing();";
        std::fs::write(&edited, edited_source).unwrap();
        session.absorb(&[edited.clone()]);
        let second = super::completed_cycle(&mut session, &mut watcher, &mut output, 1).unwrap();
        assert_eq!(second.diagnostics.len(), 1, "the cycle sees the edit");

        let after_second = std::fs::read(&diagnostics_pack).unwrap();
        assert_ne!(after_first, after_second, "the cycle's persist rewrote the pack");

        let header = PackHeader::current(
            session.configuration.php_version_range(&session.database),
        );
        let pack: Pack<Vec<([u8; 32], StoredVerdict)>> =
            decode(&after_second, &header).unwrap();
        assert!(
            pack.entries
                .iter()
                .any(|(key, _)| key == blake3::hash(edited_source.as_bytes()).as_bytes()),
            "the pack on disk is keyed by the edited content",
        );
    }
```

Note: the direct `session.absorb` call is what makes the test
deterministic — it does not wait on `notify` delivery. The watcher may
additionally deliver the same edit as an event during the second
`cycle`; `cycle` then absorbs identical bytes and restarts, which is
harmless and converges.

- [ ] **Step 4: Run it**

Run: `cargo test -p celerrate_cli a_cycle_rewrites_the_packs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/watch.rs
git commit -m "✅ test(watch): prove every cycle persists the packs"
```

---

### Task 8: The checksum-valid adversarial entry matrix (I7)

**Files:**
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs` (four tests)

**Interfaces:**
- Consumes: the `project`, `write_item_trees_pack`,
  `write_diagnostics_pack`, `probe_verdict`, `run_check` helpers already
  in the file; `celerrate_semantics::{AstId, Declaration, DeclarationKind}`
  (public, constructed by `stored.rs` itself); `StoredRecord` is already
  imported.
- Produces: tests only. Decision recorded here (spec section 3): the
  fuzz waiver for the pack format is **kept** — after this matrix, every
  post-checksum conversion failure mode (`start > end`, span past end,
  unknown identifier, absurd `ast_index`, duplicate keys, empty record
  lists) is individually pinned, postcard's decode is structurally
  panic-free on hostile input, and no unchecked indexing survives
  (`indexing_slicing` is denied workspace-wide). The close-out amendment
  (Task 13) states this.

- [ ] **Step 1: Write the matrix**

Append to `crates/celerrate_cli/tests/cache_seeding.rs`. Extend the
existing `use celerrate_semantics::...` import line with `AstId`,
`Declaration`, `DeclarationKind`:

```rust
// ---------------------------------------------------------------------
// The checksum-valid adversarial matrix (audit finding I7): entries
// that pass the checksum but could not come from any real computation.
// This is the only hostile class that reaches the post-decode
// conversion code. The contract for every row: never a panic, never a
// user-visible internal error; entries that fail conversion recompute
// honestly.
// ---------------------------------------------------------------------

/// A span reaching past the file's end is discarded and the file
/// recomputed (pins the M4 fix at integration level).
#[test]
fn a_verdict_with_a_span_past_the_files_end_is_discarded() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.diagnostics[0].start = 100;
    verdict.diagnostics[0].end = 200;
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert!(outcome.panicked.is_empty(), "{:?}", outcome.panicked);
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message != "planted by the cache probe"),
        "the oversized entry must not be served: {:?}",
        outcome.diagnostics,
    );
    assert_eq!(outcome.diagnostics.len(), 1, "recomputed honestly");
    assert!(outcome.diagnostics[0].message.contains("Missing"));
}

/// A stored tree whose declaration names an AST index no tree of this
/// file has. Whatever the engine answers about the declaration, it must
/// answer without panicking and without an internal error: `AstId`
/// lookups never index unchecked.
#[test]
fn an_item_tree_with_an_absurd_ast_index_never_panics() {
    let source = "<?php class Marker {} new Marker();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();

    let mut lying_tree = ItemTree::default();
    lying_tree.declarations.push(Declaration {
        kind: DeclarationKind::Class,
        name: "Marker".to_owned(),
        namespace: String::new(),
        ast_id: AstId {
            file: celerrate_source::FileId::new(0),
            index: u32::MAX,
        },
        extends: Vec::new(),
        implements: Vec::new(),
        trait_uses: Vec::new(),
    });
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_item_trees_pack(root.path(), &header, vec![(hash, StoredItemTree::of(&lying_tree))]);

    let (outcome, _) = run_check(root.path());
    assert_ne!(
        outcome,
        celerrate_cli::Outcome::InternalError,
        "an absurd AST index must never surface as an internal error",
    );
}

/// Two entries under one content hash: the loader collects into a map,
/// so one wins; which one is not contractual. What is: no panic, no
/// internal error.
#[test]
fn duplicate_keys_in_a_pack_never_panic() {
    let source = "<?php class Marker {}";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_item_trees_pack(
        root.path(),
        &header,
        vec![
            (hash, StoredItemTree::of(&ItemTree::default())),
            (hash, StoredItemTree::of(&parsed_marker_tree(source))),
        ],
    );

    let (outcome, _) = run_check(root.path());
    assert_ne!(outcome, celerrate_cli::Outcome::InternalError);
}

/// Builds the honest tree for `source`, so the duplicate above differs.
fn parsed_marker_tree(source: &str) -> ItemTree {
    let parse = celerrate_syntax::parse(source);
    ItemTree::from_root(celerrate_source::FileId::new(0), &parse.tree())
}

/// An entry with no records vacuously revalidates and its diagnostics
/// are served as-is. Within the accepted threat model (whoever writes
/// the pack controls the project), the contract is only no panic and
/// no internal error — plus the write-side invariant below, which is
/// why honest packs never look like this.
#[test]
fn an_empty_record_list_is_served_without_panicking() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let mut verdict = probe_verdict();
    verdict.records = Vec::new();
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    write_diagnostics_pack(root.path(), &header, vec![(hash, verdict)]);

    let (outcome, _) = run_check(root.path());
    assert_ne!(outcome, celerrate_cli::Outcome::InternalError);
}

/// The write side of the invariant above: a persisted verdict for a
/// file that references names always carries revalidation records, so
/// an honest pack can never hit the vacuous-acceptance path.
#[test]
fn persist_records_every_referencing_files_lookups() {
    let source = "<?php new Missing();";
    let root = project(&[("a.php", source)]);
    let (_, _) = run_check(root.path());

    let hash = *blake3::hash(source.as_bytes()).as_bytes();
    let header = PackHeader::current(PhpVersionRange::point(PhpVersion::new(8, 5)));
    let bytes =
        std::fs::read(root.path().join(".celerrate/cache/").join(DIAGNOSTICS_PACK)).unwrap();
    let pack: Pack<Vec<([u8; 32], StoredVerdict)>> =
        celerrate_cli::cache::pack::decode(&bytes, &header).unwrap();
    let (_, persisted) = pack.entries.iter().find(|(key, _)| *key == hash).unwrap();
    assert!(
        !persisted.records.is_empty(),
        "a referencing file's verdict must carry its records",
    );
}
```

Add `celerrate_syntax` to `crates/celerrate_cli/Cargo.toml`
`[dev-dependencies]` if it is not already there (check first: `grep
celerrate_syntax crates/celerrate_cli/Cargo.toml`).

- [ ] **Step 2: Run the matrix**

Run: `cargo test -p celerrate_cli --test cache_seeding`
Expected: PASS. The span-past-end row exercises the M4 fix (Task 2);
the others pin existing behavior. A FAIL on any row is a real defect:
stop and investigate before weakening any assertion.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_cli/tests/cache_seeding.rs crates/celerrate_cli/Cargo.toml
git commit -m "✅ test(cache): checksum-valid adversarial entry matrix"
```

---

### Task 9: The verdict lookup names its three answers; the statistics structure (I8 part 1)

**Files:**
- Modify: `crates/celerrate_cli/src/cache/verdict.rs` (`VerdictLookup`, `lookup_verdict`, `validated_verdict` as wrapper)
- Create: `crates/celerrate_cli/src/cache/statistics.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (add `pub mod statistics;`)

**Interfaces:**
- Produces:
  - `celerrate_cli::cache::verdict::VerdictLookup<'a>` with variants
    `Hit(&'a StoredVerdict)`, `Discarded`, `Absent`.
  - `celerrate_cli::cache::verdict::lookup_verdict(inputs: &AnalysisInputs, file: SourceFile) -> VerdictLookup<'_>`.
  - `validated_verdict` keeps its exact signature
    (`Option<&StoredVerdict>`) as a wrapper — the persist path keeps
    using it, deliberately without counters.
  - `celerrate_cli::cache::statistics::CacheStatistics` with public
    `AtomicU64` fields `tree_hits`, `tree_misses`, `verdicts_served`,
    `verdicts_discarded`, `verdicts_absent`, `persist_written`,
    `persist_skipped`, `persist_failed`; methods
    `render(&self) -> String` and `report(&self)` (stderr, gated on
    `CELERRATE_CACHE_STATS=1`).

- [ ] **Step 1: Write the failing statistics tests**

Create `crates/celerrate_cli/src/cache/statistics.rs`:

```rust
//! Cache observability (audit findings I8 and M5): cheap process-wide
//! counters, printed to stderr when `CELERRATE_CACHE_STATS=1`. The
//! counters never feed analysis — salsa's determinism is untouched —
//! and the stderr line is not a contractual surface; it exists so the
//! parent spec's economics rule ("an artifact class that does not pay
//! for itself is dropped") is measurable without a profiler: hit rate,
//! revalidation acceptance, and persist health, per class. The
//! environment variable is read here, at the orchestration layer,
//! never inside a query.

use std::sync::atomic::{AtomicU64, Ordering};

/// One session's cache traffic. Atomic because the item-tree lookups
/// happen under the rayon fan-out.
#[derive(Debug, Default)]
pub struct CacheStatistics {
    /// Item-tree lookups answered from the pack.
    pub tree_hits: AtomicU64,
    /// Item-tree lookups the pack could not answer.
    pub tree_misses: AtomicU64,
    /// Verdicts served: present, every record revalidated, every
    /// diagnostic converted.
    pub verdicts_served: AtomicU64,
    /// Verdicts present but refused: a record's answer moved, or a
    /// stored diagnostic failed conversion.
    pub verdicts_discarded: AtomicU64,
    /// Verdicts absent: no entry under the file's content hash.
    pub verdicts_absent: AtomicU64,
    /// Pack writes that happened.
    pub persist_written: AtomicU64,
    /// Pack writes skipped because nothing changed.
    pub persist_skipped: AtomicU64,
    /// Pack writes that failed — the silent failure of audit finding
    /// M5, now at least countable.
    pub persist_failed: AtomicU64,
}

impl CacheStatistics {
    /// The one-line summary the environment variable asks for.
    pub fn render(&self) -> String {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        format!(
            "cache: trees {} hit / {} miss; verdicts {} served / {} discarded / {} absent; persist {} written / {} skipped / {} failed",
            load(&self.tree_hits),
            load(&self.tree_misses),
            load(&self.verdicts_served),
            load(&self.verdicts_discarded),
            load(&self.verdicts_absent),
            load(&self.persist_written),
            load(&self.persist_skipped),
            load(&self.persist_failed),
        )
    }

    /// Prints the line to stderr when `CELERRATE_CACHE_STATS=1`.
    pub fn report(&self) {
        let variable = std::env::var("CELERRATE_CACHE_STATS").ok();
        if wants_statistics(variable.as_deref()) {
            eprintln!("{}", self.render());
        }
    }
}

/// The gate, as a pure function so it is testable without mutating the
/// process environment.
fn wants_statistics(value: Option<&str>) -> bool {
    value == Some("1")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::{CacheStatistics, wants_statistics};

    #[test]
    fn the_rendered_line_carries_every_counter() {
        let statistics = CacheStatistics::default();
        statistics.tree_hits.fetch_add(3, Ordering::Relaxed);
        statistics.verdicts_served.fetch_add(2, Ordering::Relaxed);
        statistics.persist_failed.fetch_add(1, Ordering::Relaxed);
        assert_eq!(
            statistics.render(),
            "cache: trees 3 hit / 0 miss; verdicts 2 served / 0 discarded / 0 absent; persist 0 written / 0 skipped / 1 failed",
        );
    }

    #[test]
    fn only_the_exact_opt_in_enables_the_report() {
        assert!(wants_statistics(Some("1")));
        assert!(!wants_statistics(Some("0")));
        assert!(!wants_statistics(Some("true")));
        assert!(!wants_statistics(None));
    }
}
```

Register it in `crates/celerrate_cli/src/cache/mod.rs`'s module list:

```rust
pub mod identity;
pub mod pack;
pub mod snapshot;
pub mod statistics;
pub mod stored;
pub mod verdict;
```

- [ ] **Step 2: Run the statistics tests**

Run: `cargo test -p celerrate_cli statistics`
Expected: PASS (the module is self-contained).

- [ ] **Step 3: Name the verdict lookup's three answers**

Replace the body of `crates/celerrate_cli/src/cache/verdict.rs` (the
module doc comment stays):

```rust
/// What the diagnostics pack answers for one file. The three cases are
/// distinct because the statistics distinguish them: a `Discarded` is
/// revalidation doing its job, an `Absent` is an ordinary cold miss.
pub enum VerdictLookup<'a> {
    /// Present and every record revalidated: the verdict may speak.
    Hit(&'a StoredVerdict),
    /// Present, but a recorded answer no longer holds: recompute.
    Discarded,
    /// No entry under this content hash: recompute.
    Absent,
}

/// Looks the file's verdict up and revalidates it.
pub fn lookup_verdict(inputs: &AnalysisInputs, file: SourceFile) -> VerdictLookup<'_> {
    let database = &inputs.database;
    let Some(stored) = inputs
        .cache
        .verdicts
        .get(&celerrate_db::content_hash(database, file))
    else {
        return VerdictLookup::Absent;
    };
    let sources = SymbolSources {
        files: inputs.files,
        stubs: inputs.stubs,
        configuration: inputs.configuration,
    };
    let tree = item_tree(database, file);
    let mut tables_by_namespace: HashMap<&str, UseTables> = HashMap::new();
    for record in &stored.records {
        let tables = tables_by_namespace
            .entry(record.namespace.as_str())
            .or_insert_with(|| UseTables::for_namespace(tree, &record.namespace));
        let answer = answer_of(resolve_name(
            database,
            sources,
            &record.namespace,
            tables,
            &record.written,
            record.space(),
        ));
        if !record.matches(answer) {
            return VerdictLookup::Discarded;
        }
    }
    VerdictLookup::Hit(stored)
}

/// The stored verdict if it may speak; `None` means recompute. This is
/// the persist path's mirror of the pass's decision, deliberately
/// without statistics attached: only the pass itself counts, or
/// `persist`'s re-lookup would double-count every file.
pub fn validated_verdict(inputs: &AnalysisInputs, file: SourceFile) -> Option<&StoredVerdict> {
    match lookup_verdict(inputs, file) {
        VerdictLookup::Hit(stored) => Some(stored),
        VerdictLookup::Discarded | VerdictLookup::Absent => None,
    }
}
```

- [ ] **Step 4: Run the suite (refactor is behavior-neutral)**

Run: `cargo test -p celerrate_cli`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/cache/verdict.rs crates/celerrate_cli/src/cache/statistics.rs crates/celerrate_cli/src/cache/mod.rs
git commit -m "✨ feat(cache): statistics structure and named verdict lookup"
```

---

### Task 10: Wire the counters through session, analysis, and persist (I8 part 2, M5)

**Files:**
- Modify: `crates/celerrate_cli/src/session.rs` (`Session.statistics`, `SnapshotCache` construction, `inputs()`)
- Modify: `crates/celerrate_cli/src/cache/snapshot.rs` (`SnapshotCache` gains the statistics)
- Modify: `crates/celerrate_cli/src/analysis.rs` (`AnalysisInputs.statistics`, `analyze_one` counts)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`PackWrite`, persist counts)
- Modify: `crates/celerrate_cli/src/lib.rs` and `crates/celerrate_cli/src/watch.rs` (`statistics.report()` after persist)
- Test: `crates/celerrate_cli/tests/cache_seeding.rs` (counter integration tests), `crates/celerrate_cli/src/cache/mod.rs` (persist-counter test)

**Interfaces:**
- Consumes: `CacheStatistics`, `VerdictLookup`, `lookup_verdict` from Task 9.
- Produces:
  - `Session.statistics: Arc<CacheStatistics>` (public field).
  - `AnalysisInputs.statistics: Arc<CacheStatistics>` (public field).
  - `SnapshotCache { snapshot: Arc<CacheSnapshot>, statistics: Arc<CacheStatistics> }`
    (named fields replace the tuple).
  - Private `enum PackWrite { Unchanged, Written, Failed }` in `cache/mod.rs`.

- [ ] **Step 1: Write the failing integration tests**

In `crates/celerrate_cli/tests/cache_seeding.rs`:

```rust
/// Audit finding I8: hit rate, revalidation acceptance, and persist
/// health were unobservable without a profiler. A warm session over an
/// unchanged project counts tree hits and served verdicts and nothing
/// discarded.
#[test]
fn a_warm_session_counts_tree_hits_and_served_verdicts() {
    use std::sync::atomic::Ordering;

    let root = project(&[("a.php", "<?php new Missing();")]);
    let (_, _) = run_check(root.path());

    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);

    let statistics = &session.statistics;
    assert!(
        statistics.tree_hits.load(Ordering::Relaxed) >= 1,
        "the warm pass served at least one tree from the pack",
    );
    assert_eq!(statistics.verdicts_served.load(Ordering::Relaxed), 1);
    assert_eq!(statistics.verdicts_discarded.load(Ordering::Relaxed), 0);
    assert_eq!(statistics.verdicts_absent.load(Ordering::Relaxed), 0);
}

/// The cold side: no pack, everything misses and every verdict is
/// absent.
#[test]
fn a_cold_session_counts_misses_and_absences() {
    use std::sync::atomic::Ordering;

    let root = project(&[("a.php", "<?php new Missing();")]);
    let session = Session::start(root.path());
    let outcome = analyze(&session.inputs()).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);

    let statistics = &session.statistics;
    assert!(statistics.tree_misses.load(Ordering::Relaxed) >= 1);
    assert_eq!(statistics.verdicts_absent.load(Ordering::Relaxed), 1);
    assert_eq!(statistics.verdicts_served.load(Ordering::Relaxed), 0);
}
```

In the `tests` module of `crates/celerrate_cli/src/cache/mod.rs`:

```rust
    /// Audit finding M5 through I8's counters: a persist that writes, a
    /// persist that skips, and a persist that fails are each counted, so
    /// a permanently unwritable cache directory is at least observable.
    #[test]
    fn persist_outcomes_are_counted() {
        use std::sync::atomic::Ordering;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let outcome = AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: Vec::new(),
        };

        super::persist(&mut session, &outcome);
        assert_eq!(session.statistics.persist_written.load(Ordering::Relaxed), 2);

        super::persist(&mut session, &outcome);
        assert_eq!(session.statistics.persist_skipped.load(Ordering::Relaxed), 2);

        // Obstruct one pack: its rename fails deterministically (rename
        // onto a directory), the other pack is unchanged.
        let cache_directory = root.path().join(".celerrate/cache");
        std::fs::remove_file(cache_directory.join(super::snapshot::ITEM_TREES_PACK)).unwrap();
        std::fs::create_dir(cache_directory.join(super::snapshot::ITEM_TREES_PACK)).unwrap();
        super::persist(&mut session, &outcome);
        assert_eq!(session.statistics.persist_failed.load(Ordering::Relaxed), 1);
    }
```

- [ ] **Step 2: Run them to verify they fail to compile**

Run: `cargo test -p celerrate_cli persist_outcomes_are_counted`
Expected: FAIL — `Session` has no `statistics` field.

- [ ] **Step 3: Wire everything**

`crates/celerrate_cli/src/cache/snapshot.rs` — replace `SnapshotCache`:

```rust
use super::statistics::CacheStatistics;
```

```rust
/// The snapshot as the artifact cache the semantics layer consults:
/// a lookup by content address, with the current file identity stamped
/// back in, counting hits and misses as it answers.
pub struct SnapshotCache {
    pub snapshot: Arc<CacheSnapshot>,
    pub statistics: Arc<CacheStatistics>,
}

impl ArtifactCache for SnapshotCache {
    fn item_tree(&self, file: FileId, content: ContentHash) -> Option<ItemTree> {
        use std::sync::atomic::Ordering;
        match self.snapshot.item_trees.get(&content) {
            Some(stored) => {
                self.statistics.tree_hits.fetch_add(1, Ordering::Relaxed);
                Some(stored.to_item_tree(file))
            }
            None => {
                self.statistics.tree_misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }
}
```

`crates/celerrate_cli/src/session.rs`:
- Add imports: `use crate::cache::statistics::CacheStatistics;`
- Add the field to `Session` (after `cache_loaded_range`):

```rust
    /// The session's cache counters, shared with the registered
    /// `SnapshotCache` and with every `AnalysisInputs` clone. Never
    /// read by analysis; rendered to stderr on opt-in.
    pub statistics: Arc<CacheStatistics>,
```

- In `Session::start`, before the `CacheSnapshot::load` block:

```rust
        let statistics = Arc::new(CacheStatistics::default());
```

- The `ArtifactCacheInput` registration becomes:

```rust
        let _ = ArtifactCacheInput::builder(CacheHandle(Arc::new(SnapshotCache {
            snapshot: cache.clone(),
            statistics: statistics.clone(),
        })))
        .durability(salsa::Durability::HIGH)
        .new(&database);
```

- The `Self { ... }` literal gains `statistics,`.
- `Session::inputs` gains `statistics: self.statistics.clone(),`.

`crates/celerrate_cli/src/analysis.rs`:
- `AnalysisInputs` gains (after `cache`):

```rust
    /// The session's cache counters. Written by the pass, never read
    /// by it: statistics do not feed analysis.
    pub statistics: Arc<crate::cache::statistics::CacheStatistics>,
```

- `analyze_one` becomes (this replaces the Task 2 version; the counting
  happens here and only here — `persist`'s mirror uses the uncounted
  `validated_verdict`):

```rust
fn analyze_one(inputs: &AnalysisInputs, file: SourceFile) -> Result<Vec<Diagnostic>, FileId> {
    use std::sync::atomic::Ordering;

    use crate::cache::verdict::VerdictLookup;

    let database = &inputs.database;
    let file_id = file.file_id(database);
    let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
    guarded(file_id, || {
        let statistics = &inputs.statistics;
        match crate::cache::verdict::lookup_verdict(inputs, file) {
            VerdictLookup::Hit(stored) => {
                if let Some(diagnostics) = stored
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length))
                    .collect::<Option<Vec<_>>>()
                {
                    statistics.verdicts_served.fetch_add(1, Ordering::Relaxed);
                    return diagnostics;
                }
                // Revalidated, but a stored diagnostic failed conversion:
                // the same refusal as a moved answer.
                statistics.verdicts_discarded.fetch_add(1, Ordering::Relaxed);
            }
            VerdictLookup::Discarded => {
                statistics.verdicts_discarded.fetch_add(1, Ordering::Relaxed);
            }
            VerdictLookup::Absent => {
                statistics.verdicts_absent.fetch_add(1, Ordering::Relaxed);
            }
        }
        let mut diagnostics = celerrate_db::file_diagnostics(database, file).clone();
        diagnostics.extend(
            celerrate_semantics::semantic_diagnostics(
                database,
                file,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
            )
            .iter()
            .cloned(),
        );
        diagnostics
    })
}
```

`crates/celerrate_cli/src/cache/mod.rs` — `write_when_changed` returns a
named outcome and `persist` counts it:

```rust
/// How one pack write ended.
enum PackWrite {
    /// Already on disk, byte-identical, under the current header.
    Unchanged,
    /// Encoded and atomically written.
    Written,
    /// Encoding or the atomic write failed; whatever was on disk before
    /// (if anything) is untouched.
    Failed,
}
```

`write_when_changed`'s signature and tail become:

```rust
fn write_when_changed<Entry: Serialize + PartialEq + Clone>(
    path: &Path,
    header: &PackHeader,
    entries: &[(ContentHash, Entry)],
    loaded: &HashMap<ContentHash, Entry>,
    header_moved: bool,
) -> PackWrite {
    let unchanged = !header_moved
        && entries.len() == loaded.len()
        && entries
            .iter()
            .all(|(key, value)| loaded.get(key) == Some(value));
    if unchanged && path.is_file() {
        return PackWrite::Unchanged;
    }
    let Some(bytes) = pack::encode(&Pack {
        header: header.clone(),
        entries: entries.to_vec(),
    }) else {
        return PackWrite::Failed;
    };
    if pack::write_atomically(path, &bytes).is_ok() {
        PackWrite::Written
    } else {
        PackWrite::Failed
    }
}
```

In `persist`, the directory-preparation failure counts both packs as
failed, and the write results are counted before the snapshot swap:

```rust
    if prepare_directory(&session.cache_directory).is_err() {
        session
            .statistics
            .persist_failed
            .fetch_add(2, std::sync::atomic::Ordering::Relaxed);
        return;
    }
```

```rust
    let trees_written = write_when_changed(/* unchanged arguments */);
    let verdicts_written = write_when_changed(/* unchanged arguments */);
    for write in [&trees_written, &verdicts_written] {
        let counter = match write {
            PackWrite::Unchanged => &session.statistics.persist_skipped,
            PackWrite::Written => &session.statistics.persist_written,
            PackWrite::Failed => &session.statistics.persist_failed,
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if !matches!(trees_written, PackWrite::Failed) && !matches!(verdicts_written, PackWrite::Failed)
    {
        session.cache = Arc::new(CacheSnapshot {
            item_trees: trees.into_iter().collect(),
            verdicts: verdicts.into_iter().collect(),
        });
        session.cache_loaded_range = current_range;
    }
```

(The snapshot-swap condition is behavior-identical to the old
`trees_written && verdicts_written`: swap unless something failed.)

Finally, the opt-in report after every persist:
- `crates/celerrate_cli/src/lib.rs`, in `run` after
  `cache::persist(&mut session, &outcome);`:

```rust
            session.statistics.report();
```

- `crates/celerrate_cli/src/watch.rs`, in `completed_cycle` after
  `crate::cache::persist(session, &outcome);`:

```rust
    session.statistics.report();
```

- [ ] **Step 4: Run everything**

Run: `cargo test -p celerrate_cli`
Expected: PASS, including the three new tests. The compiler is the
checklist for construction sites: `Session`'s literal, `inputs()`, and
the `SnapshotCache` registration are the only ones.

- [ ] **Step 5: Manual smoke check of the stderr line**

Run: `CELERRATE_CACHE_STATS=1 cargo run -p celerrate_cli -- check crates/celerrate_cli 2>&1 >/dev/null | tail -1`
Expected: one `cache: trees ... persist ...` line on stderr.
(Any project directory works; run it twice to see hits move.)

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_cli/src/session.rs crates/celerrate_cli/src/cache/snapshot.rs crates/celerrate_cli/src/analysis.rs crates/celerrate_cli/src/cache/mod.rs crates/celerrate_cli/src/lib.rs crates/celerrate_cli/src/watch.rs crates/celerrate_cli/tests/cache_seeding.rs
git commit -m "✨ feat(cache): count hits, revalidation, and persist outcomes"
```

---

### Task 11: One composition point for a file's diagnostics (I2, the cheap unification)

**Files:**
- Modify: `crates/celerrate_cli/src/analysis.rs` (extract `composed_diagnostics`)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`composed_verdict` consumes it — the first hand-maintained mirror dies)

**Interfaces:**
- Produces: `celerrate_cli::analysis::composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic>`
  (public — Task 12's equivalence harness recomputes through it). Order
  is the composition order (`file_diagnostics` then
  `semantic_diagnostics`), exactly what both call sites produced before.
- The second mirror (`resolution_records` versus `reference_diagnostics`
  traversal, in `celerrate_semantics`) is **deferred to the type-engine
  sub-project**, which reshapes those check paths; the close-out
  amendment (Task 13) records the deferral. Unifying it now would mean
  restructuring the semantics layer's reference traversal for code the
  next sub-project rewrites.

- [ ] **Step 1: Extract**

In `crates/celerrate_cli/src/analysis.rs`, add above `analyze_one`:

```rust
/// One file's diagnostics, computed: decode and syntax, then references
/// and gating. The single composition point — `analyze_one` serves it on
/// a cache miss, `persist` re-composes through it, and the equivalence
/// harness recomputes through it — so the composers cannot drift (audit
/// finding I2's first hand-maintained mirror).
pub fn composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let mut diagnostics = celerrate_db::file_diagnostics(database, file).clone();
    diagnostics.extend(
        celerrate_semantics::semantic_diagnostics(
            database,
            file,
            inputs.files,
            inputs.stubs,
            inputs.configuration,
        )
        .iter()
        .cloned(),
    );
    diagnostics
}
```

In `analyze_one`, replace the trailing recompute block (from
`let mut diagnostics = ...` to `diagnostics` inclusive) with:

```rust
        composed_diagnostics(inputs, file)
```

In `crates/celerrate_cli/src/cache/mod.rs`, `composed_verdict` becomes:

```rust
/// One reported file's verdict — its diagnostics through the shared
/// composition point, with the records the entry must revalidate
/// against. Every query here is memoized from the pass.
fn composed_verdict(inputs: &AnalysisInputs, file: celerrate_db::SourceFile) -> StoredVerdict {
    let database = &inputs.database;
    let diagnostics = crate::analysis::composed_diagnostics(inputs, file);
    let records = celerrate_semantics::resolution_records(
        database,
        file,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
    );
    StoredVerdict {
        diagnostics: diagnostics.iter().map(StoredDiagnostic::of).collect(),
        records: records.iter().map(StoredRecord::of).collect(),
    }
}
```

(Remove the now-unused direct imports from `cache/mod.rs` if the
compiler flags them.)

- [ ] **Step 2: Run the suite (behavior-neutral refactor)**

Run: `cargo test -p celerrate_cli`
Expected: PASS — in particular `a_second_run_leaves_equivalent_packs_behind`,
which would catch any composition-order drift byte-for-byte.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_cli/src/analysis.rs crates/celerrate_cli/src/cache/mod.rs
git commit -m "♻️ refactor(cli): one composition point for file diagnostics"
```

---

### Task 12: The served-equals-recomputed equivalence net (I2)

**Files:**
- Create: `crates/celerrate_cli/tests/cache_equivalence.rs`

**Interfaces:**
- Consumes: `composed_diagnostics` (Task 11), `lookup_verdict` /
  `VerdictLookup` (Task 9), `to_diagnostic(file_id, content_length)`
  (Task 2), `Session`, `run`.
- Produces: tests only. This is the mechanical net the spec names: it
  would have caught C1, and it catches the first future check whose
  diagnostics depend on more than the recorded answers.

- [ ] **Step 1: Write the harness and its fixtures**

Create `crates/celerrate_cli/tests/cache_equivalence.rs`:

```rust
//! The revalidation-sufficiency net (audit finding I2): for every file
//! whose records all revalidate, the diagnostics the pack serves must
//! equal, value for value, what a full recomputation produces. Sound
//! today because every reference check is a pure function of the
//! recorded answers plus the header-pinned range; the first future
//! check that reads more than the answer captures — a declaration's
//! kind, its defining file, index-global state — fails here, on a warm
//! run, instead of silently serving wrong diagnostics.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::Path;

use celerrate_cli::analysis::composed_diagnostics;
use celerrate_cli::cache::verdict::{VerdictLookup, lookup_verdict};
use celerrate_cli::run;
use celerrate_cli::session::Session;

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn run_check(root: &Path) {
    let mut output = Vec::new();
    let _ = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
    );
}

/// Analyzes and persists in one process, restarts a session over the
/// packs, and for every file whose verdict revalidates asserts the
/// served diagnostics equal a recomputation through the shared
/// composition point. Answers the set of diagnostic identifiers the
/// served verdicts carried, so callers can assert the fixture really
/// exercised the intended answer shapes rather than validating nothing.
fn served_equals_recomputed(files: &[(&str, &str)]) -> BTreeSet<String> {
    let root = project(files);
    run_check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let database = &inputs.database;
    let mut served_identifiers = BTreeSet::new();
    let mut validated = 0;
    for &file in session.sources.values() {
        let VerdictLookup::Hit(stored) = lookup_verdict(&inputs, file) else {
            continue;
        };
        validated += 1;
        let file_id = file.file_id(database);
        let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
        let served: Option<Vec<_>> = stored
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length))
            .collect();
        let served = served.expect("a revalidated verdict's diagnostics all convert");
        let recomputed = composed_diagnostics(&inputs, file);
        assert_eq!(
            served, recomputed,
            "a validated verdict must equal what recomputation produces",
        );
        for diagnostic in &served {
            served_identifiers.insert(diagnostic.id.as_str().to_owned());
        }
    }
    assert!(
        validated > 0,
        "the fixture produced no validated verdict: the net caught nothing",
    );
    served_identifiers
}

/// Source resolution (no diagnostic), unknown class (CEL0018), unknown
/// function (CEL0019), unknown constant (CEL0020).
#[test]
fn source_and_unknown_answers_replay_equal() {
    let identifiers = served_equals_recomputed(&[(
        "a.php",
        "<?php class Known {} new Known(); new Missing(); absent_function(); echo ABSENT_CONSTANT;",
    )]);
    for expected in ["CEL0018", "CEL0019", "CEL0020"] {
        assert!(
            identifiers.contains(expected),
            "the fixture must exercise {expected}: {identifiers:?}",
        );
    }
}

/// Stub answers with an availability window: a symbol introduced after
/// the project's minimum (CEL0021) and a symbol deprecated within the
/// range (CEL0023), beside an always-available stub answer (`strlen`,
/// no diagnostic). If either identifier is missing, the chosen stub
/// symbol's metadata differs from the embedded snapshot's — pick
/// another symbol carrying the same window shape rather than weakening
/// the assertion.
#[test]
fn stub_window_answers_replay_equal() {
    let identifiers = served_equals_recomputed(&[
        ("composer.json", r#"{"require": {"php": ">=8.1"}}"#),
        (
            "a.php",
            "<?php strlen('x'); json_validate('{}'); utf8_encode('x');",
        ),
    ]);
    for expected in ["CEL0021", "CEL0023"] {
        assert!(
            identifiers.contains(expected),
            "the fixture must exercise {expected}: {identifiers:?}",
        );
    }
}

/// A multi-file project where answers cross files: the consumer's
/// verdict records a `Source` answer for a class another file declares.
#[test]
fn cross_file_source_answers_replay_equal() {
    served_equals_recomputed(&[
        ("src/Consumer.php", "<?php new Widget(); new Gone();"),
        ("src/Widget.php", "<?php class Widget {}"),
    ]);
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p celerrate_cli --test cache_equivalence`
Expected: PASS. If `stub_window_answers_replay_equal` fails on a
missing identifier, the fixture symbol's stub metadata differs from the
embedded snapshot: substitute a symbol that carries the needed window
(introduced after 8.1 for CEL0021, deprecated within the range for
CEL0023) — `celerrate explain` does not exist yet, so check
`crates/celerrate_stubs`' snapshot or grep the phpstorm-stubs pin. Do
not weaken the identifier assertions.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_cli/tests/cache_equivalence.rs
git commit -m "✅ test(cache): served verdicts must equal recomputation"
```

---

### Task 13: Full verification, protocol run, close-out amendments

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md` (amendment entry)
- Modify: `.claude/superpowers/audits/2026-07-13-persistent-cache-audit.md` (settlement header)

**Interfaces:**
- Consumes: everything above, merged on the branch.

- [ ] **Step 1: Full local verification**

Run, in order, each expected clean:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

- [ ] **Step 2: The protocol run (I1's cost acceptance)**

Follow `benchmarks/PROTOCOL.md` on the maintainer's machine (medians of
three protocol runs via `cargo xtask bench`). Record the three numbers.
Acceptance: warm one-edit stays comfortably sub-second (baseline
0.29 s; the self-hash adds one blake3 pass over the executable at
startup, expected ~5–10 ms). If warm one-edit exceeds one second, stop:
the I1 mechanism needs revisiting (for example hashing lazily or
capping), and that is a spec-level conversation, not a silent tweak.

- [ ] **Step 3: Amend the 8-closure spec**

Append to the amendment history of
`.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md`
(after the last entry), with the protocol numbers filled in from
Step 2:

```markdown
- 2026-07-14 - the cache audit's Important debt is settled, per
  `.claude/superpowers/specs/2026-07-14-cache-audit-debt-design.md`:
  binary identity is now the blake3 self-hash of the executable
  (uniform dev and release, `CARGO_PKG_VERSION` only as fallback,
  header shape unchanged so no schema bump); out-of-bounds stored
  spans are discarded like reversed ones; crash debris is swept and
  the `.gitignore` written atomically; the stub-blob field, the
  vendor boundary, torn-read safety, cross-pack independence, and
  persist-per-watch-cycle are each pinned by tests; a checksum-valid
  adversarial entry matrix covers the post-decode conversion surface
  (the fuzz waiver for the pack format is kept on that ground);
  cache traffic is countable behind `CELERRATE_CACHE_STATS=1`
  (stderr, non-contractual); and the served-equals-recomputed
  equivalence net guards revalidation sufficiency mechanically. The
  first revalidation mirror (`composed_verdict`/`analyze_one`) is
  gone — one shared composition point; the second
  (`resolution_records`/`reference_diagnostics`) is explicitly
  deferred to the type-engine sub-project, which reshapes those
  paths. Protocol re-run with the self-hash: cold full <X> s, warm
  no-change <Y> s, warm one-edit <Z> s (medians of three, same
  corpus, same machine, per `benchmarks/PROTOCOL.md`) — warm
  one-edit remains sub-second, closing I1's cost acceptance. The
  audit's Minor findings M1, M3, M6, M7 remain recorded, accepted
  polish; the architecture audit's `source_symbol_table` note is
  unchanged, separate debt.
```

- [ ] **Step 4: Mark the audit settled**

In `.claude/superpowers/audits/2026-07-13-persistent-cache-audit.md`,
after the tally line (`Tally: **1 Critical, 8 Important, 8 Minor.**`),
insert:

```markdown
Settlement (2026-07-14): C1 was fixed before v0.0.1. I1–I8 and M2, M4,
M5, M8 are settled by the cache-audit-debt part (spec:
`.claude/superpowers/specs/2026-07-14-cache-audit-debt-design.md`).
M1, M3, M6, and M7 remain open as recorded, accepted polish.
```

- [ ] **Step 5: Commit**

```bash
git add .claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md .claude/superpowers/audits/2026-07-13-persistent-cache-audit.md
git commit -m "📝 docs(specs): record the cache-audit debt settlement"
```
