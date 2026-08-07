# Celerrate — Vision and Engine Architecture Design

Date: 2026-07-09 (amended 2026-07-23)
Status: Approved (brainstorming output; each sub-project gets its own detailed spec)

Amendment history:

- 2026-07-11 — amended after a five-lens architecture review (incremental
  engine, crate layering, type engine, extensibility, product/delivery):
  honest persistent-cache design, engine invariants (cycle recovery,
  cancellation, parallelism discipline), corrected crate layout with
  dependency inversion, re-scoped v0.1 success criterion, deterministic
  plugin failure semantics, pinned benchmark protocol, and sequencing
  adjustments (preview milestone, framework-providers sub-project).
- 2026-07-11 — recorded the `celerrate_diagnostics` extraction as assumed
  debt from Foundations toward the semantic-core sub-project (section 11):
  lexer and parser diagnostics stay in `celerrate_syntax` until the second
  diagnostic producer appears.
- 2026-07-14 — recorded a second public preview milestone, added by the
  type-engine sub-project's design
  (`.claude/superpowers/specs/2026-07-14-type-engine-design.md`): a
  `v0.0.x` preview ships at the end of sub-project 3 carrying the
  unknown-members, nullability, and argument-types families, completing
  the v0.1 criterion's diagnostic set before sub-projects 4 and 5, which
  then owe only rendering and product surface. The sequencing of
  section 11 named one preview (end of sub-project 2); this is an
  addition, with the same anti-false-positive gate per family and a
  minimum shippable set defined in the sub-project's design.
- 2026-07-19: resolved an internal contradiction about where the
  persistent cache lives. Section 3 lists the disk cache under
  `celerrate_cli`, while section 11 says the cache was "pulled forward
  from the CLI sub-project" into semantic core and later that "the disk
  cache moved to sub-project 2", which reads as a relocation in the
  layering. The pull-forward was sequencing only (when the cache was
  built), not placement: the cache seams (the artifact-cache inputs and
  the pure queries that consult and revalidate them) live
  dependency-inverted in the domain crates, and the persistence
  orchestration stays at the composition root until a second binary
  needs it. Section 3 now carries the clarification.
- 2026-07-23 — sub-project 4 (diagnostics and fixes) closed: the rule
  framework with four phase traits and sealed contexts, the
  structured-edit library, identifier-level suppression with the
  native directive, the autofix engine, rustc-style rendering, and
  `celerrate explain` with an executable page per identifier. No
  version tagged; the next public event is v0.1 at the end of
  sub-project 5.
- 2026-08-07: amended the published cold-run performance target down from
  "at least ~20x faster than PHPStan" to "at least ~9x", on the evidence of
  the cold-run performance diagnostic
  (`.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`):
  the arithmetic ceiling on the pinned corpus is 30.1x, the local path's
  levers with a measured mechanism reach a ceiling of about 9.2x and the
  full priced lever list one of about 12.3x, ideal ten-core scaling of every
  phase would reach 17.7x against a measured stagnation at eight threads,
  and the shared-nothing architectural alternative was measured to deliver
  no gain at any partitioning or thread budget tried. Section 7's
  published-performance-targets paragraph carries the derivation.

## 1. Vision

Celerrate is a complete toolchain for PHP, written in Rust, published as open
source under JDevelop, dual-licensed **MIT OR Apache-2.0** (the Rust
ecosystem convention: Apache-2.0 adds an explicit patent grant, MIT keeps
maximum simplicity). The "Celerrate" name is a JDevelop trademark. It is a
serious competitor to the existing PHP tooling ecosystem and aims to replace
it over time.

Five pillars, all built on one shared engine:

1. **Static analysis** — replaces PHPStan / Psalm. Interprocedural type
   analysis with fine-grained incremental computation. This is the first
   public deliverable and the primary differentiator.
2. **Lint + formatting** — replaces PHP-CS-Fixer / PHP_CodeSniffer. Style
   rules join `celerrate check` as a rule group; the formatter is a separate
   command.
3. **LSP / editor** — replaces Intelephense / Phpactor. Completion,
   navigation, refactoring in the IDE, served by the same engine.
4. **Automated refactoring** — replaces Rector. PHP version migrations and
   large-scale code transformations.
5. **Security analysis** — taint analysis (sources → sanitizers → sinks,
   interprocedural), a market gap: Psalm's implementation is slow and little
   used; PHPStan has none. Incremental taint analysis usable on every commit
   is a major differentiator against Psalm and against Semgrep/Snyk.

Additional future consumers of the engine: architecture rules (deptrac
replacement) and semantic code generation.

Competitive context: Mago already exists (PHP toolchain in Rust, lint +
format). Celerrate's differentiator is deep interprocedural analysis and
incremental computation — the PHPStan/Psalm territory nobody has conquered in
Rust.

### Positioning: a new norm, with bridges

Celerrate defines its own type annotation norm (clean slate) and aims to
become the new standard. Compatibility with existing conventions is provided
by plugins:

- The PHPStan/Psalm syntax bridge is **first-party, shipped with the tool,
  and enabled by default**. Day-1 experience on existing codebases is
  seamless; the Celerrate norm becomes the promoted path once it freezes
  (section 4), the bridge is a migration ramp.
