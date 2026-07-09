# Celerrate

A complete PHP toolchain written in Rust: static analysis (interprocedural,
incremental), lint, formatting, LSP, refactoring, security taint analysis.
Open source (MIT OR Apache-2.0) under JDevelop.

## Authoritative documents

- Design spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md`
- Implementation plans: `.claude/superpowers/plans/`

Read the spec before architectural work. It is the source of truth.

## Engineering rules (non-negotiable)

- **Zero panic, mechanically enforced**: Clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code`
  is forbidden. Production code returns `Result`; make invalid states
  unrepresentable. Test modules may locally `#[allow]` these lints.
- **TDD**: failing test → minimal implementation → refactor. No production
  code without a test that demanded it.
- **Strict layering**: a crate depends only on crates below it:

      celerrate_source      source files, spans, line index (bottom)
      celerrate_syntax      lexer + parser → lossless syntax tree
      celerrate_db          salsa query database
      celerrate_semantics   project discovery, symbol index, name resolution
      celerrate_types       inference and type system
      celerrate_rules       rule framework + diagnostics + structured edits
      celerrate_plugin      plugin API
      celerrate_cli         the binary (top)

- **Error resilience**: no user input may ever crash the tool; parsers and
  loaders produce diagnostics, never failures.
- **Determinism**: all analysis results are pure functions of their inputs
  (salsa requirement). No wall-clock time, no randomness, no environment
  reads inside queries.

## Local commands

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all`
- `cargo deny check`

## Conventions

- Everything is written in English. Full words, no abbreviated names
  (standard acronyms fine).
- Commits: gitmoji + Conventional Commits, e.g.
  `✨ feat(syntax): parse readonly class declarations`. Commits are
  authored with the repository-configured GitHub noreply email.
