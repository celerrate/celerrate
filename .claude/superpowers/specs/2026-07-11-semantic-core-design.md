# Semantic Core (Design)

Date: 2026-07-11
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md`
(sections 2, 3, 9, 11)
Predecessor: the Foundations sub-project, closed by
`.claude/superpowers/specs/2026-07-10-foundations-4-parser-design.md`

## 1. Goal and scope

Turn the syntactic foundations into an incremental analysis engine: the
salsa query database, file loading, Composer project discovery, the
symbol index, top-level name resolution, compiled stubs, and the
persistent artifact cache. The sub-project closes with the public
`v0.0.x` preview: a real but minimal `celerrate check` carrying two
diagnostic families (top-level unknown symbols and version gating over
syntax and symbols), zero-configuration detection, `--watch`, and the
published incremental benchmark on the pinned Symfony corpus.

This is the umbrella design for sub-project 2. It fixes the
cross-cutting decisions the parent spec delegated here (the invalidation
boundary, the stable-identifier scheme, the engine invariants, the part
sequencing); each part then gets its own TDD implementation plan, with a
dedicated part-level design only if a part turns out to justify one.

New crates, all planned by the parent spec's layout:
`celerrate_diagnostics` (the extraction recorded as assumed debt by the
parser design; this sub-project discharges it), `celerrate_vfs`,
`celerrate_db`, `celerrate_project`, `celerrate_stubs`,
`celerrate_semantics`, and `celerrate_cli` (the binary is born here,
reduced to the preview `check`; the full product surface remains
sub-project 5).

Out of scope (deliberately):

- Any type judgment. Inference, signatures, the Celerrate norm, and the
  compat bridge are sub-project 3.
- Class members in unknown-symbol checks. Member existence requires
  inheritance linearization and receiver reasoning; it arrives with the
  type engine. The preview checks top-level symbols only.
- Rich diagnostic rendering. Annotated spans, notes, suggestions, and
  `celerrate explain` are sub-project 4; the preview renders a simple
  text format documented as temporary.
- `celerrate.toml`. The preview is zero-configuration only:
  `composer.json` is the sole configuration source.
- Baseline, output formats (JSON, SARIF, GitHub), and
  `migrate --from-phpstan`: sub-project 5.
- The WASM plugin host, the declarative plugin tier, and stub curation
  (the refinements overlay): later sub-projects.
- The LSP. The engine invariants it needs (cancellation, overlays) are
  built and exercised here, but no server ships.

## 2. The salsa foundation

`celerrate_db` is the base-db layer: the salsa inputs (file text by
`FileId`, the project configuration, the PHP version range, the stub
index, the analyzed file set) and the foundational queries every layer
shares. `parse(file)` chains `SourceText::from_bytes` and
`celerrate_syntax::parse`; `line_index(file)` becomes the derived query
foundations part 2 anticipated. Higher-level query definitions live in
their domain crates, and — recorded here after implementation review —
so do the inputs whose field types live above this layer: the project
configuration is defined in `celerrate_project`, the stub index in
`celerrate_stubs`; the input list above names the base-db role, not one
crate's contents. The concrete `salsa::Database` aggregating all storage
is assembled at the composition root (`celerrate_cli`).

The engine uses the upstream `salsa` crate (the one ty builds on),
assumed openly as the engine's spine. Unlike `rowan`, salsa cannot be
contained behind crate-owned aliases and there is no point pretending:
the discipline is about where things live (query definitions in domain
crates, the concrete database at the composition root), not about
hiding the dependency.

**Engine invariants**, active from the first query because they cannot
be retrofitted:

- **Cancellation.** Every input mutation triggers `salsa::Cancelled` in
  in-flight queries. The preview's `--watch` is the first real
  consumer, which validates the invariant before the LSP era.
- **Parallelism discipline.** Rayon fan-out happens only through
  database snapshots at declared boundaries (the project-diagnostics
  loop), never inside queries. Parallel collection is deterministically
  ordered before rendering or comparison.
- **Panic isolation.** `catch_unwind` wraps only the outermost per-file
  diagnostics query call in the CLI, is transparent to
  `salsa::Cancelled` (always re-raised), and its product is never
  memoized.
- **Cycle recovery posture.** Top-level resolution is designed to be
  acyclic: degenerate inputs like `class A extends A` are detected by a
  dedicated check, not by a salsa cycle. The fixpoint-and-widening
  infrastructure belongs to sub-project 3, but the rule is fixed now:
  any potentially cyclic query must declare its recovery strategy
  before it lands.

## 3. The virtual file system

`celerrate_vfs` is the bridge between the outside world and the salsa
inputs. It owns:

- The `FileId ↔ path` mapping (interned), with the path decisions
  foundations part 2 deferred: absolute normalized paths; platform
  case sensitivity handled per platform, with Windows explicitly tier 2
  as the parent spec's distribution section states.
- The disk walk, driven by the autoload rules `celerrate_project`
  derives: the VFS walks what the project declares, not the whole tree.
- In-memory overlays. The API exists now for the incremental harness
  and the tests; the LSP will be its second client.
- The watcher (`notify`), feeding `--watch` as plain input mutations.

The VFS never reads anything during a query: it pushes states into
salsa inputs, and salsa pulls derivations. File contents arrive from
outside (disk walk, overlays, literal strings in tests), exactly as
foundations part 2 designed.

## 4. Project discovery

`celerrate_project` locates `composer.json` and parses it with the same
resilience as everything else: a corrupted or missing file produces a
diagnostic and defaults, never a failure. It derives three things:

- **Autoload rules** (PSR-4, PSR-0, classmap, files), for the project
  and for dependencies through `vendor/composer/installed.json`. They
  drive the VFS walk and classify every file as project or vendor; the
  classification feeds salsa durability directly (vendor is
  high-durability input, invalidated wholesale only when
  `composer.lock` changes).
- **The PHP version range**, with the parent spec's precedence minus
  its first stage (no `celerrate.toml` in the preview):
  `config.platform.php`, then the `require.php` constraint interpreted
  as a range, then the latest supported stable version as fallback,
  with a warning suggesting explicit configuration.
- **The analysis roots.** Without any `composer.json`, the fallback is
  "analyze the current directory" with a warning: zero-configuration
  never blocks.

## 5. Stubs

`celerrate_stubs` compiles a **pinned** snapshot of phpstorm-stubs
(vendored, bumped deliberately, like the corpus SHAs) into a versioned
binary blob:

- Compiled by a dedicated compiler, not by a `build.rs`, consistent
  with the typed-AST sourcegen pattern: the blob is committed and a
  freshness test asserts it matches the pinned snapshot. Placement,
  recorded here after implementation review: the compiler is a
  feature-gated binary owned by `celerrate_stubs` (parent-spec
  ownership), because it parses PHP with `celerrate_syntax` while
  xtask's invariant — no dependency on any `celerrate_*` crate, so a
  broken generated file can never prevent regenerating it — must
  survive. `cargo xtask compile-stubs` remains the entry point: xtask
  fetches the pinned snapshot (git, network only here) and spawns the
  compiler.
- Contents, per the sub-project scope: the top-level symbol index
  (classes, interfaces, traits, enums, functions, constants) with
  per-version availability metadata (introduced, removed, deprecated)
  extracted from the phpstorm-stubs metadata.
- The format is version-stamped and explicitly reserves two extension
  points: the signature payload (per-version deltas, sub-project 3) and
  the overlay merge (Celerrate refinements, plugin stubs). The merge
  point is designed; only the base layer is implemented.
- At runtime the embedded blob loads as a high-durability salsa input,
  filtered by the project's version range.

## 6. The invalidation boundary and name resolution

`celerrate_semantics` materializes the boundary the parent spec fixed
as a principle, in its minimal-but-real form (the approach decision of
this design): the identity scheme is complete and permanent, the
payload is only what this sub-project consumes.

**Stable identity.** Per file, an `AstIdMap` numbers the declaration
nodes in tree order: an `AstId` is `(FileId, index)`, and
reconciliation back to the concrete node goes through `SyntaxNodePtr`.
Editing a function body renumbers nothing: item identity survives
everyday editing.

**The minimal `ItemTree`.** Per file, the `Eq`-comparable,
deterministically ordered projection of declarations: namespaces, `use`
imports, and top-level classes, interfaces, traits, enums, functions,
and constants, each carrying kind, name, `AstId`, and the inheritance
names (`extends`, `implements`, trait `use`) as unresolved names
(sub-project 3 consumes them; they cost one field now). No members, no
bodies. This is the early-cutoff mechanism: a body edit produces an
identical `ItemTree`, salsa backdates it, and no dependent re-runs.
The signature/body query split is fixed structurally: everything above
this crate consumes `ItemTree`s and the index, never another file's
syntax tree. Body queries do not exist yet, but their boundary is
already the correct one. Syntax trees themselves are LRU-evicted and
reparsed on demand.

Deliberate narrowings, recorded here after implementation review. The
traversal that defines "the declaration nodes" (shared by the `AstIdMap`
and the `ItemTree`, so their numbering agrees by construction) descends
into control-flow blocks and function bodies — a declaration behind an
`if (!function_exists(...))` guard is an item, the section 7 stance
honored structurally — but never into a member list: class constants
and declarations nested inside method bodies are invisible to the
boundary, and nameless declarations (anonymous classes, error-recovery
wreckage) are skipped until the type engine gives them meaning.
Namespaces are carried as a field on each declaration and import rather
than as standalone items: the same information, in the `Eq`-stable
encoding the early cutoff wants. LRU eviction of syntax trees is
deferred to part 8, where the memory economics are measured alongside
the persistent cache; part 4 delivers the structural property that
makes eviction safe (no layer above the boundary holds a syntax node —
the `AstIdMap` and the `ItemTree` are plain `Send + Sync` values).

**The symbol index.** The project and vendor `ItemTree`s plus the stub
index merge into a global FQN-to-symbol index. Lookups go through
per-name queries, not through a dependency on "the whole index", so
adding a symbol in one file does not invalidate the checks of files
that never reference it. One PHP reality is assumed from day 1: class
and function names are case-insensitive, constant names are
case-sensitive; the index stores case-folded keys and retains the
original spelling (the "did you mean" diagnostics of sub-project 4
will need it).

**Name resolution**, full PHP rules at the top level: fully qualified
names, relative qualified names (`Foo\Bar`), and unqualified names with
the real fallback rules: classes resolve in the current namespace with
no global fallback; functions and constants try the current namespace,
then fall back to the global one. The per-file `use` tables (class,
function, const, with aliases and group forms) are part of the
`ItemTree`.

Deliberate narrowings and shapes, recorded here after implementation
review (part 5). The global index is realized as two tables: the source
table over the analyzed file set's `ItemTree`s and the stub table over
the version-filtered stub view, consulted in that order by the per-name
lookup — a project declaration shadows a stub, and a source edit never
re-copies the stub side. Case folding is ASCII (the engine's own
folding), and a constant folds its namespace segments while keeping its
terminal segment case-sensitive. Import tables group by the item tree's
namespace field (a whole namespace block sees its imports, position
within the block does not matter); class and function aliases match
case-insensitively, constant aliases case-sensitively, and a duplicate
alias keeps the last import. Duplicate declarations of one name resolve
to the deterministic first entry (file set order, then tree order);
duplicate-declaration diagnostics are later work. The analyzed file set
lives in `celerrate_db` as the section 2 input list names it.

**Syntax gating bypasses this boundary, deliberately.** It is a
per-file query reading its own file's typed AST (the construct-version
table). The boundary rule forbids cross-file syntax-tree reads; an
output strictly local to the edited file may read its own tree.

## 7. Diagnostics and the preview product

**Unknown symbols (top level).** A per-file query collects the
statically named references (`new X`, `extends`/`implements`, trait
`use`, type positions, `instanceof`, `catch`, `X::class`, attributes,
function calls, constant references) and resolves each one; an
unresolved reference produces a diagnostic. Two conservative stances
are documented engine semantics, not scope workarounds:

- Dynamic references (`new $class`, call-by-string) are out of scope.
- A symbol declared anywhere in project, vendor, or stubs counts as
  declared: no reachability analysis of conditional declarations
  (`if (!function_exists(...))` guards are too common to punish).

**Version gating.** The syntax family: a construct-to-minimum-version
table over the typed AST (`readonly class` is 8.2, property hooks and
asymmetric visibility are 8.4, `|>` and clone-with are 8.5, and so on),
checked against the range **min**. The symbol family, derived from the
stub availability metadata: availability checked at **min**, removal
and deprecation checked at **max**: the parent spec's range rule,
applied without signatures.

**`celerrate_diagnostics`, the minimal shared model.** The extraction
recorded as assumed debt takes this shape: a stable identifier, a
severity, and a primary span (`FileId` plus `TextRange`). Deliberate
narrowing, recorded here after implementation review: the structured
kind this section originally listed stays with the producing crate —
the identifier names the kind class, and the parameterized detail a
renderer needs (for example which token was expected) joins the shared
model when the preview renderer consumes it (parts 6 and 7), not
before. Stable identifiers (`CEL####`) are born here, with the first
publication: renumbering after users script against them would break
suppressions and tooling. The rich anatomy (annotated spans, notes,
structured suggestions) remains sub-project 4. `LexerDiagnostic` and
`ParserDiagnostic` project into the shared model with their own
identifiers.