- A future `celerrate migrate --to-celerrate-types` autofix converts existing
  PHPDoc annotations to the Celerrate norm.

### Success criterion for v0.1

v0.1 = `celerrate check` can analyze a real Symfony codebase end to end with
a restricted but reliable set of diagnostics (nullability, argument types,
unknown symbols), no visible false positives, with speed as the proof. Depth
comes later.

Two deliberate boundaries make this criterion honest:

- **Magic-method semantics.** On classes with `__get`/`__set`/`__call`/
  `__callStatic`, unknown-symbol diagnostics are conservatively suppressed:
  the engine cannot know what the magic resolves, so it stays silent rather
  than guessing — the same stance PHPStan and Psalm take. This is documented
  engine semantics, not a scope workaround; dynamic type providers
  (section 5) progressively restore precision by declaring what the magic
  actually exposes.
- **Laravel enters the measured corpus with the framework-providers
  sub-project** (section 11), not before: Laravel's idioms (Eloquent magic
  members, facades, container resolution) are dominated by exactly the
  patterns that need providers, and claiming "no false positives" there
  without them would be either untrue or vacuous. v0.1 runs on Laravel
  without crashing and without a false-positive storm (the suppression rule
  guarantees that), but the published claim is Symfony.

The v0.1 positioning is "reads what you already have": the PHPStan/Psalm
bridge carries the launch, and the Celerrate norm is not part of the v0.1
public surface (section 4).

## 2. Language support

- **PHP 8.1+.** The parser always parses the full, most recent grammar it
  knows. Version gating is a semantic diagnostic, not a parse failure: using
  `readonly class` (8.2 syntax) in a project whose range starts at 8.1
  produces a precise, actionable diagnostic while the rest of the file is
  still analyzed.
- **Version range model.** The supported PHP version is a range `[min, max]`:
  - Availability checks (does this symbol/syntax exist?) run at **min** —
    the code must work on the whole declared range.
  - Deprecation and removal checks run at **max** — the code must survive
    the newest supported version.
  - Signatures that change across the range are checked against the whole
    range: arguments must satisfy the **intersection** (most restrictive
    form) of the signature across `[min, max]`, and the call's return type
    is the **union** (least restrictive) across the range. The stub compiler
    therefore stores per-version signature deltas, not a single signature
    plus availability flags (section 3).
- **Version detection precedence:**
  1. Explicit `php = "8.2"` in `celerrate.toml` (range collapses to a point).
  2. `config.platform.php` in `composer.json`.
  3. The `require.php` constraint in `composer.json`, interpreted as a range.
  4. Fallback: latest supported stable version, with a warning suggesting
     explicit configuration.
- No detection of the local runtime (`php -v`): it is not reproducible across
  machines and would poison a deterministic, cacheable analysis.
- Simultaneous multi-version analysis is out of scope for v1.

## 3. Engine architecture

### Execution model: incremental query engine from day 1

The core is a memoized query database with dependency tracking (the `salsa`
model, as used by rust-analyzer and ty). Parsing a file, resolving a symbol,
inferring a function's type: each step is a query whose result is cached and
finely invalidated when an input changes. Incrementality is the structure of
the engine, not a feature added later.

Rationale: the three structural requirements — interprocedural analysis,
incremental computation, a future LSP — all point to the query model. History
shows batch engines do not get incrementalized after the fact; they get
rewritten (rust-analyzer exists because rustc could not be converted). ty
proves the model is viable for a dynamic language.

Data flow is demand-driven, not pipeline-pushed: the CLI asks for "project
diagnostics", which asks for per-file diagnostics, which ask for types, which
ask for name resolution, which asks for indexes, which ask for syntax trees.
File-level fan-out is parallelized with rayon.

**Invalidation boundary.** Semantic queries must not depend on raw syntax
trees: any edit rebuilds the whole tree, so a query reading it directly is
invalidated by every keystroke, and the engine degrades to batch behind an
incremental facade. `celerrate_semantics` therefore exposes stable-identifier
representations (the rust-analyzer `ItemTree`/`AstId` pattern) as the sole
input of everything above it, with signatures and bodies split into separate
queries so that editing a function body does not invalidate dependents of its
signature. The exact identifier scheme, the signature/body query split, and
whether bodies are lowered to a desugared form before inference are designed
in the semantic-core sub-project's spec; this document fixes the principle,
because the published incremental performance targets (section 7) depend on
it, and the incremental correctness harness (section 9) cannot catch its
absence: that harness verifies the incremental result, not how little work
was redone to produce it.

