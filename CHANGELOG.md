# Changelog

All notable changes to Celerrate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/celerrate/celerrate/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/celerrate/celerrate/releases/tag/v0.0.1