**The preview product.** `celerrate_cli` ships `celerrate check`:
zero-configuration Composer detection, parallel per-file fan-out
through snapshots, a simple text rendering
(`file:line:column identifier message`) documented as temporary, and
fixed exit codes (0 clean, 1 diagnostics reported, 2 internal error).
`--watch` re-analyzes on VFS watcher events with cancellation of
in-flight work and incremental re-rendering: it is the showcase of the
differentiator and the first real exercise of the engine invariants.
The minimal internal-error report (catch_unwind produces a clean
message with a pre-filled issue invitation) ships with the binary, not
later. The published claim follows the parent spec: the preview is
honest about being a preview.

## 8. The persistent artifact cache

Per the parent spec's honest design: salsa's in-memory state is not
serialized; the cache is content-addressed and sits **above** salsa.
`.celerrate/cache/` persists selected query outputs (`ItemTree`s, the
symbol index, per-file diagnostics), keyed by content hash plus the
binary, configuration, and stub versions, and re-seeds a fresh database
at startup. The economics are an acceptance criterion, measured, not
hoped: if deserialization plus revalidation does not beat recomputation
for an artifact class, that class is dropped from the cache. Corruption
is detected (versioning plus checksums) and silently regenerated, never
fatal. The flagship incremental number is measured **warm one-shot**: a
full CLI run including process startup and cache loading, exactly as
the parent spec's benchmark protocol names it.

