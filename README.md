# Celerrate

An extremely fast, all-in-one toolchain for PHP, written in Rust.

> **Status: early development.** Celerrate is not yet usable. The design is
> settled, the engine is being built. Watch the repository to follow along.

## What is Celerrate?

Celerrate aims to replace the fragmented PHP tooling ecosystem with a single
coherent toolchain built on one engine:

- **`celerrate check`** — static analysis with interprocedural type
  inference and fine-grained incremental computation, plus lint, taint
  (security), and architecture rule groups. One command answers "is my code
  OK?".
- **`celerrate format`** — an opinionated, lossless formatter.
- **`celerrate lsp`** — a language server built on the same engine.
- **`celerrate migrate` / `celerrate generate`** — automated refactoring and
  semantic code generation.

## Why another tool?

- **Speed as a feature.** A Rust core, parallel by default, incremental by
  construction: full analyses in seconds, single-file updates in
  milliseconds.
- **Diagnostics that teach.** Rust-quality diagnostics: annotated spans,
  the engine's reasoning, concrete suggestions, and safe automatic fixes.
- **One engine, many tools.** Types, lint, security taint analysis, and
  architecture rules are rule groups over the same semantic model — not
  separate tools to install and configure.
- **Extensible by design.** First-party plugins in Rust, community plugins
  through a sandboxed WASM API, with extension points at every layer.

## Compatibility

Celerrate targets PHP 8.1+ projects. It defines its own type annotation
norm and ships a first-party PHPStan/Psalm syntax bridge, enabled by
default, so existing annotated codebases work on day 1.

## Roadmap

One pillar at a time, in this order:

1. **`celerrate check`** — the static analysis engine is the first public
   deliverable; the lint, taint, and architecture rule groups build on it.
2. **`celerrate format`** — the formatter, once the lossless syntax tree
   is proven by the analyzer.
3. **`celerrate lsp`** — the language server, reusing the same
   incremental engine.
4. **`celerrate migrate` / `celerrate generate`** — refactoring and code
   generation, last because they lean on everything above.

## License

Dual-licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option. "Celerrate" is a trademark of JDevelop.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
