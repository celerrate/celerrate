# Changelog

All notable changes to Celerrate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

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
