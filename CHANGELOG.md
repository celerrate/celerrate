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
- The parser covers the full PHP 8.5 expression grammar: the Zend
  precedence table, calls and access chains, arrays, string
  interpolation, `new`/`clone` (clone-with), intrinsics, `match`,
  closures and arrow functions, and the pipe operator.

[Unreleased]: https://github.com/celerrate/celerrate/commits/main
