# Changelog

All notable changes to Celerrate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cargo workspace with the zero-panic lint policy.
- `celerrate_source`: source text primitives (spans, line/column index,
  file identifiers, byte decoding with BOM and invalid-UTF-8 provenance).
- `celerrate_syntax`: complete PHP 8.1+ lexer (lossless token stream,
  string interpolation, structured diagnostics), snapshot corpus, and a
  continuous fuzz target.
- The parser covers the full PHP 8.5 grammar (except `__halt_compiler`,
  recorded out of scope for Foundations): the complete expression
  grammar (Zend precedence table, calls and access chains, `match`,
  closures, the pipe operator), the complete statement grammar (control
  flow in classic and alternative syntax, `try`/`catch`/`finally`,
  inline HTML interruption), and the complete declaration grammar
  (classes with anonymous forms, interfaces, traits, enums, property
  hooks and asymmetric visibility, constructor promotion, union /
  intersection / DNF types, attributes, `const`/`namespace`/`use`).
- `celerrate_syntax`: a typed AST layer generated from `php.ungram` by
  the new dev-only `xtask` workspace member: the `SyntaxKind` node
  kinds and the typed node structs (`Option`/iterator accessors
  everywhere, so partial trees from error recovery are normal
  citizens), plus hand-written accessors for semi-reserved names and
  position-dependent roles. A sourcegen test keeps the committed
  generated code fresh. This closes the Foundations sub-project.
- `celerrate_diagnostics`: the shared diagnostic data model (stable
  `CEL####` identifiers, severity, primary span); lexer and parser
  diagnostics project into it.
- `celerrate_vfs`: the virtual file system (interned file identifiers,
  disk state, in-memory overlays, change draining).
- `celerrate_vfs`: lexical path normalization and the deterministic
  disk walk (PHP files under declared roots, explicit file entries,
  symbolic-link cycle protection, disk loading into the file state).
- `celerrate_project`: zero-configuration Composer discovery — tolerant
  `composer.json` and `installed.json` readers, autoload rules (PSR-4,
  PSR-0, classmap, files) deriving the walk roots and the
  project/vendor classification, the PHP version range with the
  parent-spec detection precedence at minor precision, and the
  structured discovery notices (`CEL0025` through `CEL0029`).
- `celerrate_stubs`: the pinned phpstorm-stubs snapshot compiled by
  `cargo xtask compile-stubs` into a committed, versioned binary blob
  (top-level symbols with per-version availability metadata), embedded
  in the binary and exposed as a high-durability salsa input with a
  version-range-filtered view; the project configuration becomes a
  salsa input in `celerrate_project`.
- `celerrate_db`: the salsa base layer (`SourceFile` input;
  `source_text`, `parse`, `line_index`, and `file_diagnostics` as
  incremental queries), with the invalidation-scope tests and the
  incremental-consistency harness skeleton. This opens the semantic
  core sub-project.

[Unreleased]: https://github.com/celerrate/celerrate/commits/main
