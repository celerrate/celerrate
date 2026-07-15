# Celerrate

[![CI](https://github.com/celerrate/celerrate/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/celerrate/celerrate/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/celerrate/celerrate)](https://github.com/celerrate/celerrate/releases/latest)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-555)](https://github.com/celerrate/celerrate/releases/latest)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**An extremely fast, all-in-one toolchain for PHP, written in Rust.**

Celerrate analyzes 1.3 million lines of PHP in 1.11 seconds cold, and
in 0.29 seconds after you edit one file. Measured end to end, protocol
committed to the repository.

> **Early preview (v0.0.2).** The engine is real; the rule surface is
> deliberately small, and growing. Today `celerrate check` resolves
> every symbol and gates PHP versions, with zero configuration and
> without ever crashing on any input. Type inference is next.

## Quick start

Download the archive for your platform from the
[latest release](https://github.com/celerrate/celerrate/releases/latest),
unpack it, and run the binary inside a Composer project:

```sh
celerrate check .
```

There is nothing to configure. Composer discovery finds your code, your
dependencies, and your PHP version range on its own:

```text
src/Service/Search.php:27:16 CEL0021 `array_find` requires PHP 8.4, but the project's minimum PHP version is 8.1
src/Controller/PostController.php:42:19 CEL0018 unknown class `App\Service\Mailer`

0 notices, 2 diagnostics
```

## Performance

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/benchmark-dark.svg">
  <img src="assets/benchmark-light.svg" width="720" alt="Bar chart of median wall clock on symfony/demo: cold full analysis 1.11 seconds, warm with one file edited 0.29 seconds">
</picture>

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
terminal report. Those are the next sub-projects, in the
[roadmap](#roadmap)'s order.

## One engine, a whole toolchain

Every Celerrate command is a view over the same incremental semantic
model. Index a project once; everything else is a query:

- **`celerrate check`**: static analysis with interprocedural type
  inference, plus lint, security taint, and architecture rule groups.
  One command answers "is my code OK?".
- **`celerrate format`**: an opinionated, lossless formatter.
- **`celerrate lsp`**: a language server with the same diagnostics as
  CI, at typing speed.
- **`celerrate migrate` / `celerrate generate`**: automated refactoring
  and semantic code generation.

Speed stays a feature throughout: a Rust core, parallel by default,
incremental by construction. Diagnostics are meant to teach: annotated
spans, the engine's reasoning, concrete suggestions, and safe automatic
fixes. Extensibility is designed in: first-party plugins in Rust,
community plugins through a sandboxed WASM API.

## Built to be trusted

The engineering rules behind the numbers, enforced mechanically in CI:

- **Zero panic**: clippy denies `unwrap`, `expect`, indexing, and
  `panic` across the workspace; `unsafe` code is forbidden.
- **No input can crash it**: parsers and loaders produce diagnostics,
  never failures. Fuzzing keeps them honest.
- **Deterministic**: every analysis result is a pure function of its
  inputs. Same input, same output, on any machine.
- **Incremental by construction**: invalidation is computed, not
  guessed; warm runs reuse everything that did not change.
- **Measured, not claimed**: performance numbers come from a committed,
  reproducible benchmark protocol.
- **Test-driven**: no production code without a test that demanded it.

## Compatibility

Celerrate targets PHP 8.1+ projects. It defines its own type annotation
norm and ships a first-party PHPDoc syntax bridge, enabled by default,
so existing annotated codebases work on day 1.

## Roadmap

One pillar at a time, in this order:

1. **`celerrate check`**: the static analysis engine is the first
   public deliverable (previewed since v0.0.1); type inference and the
   lint, taint, and architecture rule groups build on it.
2. **`celerrate format`**: the formatter, once the lossless syntax tree
   is proven by the analyzer.
3. **`celerrate lsp`**: the language server, reusing the same
   incremental engine.
4. **`celerrate migrate` / `celerrate generate`**: refactoring and code
   generation, last because they lean on everything above.

## Contributing

Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md). The
engineering rules above are enforced by CI, not by review comments.

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