## 9. Testing

1. **Unit tests, TDD**: every part starts from failing tests.
2. **The incremental correctness harness**, born with the first query
   and grown by every part, never bolted on at the end: edit sequences
   replayed over the corpus, with the incremental result asserted
   byte-for-byte identical to a from-scratch analysis, under varying
   thread counts.
3. **Invalidation-scope tests**: salsa event instrumentation asserting,
   per canonical edit class (body edit, signature edit, comment-only
   edit, new file, configuration change), exactly which queries
   re-executed. This is the direct test of the `ItemTree` early cutoff,
   which the correctness harness alone cannot catch.
4. **The pinned Symfony corpus enters CI**: anti-false-positive runs
   for the unknown-symbol family, and the benchmark corpus.
5. **The benchmark protocol is committed** with the criterion harness,
   per the parent spec's pinned-protocol requirements.
6. **Zero-panic lints** apply to every new crate with no exceptions
   outside test modules; the existing fuzz targets continue unchanged.

## 10. Implementation plans

One design (this document), eight implementation plans, each its own
TDD cycle, in order:

1. **Foundation**: the `celerrate_diagnostics` extraction,
   `celerrate_vfs`, and `celerrate_db` (salsa in place, `parse` and
   `line_index` as queries, the harness and instrumentation skeletons).
2. **Project**: `celerrate_project` (Composer discovery, autoload rules
   driving the VFS walk, the PHP version range).
3. **Stubs**: the pinned snapshot, the `xtask` compiler, the committed
   blob, availability metadata.
4. **Boundary**: `AstIdMap`, the `ItemTree`, and the invalidation-scope
   tests proving the early cutoff.
5. **Resolution**: the symbol index and top-level name resolution.
6. **Checks**: unknown-symbol and version-gating diagnostics, with
   their `CEL####` identifiers.
7. **Product**: `celerrate_cli`, `check`, `--watch`, panic isolation,
   the internal-error report.
8. **Closure**: the persistent artifact cache, the Symfony corpus in
   CI, the committed benchmark protocol and published number, the
   `v0.0.x` release.

The incremental harness is not a separate plan: every plan ships its
own invalidation-scope assertions as it goes, the way error recovery
shipped with every parser plan.
