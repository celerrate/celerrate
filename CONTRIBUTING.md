# Contributing to Celerrate

Thank you for considering a contribution. This document is the contract for
working on the codebase.

## Development setup

- Install Rust via [rustup](https://rustup.rs/); the pinned toolchain in
  `rust-toolchain.toml` is picked up automatically.
- `cargo test --workspace` — run the test suite.
- `cargo clippy --workspace --all-targets -- -D warnings` — lints (CI runs
  exactly this).
- `cargo fmt --all` — formatting.
- `cargo deny check` — dependency license and advisory audit.

## Engineering rules

- **Test-driven development is the expected workflow**: write the failing
  test first, then the minimal implementation, then refactor.
- **Zero panic**: `unwrap`, `expect`, `panic!`, and slice indexing are denied
  by Clippy in production code. Use `Result` and total functions. Test
  modules may locally `#[allow]` these lints.
- **No `unsafe`**: forbidden workspace-wide.
- **Strict layering**: a crate only depends on crates below it in the layer
  diagram (see `CLAUDE.md`).
- **A reported false positive is a priority bug, not an opinion.**

## Commit conventions

Commits use gitmoji + Conventional Commits:

    <emoji> <type>(<optional scope>): <summary>

Example: `✨ feat(syntax): parse readonly class declarations`.
References: <https://gitmoji.dev/> and <https://www.conventionalcommits.org/>.

## Pull requests

- Keep pull requests focused: one concern per pull request.
- Every code change comes with tests; every user-visible change updates
  `CHANGELOG.md` under `[Unreleased]`.
- CI (lint, test, deny) must be green.

## Adding a rule or a diagnostic

The rule framework does not exist yet (it arrives with the rules
sub-project); this section will document the full workflow when it lands.
Until then, propose new rules through the "Rule proposal" issue template.

## Licensing of contributions

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as MIT OR Apache-2.0, without any
additional terms or conditions.
