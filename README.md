# Celerrate

[![CI](https://github.com/celerrate/celerrate/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/celerrate/celerrate/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/celerrate/celerrate)](https://github.com/celerrate/celerrate/releases/latest)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-555)](https://github.com/celerrate/celerrate/releases/latest)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**An extremely fast, all-in-one toolchain for PHP, written in Rust.**

Celerrate type-checks 1.3 million lines of PHP in 1.533 seconds
cold, and in 0.521 seconds after you edit a function body.
Measured end to end, protocol committed to the repository.

> **Early preview (v0.0.3).** The engine is now type-aware:
> interprocedural inference, your existing PHPDoc/PHPStan/Psalm
> annotations honored out of the box, and five diagnostic families,
> with zero configuration and without ever crashing on any input.
> The rule surface is still deliberately small, and growing.

## Installation

```sh
curl -fsSL https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh
```

Or, for Composer projects, from v0.1.0:

```sh
composer require --dev celerrate/celerrate
```

All channels, manual downloads, and checksum verification:
[docs/installation.md](docs/installation.md).

## Quick start

With `celerrate` installed, run it inside a Composer project:

```sh
celerrate check .
```

There is nothing to configure. Composer discovery finds your code, your
dependencies, and your PHP version range on its own:

```text
error[CEL0018]: unknown class `App\Service\Mailer`
 --> src/Controller/PostController.php:7:41
  |
7 |     public function __construct(private App\Service\Mailer $mailer)
  |                                         ^^^^^^^^^^^^^^^^^^

error[CEL0034]: accessing `format` on a possibly null `DateTimeImmutable|null`
  --> src/Notification/Mailer.php:11:16
   |
11 |         return $sentAt->format('c');
   |                ^^^^^^^^^^^^^^^

error[CEL0021]: `array_find` requires PHP 8.4, but the project's minimum PHP version is 8.1
 --> src/Service/Search.php:9:16
  |
9 |         return array_find($items, fn ($item) => $item !== null);
  |                ^^^^^^^^^^

0 notices, 3 diagnostics

for more information, run `celerrate explain CEL0018`
for more information, run `celerrate explain CEL0021`
for more information, run `celerrate explain CEL0034`
```

Every identifier ships its page in the binary:

```console
$ celerrate explain CEL0018
CEL0018: unknown class

The referenced class does not exist under any name the project can
resolve: …
```

## Performance

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/benchmark-dark.svg">
  <img src="assets/benchmark-light.svg" width="720" alt="Bar chart of median wall clock on symfony/demo: cold full analysis 1.533 seconds, warm with one function body edited 0.521 seconds">
</picture>

Measured by the committed [benchmark protocol](benchmarks/PROTOCOL.md)
on symfony/demo (9447 PHP files, 1.3 million lines, vendor tree
included), with type inference active, on the hardware the protocol
names:

| Scenario | Median wall clock |
| --- | --- |
| Cold full analysis | 1.533 s |
| Warm, one function body edited | **0.521 s** |
| Warm, one signature edited | 0.471 s |

All numbers are full CLI runs: process startup, cache loading,
analysis, and reporting. No comparison against other tools is
published at this scope; the protocol states why.

## What works today

`celerrate check .` reports five diagnostic families
([the identifier reference](docs/diagnostics.md)):

- **Unknown symbols**: references to classes, functions, or constants
  that resolve nowhere, with your project, your Composer dependencies,
  and the bundled PHP stubs all considered.
- **PHP version gating**: symbols or syntax used outside the PHP
  version range your `composer.json` declares, including removals and
  deprecations.
- **Unknown members**: methods, properties, class constants, and enum
  cases that do not exist on the receiver's inferred type, silent on
  anything dynamic, aware of `__call`/`__get` and
  `@property`/`@method` docblocks.
- **Nullability**: dereferencing a value that may be `null`, with
  flow narrowing (`instanceof`, `isset()`, `??`, `?->` chains,
  `match`, early returns, assertion annotations) deciding what is
  still nullable at each use site.
- **Argument types**: per-argument assignability and arity, named
  arguments included, honoring each file's `declare(strict_types)`
  mode.

Around them:

- **Your annotations, honored**: standard PHPDoc, the PHPStan
  dialect, and Psalm synonyms, through the bundled
  [PHPDoc bridge](docs/phpdoc-bridge.md), including inline
  suppressions (`@phpstan-ignore-line`, `@psalm-suppress`, and
  friends).
- **Interprocedural inference**: declared types are trusted,
  unannotated returns are inferred across the call graph, generics
  are resolved for precision (and never reported on).
- **Zero configuration**: Composer discovery derives what to analyze
  and which PHP versions to check against. Installed dependencies are
  indexed but never reported on.
- **`--watch`**: re-analysis on every change.
- **A persistent cache** (`.celerrate/cache/`, self-ignoring): warm
  runs reuse everything that did not change, across processes,
  inferred types included.

### What it does not do yet

No lint rules, no formatter, no language server, no baseline, and no
output formats beyond the terminal report.
Generic mismatches are not reported (generics serve precision only),
and unannotated parameters are treated as `mixed`. Those are the next
sub-projects, in the [roadmap](#roadmap)'s order.

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
norm and ships a first-party [PHPDoc bridge](docs/phpdoc-bridge.md),
enabled by default, so existing annotated codebases work on day 1.

## Roadmap

One pillar at a time, in this order:

1. **`celerrate check`**: the static analysis engine is the first
   public deliverable (previewed since v0.0.1, type-aware since
   v0.0.3); the lint, taint, and architecture rule groups build on it.
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
