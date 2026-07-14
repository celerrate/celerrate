# Incremental-Computation Architecture Audit

Audit of `276d4df`; the defined_constants gap this audit identified was
fixed on this branch (see the 8-closure spec amendment history).

Date: 2026-07-13
Scope: warm `celerrate check` over symfony/demo (9447 files); trigger:
`defined_constants` re-parsing every file serially inside the symbol-index
build (~680 ms of a 1.10 s warm run). All paths below are relative to the
worktree root
`/Users/jh3ady/Businesses/JDevelop/OpenSource/celerrate/.claude/worktrees/semantic-core-8b-corpus-benchmark`.

## 1. Complete parse-consumer census

`celerrate_db::parse` is defined at `crates/celerrate_db/src/queries.rs:23`.
Every non-test call path that can force it (or otherwise touch the full
syntax tree) during a `check` run, exhaustively (grep over celerrate_db,
celerrate_semantics, celerrate_cli; celerrate_types/rules/edit/plugin do
not exist yet):

| # | Query | Forces parse at | Scope | Class | Warm run pays? | One-edit watch iteration pays? |
|---|-------|-----------------|-------|-------|----------------|-------------------------------|
| 1 | `item_tree` | `crates/celerrate_semantics/src/queries.rs:41` (cache consult :33-40) | per file | (a) | No — `item_trees.bin` hit returns before parse | Only the edited file |
| 2 | `defined_constants` | `crates/celerrate_semantics/src/defines.rs:45` | per file, **no persisted artifact** | **(b)**, consumed only inside a class-(c) query | **Yes — every file, serially** | Only the edited file (memoized in-process) |
| 3 | `file_diagnostics` | `crates/celerrate_db/src/queries.rs:72` | per file | (a) | No — skipped on verdict hit (`crates/celerrate_cli/src/analysis.rs:138-145`) | Only the edited file |
| 4 | `reference_diagnostics` | `crates/celerrate_semantics/src/reference_checks.rs:71` (also full-tree walk via `collect_references` at :76) | per file | (a) | No — the stored verdict (`diagnostics.bin`) substitutes; revalidation (`crates/celerrate_cli/src/cache/verdict.rs:19-49`) resolves stored records, never the tree | Only the edited file |
| 5 | `syntax_version_diagnostics` | `crates/celerrate_semantics/src/syntax_gating.rs:34` (`descendants()` walk at :57) | per file | (a) | No — same verdict coverage, composed via `semantic_diagnostics` (`crates/celerrate_semantics/src/queries.rs:49-66`) | Only the edited file |
| 6 | `resolution_records` | `crates/celerrate_semantics/src/revalidation.rs:70` | per file | (a) | No — run only at persist for verdict-miss files (`crates/celerrate_cli/src/cache/mod.rs:131-144,166-172`); all-hit warm pass never calls it | Only the edited file |
| 7 | `ast_id_map` | `crates/celerrate_semantics/src/queries.rs:22` | per file | (b) latent | **Never** — no check-time consumer exists (only the `lib.rs:32` export and tests); it is the future LSP span-reconciliation query | No |

The one class-(c) query — global, iterating many files' derived state —
is `source_symbol_table` (`crates/celerrate_semantics/src/index.rs:89-127`):
a single salsa query looping `for &file in files.files(db)` over all 9447
files, calling `item_tree` (:92, pack-covered, cheap) and
`defined_constants` (:103, **not** covered, forces parse). Salsa queries
run on one thread — the fan-out is deliberately confined to the rayon
boundary (`crates/celerrate_cli/src/analysis.rs:1-6`) — so the 9447
parses run serially inside whichever thread first triggers the table.
On a warm run the trigger is unavoidable: `analyze_one` →
`validated_verdict` → `resolve_name` (`crates/celerrate_semantics/src/resolve.rs:160,172`)
→ `lookup_symbol` (`crates/celerrate_semantics/src/lookup.rs:53`) →
`source_symbol_table`. That is the entire ~680 ms.

Rendering never parses: `render.rs:111` uses `line_index`
(`crates/celerrate_db/src/queries.rs:33`), which depends on `source_text`
only, and only for files that actually report.

