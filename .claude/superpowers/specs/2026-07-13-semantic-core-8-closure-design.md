# Semantic Core Part 8: Closure (Design)

Date: 2026-07-13
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-11-semantic-core-design.md`
(sections 6, 8, 9, 10)
Predecessor plan: `.claude/superpowers/plans/2026-07-12-semantic-core-7-product.md`

## 1. Goal and scope

Close the semantic-core sub-project: the persistent artifact cache, the
pinned Symfony corpus in CI, the committed benchmark protocol with the
published warm one-shot incremental number, and the `v0.0.1` release.
This is the part that turns the engine the previous seven parts built
into the public preview the umbrella design promised: proof of the
differentiator (interprocedural plus incremental) against real
feedback, before the riskiest sub-project (the type engine) begins.

The ordering inside the part is forced, not chosen: the flagship
incremental number cannot be measured without the cache (which is why
the cache was pulled forward from the CLI sub-project in the first
place), and the release cannot ship without the number.

In scope:

- The persistent artifact cache under `.celerrate/cache/` (section 2).
- The syntax-tree retention measurement and, if the measurement demands
  it, LRU eviction: the memory-economics debt part 4 recorded against
  this part (section 2).
- The pinned symfony/demo corpus in CI: snapshot regression and the
  anti-false-positive contract (section 3).
- The committed benchmark protocol, the `cargo xtask bench` harness,
  the CI performance guard rail, and the published number (section 4).
- The `v0.0.1` release: release workflow, binaries for five targets,
  the cross-platform test matrix, `CHANGELOG.md`, the README as the
  preview's landing page, and the workspace version correction
  (section 5).

Out of scope, deliberately: any PHPStan comparison (the v0.0.x preview
runs a handful of diagnostic families while PHPStan runs hundreds of
rules; the matched-scope comparison is the v0.1 claim, and the
protocol says so explicitly), `celerrate.toml`, JSON, SARIF, and
GitHub output formats, baseline, color and terminal styling, and the
comfortable distribution channels (install script, Homebrew, Composer
bootstrap, Docker, GitHub Action), all of which the umbrella design
assigns to the CLI product sub-project.

## 2. The persistent artifact cache

Per the parent spec's honest design: salsa's in-memory state is not
serialized. The cache is a content-addressed derived-artifact cache
sitting above salsa, persisted to `.celerrate/cache/` inside the
analyzed project, and used to re-seed a fresh database at startup.
The `.celerrate/` directory writes its own `.gitignore` containing
`*` on creation, the way Cargo's `target/` does, so no user has to
remember to ignore it.

### Layout: one pack per artifact class

Three artifact classes, one pack file each:

| Pack | Contents | Entry key |
| --- | --- | --- |
| `item_trees.bin` | one `ItemTree` per source file | blake3 hash of the file's content |
| `symbol_index.bin` | the two global tables (source, stub view) | blake3 hash over the ordered set of the file set's `ItemTree` hashes |
| `diagnostics.bin` | per-file semantic diagnostics plus revalidation records | blake3 hash of the file's content |

A pack is a versioned, checksummed table, rewritten atomically
(temporary file plus rename) after every completed analysis, including
every `--watch` iteration. There is no garbage collection problem:
rewriting drops dead entries. And the parent spec's economics
criterion becomes mechanical: an artifact class that does not pay for
itself is dropped by deleting its pack and the code that writes it,
without touching the other two.

Every pack carries a header naming the cache schema version (a
manually bumped constant), the binary version, the stub blob version,
and the PHP version range. Any mismatch discards the whole pack,
silently regenerated, never fatal. The entry keys therefore only need
to encode what varies within one configuration: file content.

### The three classes, in decreasing honesty of their keys

**`ItemTree`.** A pure function of one file's content, the perfect
content-addressing case, and the class that carries the cache's
economics: on a warm start, unchanged files get their boundary
representation without being read, parsed, or lowered.

**The symbol index.** One global artifact. Its key is the hash over
the file set's `ItemTree` hashes (the stub side is covered by the
header's stub version): if no boundary representation changed, the
index is the same index.

**Per-file diagnostics.** The hard case, because a file's diagnostics
depend on global name resolution, not just on the file. Each entry
stores, beside the diagnostic data-model values, its revalidation
records: the list of names the file references, each with the blake3
hash of its resolution answer. On load, an entry is accepted only
after every recorded name is re-resolved against the fresh index and
every answer hash matches; one mismatch discards the entry. This is
the parent spec's word made concrete: deserialization plus
revalidation must beat recomputation, and per-name lookups are exactly
the cheap revalidation that can win. If measurement says the class
loses, the pack is deleted; the release does not wait for it.

Two boundaries the entries respect, both inherited from part 7: only
analyzed project files have diagnostics entries (installed
dependencies are indexed, never reported), and the internal-error
report produced by panic isolation is never persisted, extending the
parent spec's "never memoized" to "never cached on disk".

### Salsa integration, without lying

The loaded cache is an immutable snapshot, fixed for the lifetime of
the process, consulted by a query (for example `item_tree`) before it
computes. Determinism holds because the lookup is a pure function of
the content hash and the stored artifact is byte-for-byte what the
computation would have produced. That guarantee is tested, not
asserted: the incremental correctness harness extends across
processes. A cold run and a cache-re-seeded run of the same inputs
must produce byte-for-byte identical output, replayed over edit
sequences on the corpus (edit, run, persist, restart, compare against
a from-scratch analysis at every step). This extension is the most
critical test of the part, in the same sense the in-memory harness was
the most critical test of the sub-project.

### Serialization

serde plus postcard: compact, maintained, dual-licensed MIT OR
Apache-2.0 (cargo-deny clean), and structurally panic-free on
malformed input. Corruption of any kind (truncated pack, unknown
schema version, checksum mismatch, undecodable entry) is detected and
answered by regeneration, never by an error the user sees, and
deserializing a hostile cache must never panic: the zero-panic lints
apply, and whether the pack format joins the existing fuzz targets is
decided in the plan, on the format's actual attack surface.

### Syntax-tree retention: the part 4 debt

Part 4 delivered the structural property that makes eviction safe (no
layer above the boundary holds a syntax node; the `AstIdMap` and the
`ItemTree` are plain values). This part delivers the measurement the
debt was deferred for: peak memory on the corpus with and without
retained syntax trees, measured alongside the cache. If retention is
acceptable at corpus scale, the debt closes with a recorded number and
no mechanism; if it is not, trees are evicted and reparsed on demand,
with the mechanism chosen by the plan from what the measurement shows.
Either way the outcome is a measured decision, recorded in this spec's
amendment history, not a hope.

## 3. The corpus in CI

**The pin.** A committed `corpus.pin` names the repository URL and the
commit SHA of symfony/demo, the same mechanism as the existing
`phpstorm-stubs.pin`. symfony/demo is the corpus because it has the
exact shape `celerrate check` is aimed at: a real user project, with
application code, a real `composer.json`, and the full Symfony vendor
tree installed from its `composer.lock`. Bumping the corpus is a
deliberate pin change with a human-reviewed snapshot diff, never a
floating HEAD: an unpinned corpus would destroy the regression signal
the anti-false-positive policy depends on.

**The workflow.** `corpus.yml`, on every pull request and on main:
check out celerrate, check out symfony/demo at the pinned SHA, run
`composer install` from the corpus's lock file (the vendor tree is CI-
cached, keyed by the pin SHA), build the release binary, and run
`celerrate check` over the corpus.

**The anti-false-positive contract.** The complete `check` output is
compared against a committed expected snapshot. The contract on that
snapshot: it contains no unknown-symbol diagnostics, because
symfony/demo is correct code and any unknown-symbol report there is a
false positive, which the umbrella design classifies as a priority
bug, not an opinion. Any divergence in either direction (a new
diagnostic, a vanished one) fails CI and forces a human review of the
snapshot change.

**The guard rail.** The same workflow runs the three benchmark
scenarios of section 4 with generous ceiling thresholds (for example:
warm one-edit under 3 seconds where the local target is sub-second).
Shared GitHub runners are too noisy to measure on, so the guard rail
catches structural regressions (the cache silently ceasing to work)
and claims nothing more.

## 4. The benchmark protocol

**The committed document.** `benchmarks/PROTOCOL.md` fixes everything
the parent spec requires of a publishable number: the corpus
repository and pinned SHA, the corpus size in files and lines, the
named hardware (the maintainer's machine, described precisely: CPU,
memory, storage, OS version), the toolchain and binary version, the
number of runs and the aggregate (median), and the exact definition of
each measured scenario:

1. **Cold full**: no cache, complete analysis.
2. **Warm no-change**: cache present, nothing changed, the floor of
   the one-shot run.
3. **Warm one-edit**: cache present, one source file modified. This is
   the flagship number: a full CLI run, wall clock, including process
   startup and cache loading, exactly the execution mode the parent
   spec's protocol names. Target: sub-second.

**The harness.** `cargo xtask bench` orchestrates: prepare the corpus,
prime the cache, apply the scripted edit, and invoke hyperfine on the
built binary. One assumed deviation from the umbrella design's
section 9, documented here: it names criterion, but criterion measures
in-process, and the flagship number is defined end to end, process
included. hyperfine is the honest tool for that definition; criterion
remains available for in-process query benchmarks when a later part
needs them.

**The published number** comes from the protocol run on the named
hardware, reproducible by a third party following the document, and it
appears in the README with a link to the protocol. No PHPStan
comparison is published in v0.0.x; the protocol states this and states
why (scope asymmetry, resolved by the matched-scope comparison at
v0.1). A benchmark that would not survive third-party scrutiny is not
published.

## 5. The release

**The workflow.** `release.yml`, triggered by a `v*` tag: a build
matrix over five targets (Linux x64 and arm64 as static musl builds,
macOS x64 and arm64, Windows x64), stripped binaries, `.tar.gz`
archives (`.zip` for Windows), a `SHA256SUMS` file, and a GitHub
Release whose notes are extracted from the changelog. The binary
answers `--version` with the crate version.

**The test matrix widens.** The current CI tests only on Ubuntu, and
binaries do not ship for platforms the tests never ran on: the `test`
job of `ci.yml` becomes a matrix over ubuntu, macos, and windows on
every pull request. This makes the parent spec's tier 2 real (Windows
is built and tested) without claiming more: the corpus and the
benchmarks stay on Linux.

**Versioning.** The workspace version, currently `0.1.0`, is corrected
to `0.0.1`; the tag is `v0.0.1`. `0.1.0` is the CLI product
sub-project's number and was never released, so the correction is
safe. `CHANGELOG.md` is introduced in Keep a Changelog format, with a
`0.0.1` entry describing what the preview contains: `celerrate check`,
`--watch`, the unknown-symbol and version-gating diagnostic families,
the persistent cache, and the published incremental number.

**The README becomes the preview's landing page.** The status moves
from "not yet usable" to an honest preview statement: what it does
(the two diagnostic families, watch mode, incremental analysis), what
it does not do yet (no type inference, no lint, no configuration
file), a quick start (download the binary from the Release, run
`celerrate check .`), and the published number linking to the
protocol. No promise beyond what is measured.

## 6. Testing

1. **The cross-process extension of the incremental correctness
   harness** (section 2): cold versus cache-re-seeded, byte-for-byte,
   over edit sequences on the corpus. The most critical test of the
   part.
2. **Unit tests, TDD, on the cache**: pack headers, checksums, every
   corruption mode (truncated pack, unknown schema version, checksum
   mismatch, undecodable entry) answered by silent regeneration, write
   atomicity, and the per-name revalidation of diagnostics entries
   (acceptance on full match, discard on any mismatch).
3. **The corpus snapshot in CI** (section 3): regression detection and
   the anti-false-positive contract.
4. **The performance guard rail in CI** (section 3) with ceiling
   thresholds.
5. **Zero-panic lints** on every new module with no exception outside
   test modules; hostile-cache deserialization never panics; whether
   the pack format joins the fuzz targets is a plan decision.

## 7. Implementation plans

The part is large and its dependencies are serial, so it splits into
three plans, the way the parser part did:

1. **8a, the cache**: the pack format, the three artifact classes, the
   salsa-consulting load path, the atomic write path, the
   cross-process harness extension, and the syntax-tree retention
   measurement with its decision.
2. **8b, the corpus and the benchmark**: `corpus.pin`, `corpus.yml`,
   the expected snapshot and its contract, `benchmarks/PROTOCOL.md`,
   `cargo xtask bench`, the guard rail, and the protocol run that
   produces the number.
3. **8c, the release**: `release.yml`, the widened test matrix, the
   version correction, `CHANGELOG.md`, the README rewrite, and the
   `v0.0.1` tag.

The order is forced: the number requires the cache, and the release
requires the number.