The boundary only cuts invalidation through three mechanisms the
semantic-core spec must treat as part of the same principle: **early
cutoff** (salsa re-executes the boundary query after an edit, gets an
`Eq`-identical result, and backdates it so dependents never re-run — which
requires boundary types with cheap, deterministic `Eq`/`Hash` and
deterministically ordered contents, or backdating silently never fires),
**interned identifiers** for the stable IDs, and **durability tiers**
(stubs, `vendor/`, `celerrate.toml`, and the PHP version range are
high-durability inputs whose dependency subgraphs are skipped wholesale on
a user-code edit; the stdlib index is a salsa input, but not "like any
other"). Syntax trees are evicted under an LRU policy and reparsed on
demand — cheap to recompute, expensive to retain. The boundary is also
regression-tested directly, not just asserted: see the invalidation-scope
tests in section 9.

**Engine invariants.** Four properties are fixed here because they cannot
be retrofitted:

- **Cycle recovery.** Interprocedural inference over user-controlled code
  guarantees query cycles: mutual recursion is ordinary PHP, and degenerate
  inputs like `class A extends A` reach the resolver because parsing never
  fails. Salsa's default response to a cycle is a panic, which would violate
  both the zero-panic rule and error resilience (section 8). Recursive
  queries therefore resolve through salsa cycle recovery with **fixpoint
  iteration and widening** (section 4), and cycle handling is deterministic
  regardless of which participant is queried first.
- **Cancellation.** The future LSP requires in-flight queries to be
  cancelled when an input changes; salsa signals this by unwinding with
  `salsa::Cancelled`. Cancellation-on-edit is an engine requirement from the
  start, not an LSP-era addition.
- **Panic isolation.** The per-file `catch_unwind` safety belt (section 8)
  wraps only the outermost per-file diagnostics query call, is transparent
  to `salsa::Cancelled` (always re-raised), and its product — the internal
  error report — is never memoized.
- **Parallelism discipline.** Rayon fan-out happens only through database
  snapshots at declared fan-out boundaries, never inside queries (a rayon
  thread blocking on a query claimed by another stalled task in the same
  pool deadlocks). Parallel collection is deterministically ordered before
  rendering or comparison.

### Crate layout (Cargo workspace, strict layering)

```
celerrate_source       source files, spans, positions
celerrate_diagnostics  the diagnostic data model (identifiers, annotated
                       spans, notes, structured suggestions) — used by every
                       layer that reports, from the parser up
celerrate_syntax       lexer + parser → lossless syntax tree
celerrate_edit         structured-edit library on the syntax tree (powers
                       autofixes, later the formatter, migrations, codegen)
celerrate_vfs          file loading and in-memory overlays feeding the inputs
celerrate_db           salsa inputs and foundational queries (files, config,
                       parse) — the base-db layer, not the whole engine
celerrate_project      Composer discovery, autoload rules, PHP version range
celerrate_stubs        phpstorm-stubs snapshot, build-time stub compiler,
                       versioned binary format, overlay merging
celerrate_semantics    symbol index, name resolution, stable identifiers
celerrate_types        inference and type system
celerrate_rules        rule framework, rule registry, rendering
celerrate_plugin       plugin API facade (native traits first, WASM host later)
celerrate_cli          the binary: config, orchestration, disk cache
```

The dependency rule is a **DAG with no upward edges**, not a total order: a
crate depends only on crates strictly below it, but need not depend on all
of them (a pure-syntax style rule does not pull in `celerrate_types`).
`celerrate_ide` is reserved for the LSP-era feature layer between
`celerrate_rules` and the binaries.

Three clarifications the list alone does not carry:

- **The concrete salsa database lives at the top, not the bottom.** Query
  definitions live with their domain crates; `celerrate_db` holds only the
  input definitions and the foundational queries every layer shares (the
  rust-analyzer `base-db` pattern). The concrete `salsa::Database`
  implementation that aggregates all storage is assembled at the composition
  root (`celerrate_cli`, later the LSP binary).
- **Extension points are dependency-inverted.** Each consuming layer owns
  its extension-point traits: `celerrate_types` owns the type-syntax and
  dynamic-type-provider traits, `celerrate_semantics` owns the
  virtual-symbol and stub-provider traits, `celerrate_project` owns the
  discovery traits. Implementations are registered as salsa inputs at the
  composition root; `celerrate_plugin` is the aggregation facade that
  re-exports the stable API surface and, later, hosts the WASM adapter
  implementing those same traits. Nothing ever depends upward.
- **The persistent cache splits across the same boundary** (amendment
  2026-07-19). The cache seams (the artifact-cache inputs and the pure
  queries that consult and revalidate them) live dependency-inverted in
  the domain crates (`celerrate_semantics`, `celerrate_types`); the
  persistence orchestration (on-disk format, pack collection, write
  policy) is a composition-root concern in `celerrate_cli`, like the
  concrete database. Section 11's "pulled forward from the CLI
  sub-project" is sequencing, not placement: the cache was built during
  semantic core because the flagship incremental number cannot be
  measured without it, and it landed in this layout, not below it.
  Extracting the persistence machinery into a shared library crate is
  deferred until the second binary (LSP or daemon) exists to consume it.

The stub compiler uses `celerrate_syntax` as a build-dependency (a separate
dependency graph in Cargo, so no layering violation), and the compiled
blob's binary format is version-stamped because its schema is coupled to
the runtime crates that read it.

### Parser

- Produces a **lossless concrete syntax tree** (red-green trees, rowan-style):
  every whitespace and comment is preserved. Required by the future
  formatter, autofixes, refactoring, and codegen.
- **Error-resilient: the parser never stops.** A file with syntax errors
  yields a partial tree with error nodes. An LSP and a useful CI must analyze
  code that is mid-edit.
- A typed AST layer is generated on top of the CST for ergonomic rule
  authoring.

### Stubs (stdlib and extension definitions)

- Source of truth: **phpstorm-stubs** (the de-facto standard, maintained,
  version-annotated).
- Compiled **at build time** (owned by `celerrate_stubs`) into a compact
  binary format embedded in the `celerrate` binary: pre-parsed, pre-indexed,
  carrying per-version metadata as **signature deltas** (added in 8.2,
  signature changed in 8.3, deprecated...) so range checks can apply the
  intersection/union rule of section 2. Zero startup cost; the stdlib index
  is a high-durability salsa input.
- **Overlay system**, increasing priority: base stubs → Celerrate refinements
  (enriched signatures written in the Celerrate norm, the equivalent of
  PHPStan's functionMap) → plugin-contributed stubs (required by framework
  plugins).
- The target version range filters stub visibility.

### Persistent cache

Salsa does not support serializing its database — memo tables, revisions,
and interned IDs are in-memory only, and neither rust-analyzer nor ty
persists them — so Celerrate does not pretend to. The persistent cache is a
**content-addressed derived-artifact cache above salsa**: selected query
outputs (item trees, symbol indexes, per-file diagnostics) are persisted to
`.celerrate/cache/`, keyed by content hash plus the binary, configuration,
stub, and plugin-set versions, and used to re-seed a fresh database at
startup. First run: full parallel analysis. Subsequent runs — including in
CI when the cache is restored between jobs — recompute only what the
changed inputs invalidate. The economics are a design constraint, not a
hope: deserialization plus revalidation must beat recomputation, or the
affected artifact class is dropped from the cache as a net loss. Corrupted
caches are detected (versioning + checksums) and silently regenerated,
never fatal.

## 4. Type engine

Three components:

1. **Inference.** Infers everything not annotated: local variables by
   propagation, function returns from bodies, control-flow narrowing (after
   `if ($x instanceof User)`, `$x` is `User`; `assert()`, `match`,
   comparisons). Interprocedural: the type of a call is computed from the
   callee's body when unannotated — a memoized salsa query. Three structural
   decisions are fixed here and detailed in the type-engine sub-project's
   spec:

   - **Recursion resolves by fixpoint iteration with widening.** Mutual
     recursion makes callee-return queries cyclic; iteration terminates
     because the lattice widens (literal-to-general widening, union arity
     caps, array-shape depth caps), deterministically regardless of entry
     point (section 3, engine invariants).
   - **Callee queries are parameterized by the resolved receiver class**
     where PHP semantics demand it: `static`/`$this` returns under late
     static binding, and trait methods (analyzed per using class, as PHPStan
     does). "Paid once" means once per (callee, receiver) key, not once per
     function.
   - **Unannotated parameters are monovariant (`mixed`) in v0.1**;
     call-site-sensitive parameter inference is an explicit later extension.

   The type system covers unions (`User|null`), literal types (`'active'`,
   `42`), array shapes (`array{id: int, name: string}`), intersection types
   (native in PHP 8.1), enums (including `match` exhaustiveness), callable
   signatures (`callable(int): string`, first-class callable syntax), and
   generics on classes and functions. Generics ship with call-site template
   inference and basic variance, or they ship **inference-only** (inferred
   and propagated but never reported as mismatches): generics diagnostics
   without template solving produce exactly the visible false positives
   v0.1 forbids. A first-party **stdlib type provider written in code** (the
   dynamic-type-provider extension point of section 5, compiled in) covers
   the computation-dependent stdlib signatures no declarative stub can
   express: `array_map` from its callable, `json_decode` from its flags,
   `preg_match` `$matches` shapes.
2. **The Celerrate norm.** Our own annotation syntax, designed after twenty
   years of PHPDoc lessons — living in docblocks (full runtime compatibility)
   but with a clean, strict, formally specified, versioned grammar. The exact
   syntax is designed in the type-engine sub-project's own spec; this
   document fixes the principle: one official norm, formally specified.
   **Timing:** the norm is designed as an internal draft during the
   type-engine sub-project (designing the bridge and the lattice against a
   target keeps both honest) but is neither published nor frozen in v0.1 —
   no public documentation, no `migrate --to-celerrate-types`, no stability
   promise. It freezes in v1.x, informed by real-world feedback; freezing a
   public grammar before any user feedback would be a premature API commit.
3. **The compat bridge** (first-party plugin, enabled by default) translates
   PHPStan/Psalm annotations into internal types. It is one plugin and one
   lexer, but **two explicit semantic dialects**: PHPStan and Psalm diverge
   where it hurts (assertion tags, template variance, purity tags,
   conditional-return details), and each tool lets its own prefixed tag win
   when several coexist on one docblock. The bridge carries a documented
   per-tag precedence and conflict-resolution table rather than one merged
   grammar that would silently mistranslate one tool's semantics. It also
   honors inline suppressions (`@phpstan-ignore-line`, `@psalm-suppress`):
   a codebase's existing suppression decisions are respected by default,
   because re-reporting them reads as false positives to the user. Internal
   engine types are **the only currency**: plugins produce internal types,
   never their own representation.

## 5. Extensibility

Model: **hybrid Rust native + WASM.**

- First-party plugins (PHPStan/Psalm bridge, framework rules for
  Laravel/Symfony) are Rust crates compiled into the binary: maximum
  performance on the hot path.
- Between configuration and WASM sits a **declarative tier**, the primary
  community extension mechanism and the day-1 authoring path for PHP
  developers (who should not need to learn Rust to extend a PHP tool): stub
  files written in annotation form for virtual symbols, and
  configuration-declared dynamic return types (the functionMap pattern).
  This covers most of what framework ecosystem packages actually need, and
  it is deterministic and sandboxed for free.
- Community plugins that need real computation go through a WASM API
  (writable in Rust, Go, AssemblyScript, and potentially compiled PHP
  later). WASM plugins are sandboxed with **deterministic failure
  semantics**: runaway plugins are bounded by deterministic fuel metering
  (wasmtime fuel), so "this plugin exceeded its budget on this input" is a
  pure function of the input and can be memoized; a plugin that crashes is
  disabled for the **entire run** — its contributions removed uniformly,
  the run reported as degraded, nothing from it entering the persistent
  cache. Never a per-query fallback: a wall-clock kill inside the
  dependency graph would make results machine-speed-dependent and poison
  the cache. Plugin identity (name, version, configuration) is itself a
  salsa input, so upgrading a plugin invalidates its contributions. The
  WASM host is out of scope for v0.1; the native plugin API (traits) ships
  first and the same extension points back both.

Extension points span **every layer**, as declared interfaces:

- Type syntaxes (understand other annotation notations — the compat bridge).
- Dynamic type providers (`Container::make($class)` returns an instance of
  `$class`; Eloquent magic members).
- Virtual symbols and stubs (symbols absent from sources: PHP extensions,
  framework magic).
- Project discovery (non-standard autoload, DI container dumps). Discovery
  plugins only **declare additional input files** (container dumps, route
  caches, generated manifests) that enter salsa as tracked inputs like any
  source file; they never execute project code or read the environment, and
  staleness of a dump is the user's visible responsibility, not hidden
  nondeterminism.
- Rules and diagnostics with their autofixes — same API for core and plugins.
- Later: migration/codegen recipes, architecture rules, LSP actions.

Contract: extension points are **declared, deterministic functions of their
inputs** — plugin contributions enter salsa's dependency graph, so imperative
hooks mutating internal state would break cache correctness. Declared APIs
also stay stable while internals evolve, which is the same contract the WASM
API imposes anyway.

Three commitments keep that contract honest as the API ages:

- **The native API is WASM-projectable from day 1.** Plugins hold opaque
  handles (`TypeId`, `SymbolId`) and query the engine back through a narrow
  host interface — no borrowed internals, no retained database references,
  no closures. Dynamic type providers are bidirectional (they query types
  while producing types), which is trivial for native traits and the hard
  case across a WASM boundary; shaping the API for that case now is what
  makes "the same extension points back both" true rather than hopeful. A
  sketch of the WASM-level interface is an acceptance artifact of the
  type-engine sub-project, even though the host ships later, and the API is
  not called v1 before at least two dissimilar first-party consumers
  exercise it (the compat bridge plus one framework dynamic type provider).
- **The type representation is never exposed.** Plugins construct types
  through builders and interrogate them through query methods; there is no
  exhaustively matchable type enum in the public API, because the
  representation is exactly the internal that evolves most. The plugin API
  carries an explicit version, checked at plugin load.
- **First-party plugins go through the public API, mechanically.** They
  depend only on `celerrate_plugin`, enforced in CI like the zero-panic
  policy. An extension point that proves insufficient is extended, never
  bypassed — otherwise the public API becomes an untested facade.

Deliberate exception (decided later): the formatter will likely be
non-extensible (opinionated, Prettier/rustfmt-style) — a product choice, not
a technical limit.

## 6. Diagnostics, suggestions, autofixes

Rust-ecosystem-level DX is a hard requirement. Four commitments:

1. **Anatomy of a diagnostic:** a stable documented identifier (`CEL0231`), a
   one-sentence main message, one or more **annotated spans** rendered
   rustc-style (code excerpt, labeled underlines; primary span points at the
   error, secondary spans give context: "the parameter is declared `int`
   here"), **notes** explaining the engine's reasoning ("inferred type is
   `string|null` because this path returns `null`"), and zero or more
   **suggestions**.
2. **Structured suggestions and autofixes.** A suggestion is not text: it is
   a set of structured edits on the syntax tree with a confidence level —
   **safe** (mass-applicable via `celerrate check --fix`, guaranteed not to
   change semantics) or **needs review** (`--fix-suggestions`, or interactive
   choice in the future LSP). The edit engine preserves surrounding style.
   It is designed as a **general structured-edit library**, not a
   diagnostics-internal utility — the same machinery powers codegen,
   migrations, and the formatter later.
3. **`celerrate explain CEL0231`.** Every identifier has a long-form
   explanation page: why it is a problem, failing and fixed examples, how to
   configure the rule. Embedded in the binary, mirrored on the documentation
   site.
4. **Anti-false-positive policy.** A doubtful diagnostic does not ship. Every
   rule is tested against a corpus of real projects (Symfony, Laravel, and
   their ecosystems); a reported false positive is a priority bug, not an
   opinion. The corpus runs in Celerrate's own CI. The policy is structural,
   not heroic: corpus projects are **pinned to commit SHAs** with a
   scheduled bump cadence (unpinned HEADs would destroy the regression
   signal), and a rule with a confirmed open false positive is
   **automatically demoted** out of the default tier (nursery-style) until
   fixed — the promise "no false positive ships at default severity" is
   enforced by process, not by maintainer response time.

Rule framework: a rule is a declarative unit (metadata, consumed queries,
visit function), registrable by core or by a plugin through the same API.

## 7. Product surface (CLI)

### One analysis command

**One engine, one analysis command.** `celerrate check` runs every enabled
rule group: `correctness` (types), `style` (lint), `security` (taint),
`architecture` (dependency rules). Groups are enabled, configured, and
severity-mapped in `celerrate.toml`. There are no separate `lint` or `taint`
subcommands: "is my code OK?" has one answer. Enabling taint analysis is a
configuration line, not a new tool to install.

Separate subcommands exist only for what is not a diagnostics pass:
`format` (rewriting), `migrate` and `generate` (transformations), `lsp`
(server mode), `explain`, `daemon`.

**Naming convention: subcommands are full words** (`format`, not `fmt`;
`generate`, not `gen`; `lsp` is a standard acronym, like API). Short aliases
may exist for convenience, but the canonical form, documentation, and
examples always use the full word.

### Configuration

`celerrate.toml` at the project root (TOML: readable, commentable, none of
YAML's traps). **Zero config required**: without a file, Celerrate detects
`composer.json`, the autoload, the PHP range, and analyzes with defaults.
Configuration covers: PHP range, include/exclude paths, per-diagnostic or
per-group severity, plugin activation.

### Baseline

`celerrate check --baseline` records current diagnostics in a versionable
file; only new problems fail afterwards. The format is designed to survive
line movement (structural fingerprint rather than line number). Essential for
adoption on existing codebases. The fingerprint format and its failure modes
(collisions, renames orphaning entries, one entry masking a genuinely new
duplicate) are designed in the CLI sub-project's spec; this document fixes
the invariants: an entry survives line movement, dies with its diagnostic,
and never suppresses more than one occurrence.

### Migration from PHPStan

The bridge covers annotations; migration covers workflow. `celerrate migrate
--from-phpstan` imports `phpstan.neon` (paths, excludes, level mapped to a
severity profile) and converts an existing `phpstan-baseline.neon`, so a
PHPStan project's first run needs zero configuration and keeps its "only new
problems fail" continuity at the exact moment of switching. Inline
suppression comments are honored by the bridge (section 4). This is the
Ruff-versus-flake8 lesson: the difference between a one-command migration
and a weekend project decides adoption.

### Outputs

Rich human rendering by default (colors, annotated spans); `--output=json`
(stable, versioned schema for tooling); GitHub Actions (native PR
annotations); SARIF (GitLab, IDEs, security platforms).

### Distribution

Single static binary per platform (Linux x64/arm64, macOS x64/arm64,
Windows). Channels: install script, Homebrew, a bootstrap Composer package
that downloads the binary (the bridge to PHP habits, as Biome does via npm),
official Docker image, GitHub Action. Platform tiers are explicit: Linux and
macOS are tier 1 (the corpus and benchmarks run there in CI); Windows is
tier 2 — built and tested, best-effort analysis correctness — until the
corpus runs on it, because path separators and case-insensitive filesystems
affect autoload resolution and cache identity, which is real analysis work,
not packaging.

### Published performance targets

Held in CI by benchmarks: at least ~9x faster than PHPStan on a cold full
analysis, and sub-second incremental updates on single-file changes in a
Symfony-sized project.

Position at the end of the CLI product sub-project, re-measured after the
check-pipeline performance work (issue #124): the incremental target is
met and published. The cold comparison is re-measured on the same pinned
corpus (issue #118): PrestaShop 9.0.3, 6932 first-party PHP files. On
that corpus, at PHPStan rule level 5, Celerrate is now 8.01x faster on
the wall clock (the pooled median over three full runs, a 6.87 % span
across the per-run ratios) and consumes 11.0x less CPU (2026-08-06, the
median of the three full runs' own CPU totals — hyperfine reports one
CPU total per invocation, not per timed run, so the two columns do not
pool the same way). The "at least ~20x faster" ambition this section
previously held is amended down to "at least ~9x" on the evidence of the
2026-08-07 cold-run performance diagnostic
(`.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`).
Both reasons the previous measurement gave for not testing the gap are gone:
the quadratic did-you-mean pass in the presentation layer is fixed, and the
whole process now runs at 4.51 effective cores of 10 (22.0 s of CPU over
4.874 s of wall clock), up from the 1.27 the superseded run measured the
same way (17.0 s over 13.41 s), still short of PHPStan's own 6.21. The 8.01x
above is the highest of three same-session cold ratios now on record, and
the diagnostic explains the spread rather than resolving it: it paired
against a PHPStan median near 39.0 s, the top of PHPStan's 31 s to 39 s
band, while the diagnostic's own two sessions paired against 32.652 s (6.6x)
and 38.361 s (7.3x), PHPStan's median having moved 17.5 % between two
sessions on the same day and the same machine. Against that spread, the
diagnostic measured what fills the remaining gap, and three of its findings
set the new number. First, the arithmetic ceiling on this corpus and machine
is 30.1x: walking, reading, lexing and parsing all 24033 PHP files a cold
run touches costs 1.274 s against PHPStan's same-session cold median of
38.361 s, so no optimization of anything Celerrate does above parsing can
beat that ratio, and the truly reachable ceiling is lower than 30.1x by a
margin the diagnostic deliberately does not measure. Second, the local
path's priced levers reach a ceiling of about 9.2x with mechanisms behind
them and about 12.3x in total: bringing every phase that runs under rayon
today up to the analysis fan-out's own measured parallel efficiency is worth
about 8.0x, the measured allocator gain takes it to between 8.1x and 8.4x,
and the filesystem walk and the salsa interning lock take it to 9.2x, which
is that subset's ceiling and not its floor, since only one of its four
components (the allocator's 95 ms inside the fan-out, worth 7.4x on its own)
is a measured gain rather than an upper bound. Reaching 12.3x needs every
priced lever to land at its upper bound at once, including two whose bounds
no mechanism supports and one contradicted by its own phase's measured
curve. Ideal ten-core scaling of every phase would reach 17.7x, and the same
diagnostic measured that scaling has already stagnated at eight threads, so
that bound describes a class of work rather than an outcome. The ~9x
published above is a deliberate stretch: the largest figure that sits
comfortably inside the levers with a measured mechanism is about 8x, and 9x
needs nearly everything those levers can give. Third, the architectural
alternative was priced and rejected: a shared-nothing, PHPStan-style split
into isolated worker processes is slower in wall clock at every partitioning
and thread budget measured, and burns 1.46x to 2.06x the single process's
processor work. Reaching ~20x would require compressing everything Celerrate
does above parsing by about 6.2x, and no measurement in that campaign bounds
whether any part of that compression is achievable, so ~20x stays
arithmetically possible on this corpus while being reached by no path the
campaign priced. It is recorded here as an unbounded aspiration, not as a
held target. Section 11 of
`.claude/superpowers/specs/2026-08-02-benchmark-comparison-corpus-design.md`
carries the evidence for the earlier measurement and the estimate of what
removing that churn and parallelising the rest would do to the
wall-clock ratio — near 4.5-6 s, 6x-8x — which this measurement has
since tested: 4.874 s, 8.01x, the top of that range.

Published numbers follow a **pinned benchmark protocol**, committed to the
repository with the harness: PHPStan version, rule level, result cache
explicitly off, `--parallel` setting, PHP version and opcache state, corpus
commit SHA, corpus size (files and lines), and hardware. Since v0.1 runs a
handful of diagnostic families while PHPStan runs hundreds of rules, the
comparison either matches enabled-rule scope or discloses the asymmetry
explicitly. The incremental target names its execution mode: a warm
one-shot CLI run, including process startup and artifact-cache loading. A
benchmark that would not survive third-party scrutiny is not published —
the anti-false-positive policy applies to performance claims too.

## 8. Safety, error handling

**Safe by design, zero panic — mechanically enforced, not promised:**

- `#![forbid(unsafe_code)]` across the workspace (audited exceptions only if
  a critical dependency requires one).
- Clippy lints `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` at
  **deny** level in CI. Production code paths go through `Result` and types
  that make invalid states unrepresentable.
- No user input ever crashes Celerrate: malformed PHP, corrupted
  `composer.json`, exotic encodings produce diagnostics, never a crash.
- Internal engine bugs are isolated per analyzed file (analysis of other
  files continues) and surface rustc-style: a clean "internal error" report
  with minimal context and a pre-filled issue invitation. Per-file
  `catch_unwind` remains as a last-resort safety belt, not a handling mode.
- WASM plugins are sandboxed with deterministic failure semantics (fuel
  metering for runaways, whole-run disablement on crash), as specified in
  the extensibility section.
- Continuous fuzzing verifies the promise.

## 9. Testing strategy

TDD as the default development loop, with five tiers:

1. **Unit tests per crate**, at layer boundaries.
2. **Snapshot tests** (`insta`) for the parser and diagnostics: a corpus of
   PHP files → snapshots of trees and rendered output. The dominant tool of
   day-to-day rule development.
3. **Incremental correctness harness** — the most critical of the project:
   after any simulated modification, the incremental result must be
   **byte-for-byte identical** to a from-scratch analysis. A dedicated
   harness replays edit sequences over the corpus, under varying thread
   counts to flush ordering nondeterminism. Most of a salsa engine's subtle
   bugs live here. Its complement, **invalidation-scope tests**, instruments
   salsa's execution events and asserts, per canonical edit class (body
   edit, signature edit, comment-only edit, new file, configuration change),
   exactly which queries re-executed — e.g. editing a function body must not
   re-run inference for its callers. The correctness harness verifies the
   result; the scope tests verify how little work produced it, which is what
   the section 7 targets actually depend on.
4. **Real-project corpus in CI** (Symfony, Laravel, popular packages):
   diagnostic regression detection, the anti-false-positive policy, and
   continuous parser fuzzing (never panics; the tree is always lossless:
   `text(tree) == source`).
5. **Benchmarks tracked in CI** (criterion) with regression thresholds —
   performance is a feature and is tested like one.

## 10. Repository standards and open-source hygiene

The repository itself is a product surface: for a project claiming to
replace the ecosystem, repo quality is a credibility signal. Delivered with
sub-project 1 (foundations) and maintained from the first commit:

- **README.md** — the pitch (what, why, differentiators), an honest project
  status ("early development, not yet usable"), the roadmap by pillar, and a
  quick start once one exists. The README is the landing page: written with
  the same care as the diagnostics.
- **Licensing files** — `LICENSE-MIT` and `LICENSE-APACHE` (dual-license
  convention of the Rust ecosystem), copyright JDevelop.
- **CONTRIBUTING.md** — development setup, the TDD loop as the expected
  workflow, commit conventions (gitmoji + Conventional Commits), how to add
  a rule or a diagnostic, the anti-false-positive policy as a contribution
  rule.
- **CODE_OF_CONDUCT.md** — Contributor Covenant.
- **SECURITY.md** — private vulnerability reporting channel. Non-negotiable
  for a tool that will ship security analysis itself.
- **Issue and PR templates** (`.github/`) — bug report, rule proposal,
  false-positive report (first-class template, per the policy), and the
  pre-filled internal-error template that the CLI's crash reporter links to.
- **CHANGELOG.md** — Keep a Changelog format, SemVer releases.
- **Toolchain pinning and workspace hygiene** — `rust-toolchain.toml`,
  `rustfmt.toml`, workspace-level Clippy configuration carrying the
  zero-panic lint policy (section 8), and `cargo-deny` (license and security
  advisory auditing of dependencies — a tool preaching safety audits its own
  supply chain).
- **CI from day 1** (GitHub Actions) — tests, Clippy at deny level,
  formatting check, cargo-deny; the real-project corpus and benchmark
  tracking join as soon as they exist (section 9).
- **CLAUDE.md** — project-level instructions for AI-assisted development:
  the engineering rules of this spec (zero panic, strict layering, TDD), the
  crate map, and pointers to the specs. Development is heavily AI-assisted;
  the repository must carry its own operating manual.

## 11. Sub-project sequencing

Each sub-project gets its own spec → plan → implementation cycle:

1. **Foundations** — Cargo workspace; error-resilient PHP 8.1+ lexer and
   parser producing the lossless syntax tree; span and source-file
   infrastructure.
2. **Semantic core** — the salsa query database; project discovery (Composer
   autoload); symbol indexing; name resolution; compiled stubs; the
   persistent artifact cache (pulled forward in time from the CLI
   sub-project, not in layering, see the section 3 clarification: the
   flagship incremental number cannot be measured without it); incremental
   by construction. Also carries an assumed debt from Foundations: the
   extraction of the shared diagnostic data model into
   `celerrate_diagnostics` (section 3) was deliberately deferred — with
   `celerrate_syntax` as sole producer the crate would have been an empty
   shell — and must be scheduled here, where the second diagnostic
   producer (unknown-symbol and version-gating checks) appears.

   **Public milestone: a `v0.0.x` preview ships at the end of this
   sub-project** — unknown-symbol and version-gating checks, watch mode, and
   a published incremental benchmark on the Symfony corpus. It proves the
   differentiator (interprocedural + incremental) against real feedback
   before the riskiest sub-project begins, while the competitive window is
   open.
3. **Type engine** — inference; the Celerrate type norm as an internal draft
   (no public freeze; section 4); the native plugin API with the
   PHPStan/Psalm bridge as first plugin (enabled by default); the stdlib
   type provider. **Stub curation** (the Celerrate refinements overlay, the
   functionMap equivalent) is a named workstream inside this sub-project
   with a "good enough for the corpus" exit criterion, not completeness —
   this is where PHPStan spent years, and treating it as a bullet point is
   how plans stall.
4. **Diagnostics and fixes** — rule framework; the structured-edit library;
   autofix engine; rustc-style rendering; `celerrate explain`.
5. **CLI product v0.1** — `celerrate check` with the `correctness` group:
   configuration, baseline, `migrate --from-phpstan`, output formats, the
   public release (the disk cache was built in sub-project 2; its
   placement stays at the composition root, section 3).
6. **Framework providers** — the first post-v0.1 sub-project: dynamic type
   providers for Eloquent (magic members, builder chains), Laravel facades,
   and the Symfony container. Laravel joins the measured corpus and the
   public claims when this ships (section 1).
7. **Later** — WASM plugin host; the declarative plugin tier as a public
   surface; the Celerrate norm freeze and `migrate --to-celerrate-types`;
   `style` (lint), `security` (taint), and `architecture` rule groups;
   `format`; LSP; `migrate`; `generate`; `daemon` mode.

Out of scope for v0.1: daemon/LSP, WASM host, taint analysis, lint group,
formatter, multi-version analysis, the Celerrate norm as a public surface,
Laravel in the measured corpus.