**Census verdict: `defined_constants` is the only per-file computation a
warm run pays that no persisted artifact covers, and `source_symbol_table`
is the only global serial query. There are zero further class-(b) queries
paid at check time (`ast_id_map` is class (b) by shape but has no
consumer), and zero further class-(c) queries over source files.** The
incremental architecture is otherwise exactly as designed: every other
parse consumer sits behind either the item-tree pack or the
verdict-revalidation pack.

## 2. `defined_constants` specifically

**Granularity.** It is a proper per-file `#[salsa::tracked]` query
(`crates/celerrate_semantics/src/defines.rs:43-46`), memoized per
`SourceFile`, and an early-cutoff unit in its own right: a body edit that
touches no `define()` produces an equal `Vec<DefinedConstant>` and salsa
backdates it (module doc, defines.rs:11-14). In watch mode (in-process
memos) a one-file edit recomputes exactly one file's defines. The per-file
granularity is **not** lost in memory; what is lost is the **process
boundary**: unlike `item_tree`, it has no `ArtifactCache` hook
(`crates/celerrate_semantics/src/cache.rs:20-24` exposes only
`fn item_tree`), so every fresh process re-derives all 9447 files from
parse, inside the serial table build.

**Why not in the ItemTree — recorded reason or omission?** Both, in
layers:

- The exclusion from the item *traversal* is recorded and deliberate:
  defines.rs:1-14 explains that `item_nodes` never descends into member
  lists, and that making the traversal see into bodies would (1) renumber
  every later `AstId` when a body gains a `define()` and (2) let body
  edits change the tree, killing the early cutoff. `DefineId`
  (defines.rs:24-28) exists precisely because minting item indexes for
  defines "would collide with the real ones". The lowering side agrees:
  `crates/celerrate_semantics/src/item_nodes.rs` / `items.rs:108-111`
  ("The traversal never descends into member lists; [trait uses are] the
  one place the tree looks inside one").
- The absence of any *persisted artifact* for defines is an omission. The
  part-8 design spec's pack table
  (`.claude/superpowers/specs/2026-07-13-semantic-core-8-closure-design.md:104`)
  planned `symbol_index.bin` "keyed by the blake3 hash over the ordered
  set of the file set's ItemTree hashes", which would have masked the
  cost — but that key is itself unsound as specified (see 4c below),
  because defines are not a function of ItemTrees. When 8a shipped only
  `item_trees.bin` + `diagnostics.bin`
  (`crates/celerrate_cli/src/cache/snapshot.rs:18-19`), defines fell
  through the crack: per-file boundary facts with no boundary artifact.
  Nothing in the 8a plan or the spec records a decision to leave them
  parse-derived on warm starts.

Note the recorded reason conflates two things: numbering defines as
*items* (genuinely harmful, for the reasons given) and carrying define
*names* as a separate range-free list on the `ItemTree` value (harmless —
a body edit that adds a `define()` *should* change the boundary, because
it changes the project's symbols; a body edit that does not still
produces an equal value and backdates).

**Span usage.** `DefinedConstant` carries a `range: TextRange`
(defines.rs:38), but its single consumer drops it:
`source_symbol_table` keeps only `DefineId { file, index }` as the origin
(`crates/celerrate_semantics/src/index.rs:103-125`), and `SymbolEntry`
(:32-39) stores no span. `lookup_symbol` reduces further to
`SymbolResolution::Source { kind }` (lookup.rs:53-54). No diagnostic, no
renderer, nothing reads a define's span today; only defines.rs's own unit
tests assert it (:553-558, :573-578). The range is dead weight for the
current product.

## 3. Invalidation targeting after one edit

Evidence: `crates/celerrate_semantics/tests/invalidation_scope.rs`
(execution-log assertions), `crates/celerrate_semantics/src/index.rs`
tests, `crates/celerrate_cli/tests/cache_consistency.rs` /
`cache_seeding.rs` (cross-process).

Firewalled, with direct proof:

- **Body/comment/whitespace edit, same file**: reparse + one `item_tree`
  re-run, equal value backdates, zero consumer re-runs
  (invalidation_scope.rs:46-76, :79-96, :99-126). The symbol table is
  never rebuilt on a body edit (index.rs:365-391,
  `a_body_edit_never_rebuilds_the_table`).
- **Body edit in another file**: stops at that file's item tree; no
  `source_symbol_table`, no `lookup_symbol`, no consumer
  (invalidation_scope.rs:368-394).
- **Signature change / new declaration anywhere**: `source_symbol_table`
  rebuilds once (global, by design), the interned per-name
  `lookup_symbol` memos re-run as cheap binary searches, unchanged
  answers backdate, and consumers — including other files'
  `reference_diagnostics` — are spared (invalidation_scope.rs:333-365,
  :500-538, :578-617). This is the lookup firewall documented at
  lookup.rs:1-7.
- **Set membership churn with no symbol change**: table re-runs once,
  equal value backdates, consumers spared (invalidation_scope.rs:620-645).
- **PHP-range change**: stub table only, source table untouched
  (invalidation_scope.rs:441-497).
- **Cross-process**: byte-for-byte equality of cache-seeded vs
  from-scratch runs over edit sequences
  (cache_consistency.rs:106-215), and verdict revalidation
  accepts/discards correctly (cache_seeding.rs:149-215).

Not firewalled:

1. **The defines process boundary** (the trigger): in a fresh process the
   table build re-parses every file because `defined_constants` has no
   artifact. In-memory the design intent holds; across processes it does
   not.
2. **The table rebuild is global and serial**: any signature edit anywhere
   recomputes `source_symbol_table` over all files' entries (clone + sort,
   index.rs:89-127). Downstream is protected by backdating, but the
   rebuild itself is O(all symbols) on one thread each time it fires.
   Accepted by design; scales with corpus size.
