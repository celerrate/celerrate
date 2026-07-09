# Celerrate — Vision and Engine Architecture Design

Date: 2026-07-09
Status: Approved (brainstorming output; each sub-project gets its own detailed spec)

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
  seamless; the Celerrate norm is the promoted path, the bridge is a
  migration ramp.
- A future `celerrate migrate --to-celerrate-types` autofix converts existing
  PHPDoc annotations to the Celerrate norm.

### Success criterion for v0.1

v0.1 = `celerrate check` can analyze a real Symfony/Laravel codebase end to
end with a restricted but reliable set of diagnostics (nullability, argument
types, unknown symbols), no visible false positives, with speed as the proof.
Depth comes later.

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

### Crate layout (Cargo workspace, strict layering)

```
celerrate_source      source files, spans, positions
celerrate_syntax      lexer + parser → lossless syntax tree
celerrate_db          the salsa query database (the "engine")
celerrate_semantics   project discovery (Composer), symbol index, name resolution
celerrate_types       inference and type system
celerrate_rules       rule framework + diagnostics + structured edit engine
celerrate_plugin      plugin API (native traits first, WASM host later)
celerrate_cli         the binary: config, orchestration, rendering, disk cache
```

Each layer depends only on layers below it.

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
- Compiled **at build time** into a compact binary format embedded in the
  `celerrate` binary: pre-parsed, pre-indexed, carrying per-version
  availability metadata (added in 8.2, signature changed, deprecated...).
  Zero startup cost; the stdlib index is a salsa input like any other.
- **Overlay system**, increasing priority: base stubs → Celerrate refinements
  (enriched signatures written in the Celerrate norm, the equivalent of
  PHPStan's functionMap) → plugin-contributed stubs (required by framework
  plugins).
- The target version range filters stub visibility.

### Persistent cache

The salsa state is serialized to `.celerrate/cache/` between CLI runs. First
run: full parallel analysis. Subsequent runs: only queries invalidated by
changed files are recomputed — including in CI when the cache is restored
between jobs. Corrupted caches are detected (versioning + checksums) and
silently regenerated, never fatal.

## 4. Type engine

Three components:

1. **Inference.** Infers everything not annotated: local variables by
   propagation, function returns from bodies, control-flow narrowing (after
   `if ($x instanceof User)`, `$x` is `User`; `assert()`, `match`,
   comparisons). Interprocedural: the type of a call is computed from the
   callee's body when unannotated — a memoized salsa query, paid once. The
   type system covers unions (`User|null`), literal types (`'active'`, `42`),
   array shapes (`array{id: int, name: string}`), and generics on classes and
   functions.
2. **The Celerrate norm.** Our own annotation syntax, designed after twenty
   years of PHPDoc lessons — living in docblocks (full runtime compatibility)
   but with a clean, strict, formally specified, versioned grammar. The exact
   syntax is designed in the type-engine sub-project's own spec; this
   document fixes the principle: one official norm, formally specified.
3. **The compat bridge** (first-party plugin, enabled by default) translates
   PHPStan/Psalm annotations into internal types. Internal engine types are
   **the only currency**: plugins produce internal types, never their own
   representation.

## 5. Extensibility

Model: **hybrid Rust native + WASM.**

- First-party plugins (PHPStan/Psalm bridge, framework rules for
  Laravel/Symfony) are Rust crates compiled into the binary: maximum
  performance on the hot path.
- Community plugins go through a WASM API (writable in Rust, Go,
  AssemblyScript, and potentially compiled PHP later). WASM plugins are
  sandboxed: a plugin that crashes or hangs is isolated, killed cleanly, and
  reported — never fatal to the analysis. The WASM host is out of scope for
  v0.1; the native plugin API (traits) ships first and the same extension
  points back both.

Extension points span **every layer**, as declared interfaces:

- Type syntaxes (understand other annotation notations — the compat bridge).
- Dynamic type providers (`Container::make($class)` returns an instance of
  `$class`; Eloquent magic members).
- Virtual symbols and stubs (symbols absent from sources: PHP extensions,
  framework magic).
- Project discovery (non-standard autoload, DI container bootstrap).
- Rules and diagnostics with their autofixes — same API for core and plugins.
- Later: migration/codegen recipes, architecture rules, LSP actions.

Contract: extension points are **declared, deterministic functions of their
inputs** — plugin contributions enter salsa's dependency graph, so imperative
hooks mutating internal state would break cache correctness. Declared APIs
also stay stable while internals evolve, which is the same contract the WASM
API imposes anyway.

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
   opinion. The corpus runs in Celerrate's own CI.

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
adoption on existing codebases.

### Outputs

Rich human rendering by default (colors, annotated spans); `--output=json`
(stable, versioned schema for tooling); GitHub Actions (native PR
annotations); SARIF (GitLab, IDEs, security platforms).

### Distribution

Single static binary per platform (Linux x64/arm64, macOS x64/arm64,
Windows). Channels: install script, Homebrew, a bootstrap Composer package
that downloads the binary (the bridge to PHP habits, as Biome does via npm),
official Docker image, GitHub Action.

### Published performance targets

Held in CI by benchmarks: at least ~20x faster than PHPStan on a cold full
analysis, and sub-second incremental updates on single-file changes in a
Symfony-sized project.

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
- WASM plugins are sandboxed (crash/hang isolation), as specified in the
  extensibility section.
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
   harness replays edit sequences over the corpus. Most of a salsa engine's
   subtle bugs live here.
4. **Real-project corpus in CI** (Symfony, Laravel, popular packages):
   diagnostic regression detection, the anti-false-positive policy, and
   continuous parser fuzzing (never panics; the tree is always lossless:
   `text(tree) == source`).
5. **Benchmarks tracked in CI** (criterion) with regression thresholds —
   performance is a feature and is tested like one.

## 10. Sub-project sequencing

Each sub-project gets its own spec → plan → implementation cycle:

1. **Foundations** — Cargo workspace; error-resilient PHP 8.1+ lexer and
   parser producing the lossless syntax tree; span and source-file
   infrastructure.
2. **Semantic core** — the salsa query database; project discovery (Composer
   autoload); symbol indexing; name resolution; compiled stubs; incremental
   by construction.
3. **Type engine** — inference; the Celerrate type norm (own syntax spec);
   the native plugin API with the PHPStan/Psalm bridge as first plugin
   (enabled by default).
4. **Diagnostics and fixes** — rule framework; the structured-edit library;
   autofix engine; rustc-style rendering; `celerrate explain`.
5. **CLI product v0.1** — `celerrate check` with the `correctness` group:
   configuration, persistent disk cache, baseline, output formats, the
   public release.
6. **Later** — WASM plugin host; `style` (lint), `security` (taint), and
   `architecture` rule groups; `format`; LSP; `migrate`; `generate`;
   `daemon` mode.

Out of scope for v0.1: daemon/LSP, WASM host, taint analysis, lint group,
formatter, multi-version analysis.
