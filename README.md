# Celerrate

[![CI](https://github.com/celerrate/celerrate/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/celerrate/celerrate/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

An extremely fast, all-in-one toolchain for PHP, written in Rust.

> **Status: early preview (v0.0.1).** The engine is real and fast; the
> rule surface is deliberately small. `celerrate check` analyzes a
> Composer project with zero configuration, incrementally, and it will
> not crash on any input. It does not do type inference yet.

## What works today

`celerrate check .` reports two diagnostic families:

- **Unknown symbols**: references to classes, functions, or constants
  that resolve nowhere, with your project, your Composer dependencies,
  and the bundled PHP stubs all considered.
- **PHP version gating**: symbols or syntax used outside the PHP
  version range your `composer.json` declares, including removals and
  deprecations.

Around them:

- **Zero configuration**: Composer discovery derives what to analyze
  and which PHP versions to check against. Installed dependencies are
  indexed but never reported on.
- **`--watch`**: re-analysis on every change.
- **A persistent cache** (`.celerrate/cache/`, self-ignoring): warm
  runs reuse everything that did not change, across processes.

### What it does not do yet

No type inference, no lint rules, no formatter, no language server, no
configuration file, no baseline, and no output formats beyond the
terminal report. Those are the next sub-projects, in the roadmap's
order below.

## Quick start

Download the archive for your platform from the
[latest release](https://github.com/celerrate/celerrate/releases/latest),
unpack it, and run the binary inside a Composer project:

```sh
celerrate check .
```

There is nothing to configure.

## Performance

Measured by the committed [benchmark protocol](benchmarks/PROTOCOL.md)
on symfony/demo (9447 PHP files, 1.3 million lines, vendor tree
included), on the hardware the protocol names:

| Scenario | Median wall clock |
| --- | --- |
| Cold full analysis | 1.11 s |
| Warm, one file edited | **0.29 s** |

Both numbers are full CLI runs: process startup, cache loading,
analysis, and reporting. No comparison against other tools is
published at this scope; the protocol states why.

## What is Celerrate?

Celerrate aims to replace the fragmented PHP tooling ecosystem with a
single coherent toolchain built on one engine:

- **`celerrate check`** — static analysis with interprocedural type
  inference and fine-grained incremental computation, plus lint, taint
  (security), and architecture rule groups. One command answers "is my
  code OK?".
- **`celerrate format`** — an opinionated, lossless formatter.
- **`celerrate lsp`** — a language server built on the same engine.
- **`celerrate migrate` / `celerrate generate`** — automated
  refactoring and semantic code generation.

## Why another tool?

- **Speed as a feature.** A Rust core, parallel by default,
  incremental by construction: full analyses in seconds, single-file
  updates in milliseconds.
- **Diagnostics that teach.** Rust-quality diagnostics: annotated
  spans, the engine's reasoning, concrete suggestions, and safe
  automatic fixes.
- **One engine, many tools.** Types, lint, security taint analysis,
  and architecture rules are rule groups over the same semantic model
  — not separate tools to install and configure.
- **Extensible by design.** First-party plugins in Rust, community
  plugins through a sandboxed WASM API, with extension points at every
  layer.

## Compatibility

Celerrate targets PHP 8.1+ projects. It defines its own type
annotation norm and ships a first-party PHPStan/Psalm syntax bridge,
enabled by default, so existing annotated codebases work on day 1.

## Roadmap

One pillar at a time, in this order:

1. **`celerrate check`** — the static analysis engine is the first
   public deliverable (previewed in v0.0.1); type inference and the
   lint, taint, and architecture rule groups build on it.
2. **`celerrate format`** — the formatter, once the lossless syntax
   tree is proven by the analyzer.
3. **`celerrate lsp`** — the language server, reusing the same
   incremental engine.
4. **`celerrate migrate` / `celerrate generate`** — refactoring and
   code generation, last because they lean on everything above.

## License

Dual-licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option. "Celerrate" is a trademark of JDevelop.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