3. **Coverage gap**: `invalidation_scope.rs` contains no test exercising
   `define()` edits (grep for "define" in that file: zero hits). There is
   no proof that adding a `define()` in a body reaches the table, or that
   a define-free body edit in a define-carrying file spares it. The
   behavior is almost certainly correct (defines.rs's early cutoff), but
   it is the one edge of the boundary with no execution-log evidence.

Verdict: in-memory invalidation is well targeted and mechanically proven;
the persistent tier has exactly one hole, and it is the defines hole.

## 4. The architecturally right fix

Constraints (from CLAUDE.md layering + the 8-closure spec:170-185):
boundary artifacts are plain `Eq` values; no layer above the boundary
holds syntax nodes; cache consultation only via the dependency-inverted
`ArtifactCache` registered at the composition root; `CACHE_SCHEMA_VERSION`
(`crates/celerrate_cli/src/cache/pack.rs:19`) is manually bumped and
discards packs wholesale.

### (a) Move define()-detected constants into the ItemTree — RECOMMENDED

- **Correctness**: the define names must enter as a *separate, range-free
  list* (e.g. `pub defines: Vec<String>` in walk order, `DefineId.index`
  = position), NOT as item-numbered declarations — this respects both
  recorded objections in defines.rs:1-14: `AstId` numbering is untouched
  (defines keep their own `DefineId` space, exactly as today), and the
  range-free invariant of items.rs:1-5 is preserved so
  whitespace/comment/define-free body edits still produce equal values
  that backdate. The dropped `TextRange` is consumed by nobody
  (section 2); when a future feature needs define spans, the established
  pattern is a separate volatile per-file query reconciling late, exactly
  like `ast_id_map` (queries.rs:17-19). A body edit that adds/removes a
  `define()` now changes the ItemTree — that is correct, it changes the
  project's symbols, and it invalidates exactly what the current
  `defined_constants` change invalidates.
- **Invalidation granularity**: identical to today in-memory (per file,
  early cutoff by value equality); strictly better across processes
  (content-hash keyed via the existing pack).
- **Cache/schema**: `StoredItemTree` gains a `defines` field
  (`crates/celerrate_cli/src/cache/stored.rs:100-103`); bump
  `CACHE_SCHEMA_VERSION` 1 → 2; old packs discard wholesale by the
  existing header check (pack.rs:71-86) — the designed mechanism, no
  migration.
- **Parallelism**: the serial full-corpus parse loop disappears
  entirely — `source_symbol_table` reads `item_tree(db, file).defines`
  and the warm-run table build touches only pack-served plain values.
  `index.rs` stops importing `defined_constants`; the semantics crate's
  only remaining warm-path parse consumers are the ones the verdict pack
  already covers. Cold runs get marginally *cheaper* too: today every
  file is walked twice (item_nodes top-level walk + defines_in full
  walk); the merged lowering walks once.
- **Size of change**: small. See the change list below.

### (b) A separate per-file defines artifact beside the trees

Sound, and it is the only option that could persist the span (a
content-addressed entry may carry ranges — they are valid for exactly
those bytes, as `StoredDiagnostic` already proves, stored.rs:179-185).
But nothing consumes the span, so this buys nothing (a) does not, at
strictly more machinery: a second `ArtifactCache` method
(cache.rs:20-24), a second stored type, a new pack file or entry kind in
`snapshot.rs`, its own seeding/persist/corruption coverage, and
`source_symbol_table` still makes two per-file query calls. Same schema
bump anyway. Choose this only if a span-bearing consumer of defines were
imminent; none is (the "did you mean" work consumes `original` spellings,
not spans — index.rs:30-31).

### (c) The symbol-index pack as originally specified

As written in the spec (:104, :128-131 — key = hash over the file set's
ItemTree hashes, "if no boundary representation changed, the index is the
same index") it is **unsound today**: `source_symbol_table` is not a
function of ItemTrees — a `define()` added inside a method body changes
the table while every ItemTree hash is unchanged, so a warm run would
serve a stale index missing a symbol → unknown-constant false positives,
the one direction the policy forbids. It only becomes sound *after* (a)
or (b) puts defines into the keyed material. It is also the coarsest
granularity (any signature edit anywhere misses the whole pack and pays
the full serial rebuild), the largest artifact, and the biggest change.
It remains worth doing later, *on top of* (a), to erase the remaining
table-assembly cost (section 5) — and the spec's key sentence must be
amended when it happens. The 8b measurement escalation (spec amendment
:24-38) already left this pack as a human scope decision; (a) changes
that economics substantially, so re-measure after (a) before building it.

### Recommendation and concrete change list for (a)

1. `crates/celerrate_semantics/src/items.rs` — add `pub defines:
   Vec<String>` (walk order) to `ItemTree` (:62-66); extend `from_root`
   (:72-81) to run the define collection walk (single traversal shared
   with lowering, or a second walk initially — equality is what matters).
2. `crates/celerrate_semantics/src/defines.rs` — keep `DefineId` and the
   name-extraction machinery (`is_define_call`, `defined_name`,
   `literal_name`, the unescapers) as the items.rs helper; delete the
   `defined_constants` salsa query (:43-46) and `DefinedConstant`'s
   `range` (or the whole struct); keep the exhaustive unit tests, retargeted
   at the lowering.
3. `crates/celerrate_semantics/src/index.rs` — :103-125 reads
   `tree.defines` instead of `defined_constants(db, file)`; `DefineId`
   construction unchanged (position in the list).
4. `crates/celerrate_semantics/src/cache.rs` — update the exactness
   contract doc (:15-19): a `Some` must now include the defines.
5. `crates/celerrate_cli/src/cache/stored.rs` — `StoredItemTree` gains
   `defines: Vec<String>` (:100-103, `of`/`to_item_tree` :106-168).
6. `crates/celerrate_cli/src/cache/pack.rs:19` — `CACHE_SCHEMA_VERSION`
   1 → 2.
7. Tests to extend:
   - `crates/celerrate_cli/tests/cache_consistency.rs` — the critical
     one: a new replay sequence where a `define()` appears inside a
     method body, is edited, and vanishes across process restarts
     (mirroring `a_definition_appearing_and_vanishing_replays_consistently`
     :132), plus a fixture file whose only symbol is a body-level
     `define()` referenced from another file — this is the exact case a
     stale/underspecified tree entry would break byte-for-byte equality
     on.
   - `crates/celerrate_cli/tests/cache_seeding.rs` — probe trees
     (:33-50) gain the field; add a probe asserting a seeded tree's
     defines reach the symbol table without parsing.
   - `crates/celerrate_cli/src/cache/stored.rs` round-trip test
     (:331-342) — add a method-body `define()` to the source.
   - `crates/celerrate_semantics/tests/invalidation_scope.rs` — close
     the section-3 coverage gap regardless: (i) a body edit adding a
     `define()` reaches the table and the consumers; (ii) a define-free
     body edit in a define-carrying file backdates and spares them.
   - `crates/celerrate_semantics/src/cache.rs` probe test (:83-100)
     already proves a hit never parses; extend the assertion to cover a
     `source_symbol_table` build over the probe.
   - `crates/celerrate_semantics/src/queries.rs` / `index.rs` unit tests
     for the new field.
8. Re-run the 8b protocol; the warm no-change and one-edit numbers should
   finally separate from cold, which also re-opens the diagnostics-pack
   and symbol-index-pack economics decisions the spec amendment escalated.

## 5. Other chokepoints at the next scale-up

**`source_symbol_table` assembly itself**
(`crates/celerrate_semantics/src/index.rs:89-127`). After (a) the parses
disappear but the query still clones every declaration's name/namespace
strings for all 9447 files, folds keys, and sorts the whole entry vector
— serially, once per process on warm start, and again on every
signature-changing edit in watch mode. Backdating protects downstream,
never the rebuild. At 10x corpus size this becomes the next warm-start
and watch-latency ceiling. The eventual answer is the (c) pack with a
corrected key (hash over ItemTree hashes *including* defines), or an
incrementally mergeable table; neither is warranted before re-measuring
after (a).

- 2026-07-14 (type-engine plan 1a) — settled structurally for the
  type engine's hot edit class: members live in a sibling projection
  (`member_tree`), so member and signature edits inside a class body
  never change `item_tree` values and the global table never rebuilds
  on them (pinned by
  `invalidation_scope.rs::a_member_signature_edit_never_reaches_item_tree_consumers`).
  The rebuild still fires on genuine top-level changes (new class,
  renamed function); that residue is unchanged from the audit and
  remains accepted, scale-bounded by top-level churn only.

**`stub_symbol_table` + `stubs_in_range`**
(`crates/celerrate_semantics/src/index.rs:183-203`,
`crates/celerrate_stubs/src/query.rs:22`). Every process filters the full
compiled phpstorm-stubs symbol set by version range, clones each symbol,
and sorts — a pure function of exactly the pack-header inputs (stub blob
hash + PHP range, pack.rs:26-35), which makes it the single most
cacheable artifact in the system: a header match alone validates it, no
per-entry keys needed. Cheap today, but it grows with the stubs, not the
project, so every user pays it on every run of every project. It was half
of the spec'd `symbol_index.bin` (:104) for good reason.

**Startup walk + load + hash**
(`crates/celerrate_cli/src/session.rs:138-139` via
`enumerate_php_files`/`load`, `content_hash` at
`crates/celerrate_db/src/queries.rs:46-49`). Every run reads and blake3-hashes
every file including the whole vendor tree, single-threaded, before any
analysis. Content addressing makes the hashing irreducible in principle,
but not its serial execution; the 8b amendment (spec :39-46) already
names "process start, walk, hashing every file" as the fixed cost that
lets warm approach cold. Parallelizing the read+hash in the VFS loader is
the obvious next lever once the defines fix lands, and needs no
architectural change — it is outside salsa.

**`persist`'s full-corpus loop on the session thread**
(`crates/celerrate_cli/src/cache/mod.rs:103-147`). After every pass —
including every watch iteration — `collect_entries` walks all sources for
trees and all reported files for verdicts, re-running `validated_verdict`
per file. Everything is memoized from the pass so it is cheap today, and
`write_when_changed` (:205-227) skips identical rewrites, but it is a
second serial O(files) traversal per cycle whose cost tracks corpus size;
if watch latency ever matters at scale, this loop wants to be
incremental (dirty-set) rather than whole-corpus.

**Interned-lookup fan-out on table rebuild**
(`crates/celerrate_semantics/src/lookup.rs:43-62`). Each
`source_symbol_table` change re-executes every interned
`lookup_symbol` memo — one binary search per unique referenced name in
the corpus. lookup.rs:1-7 documents this as the intended cheap firewall,
and it is; noted only because it is O(unique names) serial work attached
to every signature edit in watch mode, and it would compound with any
future per-lookup cost (e.g. "did you mean" candidate scans must not live
inside this query).
