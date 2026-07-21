# Changelog

All notable changes to Celerrate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Project discovery distinguishes a missing Composer manifest from an
  unreadable one. A `composer.json` or `vendor/composer/installed.json`
  that exists but cannot be read (permission denied, a directory in its
  place, any IO error other than not-found) now reports a dedicated
  notice naming the error — `CEL0039` for the manifest, `CEL0040` for
  the installed packages — instead of being silently treated as absent.
  (#59)
- The structured-edit library `celerrate_edit`: structured operations on
  the syntax tree — kind-checked token replacement and line-comment
  insertion that reproduces the surrounding indentation — compile into
  the deterministic, sorted, conflict-free `TextEdit` sets suggestions
  transport, and an application primitive splices such a set into
  source text. Overlapping edits are reported errors, never silent
  resolutions, and an edit never touches trivia it was not aimed at. An
  edit-application fuzz target joins the fuzz suite. Library groundwork
  only: no rule emits edits yet and no output changes.

### Changed

- The shared diagnostic model carries the rich anatomy the
  diagnostics-and-fixes design specifies: an anchor that admits
  project-level findings, labeled secondary spans (concrete in the same
  file, symbolic across files), notes, and structured suggestions with
  confidence and finalized text edits. The persistent-cache verdict
  schema moves to 6 and bounds-checks every stored range on load. Model
  groundwork only: no producer emits anatomy yet and no output changes.
- The plugin API boundary is sealed (issue #61): `DynamicTypeProvider`
  receives a call-scoped `InvocationSite` instead of a raw salsa
  database handle, `AnnotationSite` no longer exposes `database()`,
  and both sites hand out a sealed `TypeContext` facade for type
  construction and interrogation. `celerrate_plugin` now re-exports
  boundary vocabulary nominally — no more `salsa` or whole-crate
  re-exports — and the boundary structs are `#[non_exhaustive]`.
  Breaking for the v0 plugin API; `PLUGIN_API_VERSION` stays 0.
- The comment-directive vocabulary is sealed (issue #66), the same
  treatment PR #65 gave the plugin boundary: `CommentDirective`,
  `CommentKind`, and `DirectiveScope` are now `#[non_exhaustive]`, and
  the `CommentDirective::Suppress` variant closes cross-crate literal
  construction of its fields. Cross-crate callers build a `Suppress`
  through the new `CommentDirective::suppress(scope, identifiers)`
  constructor; the bundled `phpdoc-bridge` plugin has migrated to it.
  Breaking for the v0 plugin API; `PLUGIN_API_VERSION` stays 0.
- Internal hardening (issue #67): the plugin API boundary seal (issue
  #61) now carries a compile-fail proof — five pinned `trybuild` cases
  showing that `AnnotationSite::new`, `InvocationSite::new`, and
  `TypeContext::new` cannot be called cross-crate, that
  `AnnotationSite::database()` is not reachable from the facade, and that
  `celerrate_plugin` re-exports no `salsa` item. The `xtask dependency-shape` check now
  derives its governed plugin-crate set from `cargo metadata` instead of
  a hardcoded list: any workspace package with a non-dev dependency on
  the `celerrate_plugin` facade is governed automatically, so a new
  facade dependent is caught the moment it appears, with composition
  roots excluded by an explicit allowlist. Pure test and tooling
  hardening, no production code changed; zero delta on the Symfony
  corpus and mixed-typedness baseline.
- Internal hardening (issue #63): the inference warming precondition
  is now compile-checked instead of documented. The unguarded tracked
  query behind body-type inference moved into a private `sealed`
  module and now demands a `Warmed` proof token, minted only by
  `warm_the_cycle_safe_entry_point` or, inside the two cycle-safe
  return queries, `Warmed::from_inside_the_fixpoint`. No caller
  outside `celerrate_types` can reach the query at all, and inside it
  demanding without warming is now a deliberate, greppable act rather
  than a silent omission. Pure refactor, no behavioral change; zero
  delta on the Symfony corpus and mixed-typedness baseline.
- `--watch` now shuts down gracefully on Ctrl+C and `SIGTERM` (issue
  #52): the interrupt flushes the analysis cache through the same
  quiet-cycle persist a clean exit uses, so the next run starts warm
  instead of paying for a cold rebuild. In-flight work finishes first;
  a burst not yet analyzed is dropped. A second Ctrl+C exits
  immediately (code 130), preserving the escape hatch during the final
  persist or a long cold cycle. Delivered through `ctrlc`'s
  `termination` feature, which encapsulates the platform signal
  handling the workspace's `unsafe`-forbidding lints otherwise disallow.
- The norm type-grammar parser now rejects forms outside its documented
  v0 subset (issue #48): bare collection generics (`array`, `list`,
  `iterable`, `non-empty-array`, `non-empty-list`), bare `callable`, the
  empty shape `{}`, quoted shape keys, hyphenated class names, and
  stacked nullability (`??int`) answer "no type" instead of silently
  parsing to an undocumented over-approximation. The three documented
  conveniences (`array-key`, the single-argument `array<V>` /
  `iterable<V>` sugars, single `?T`) are unchanged and pinned. No shipped
  stub refinement used a rejected form; zero corpus delta.
- Internal hardening (issue #62): a cross-provenance oracle now pins that
  the two type grammars behind the `TypeSyntax` seam — the norm (stub
  refinements) and the phpdoc bridge (docblocks) — lower every shared
  spelling to the identical interned type, and that each deliberate
  dialect gap is rejected on the expected side. One divergence is
  documented rather than silent: the norm lowers an enum-case reference
  (`Status::Active`) to its enum-case type, while the bridge keeps `mixed`
  because it has no symbol table to confirm enum-ness at lowering time
  (lowering a non-enum `Foo::BAR` would fabricate unknown-member reports).
  Tracked for unification in #86. Test-only; zero corpus delta.
- The rule framework skeleton: rules are declarative families with a
  name, a group, a closed identifier list carrying per-identifier
  default severities, and a `Default`/`Nursery` tier. Four phase
  traits (syntax, semantic, typed-body, and the core-only reporting
  phase) check through sealed contexts and report into a
  metadata-severitied finding sink; a fifth extension-point registry
  holds registrations, with core rules registered under a reserved
  core identity that never keys the plugin-set digest. The
  syntax-version-gating family (`CEL0024`) is the first migrated
  family: the gated-construct walk stays in `celerrate_semantics` as
  an outcome query, the rule constructs the diagnostics, and the
  identifier's ownership moves to `celerrate_rules`. Internal
  machinery only: reported diagnostics, exit codes, the corpus
  snapshot, and the cache format are all byte-identical.
- Every check family now rides the rule framework: the semantic
  families (unknown symbols CEL0018-CEL0020, symbol version gating
  CEL0021-CEL0023) and the typed families (unknown members
  CEL0030-CEL0033, null dereference CEL0034, argument checks
  CEL0035-CEL0038) migrated from their domain crates into
  `celerrate_rules`, joining syntax version gating (CEL0024). The
  walks, outcome computation, and cache revalidation records stay in
  `celerrate_semantics` and `celerrate_types`; the rules consume
  outcomes through the sealed contexts and construct the identical
  diagnostics. No user-visible behavior changes: messages,
  severities, spans, exit codes, and the served diagnostic output are
  preserved exactly, gated by the pinned corpus snapshot and a
  seeded-defect recall fixture per identifier (now covering
  CEL0018-CEL0024 and CEL0030-CEL0038, with CEL0022 pinned by a
  rule-level fixture over a synthetic stub rather than a product one,
  since the shipped stub blob carries no symbol removed inside the
  supported 8.1 to 8.5 window). The persisted verdict's element order
  shifted, the semantic families now arriving through the phase query
  rather than ahead of syntax, with no observable effect: every read
  path sorts. An emission-side scan (`cargo xtask emission-scan`)
  joins CI so a check family cannot quietly grow back outside the
  framework.

### Fixed

- The plugin-set cache key now describes the post-admission plugin set,
  instead of the raw registration descriptor list (#60): `plugin_set_digest`
  takes the `RegisteredPlugins` record that `register_plugins` itself
  produced, so there is no second descriptor list that could fall out of
  sync, and no risk of keying the cache on a plugin that a claim conflict
  later excluded. The admitted identities' `(name, version, configuration)`
  triples and the excluded plugins' names are sorted, then length-prefixed
  straight into the blake3 hasher, with no postcard encoding step, so the
  previous fallible encode arm (which collapsed on failure to a constant
  `[0u8; 32]`, silently discarding nothing wrongly but never varying
  either) is gone entirely. The digest value changes as a result,
  discarding existing local caches once; the Symfony corpus snapshot and
  mixed-rate baseline do not embed the digest and show no delta.
- `StubRefinements::new` now dedups its sorted `functions`, `classes`,
  and each class's `methods` by key, first entry wins (#47) — matching
  `StubIndex::new`'s precedent, so a duplicate key can no longer reach
  the compiled blob as two identically-keyed rows with an arbitrary
  binary-search winner. Unreachable on any shipped path: the sole
  production producer, `parse_refinement_source`, already rejects
  duplicate keys outright. Defense in depth for a programmatic caller.
- `foreach` list-destructuring binds are now value-change sites
  (#75): the value pattern routes through the assignment machinery,
  so every destructured target kills its stale call-result
  fingerprints and is bound to its element type — short and
  `list()` syntaxes, keyed and nested patterns, and index targets
  alike. Each missed kill could only silence a genuine
  possibly-null dereference (CEL0034), never fabricate one;
  destructured loop variables previously kept their stale pre-loop
  types.
- Call-result narrowing hardening (#72): stale call-result
  fingerprints now die at every value-change site — `foreach`
  key/value rebinds, `catch` binds, by-reference closure captures
  (`use (&$x)`), `extract()` (which now sweeps every fingerprint
  naming a local, sparing pure `$this`-based ones), and `++`/`--`.
  Each missed kill could only silence a genuine possibly-null
  dereference; none could fabricate one. The purity assumption's
  documented guarantee is also corrected: "can only silence, never
  fabricate" holds for positive guards — under a negative guard the
  surviving `null` binding makes the lazy-initialization idiom
  report (PHPStan parity, now pinned by a test).
- Possibly-null dereference (`CEL0034`) now reports on unguarded
  call-result receivers (`$repo->find($id)->title` with no guard),
  instead of silencing every call-result receiver. The narrowing
  floor tracks call-result fingerprints — `$base->method(stable
  arguments)` on a `$this` or local base — so the guards PHP
  routinely writes (`if ($e->getCommand() &&
  $e->getCommand()->getName())`, every condition form) narrow them,
  and the silence shrinks to the genuinely untrackable shapes
  (property-rooted receivers, unstable arguments). Two occurrences of
  one fingerprint are assumed to denote the same value — documented
  engine semantics whose unsoundness can only silence, never
  fabricate a report. Verified on the Symfony corpus (no new
  diagnostics). (#54)
- Too-few-arguments (`CEL0036`) now reports on builtin callees, not only
  on source functions. The stub compiler honours phpstorm-stubs'
  `@param ... $name [optional]` docblock marker, which flags a parameter
  that is optional without a PHP-expressible default (`mt_rand(int $min,
  int $max)` is callable as `mt_rand()`). Stub arity is therefore
  trustworthy, so `str_repeat("x")` — whose `$times` is genuinely
  required — reports again, while a call that omits only `[optional]`
  parameters stays silent. Verified on the Symfony corpus (no new
  diagnostics) and end to end against the recompiled stubs. (#53)
- The written-type parser (`celerrate_types`) no longer overflows the
  stack on deeply nested input. Its recursion is now bounded by a depth
  cap mirroring the norm parser's, so hostile type text (`((((…` or a
  long `?` chain), which derives from user-supplied source, answers with
  no type instead of crashing the tool. (#46)
- `declared_member_signature` no longer re-executes on docblock edits
  elsewhere in the owner's file: the declaring-site helpers
  (`declaring_site`, `owner_class_docblock`, `declares_member`) are now
  tracked queries keyed per class-like, so a docblock edit invalidates
  only the members it can actually affect. (#37)

## [0.0.3] - 2026-07-18

The type-engine preview: the incremental engine is now type-aware.
Interprocedural type inference, docblock annotations through the
bundled PHPDoc bridge, three new diagnostic families measured on the
Symfony corpus with no visible false positive, and the incremental
numbers re-published with inference active.

### Added

- The unknown-member diagnostic family (`CEL0030` to `CEL0033`):
  methods, properties, class constants, and enum cases that do not
  exist on the receiver's resolved type. Conservative by design: a
  `mixed` or dynamic receiver is silent, magic members and
  `#[AllowDynamicProperties]` suppress their own kind, and
  `@property`/`@method` docblock members count as existing.
- The nullability diagnostic family (`CEL0034`): method calls and
  property accesses on a possibly-null value, with flow narrowing
  (`instanceof`, null comparisons, `isset()`, `??`, `?->` chains,
  `match`, early returns, assertion annotations) deciding what is
  still nullable at each use site.
- The argument-type diagnostic family (`CEL0035` to `CEL0038`):
  per-argument assignability and arity, named arguments included.
  Coercion follows the calling file's `declare(strict_types)` mode;
  `mixed` passes everywhere.
- Interprocedural type inference: declared types (native
  declarations, per-PHP-version stub signatures, docblock
  annotations) are trusted; unannotated returns are inferred from
  bodies, through mutual recursion; generics are resolved and
  propagated for precision but never reported on.
- The `phpdoc-bridge` plugin, enabled by default: standard PHPDoc,
  the PHPStan dialect, and Psalm synonyms, with coverage, precedence,
  and every table published in
  [docs/phpdoc-bridge.md](https://github.com/celerrate/celerrate/blob/v0.0.3/docs/phpdoc-bridge.md).
- Inline suppressions, honored across all diagnostic families:
  `@phpstan-ignore-line`, `@phpstan-ignore-next-line`,
  `@phpstan-ignore`, and `@psalm-suppress`.
- The stdlib type provider: computation-dependent signatures
  (`array_map` from its callable, `json_decode` from its flags,
  `preg_match` matches shapes, and more) that no declarative stub
  can express.
- The persistent cache extends to typed artifacts: inferred
  signatures are persisted and revalidated recursively, and warm
  one-edit stays sub-second with inference active (median 0.460 s, with
  the flagship warm body-edit at 0.521 s), measured by the committed
  protocol
  ([benchmarks/PROTOCOL.md](https://github.com/celerrate/celerrate/blob/v0.0.3/benchmarks/PROTOCOL.md)).
- The identifier reference:
  [docs/diagnostics.md](https://github.com/celerrate/celerrate/blob/v0.0.3/docs/diagnostics.md)
  documents every `CEL####` identifier.

### Changed

- The benchmark protocol's scenario set grew from three scenarios to
  five (warm body-edit and warm signature-edit join), and the
  flagship number is now the warm body edit: cold full 1.533 s, warm
  body-edit 0.521 s, warm signature-edit 0.471 s on symfony/demo (9447
  PHP files, 1.3 million lines, vendor tree included).
- Substance, measured: 25.0 % of the corpus's expressions still
  analyze as `mixed` (the residual the stub curation and provider
  workstreams drive down); the number and its protocol are published
  so precision claims can be weighed against it.

## [0.0.2] - 2026-07-13

### Fixed

- `celerrate check` with a relative root (including `celerrate
  check .`) and no `composer.json` analyzed nothing and exited 0 under
  the CEL0025 notice: the zero-configuration fallback self-joined the
  relative root, so the walk searched a directory that does not exist.
  The command line now makes the root absolute before discovery, and a
  relative spelling produces exactly the report its absolute spelling
  produces.

## [0.0.1] - 2026-07-13

The first public preview: proof that interprocedural analysis plus
fine-grained incremental computation hold up on real projects. Two
diagnostic families, watch mode, a persistent cache, and a published,
reproducible incremental number.

### Added

- `celerrate check <path>`: zero-configuration analysis of a PHP
  project. Composer discovery derives the analyzed roots, the
  project/vendor split, and the PHP version range; installed
  dependencies are indexed but never reported on. A complete PHP 8.5
  parser with error resilience feeds the analysis: no input crashes
  the tool.
- The unknown-symbol diagnostic family (`CEL0018`, `CEL0019`,
  `CEL0020`): references to classes, functions, and constants that
  resolve nowhere, with the project, its dependencies, and the bundled
  PHP stubs all considered.
- The version-gating diagnostic family (`CEL0021` through `CEL0024`):
  symbols and syntax used outside the project's declared PHP version
  range, including removals and deprecations.
- `celerrate check --watch`: re-analysis on every change, incremental
  by construction.
- The persistent artifact cache under `.celerrate/cache/` (the
  directory writes its own `.gitignore`): warm runs reuse parsed and
  analyzed artifacts across processes, revalidated against fresh
  inputs; every corruption mode answers with silent regeneration.
- The published incremental number, measured by the committed
  protocol ([benchmarks/PROTOCOL.md](https://github.com/celerrate/celerrate/blob/v0.0.1/benchmarks/PROTOCOL.md)) on
  symfony/demo (9447 PHP files, 1.3 million lines, vendor tree
  included): warm one-edit median 0.29 s, wall clock, process startup
  and cache loading included.
- Pre-built binaries for Linux x64 and arm64 (static musl builds),
  macOS x64 and arm64, and Windows x64.

[Unreleased]: https://github.com/celerrate/celerrate/compare/v0.0.3...HEAD
[0.0.3]: https://github.com/celerrate/celerrate/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/celerrate/celerrate/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/celerrate/celerrate/releases/tag/v0.0.1
