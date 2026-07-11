# Foundations Part 4, Plan 5: Typed AST Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the typed AST layer of `celerrate_syntax`: a `php.ungram` grammar description that owns every node kind, an `xtask` generator (new dev-only workspace member) that emits both the `SyntaxKind` enum and the typed node structs with `Option`/iterator accessors, hand-written extensions for what a generator cannot express (semi-reserved names, `NamedType`'s dual nature, position-dependent fields), and a sourcegen freshness test. This closes the Foundations sub-project.

**Architecture:** `xtask` follows the rust-analyzer model: `php.ungram` describes the nominal *shape* of every node (the recursive-descent parser remains the sole authority on how text becomes nodes); the generator lowers it into field lists and emits two committed artifacts, `src/syntax_kind/generated.rs` (the whole enum: token variants from a static table inside xtask, node variants from `php.ungram`) and `src/ast/generated.rs` (typed structs, alternation enums, accessors). Hand-written code stays where it is today: `SyntaxKind` classifiers in `syntax_kind.rs`, the `AstNode` trait and `support` helpers in `ast.rs`/`ast/support.rs`, extensions in `ast/extensions.rs`. A freshness test in xtask regenerates in memory and compares against the committed files, so drift fails CI. Design: `.claude/superpowers/specs/2026-07-10-foundations-4-parser-design.md` (sections 2, 4, 5, 6).

**Tech Stack:** Rust 1.94 (edition 2024), `rowan` 0.16, `ungrammar` 1 (the rust-analyzer grammar DSL, MIT OR Apache-2.0), `insta` (existing snapshots), `rustfmt` (shelled out by the generator for stable formatting).

## Global Constraints

Copied from the parent spec and `CLAUDE.md`; every task's requirements include them.

- Zero panic, mechanically enforced: workspace denies `clippy::unwrap_used`, `clippy::expect_used`, `clippy::indexing_slicing`, `clippy::panic`; `unsafe_code` is forbidden. This applies to `xtask` too (it opts into the workspace lints); its fallible paths return `Result`, and only test modules may locally `#[allow]`.
- TDD: every step of behavior starts from a failing test. No production code without a test that demanded it. For the generator, "behavior" is the lowering and the emitted text: unit tests drive both.
- Strict layering: `celerrate_syntax` depends only on `celerrate_source` (plus external `rowan`, `text-size`). No bare `rowan` type in any public signature (`AstChildren` may hold one privately, like the existing aliases do publicly by aliasing). `xtask` is a dev tool outside the layer stack: it depends on `ungrammar` only, never on any `celerrate_*` crate (so a broken generated file can never prevent regenerating it).
- Determinism: the generator's output is a pure function of `php.ungram` and the token table. Iteration follows declaration order everywhere; no maps with arbitrary order feed emission (use `Vec` or sorted collections).
- The lossless invariant and error resilience are untouched: this plan adds no parser behavior except Task 1's one-token fix. Every accessor returns `Option` or an iterator; a partial tree is a normal citizen.
- Everything in English, full words, no abbreviated names (standard acronyms fine). No em-dashes anywhere, including generated output. Comments state constraints the code cannot show, never narration.
- Commits: gitmoji + Conventional Commits (`✨ feat(syntax): ...`), repository-configured identity, no AI attribution of any kind.
- Before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` must all pass.

## Deferred items from plan 4 absorbed here

Recorded in PR #8:

1. Add `Enum` to `new_expression`'s guarded call arm (`new enum(1)`, the same Zend backward-compatibility hack as `readonly`), with a pin (Task 1).
2. Two pre-existing em-dashes in `celerrate_source` doc comments; swept in a housekeeping commit (Task 1).
3. `php.ungram` takes ownership of every node kind, the 30 declaration kinds included (Tasks 4 and 5).
4. The bare-`CloseTag` note from plan 3: a classic `declare` body of `?>` leaves a bare `CloseTag` token with no statement-node child, so `DeclareStatement`'s statement accessor yields nothing there. Documented in `php.ungram` and pinned by a test (Tasks 4 and 9).
5. `NamedType` holds either a `Name` node or a bare keyword token; the typed layer exposes both as one concept through a hand-written extension (Task 8).

**Recorded for the semantics layer** (no code in this plan): `var` in parameter position parses clean while Zend rejects it; it belongs on the semantics layer's modifier-placement judgment list, next to the other permissiveness decisions recorded in plans 3 and 4 (`$a ?? $b = $c` tree shape, modifier order and repetition, asymmetric-visibility spacing).

## Design decisions recorded by this plan

- **The whole `SyntaxKind` enum is generated**, not only the node section. The enum is one Rust item and cannot be assembled from two files without macro gymnastics; the token variants therefore move into a static table inside xtask (name, ungrammar spelling, doc lines), exactly rust-analyzer's `KindsSrc`. The design's requirement ("the node-kind variants of `SyntaxKind`" are generated) is satisfied: node variants come from `php.ungram`, token variants from the table, and the committed result is asserted fresh against both.
- **Constrained ungrammar dialect.** `php.ungram` uses `|` between node references only in dedicated *enum rules* (`Expression`, `Statement`, `Type`, `MemberDeclaration`, `StringInterpolation`, `TraitAdaptation`), which become Rust enums. Inside node rules, alternation is allowed only between token literals (operator sets, paired delimiters). This keeps the lowering small and total instead of reimplementing rust-analyzer's full lowering.
- **Accessor policy.** Node references become accessors always (labeled name, or the snake_case type name; pluralized with `s` under repetition; the reserved spelling `type` becomes `ty`). Token literals become accessors **only when labeled** (`operator:('+' | '-')` yields `operator_token()`); unlabeled tokens are shape documentation. Semi-reserved name positions stay unlabeled on purpose: a generated `Option<Identifier-token>` accessor would silently return `None` for `const FOR = 1;`, so those positions get hand-written `name_token()` extensions instead (Task 8).
- **Positional same-type fields.** Two node fields of the same type in one rule map to `children().nth(0)`, `nth(1)`, in rule order (`BinaryExpression`'s `lhs`/`rhs`). Where source order does not determine the role (the short ternary `?:`, `[key => value]` vs `[value]`, `foreach` key/value, `match` arm conditions vs body, `yield key => value`, trait adaptations), the node is on the generator's **override list**: it gets its struct and `AstNode` impl but no generated accessors, and Task 8 hand-writes them from token anchors. Override list: `TernaryExpression`, `ArrayElement`, `ForeachStatement`, `MatchArm`, `YieldExpression`, `TraitPrecedence`, `TraitAlias`.
- **`ErrorNode` lives outside `php.ungram`** (its children are arbitrary wreckage); xtask appends it as a fixed extra node kind and emits a fieldless struct for it. `ErrorNode` belongs to no alternation enum: typed iteration simply skips wreckage, which is the design's "partial trees are normal citizens" made concrete.
- **Formatting stability.** The generator pipes emitted text through `rustfmt --edition 2024` (the toolchain pin fixes the version), so the freshness comparison is byte-exact and `cargo fmt --check` can never disagree with a fresh artifact. The CI test job gains the `rustfmt` component.
- **`rowan::ast::AstNode` is not used.** The crate owns its `AstNode` trait so no bare rowan trait appears in public signatures; the `SyntaxNodePtr` alias joins the existing tree aliases (the design's section 2 lists it; upper layers need it for salsa keys).

## File Structure

```
Cargo.toml                                        modify (Task 2): members += "xtask", ungrammar dependency
.cargo/config.toml                                create (Task 2): `cargo xtask` alias
xtask/Cargo.toml                                  create (Task 2)
xtask/src/main.rs                                 create (Task 2): thin CLI (`codegen`)
xtask/src/lib.rs                                  create (Task 2): Result alias, workspace_root, module list
xtask/src/codegen.rs                              create (Task 2), grows: Artifact, artifacts(), run(), reformat()
xtask/src/codegen/tokens.rs                       create (Task 2): the token table (variant, ungrammar name, docs)
xtask/src/codegen/grammar.rs                      create (Task 3): php.ungram loading, doc extraction, lowering
xtask/src/codegen/emit_kinds.rs                   create (Task 5): SyntaxKind emission
xtask/src/codegen/emit_ast.rs                     create (Task 7): typed node emission, override list
xtask/tests/sourcegen.rs                          create (Task 5), extended (Task 7): freshness
crates/celerrate_syntax/php.ungram                create (Task 4): the grammar description, owns node kinds
crates/celerrate_syntax/src/syntax_kind.rs        modify (Task 5): enum replaced by `mod generated`, classifiers stay
crates/celerrate_syntax/src/syntax_kind/generated.rs  create (Task 5): generated, committed
crates/celerrate_syntax/src/ast.rs                create (Task 6): AstNode, AstChildren, module wiring
crates/celerrate_syntax/src/ast/support.rs        create (Task 6): child/children/token helpers
crates/celerrate_syntax/src/ast/generated.rs      create (Task 7): generated, committed
crates/celerrate_syntax/src/ast/extensions.rs     create (Task 8): hand-written accessors
crates/celerrate_syntax/src/tree.rs               modify (Task 6): SyntaxNodePtr alias
crates/celerrate_syntax/src/lib.rs                modify (Task 6): `pub mod ast`, SyntaxNodePtr export
crates/celerrate_syntax/src/parser/grammar/expressions.rs  modify (Task 1): `new enum(` arm
crates/celerrate_syntax/tests/declarations_enums.rs        modify (Task 1): `new enum(1)` pin
crates/celerrate_source/src/lib.rs                modify (Task 1): em-dash sweep
crates/celerrate_source/src/source_text.rs        modify (Task 1): em-dash sweep
crates/celerrate_syntax/tests/ast.rs              create (Task 7), grows (Tasks 8, 9): typed accessor tests
.github/workflows/ci.yml                          modify (Task 9): rustfmt component in the test job
CHANGELOG.md                                      modify (Task 9)
```

Notes that apply to every task:

- **Test file preamble**: every new integration test file in `celerrate_syntax` starts with `#![allow(clippy::expect_used)]`, `mod support;` when it uses the render helpers, and only the imports it uses (`cargo clippy -- -D warnings` rejects unused imports).
- **xtask tests**: unit tests live in `#[cfg(test)]` modules with `#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]` at module scope, matching the workspace idiom.
- **Running the generator**: `cargo xtask codegen` (the alias added in Task 2). It rewrites both generated files in place; `git diff` shows the effect.
- **Snapshot updates**: no existing snapshot may change in this plan except through Task 1's `new enum(` fix, which improves exactly the trees containing `new enum(`; any other corpus diff is a regression, investigate before accepting.

---

### Task 1: PR #8 follow-ups: the `new enum(` call arm and the em-dash sweep

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (the `new_expression` match, around line 801)
- Test: `crates/celerrate_syntax/tests/declarations_enums.rs`
- Modify: `crates/celerrate_source/src/lib.rs` (module doc, line 4)
- Modify: `crates/celerrate_source/src/source_text.rs` (doc comment, line 8)

**Interfaces:**
- Consumes: `render_expression` from `tests/support/mod.rs` (existing).
- Produces: nothing later tasks rely on; this clears the recorded parser follow-ups so the rest of the plan is pure typed-AST work.

- [ ] **Step 1: Write the failing pin for `new enum(1)`**

Append to `crates/celerrate_syntax/tests/declarations_enums.rs` (match its existing imports; it already uses `render_expression` or add it to the `support::` import list):

```rust
#[test]
fn new_enum_parenthesized_stays_a_constructor_call() {
    // Zend keeps `enum` usable as a plain class or function name for
    // backward compatibility: directly followed by `(`, it is never the
    // declaration keyword, even right after `new`. Same hack as
    // `new readonly(...)`, pinned by the test beside this one.
    insta::assert_snapshot!(render_expression("new enum(1)"), @r#"
    NewExpression
      New "new"
      Name
        Enum "enum"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
    "#);
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p celerrate_syntax --test declarations_enums new_enum_parenthesized -- --nocapture`
Expected: FAIL. Today the `new_expression` match has no `Enum` arm, so the fallback diagnoses `ExpectedExpression` and the tree does not contain the `Name` over `enum`.

- [ ] **Step 3: Add `Enum` to the guarded call arm**

In `crates/celerrate_syntax/src/parser/grammar/expressions.rs`, inside `new_expression`, extend the existing `Readonly` arm (keep it *before* the anonymous-class arm, as its comment already requires):

```rust
        // Zend keeps `readonly` and `enum` callable as plain function
        // names for backward compatibility, even right after `new`: only
        // `readonly class` is the anonymous-class form, and `enum` never
        // declares here. This arm must precede the anonymous-class arm
        // below so the `(` case takes precedence over it.
        Some(SyntaxKind::Readonly | SyntaxKind::Enum)
            if parser.nth(1) == Some(SyntaxKind::OpenParenthesis) =>
        {
            let name_marker = parser.start();
            parser.bump();
            name_marker.complete(parser, SyntaxKind::Name);
        }
```

(The original arm matched `Readonly` only; replace its comment with the one above.)

- [ ] **Step 4: Run the test suite**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, including the new pin. If a corpus snapshot changes, it must be a `new enum(` tree improving from wreckage to a call; anything else is a regression.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax/src/parser/grammar/expressions.rs crates/celerrate_syntax/tests/declarations_enums.rs
git commit -m "🐛 fix(syntax): keep new enum( a constructor call"
```

- [ ] **Step 6: Sweep the two em-dashes in `celerrate_source`**

In `crates/celerrate_source/src/lib.rs`, the module doc currently reads:

```rust
//! Celerrate crate and performs no I/O — file contents arrive as bytes
```

Replace with:

```rust
//! Celerrate crate and performs no I/O: file contents arrive as bytes
```

In `crates/celerrate_source/src/source_text.rs`, the `SourceTooLarge` doc currently reads:

```rust
/// diagnostic. Everything else — invalid bytes, a byte-order mark — is
/// provenance data on the decoded [`SourceText`], not an error.
```

Replace with:

```rust
/// diagnostic. Everything else (invalid bytes, a byte-order mark) is
/// provenance data on the decoded [`SourceText`], not an error.
```

- [ ] **Step 7: Verify the sweep is complete**

Run: `grep -rn '—' crates/`
Expected: no matches (the plan-4 PR recorded exactly these two).

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean.

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_source/src/lib.rs crates/celerrate_source/src/source_text.rs
git commit -m "📝 docs(source): drop the em-dashes from doc comments"
```

---

### Task 2: The `xtask` workspace member and its token table

**Files:**
- Modify: `Cargo.toml` (workspace members and dependencies)
- Create: `.cargo/config.toml`
- Create: `xtask/Cargo.toml`
- Create: `xtask/src/main.rs`
- Create: `xtask/src/lib.rs`
- Create: `xtask/src/codegen.rs`
- Create: `xtask/src/codegen/tokens.rs` (tests inline)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `xtask::Result<T>` (alias over `Box<dyn Error + Send + Sync>`), `xtask::workspace_root() -> Result<PathBuf>`, `xtask::codegen::{Artifact, artifacts, run}`, `xtask::codegen::tokens::{TokenKindDefinition, TOKEN_KINDS, resolve_ungrammar_token}`. Tasks 3, 5, and 7 build on all of these; the token table's *order* is the enum's token order, so the keyword block must stay contiguous (Task 5's existing classifier test pins it).

- [ ] **Step 1: Declare the member and the dependency**

In the root `Cargo.toml`, change the two sections:

```toml
[workspace]
resolver = "3"
members = ["crates/*", "xtask"]
```

and append to `[workspace.dependencies]`:

```toml
ungrammar = "1"
```

- [ ] **Step 2: Add the cargo alias**

Create `.cargo/config.toml`:

```toml
[alias]
xtask = "run --package xtask --"
```

- [ ] **Step 3: Create the crate manifest**

Create `xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
description = "Development task runner: source generation for celerrate_syntax"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
publish = false

[dependencies]
ungrammar = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 4: Write the failing table tests**

Create `xtask/src/codegen/tokens.rs` with only the test module first (the table constant does not exist yet, so this fails to compile, which is the red state):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{TOKEN_KINDS, resolve_ungrammar_token};

    #[test]
    fn variant_names_are_unique_and_nonempty() {
        let mut seen = std::collections::HashSet::new();
        for definition in TOKEN_KINDS {
            assert!(!definition.variant.is_empty());
            assert!(
                seen.insert(definition.variant),
                "duplicate variant {}",
                definition.variant
            );
        }
    }

    #[test]
    fn ungrammar_spellings_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for definition in TOKEN_KINDS {
            if let Some(spelling) = definition.ungrammar_name {
                assert!(seen.insert(spelling), "duplicate spelling {spelling}");
            }
        }
    }

    #[test]
    fn spellings_resolve_to_variants() {
        assert_eq!(resolve_ungrammar_token("function"), Some("Function"));
        assert_eq!(resolve_ungrammar_token("("), Some("OpenParenthesis"));
        assert_eq!(resolve_ungrammar_token("yield from"), Some("YieldFrom"));
        assert_eq!(resolve_ungrammar_token("#["), Some("AttributeOpen"));
        assert_eq!(resolve_ungrammar_token("backslash"), Some("Backslash"));
        assert_eq!(resolve_ungrammar_token("identifier"), Some("Identifier"));
        assert_eq!(resolve_ungrammar_token("(int)"), Some("IntCast"));
        assert_eq!(resolve_ungrammar_token("not a token"), None);
        // Trivia and tags never appear in php.ungram.
        assert!(
            TOKEN_KINDS
                .iter()
                .find(|definition| definition.variant == "Whitespace")
                .expect("Whitespace exists")
                .ungrammar_name
                .is_none()
        );
    }

    #[test]
    fn the_keyword_block_is_contiguous_from_abstract_to_yield_from() {
        // `SyntaxKind::is_keyword` relies on this layout; the enum-side
        // test pins it again after generation.
        let start = TOKEN_KINDS
            .iter()
            .position(|definition| definition.variant == "Abstract")
            .expect("Abstract exists");
        let end = TOKEN_KINDS
            .iter()
            .position(|definition| definition.variant == "YieldFrom")
            .expect("YieldFrom exists");
        assert_eq!(end - start + 1, 70, "seventy keyword kinds");
    }
}
```

- [ ] **Step 5: Run and watch it fail**

Run: `cargo test -p xtask` (after creating the skeleton files of steps 6 and 7 so the crate exists; the table constant is still missing).
Expected: compile FAIL on the missing `TOKEN_KINDS`.

- [ ] **Step 6: Write the library skeleton**

Create `xtask/src/lib.rs`:

```rust
//! Development tasks for the Celerrate workspace. The only command today
//! is `codegen`: regenerate the committed sources of `celerrate_syntax`
//! from `php.ungram` and the token table. xtask deliberately depends on
//! no `celerrate_*` crate, so a broken generated file can never prevent
//! regenerating it.

use std::path::{Path, PathBuf};

pub mod codegen;

/// Errors are rendered once, in `main`; no variant needs matching.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// The workspace root: xtask lives one level below it.
pub fn workspace_root() -> Result<PathBuf> {
    let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_directory
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must live one level below the workspace root".into())
}
```

Create `xtask/src/main.rs`:

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    match (arguments.next().as_deref(), arguments.next()) {
        (Some("codegen"), None) => match xtask::codegen::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: cargo xtask codegen");
            ExitCode::FAILURE
        }
    }
}
```

Create `xtask/src/codegen.rs`:

```rust
//! Source generation: `artifacts()` produces every generated file as
//! text (a pure function of `php.ungram` and the token table), `run()`
//! writes them to the workspace. The freshness test compares
//! `artifacts()` against the committed files.

pub mod tokens;

use std::path::PathBuf;

use crate::Result;

/// One generated file: a workspace-relative path and its full text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub relative_path: PathBuf,
    pub text: String,
}

/// Every artifact, in a fixed order. Grows as the emitters land.
pub fn artifacts() -> Result<Vec<Artifact>> {
    Ok(Vec::new())
}

/// Writes every artifact into the workspace.
pub fn run() -> Result<()> {
    let root = crate::workspace_root()?;
    for artifact in artifacts()? {
        let path = root.join(&artifact.relative_path);
        std::fs::write(&path, &artifact.text)?;
        println!("wrote {}", artifact.relative_path.display());
    }
    Ok(())
}
```

- [ ] **Step 7: Write the token table**

Prepend to `xtask/src/codegen/tokens.rs` (above the test module). The table is the enum's token section made data: **order is discriminant order**, and the doc lines are transcribed **verbatim** from the current `crates/celerrate_syntax/src/syntax_kind.rs` variant docs (shown inline below where they exist today).

```rust
//! The token kinds of `SyntaxKind`, as data: the generator emits the
//! enum's token section from this table (order here is discriminant
//! order, and the keyword block must stay contiguous for
//! `SyntaxKind::is_keyword`), and resolves the token spellings
//! `php.ungram` uses against it.

/// One token kind: its `SyntaxKind` variant name, the spelling
/// `php.ungram` uses for it (`None` for kinds that never appear in a
/// grammar rule, like trivia and tags), and its doc lines.
#[derive(Debug, Clone, Copy)]
pub struct TokenKindDefinition {
    pub variant: &'static str,
    pub ungrammar_name: Option<&'static str>,
    pub documentation: &'static [&'static str],
}

const fn token(
    variant: &'static str,
    ungrammar_name: Option<&'static str>,
    documentation: &'static [&'static str],
) -> TokenKindDefinition {
    TokenKindDefinition {
        variant,
        ungrammar_name,
        documentation,
    }
}

/// The spelling of a `php.ungram` token, resolved to its variant name.
pub fn resolve_ungrammar_token(spelling: &str) -> Option<&'static str> {
    TOKEN_KINDS
        .iter()
        .find(|definition| definition.ungrammar_name == Some(spelling))
        .map(|definition| definition.variant)
}

pub const TOKEN_KINDS: &[TokenKindDefinition] = &[
    // Trivia.
    token("Whitespace", None, &[]),
    token("LineComment", None, &["`//` and `#` comments, up to the end of the line or a `?>`."]),
    token("BlockComment", None, &["`/* ... */` comments."]),
    token("DocComment", None, &["`/** ... */` docblocks, a distinct kind: the type engine reads them."]),
    token("Shebang", None, &["A `#!` first line."]),
    // Tags and inline HTML.
    token("OpenTag", None, &["`<?php`."]),
    token("OpenTagEcho", None, &["`<?=`."]),
    token("ShortOpenTag", None, &["`<?`, lexed unconditionally; availability is a semantic judgment."]),
    token("CloseTag", None, &["`?>`, plus the single newline PHP swallows after it, if present."]),
    token("InlineHtml", None, &["Everything outside PHP tags."]),
    // Names.
    token("Identifier", Some("identifier"), &[]),
    token("Variable", Some("variable"), &["`$name`."]),
    // Literals and string structure.
    token("IntegerLiteral", Some("integer_literal"), &[]),
    token("FloatLiteral", Some("float_literal"), &[]),
    token("SingleQuotedString", Some("single_quoted_string"), &["A whole `'...'` (or `b'...'`) string, quotes included."]),
    token("StringFragment", Some("string_fragment"), &["A literal run inside an interpolated string, heredoc, or backtick."]),
    token("DoubleQuote", Some("\""), &["A `\"` delimiter (or the opening `b\"`)."]),
    token("Backtick", Some("`"), &["A `` ` `` delimiter."]),
    token("HeredocStart", Some("heredoc_start"), &["`<<<LABEL` (or quoted label), trailing newline included."]),
    token("HeredocEnd", Some("heredoc_end"), &["The closing label of a heredoc or nowdoc, indentation included."]),
    token("DollarOpenBrace", Some("${"), &["`${` opening the deprecated interpolation form."]),
    // Keywords.
    token("Abstract", Some("abstract"), &[]),
    token("And", Some("and"), &[]),
    token("Array", Some("array"), &[]),
    token("As", Some("as"), &[]),
    token("Break", Some("break"), &[]),
    token("Callable", Some("callable"), &[]),
    token("Case", Some("case"), &[]),
    token("Catch", Some("catch"), &[]),
    token("Class", Some("class"), &[]),
    token("Clone", Some("clone"), &[]),
    token("Const", Some("const"), &[]),
    token("Continue", Some("continue"), &[]),
    token("Declare", Some("declare"), &[]),
    token("Default", Some("default"), &[]),
    token("Do", Some("do"), &[]),
    token("Echo", Some("echo"), &[]),
    token("Else", Some("else"), &[]),
    token("ElseIf", Some("elseif"), &[]),
    token("Empty", Some("empty"), &[]),
    token("EndDeclare", Some("enddeclare"), &[]),
    token("EndFor", Some("endfor"), &[]),
    token("EndForeach", Some("endforeach"), &[]),
    token("EndIf", Some("endif"), &[]),
    token("EndSwitch", Some("endswitch"), &[]),
    token("EndWhile", Some("endwhile"), &[]),
    token("Enum", Some("enum"), &[]),
    token("Eval", Some("eval"), &[]),
    token("Exit", Some("exit"), &["`exit` and its alias `die`."]),
    token("Extends", Some("extends"), &[]),
    token("Final", Some("final"), &[]),
    token("Finally", Some("finally"), &[]),
    token("Fn", Some("fn"), &[]),
    token("For", Some("for"), &[]),
    token("Foreach", Some("foreach"), &[]),
    token("Function", Some("function"), &[]),
    token("Global", Some("global"), &[]),
    token("Goto", Some("goto"), &[]),
    token("If", Some("if"), &[]),
    token("Implements", Some("implements"), &[]),
    token("Include", Some("include"), &[]),
    token("IncludeOnce", Some("include_once"), &[]),
    token("InstanceOf", Some("instanceof"), &[]),
    token("InsteadOf", Some("insteadof"), &[]),
    token("Interface", Some("interface"), &[]),
    token("Isset", Some("isset"), &[]),
    token("List", Some("list"), &[]),
    token("Match", Some("match"), &[]),
    token("Namespace", Some("namespace"), &[]),
    token("New", Some("new"), &[]),
    token("Or", Some("or"), &[]),
    token("Print", Some("print"), &[]),
    token("Private", Some("private"), &[]),
    token("Protected", Some("protected"), &[]),
    token("Public", Some("public"), &[]),
    token("Readonly", Some("readonly"), &[]),
    token("Require", Some("require"), &[]),
    token("RequireOnce", Some("require_once"), &[]),
    token("Return", Some("return"), &[]),
    token("Static", Some("static"), &[]),
    token("Switch", Some("switch"), &[]),
    token("Throw", Some("throw"), &[]),
    token("Trait", Some("trait"), &[]),
    token("Try", Some("try"), &[]),
    token("Unset", Some("unset"), &[]),
    token("Use", Some("use"), &[]),
    token("Var", Some("var"), &[]),
    token("While", Some("while"), &[]),
    token("Xor", Some("xor"), &[]),
    token("Yield", Some("yield"), &[]),
    token("YieldFrom", Some("yield from"), &["`yield from`, one token as in Zend, interior whitespace included."]),
    // Casts (single tokens, inner whitespace included).
    token("IntCast", Some("(int)"), &[]),
    token("BoolCast", Some("(bool)"), &[]),
    token("FloatCast", Some("(float)"), &[]),
    token("StringCast", Some("(string)"), &[]),
    token("BinaryCast", Some("(binary)"), &[]),
    token("ArrayCast", Some("(array)"), &[]),
    token("ObjectCast", Some("(object)"), &[]),
    // Operators and punctuation.
    token("Plus", Some("+"), &[]),
    token("Minus", Some("-"), &[]),
    token("Star", Some("*"), &[]),
    token("Slash", Some("/"), &[]),
    token("Percent", Some("%"), &[]),
    token("StarStar", Some("**"), &[]),
    token("Equals", Some("="), &[]),
    token("PlusEquals", Some("+="), &[]),
    token("MinusEquals", Some("-="), &[]),
    token("StarEquals", Some("*="), &[]),
    token("SlashEquals", Some("/="), &[]),
    token("DotEquals", Some(".="), &[]),
    token("PercentEquals", Some("%="), &[]),
    token("StarStarEquals", Some("**="), &[]),
    token("AmpersandEquals", Some("&="), &[]),
    token("PipeEquals", Some("|="), &[]),
    token("CaretEquals", Some("^="), &[]),
    token("LessLessEquals", Some("<<="), &[]),
    token("GreaterGreaterEquals", Some(">>="), &[]),
    token("QuestionQuestionEquals", Some("??="), &[]),
    token("EqualsEquals", Some("=="), &[]),
    token("EqualsEqualsEquals", Some("==="), &[]),
    token("BangEquals", Some("!="), &["`!=` and its alias `<>`."]),
    token("BangEqualsEquals", Some("!=="), &[]),
    token("Less", Some("<"), &[]),
    token("Greater", Some(">"), &[]),
    token("LessEquals", Some("<="), &[]),
    token("GreaterEquals", Some(">="), &[]),
    token("Spaceship", Some("<=>"), &["`<=>`."]),
    token("PlusPlus", Some("++"), &[]),
    token("MinusMinus", Some("--"), &[]),
    token("LessLess", Some("<<"), &[]),
    token("GreaterGreater", Some(">>"), &[]),
    token("Dot", Some("."), &[]),
    token("Bang", Some("!"), &[]),
    token("AmpersandAmpersand", Some("&&"), &[]),
    token("PipePipe", Some("||"), &[]),
    token("QuestionQuestion", Some("??"), &[]),
    token("Question", Some("?"), &[]),
    token("Colon", Some(":"), &[]),
    token("ColonColon", Some("::"), &[]),
    token("Semicolon", Some(";"), &[]),
    token("Comma", Some(","), &[]),
    token("Ampersand", Some("&"), &[]),
    token("Pipe", Some("|"), &[]),
    token("PipeGreater", Some("|>"), &["`|>`, the PHP 8.5 pipe operator."]),
    token("Caret", Some("^"), &[]),
    token("Tilde", Some("~"), &[]),
    token("At", Some("@"), &[]),
    token("Dollar", Some("$"), &[]),
    token("Backslash", Some("backslash"), &[]),
    token("Arrow", Some("->"), &["`->`."]),
    token("NullsafeArrow", Some("?->"), &["`?->`."]),
    token("FatArrow", Some("=>"), &["`=>`."]),
    token("Ellipsis", Some("..."), &["`...`."]),
    token("OpenParenthesis", Some("("), &[]),
    token("CloseParenthesis", Some(")"), &[]),
    token("OpenBracket", Some("["), &[]),
    token("CloseBracket", Some("]"), &[]),
    token("OpenBrace", Some("{"), &[]),
    token("CloseBrace", Some("}"), &[]),
    token("AttributeOpen", Some("#["), &["`#[`, distinct from the `#` line comment."]),
    token("Error", None, &["A character no rule accepts."]),
];
```

Two deliberate spellings: the backslash is written `backslash` (a word, not `'\\'`) because ungrammar's lexer treats quoted text literally and a backslash-quote sequence is not worth the ambiguity; `Backslash` therefore reads as `'backslash'` in `php.ungram`. And `DoubleQuote` is spelled `"` (one character inside ungrammar's single quotes, no escaping needed).

- [ ] **Step 8: Verify against the enum, then run the tests**

Cross-check the table against `crates/celerrate_syntax/src/syntax_kind.rs`: same variants, same order, same doc text (the table above was transcribed from it; trust the file over this plan if they ever disagree, and mirror the file).

Run: `cargo test -p xtask`
Expected: PASS (4 tests).

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean (the new member compiles under the workspace lints).

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock .cargo/config.toml xtask/
git commit -m "✨ feat(xtask): add the dev-only workspace member and its token table"
```

---

### Task 3: Grammar loading: doc extraction and the lowering

**Files:**
- Create: `xtask/src/codegen/grammar.rs` (tests inline)
- Modify: `xtask/src/codegen.rs` (module declaration)

**Interfaces:**
- Consumes: `tokens::resolve_ungrammar_token` (Task 2).
- Produces: `grammar::{load, GrammarSource, AstNodeSource, AstEnumSource, Field, FieldKind, Cardinality, HANDWRITTEN_ACCESSOR_NODES}`. Task 4 validates the real `php.ungram` through `load`; Tasks 5 and 7 emit from `GrammarSource`.

The lowering implements the constrained dialect from the design decisions: a rule whose top level is an alternation of plain node references is an *enum rule*; every other rule is a *node rule* whose walk produces fields. Node references always become fields (labeled name or snake_case type name, `type` spelled `ty`, pluralized with `s` under repetition); token literals become fields only when labeled. Two node occurrences with the same label (or both unlabeled) and the same type **merge into one field** whose cardinality is `Many` if either occurrence repeats: this is what makes the idiomatic comma-separated list `(X (',' X)*)?` lower to a single `xs()` iterator instead of a false ambiguity. Nodes on `HANDWRITTEN_ACCESSOR_NODES` get no fields at all (Task 8 hand-writes their accessors), which also exempts them from the same-type ambiguity check.

- [ ] **Step 1: Write the failing tests**

Create `xtask/src/codegen/grammar.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{Cardinality, FieldKind, load};

    const MINI: &str = r#"
/// The root of the mini grammar.
Root = Item*

/// One item, with a labeled token and an operator set.
Item = 'fn' name:'identifier' operator:('+' | '-') Body? Type? value:Expression?

Body = '{' Pair '}'

Pair = first:Item second:Item

Type = 'identifier'

Expression = Root | Item

/// A ternary lookalike: on the override list, so no fields.
TernaryExpression = Item* Body
"#;

    /// Nodes by name: ungrammar interns nodes in first-reference order,
    /// not definition order, so tests never index the node list.
    fn node<'grammar>(
        grammar: &'grammar super::GrammarSource,
        name: &str,
    ) -> &'grammar super::AstNodeSource {
        grammar
            .nodes
            .iter()
            .find(|node| node.name == name)
            .expect("node exists")
    }

    #[test]
    fn enum_rules_become_enums_and_never_nodes() {
        let grammar = load(MINI).expect("mini grammar loads");
        assert_eq!(grammar.enums.len(), 1);
        assert_eq!(grammar.enums[0].name, "Expression");
        assert_eq!(grammar.enums[0].variants, ["Root", "Item"]);
        let mut names: Vec<&str> = grammar
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["Body", "Item", "Pair", "Root", "TernaryExpression", "Type"]
        );
    }

    #[test]
    fn documentation_attaches_to_the_rule_that_follows_it() {
        let grammar = load(MINI).expect("mini grammar loads");
        assert_eq!(
            node(&grammar, "Root").documentation,
            ["The root of the mini grammar."]
        );
        assert!(
            node(&grammar, "Body").documentation.is_empty(),
            "Body has no doc"
        );
    }

    #[test]
    fn repetition_pluralizes_and_becomes_many() {
        let grammar = load(MINI).expect("mini grammar loads");
        let root = node(&grammar, "Root");
        assert_eq!(root.fields.len(), 1);
        assert_eq!(root.fields[0].name, "items");
        match &root.fields[0].kind {
            FieldKind::Node {
                type_name,
                cardinality,
                ..
            } => {
                assert_eq!(type_name, "Item");
                assert_eq!(*cardinality, Cardinality::Many);
            }
            FieldKind::Token { .. } => panic!("Item* is a node field"),
        }
    }

    #[test]
    fn labeled_tokens_become_token_fields_and_unlabeled_ones_vanish() {
        let grammar = load(MINI).expect("mini grammar loads");
        let item = node(&grammar, "Item");
        let names: Vec<&str> = item
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        // `'fn'` is unlabeled: shape documentation only. `Type` renames
        // to `ty` (reserved spelling).
        assert_eq!(names, ["name", "operator", "body", "ty", "value"]);
        match &item.fields[1].kind {
            FieldKind::Token { variants } => assert_eq!(variants, &["Plus", "Minus"]),
            FieldKind::Node { .. } => panic!("operator is a token set"),
        }
        match &item.fields[4].kind {
            FieldKind::Node { type_name, .. } => assert_eq!(type_name, "Expression"),
            FieldKind::Token { .. } => panic!("value is a node field"),
        }
    }

    #[test]
    fn same_type_twice_gets_positional_indices() {
        let grammar = load(MINI).expect("mini grammar loads");
        let pair = node(&grammar, "Pair");
        let indices: Vec<usize> = pair
            .fields
            .iter()
            .map(|field| match field.kind {
                FieldKind::Node { index, .. } => index,
                FieldKind::Token { .. } => panic!("node fields only"),
            })
            .collect();
        assert_eq!(indices, [0, 1]);
    }

    #[test]
    fn override_listed_nodes_get_no_fields() {
        // `TernaryExpression` mixes Many and single of related shapes;
        // being on HANDWRITTEN_ACCESSOR_NODES, it lowers to zero fields
        // instead of failing the ambiguity check.
        let grammar = load(MINI).expect("mini grammar loads");
        let ternary = node(&grammar, "TernaryExpression");
        assert!(ternary.fields.is_empty());
    }

    #[test]
    fn comma_separated_lists_merge_into_one_many_field() {
        let source = "Root = (Item (',' Item)*)?\nItem = 'identifier'";
        let grammar = load(source).expect("grammar loads");
        let root = node(&grammar, "Root");
        assert_eq!(root.fields.len(), 1);
        assert_eq!(root.fields[0].name, "items");
        match &root.fields[0].kind {
            FieldKind::Node { cardinality, .. } => {
                assert_eq!(*cardinality, Cardinality::Many);
            }
            FieldKind::Token { .. } => panic!("Item is a node field"),
        }
    }

    #[test]
    fn an_unknown_token_spelling_is_an_error() {
        let error = load("Root = 'no_such_token'").expect_err("must fail");
        assert!(error.to_string().contains("no_such_token"));
    }

    #[test]
    fn a_many_and_single_conflict_off_the_override_list_is_an_error() {
        // The labeled occurrence does not merge with the unlabeled
        // repeated one, so position cannot assign roles.
        let source = "Root = Item* extra:Item\nItem = 'identifier'";
        let error = load(source).expect_err("must fail");
        assert!(error.to_string().contains("Root"));
    }

    #[test]
    fn duplicate_field_names_are_an_error() {
        let source = "Root = Item Item\nItem = 'identifier'";
        // Two unlabeled `Item`s both want the name `item`: the grammar
        // must label them. (Positional indices exist, but distinct
        // accessor names cannot be derived without labels.)
        let error = load(source).expect_err("must fail");
        assert!(error.to_string().contains("item"));
    }
}
```

Note on the duplicate-name rule: `Pair` in `MINI` labels both occurrences, so it passes; the error case is the unlabeled duplicate. Positional indices are still assigned by occurrence for *labeled* same-type fields (`first` is `nth(0)`, `second` is `nth(1)`).

Add to `xtask/src/codegen.rs`, next to `pub mod tokens;`:

```rust
pub mod grammar;
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p xtask`
Expected: compile FAIL (`load` and the types do not exist).

- [ ] **Step 3: Implement the loading and lowering**

Write the production half of `xtask/src/codegen/grammar.rs` (above the test module):

```rust
//! Loads `php.ungram` into a lowered, emission-ready description. The
//! ungrammar file describes the nominal shape of every node; the
//! constrained dialect accepted here keeps the lowering total: node
//! alternations only in dedicated enum rules, token alternations
//! anywhere, and labels on atoms only.

use std::collections::HashMap;

use ungrammar::{Grammar, Rule};

use super::tokens::resolve_ungrammar_token;
use crate::Result;

/// Nodes whose accessors are hand-written in
/// `celerrate_syntax/src/ast/extensions.rs` because source position
/// alone cannot assign roles (the short ternary, `key => value` forms,
/// trait adaptations). They get structs and `AstNode` impls but no
/// generated accessors, and are exempt from the ambiguity check.
pub const HANDWRITTEN_ACCESSOR_NODES: &[&str] = &[
    "TernaryExpression",
    "ArrayElement",
    "ForeachStatement",
    "MatchArm",
    "YieldExpression",
    "TraitPrecedence",
    "TraitAlias",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarSource {
    pub nodes: Vec<AstNodeSource>,
    pub enums: Vec<AstEnumSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstNodeSource {
    pub name: String,
    pub documentation: Vec<String>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstEnumSource {
    pub name: String,
    pub documentation: Vec<String>,
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Node {
        type_name: String,
        cardinality: Cardinality,
        /// Position among same-type siblings: `children().nth(index)`.
        index: usize,
    },
    Token {
        /// `SyntaxKind` variant names; more than one for operator sets.
        variants: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    Optional,
    Many,
}

pub fn load(text: &str) -> Result<GrammarSource> {
    let grammar: Grammar = text
        .parse()
        .map_err(|error| format!("php.ungram does not parse: {error}"))?;
    let documentation = extract_documentation(text);
    let mut nodes = Vec::new();
    let mut enums = Vec::new();
    for node in grammar.iter() {
        let name = grammar[node].name.clone();
        let rule = &grammar[node].rule;
        let docs = documentation.get(&name).cloned().unwrap_or_default();
        if let Some(variants) = enum_variants(&grammar, rule) {
            enums.push(AstEnumSource {
                name,
                documentation: docs,
                variants,
            });
        } else {
            let fields = if HANDWRITTEN_ACCESSOR_NODES.contains(&name.as_str()) {
                Vec::new()
            } else {
                lower_node_rule(&grammar, &name, rule)?
            };
            nodes.push(AstNodeSource {
                name,
                documentation: docs,
                fields,
            });
        }
    }
    Ok(GrammarSource { nodes, enums })
}

/// An enum rule is a top-level alternation of plain node references.
fn enum_variants(grammar: &Grammar, rule: &Rule) -> Option<Vec<String>> {
    let Rule::Alt(branches) = rule else {
        return None;
    };
    branches
        .iter()
        .map(|branch| match branch {
            Rule::Node(node) => Some(grammar[*node].name.clone()),
            _ => None,
        })
        .collect()
}

/// One field before merging and positional indices. Node names are
/// computed after the merge pass, because pluralization depends on the
/// merged cardinality.
enum RawField {
    Node {
        label: Option<String>,
        type_name: String,
        many: bool,
    },
    Token {
        name: String,
        variants: Vec<String>,
    },
}

fn lower_node_rule(grammar: &Grammar, node_name: &str, rule: &Rule) -> Result<Vec<Field>> {
    let mut raw_fields = Vec::new();
    lower_rule(grammar, node_name, rule, None, false, &mut raw_fields)?;
    assign_indices(node_name, raw_fields)
}

fn lower_rule(
    grammar: &Grammar,
    node_name: &str,
    rule: &Rule,
    label: Option<&str>,
    many: bool,
    accumulator: &mut Vec<RawField>,
) -> Result<()> {
    match rule {
        Rule::Labeled { label: name, rule } => {
            lower_rule(grammar, node_name, rule, Some(name), many, accumulator)
        }
        Rule::Node(node) => {
            let type_name = grammar[*node].name.clone();
            accumulator.push(RawField::Node {
                label: label.map(str::to_owned),
                type_name,
                many,
            });
            Ok(())
        }
        Rule::Token(token) => {
            // Resolve even unlabeled tokens: a typo in a spelling must
            // fail loudly, not silently drop from the shape.
            let variant = resolve_token(grammar, node_name, *token)?;
            if let Some(label) = label {
                accumulator.push(RawField::Token {
                    name: label.to_owned(),
                    variants: vec![variant],
                });
            }
            Ok(())
        }
        Rule::Seq(rules) => {
            if let Some(label) = label {
                return Err(format!(
                    "{node_name}: the label {label} wraps a sequence; labels wrap atoms only"
                )
                .into());
            }
            for rule in rules {
                lower_rule(grammar, node_name, rule, None, many, accumulator)?;
            }
            Ok(())
        }
        Rule::Opt(inner) => lower_rule(grammar, node_name, inner, label, many, accumulator),
        Rule::Rep(inner) => lower_rule(grammar, node_name, inner, label, true, accumulator),
        Rule::Alt(branches) => {
            let all_tokens = branches
                .iter()
                .all(|branch| matches!(branch, Rule::Token(_)));
            if all_tokens {
                let mut variants = Vec::new();
                for branch in branches {
                    if let Rule::Token(token) = branch {
                        variants.push(resolve_token(grammar, node_name, *token)?);
                    }
                }
                if let Some(label) = label {
                    accumulator.push(RawField::Token {
                        name: label.to_owned(),
                        variants,
                    });
                }
                return Ok(());
            }
            if let Some(label) = label {
                return Err(format!(
                    "{node_name}: the label {label} wraps an alternation with node \
                     references; node alternations belong in enum rules"
                )
                .into());
            }
            for branch in branches {
                lower_rule(grammar, node_name, branch, None, many, accumulator)?;
            }
            Ok(())
        }
    }
}

fn resolve_token(grammar: &Grammar, node_name: &str, token: ungrammar::Token) -> Result<String> {
    let spelling = &grammar[token].name;
    resolve_ungrammar_token(spelling)
        .map(str::to_owned)
        .ok_or_else(|| format!("{node_name}: unknown token spelling {spelling}").into())
}

fn node_field_name(label: Option<&str>, type_name: &str, many: bool) -> String {
    match label {
        Some(label) => label.to_owned(),
        None => {
            let base = snake_case(type_name);
            if many {
                // `types`, not `tys`: the reserved-word rename only
                // matters for the singular accessor.
                format!("{base}s")
            } else if base == "type" {
                "ty".to_owned()
            } else {
                base
            }
        }
    }
}

fn snake_case(name: &str) -> String {
    let mut output = String::new();
    for (position, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if position > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

/// Merges same-key node occurrences (`X (',' X)*` is one list field),
/// assigns `children().nth(index)` positions among same-type node
/// fields, and rejects the shapes position cannot disambiguate.
fn assign_indices(node_name: &str, raw_fields: Vec<RawField>) -> Result<Vec<Field>> {
    // Merge pass: same (label, type) means one conceptual field; the
    // merged field repeats if any occurrence repeats.
    let mut merged: Vec<RawField> = Vec::new();
    'raw: for raw_field in raw_fields {
        if let RawField::Node {
            label,
            type_name,
            many,
        } = &raw_field
        {
            for existing in &mut merged {
                if let RawField::Node {
                    label: existing_label,
                    type_name: existing_type,
                    many: existing_many,
                } = existing
                {
                    if existing_label == label && existing_type == type_name {
                        *existing_many |= *many;
                        continue 'raw;
                    }
                }
            }
        }
        merged.push(raw_field);
    }
    let mut per_type_totals: HashMap<String, (usize, bool)> = HashMap::new();
    for raw_field in &merged {
        if let RawField::Node {
            type_name, many, ..
        } = raw_field
        {
            let entry = per_type_totals.entry(type_name.clone()).or_insert((0, false));
            entry.0 += 1;
            entry.1 |= *many;
        }
    }
    let mut seen_names: Vec<String> = Vec::new();
    let mut per_type_counters: HashMap<String, usize> = HashMap::new();
    let mut fields = Vec::new();
    for raw_field in merged {
        let field = match raw_field {
            RawField::Node {
                label,
                type_name,
                many,
            } => {
                let (total, any_many) = per_type_totals
                    .get(&type_name)
                    .copied()
                    .unwrap_or((0, false));
                if total > 1 && any_many {
                    return Err(format!(
                        "{node_name}: {type_name} appears both repeated and single; \
                         position cannot assign roles, add the node to \
                         HANDWRITTEN_ACCESSOR_NODES"
                    )
                    .into());
                }
                let name = node_field_name(label.as_deref(), &type_name, many);
                let counter = per_type_counters.entry(type_name.clone()).or_insert(0);
                let index = *counter;
                *counter += 1;
                Field {
                    name,
                    kind: FieldKind::Node {
                        type_name,
                        cardinality: if many {
                            Cardinality::Many
                        } else {
                            Cardinality::Optional
                        },
                        index,
                    },
                }
            }
            RawField::Token { name, variants } => Field {
                name,
                kind: FieldKind::Token { variants },
            },
        };
        if seen_names.contains(&field.name) {
            return Err(format!(
                "{node_name}: two fields want the name {}; label them apart",
                field.name
            )
            .into());
        }
        seen_names.push(field.name.clone());
        fields.push(field);
    }
    Ok(fields)
}

/// Doc lines (`///`) immediately above a rule definition attach to it.
/// Any other non-blank line resets the pending block, so stray comments
/// cannot leak onto the next rule.
fn extract_documentation(text: &str) -> HashMap<String, Vec<String>> {
    let mut documentation = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            pending.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
            continue;
        }
        if let Some(name) = rule_start_name(trimmed) {
            if !pending.is_empty() {
                documentation.insert(name.to_owned(), std::mem::take(&mut pending));
            }
        } else if !trimmed.is_empty() {
            pending.clear();
        }
    }
    documentation
}

/// `Name =` at the start of a line begins a rule; continuation lines
/// never match (their text before any `=` is not a bare capitalized
/// identifier).
fn rule_start_name(line: &str) -> Option<&str> {
    let (candidate, _rest) = line.split_once('=')?;
    let candidate = candidate.trim();
    let starts_uppercase = candidate
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase());
    (starts_uppercase
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric()))
    .then_some(candidate)
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p xtask`
Expected: PASS (the nine grammar tests plus the four token tests).

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean.

- [ ] **Step 5: Commit**

```bash
git add xtask/src/codegen.rs xtask/src/codegen/grammar.rs
git commit -m "✨ feat(xtask): lower ungrammar rules into an emission-ready source"
```

---

### Task 4: `php.ungram` takes ownership of the node kinds

**Files:**
- Create: `crates/celerrate_syntax/php.ungram`
- Modify: `xtask/src/codegen.rs` (a `php_ungram_source()` helper)
- Test: `xtask/tests/grammar_file.rs`

**Interfaces:**
- Consumes: `grammar::load` (Task 3).
- Produces: the grammar description itself, plus `codegen::php_ungram_source() -> Result<String>` (reads the file relative to the workspace root). Tasks 5 and 7 generate from it. **105 node rules** (every current node kind except `ErrorNode`, which xtask appends as an extra) and **6 enum rules** (`Expression`, `Statement`, `Type`, `MemberDeclaration`, `StringInterpolation`, `TraitAdaptation`).

The doc comments (`///`) below were transcribed from the node-kind docs in `crates/celerrate_syntax/src/syntax_kind.rs`, with new sentences only where this plan records something (the `declare` bare-`CloseTag` note, `NamedType`'s dual nature, the override notes). If a transcription and the file ever disagree, the file wins: mirror it. These docs become the generated docs of both the kind variants and the typed structs.

- [ ] **Step 1: Write the failing validation test**

Create `xtask/tests/grammar_file.rs`:

```rust
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use xtask::codegen::grammar::{Cardinality, FieldKind, load};

fn source() -> xtask::codegen::grammar::GrammarSource {
    let text = xtask::codegen::php_ungram_source().expect("php.ungram is readable");
    load(&text).expect("php.ungram loads and lowers")
}

fn node<'grammar>(
    grammar: &'grammar xtask::codegen::grammar::GrammarSource,
    name: &str,
) -> &'grammar xtask::codegen::grammar::AstNodeSource {
    grammar
        .nodes
        .iter()
        .find(|node| node.name == name)
        .expect("node exists")
}

#[test]
fn the_grammar_covers_every_node_kind() {
    let grammar = source();
    assert_eq!(grammar.nodes.len(), 105, "every node kind except ErrorNode");
    assert_eq!(grammar.enums.len(), 6);
    let enum_names: Vec<&str> = grammar
        .enums
        .iter()
        .map(|enumeration| enumeration.name.as_str())
        .collect();
    assert_eq!(
        enum_names,
        [
            "Expression",
            "Statement",
            "Type",
            "MemberDeclaration",
            "StringInterpolation",
            "TraitAdaptation"
        ]
    );
}

#[test]
fn spot_checks_on_lowered_shapes() {
    let grammar = source();

    let class_declaration = node(&grammar, "ClassDeclaration");
    let names: Vec<&str> = class_declaration
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "attribute_groups",
            "argument_list",
            "extends_clause",
            "implements_clause",
            "member_list"
        ]
    );

    let binary = node(&grammar, "BinaryExpression");
    assert_eq!(binary.fields[0].name, "lhs");
    assert_eq!(binary.fields[2].name, "rhs");
    match (&binary.fields[0].kind, &binary.fields[2].kind) {
        (
            FieldKind::Node { index: 0, .. },
            FieldKind::Node { index: 1, .. },
        ) => {}
        _ => panic!("lhs and rhs are positional Expression fields"),
    }

    let block = node(&grammar, "Block");
    assert_eq!(block.fields[0].name, "statements");
    match &block.fields[0].kind {
        FieldKind::Node { cardinality, .. } => {
            assert_eq!(*cardinality, Cardinality::Many);
        }
        FieldKind::Token { .. } => panic!("statements is a node field"),
    }

    // Override-listed nodes lower to zero fields.
    for name in ["TernaryExpression", "ForeachStatement", "MatchArm"] {
        assert!(node(&grammar, name).fields.is_empty(), "{name} overrides");
    }

    // Semi-reserved positions carry no generated name accessor.
    assert!(
        node(&grammar, "ConstantElement")
            .fields
            .iter()
            .all(|field| field.name != "name"),
        "ConstantElement names are hand-written"
    );

    let expression = grammar
        .enums
        .iter()
        .find(|enumeration| enumeration.name == "Expression")
        .expect("Expression enum");
    assert_eq!(expression.variants.len(), 33);
    let statement = grammar
        .enums
        .iter()
        .find(|enumeration| enumeration.name == "Statement")
        .expect("Statement enum");
    assert_eq!(statement.variants.len(), 28);
}
```

Add to `xtask/src/codegen.rs`:

```rust
/// The raw text of `php.ungram`.
pub fn php_ungram_source() -> Result<String> {
    let path = crate::workspace_root()?.join("crates/celerrate_syntax/php.ungram");
    std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()).into())
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p xtask --test grammar_file`
Expected: FAIL (the file does not exist yet).

- [ ] **Step 3: Write `php.ungram`**

Create `crates/celerrate_syntax/php.ungram` with exactly this content:

```
// php.ungram: the nominal shape of every PHP syntax node.
//
// This file owns the node kinds of SyntaxKind: `cargo xtask codegen`
// generates the enum's node section and the typed AST accessors from
// it. It describes shape, not parsing: the recursive-descent parser in
// src/parser/ remains the sole authority on how text becomes nodes.
//
// Conventions:
// - `|` between node references appears only in the enum rules at the
//   top; inside node rules, alternation is between token literals only.
// - Labeled tokens get generated accessors; unlabeled tokens are shape
//   documentation. Semi-reserved name positions (where any keyword may
//   stand for the name) are deliberately unlabeled: a generated
//   Identifier accessor would return None on `const FOR = 1;`, so those
//   accessors are hand-written in src/ast/extensions.rs.
// - Nodes on xtask's HANDWRITTEN_ACCESSOR_NODES list (the short
//   ternary, `key => value` shapes, trait adaptations) get no generated
//   accessors; their rules here are documentation.
// - Trivia, tags (`<?php`, `?>`, `<?=`), inline HTML, and ErrorNode
//   wreckage stay bare in the tree and appear in no rule: typed
//   iteration skips them, which is how partial trees stay free to
//   consume.

//**********************//
//     Alternations     //
//**********************//

Expression =
  Literal
| VariableReference
| DynamicVariableExpression
| ParenthesizedExpression
| BinaryExpression
| PrefixExpression
| PostfixExpression
| CastExpression
| TernaryExpression
| AssignmentExpression
| NameExpression
| CallExpression
| MemberAccessExpression
| ScopedAccessExpression
| IndexExpression
| ArrayExpression
| ListExpression
| InterpolatedString
| HeredocExpression
| ShellExecExpression
| NewExpression
| CloneExpression
| IssetExpression
| EmptyExpression
| EvalExpression
| ExitExpression
| PrintExpression
| ThrowExpression
| YieldExpression
| IncludeExpression
| MatchExpression
| ClosureExpression
| ArrowFunctionExpression

Statement =
  Block
| EmptyStatement
| EchoStatement
| ExpressionStatement
| ReturnStatement
| BreakStatement
| ContinueStatement
| GlobalStatement
| StaticStatement
| UnsetStatement
| GotoStatement
| LabelStatement
| IfStatement
| WhileStatement
| DoWhileStatement
| ForStatement
| ForeachStatement
| SwitchStatement
| TryStatement
| DeclareStatement
| FunctionDeclaration
| ConstantDeclaration
| NamespaceDeclaration
| UseDeclaration
| ClassDeclaration
| InterfaceDeclaration
| TraitDeclaration
| EnumDeclaration

Type =
  NamedType
| NullableType
| UnionType
| IntersectionType
| ParenthesizedType

MemberDeclaration =
  PropertyDeclaration
| MethodDeclaration
| ConstantDeclaration
| TraitUseClause
| EnumCase

StringInterpolation =
  SimpleInterpolation
| BraceInterpolation
| DollarBraceInterpolation

TraitAdaptation =
  TraitPrecedence
| TraitAlias

//**********************//
//      Source file     //
//**********************//

/// The root node: one parsed PHP file. Open and close tags and inline
/// HTML sit between the statements as bare tokens.
SourceFile = Statement*

//**********************//
//      Expressions     //
//**********************//

/// A literal expression: integer, float, or single-quoted string.
Literal = value:('integer_literal' | 'float_literal' | 'single_quoted_string')

/// A `$variable` used as an expression.
VariableReference = name:'variable'

/// `$$name` and `${expression}`.
DynamicVariableExpression = '$' '{'? Expression? '}'?

/// `( expression )`.
ParenthesizedExpression = '(' Expression ')'

/// One binary operation: left operand, operator token, right operand.
/// The operator token distinguishes `+` from `instanceof` from `|>`.
BinaryExpression =
  lhs:Expression
  operator:(
    'or' | 'xor' | 'and' | '??' | '||' | '&&' | '|' | '^' | '&'
  | '==' | '!=' | '===' | '!==' | '<=>' | '<' | '<=' | '>' | '>='
  | '|>' | '.' | '<<' | '>>' | '+' | '-' | '*' | '/' | '%'
  | 'instanceof' | '**'
  )
  rhs:Expression

/// A prefix operation: `!`, `~`, unary `+`/`-`, `@`, `++`, `--`.
PrefixExpression =
  operator:('!' | '~' | '+' | '-' | '@' | '++' | '--')
  operand:Expression

/// A postfix operation: `++`, `--`.
PostfixExpression = operand:Expression operator:('++' | '--')

/// A cast: the single cast token, then the operand.
CastExpression =
  operator:('(int)' | '(bool)' | '(float)' | '(string)' | '(binary)' | '(array)' | '(object)')
  operand:Expression

/// `condition ? middle : third`; the short form `?:` has no middle, so
/// position cannot assign roles and the accessors are hand-written.
TernaryExpression = Expression '?' Expression? ':' Expression?

/// `target = value` and the compound forms; `= &value` keeps its
/// ampersand as a token child. Whether the target is assignable is
/// a semantic judgment.
AssignmentExpression =
  target:Expression
  operator:(
    '=' | '+=' | '-=' | '*=' | '/=' | '.=' | '%=' | '**='
  | '&=' | '|=' | '^=' | '<<=' | '>>=' | '??='
  )
  by_reference:'&'?
  value:Expression

/// A possibly-qualified name: `Foo`, `Foo\Bar`, `\Foo`, `namespace\Foo`.
Name = 'namespace'? 'backslash'? 'identifier' ('backslash' 'identifier')*

/// A name used as an expression: a constant fetch or a callee. Also
/// `static` as a scoped-access subject, a bare keyword token with no
/// Name child.
NameExpression = Name? static_keyword:'static'?

/// `( argument, ... )`, including the lone `...` of a first-class
/// callable, which sits directly in the list.
ArgumentList = '(' (Argument (',' Argument)*)? ellipsis:'...'? ')'

/// One argument: optional `label:`, optional `...`, optional `&`,
/// then the expression. The label is semi-reserved (any keyword), so
/// its accessor is hand-written.
Argument = 'identifier'? ':'? spread:'...'? '&'? Expression?

/// A call: the callee expression, then its argument list.
CallExpression = callee:Expression ArgumentList

/// `subject->name` and `subject?->name`.
MemberAccessExpression = subject:Expression operator:('->' | '?->') MemberName

/// `subject::name`: constants, methods, static properties, `::class`.
ScopedAccessExpression = subject:Expression '::' MemberName

/// The name after `->`, `?->`, or `::`: identifier, any keyword
/// (semi-reserved, hand-written accessor), variable, or
/// `{ expression }`.
MemberName = 'identifier'? '{'? Expression? '}'?

/// `subject[index]`; the index is absent in the push form `$a[]`.
IndexExpression = subject:Expression '[' index:Expression? ']'

/// `[ elements ]` or `array( elements )`; also the destructuring
/// target shape. Empty destructuring slots keep their commas as
/// direct children.
ArrayExpression = 'array'? ('[' | '(') (ArrayElement (',' ArrayElement)*)? (']' | ')')

/// One element: optional `...`, optional `&`, expression, then
/// optionally `=>` (optional `&`) expression. `[value]` and
/// `[key => value]` put a different role at the first position, so the
/// accessors are hand-written.
ArrayElement = '...'? '&'? Expression ('=>' '&'? Expression)?

/// `list( elements )`, the keyword destructuring form.
ListExpression = 'list' '(' (ArrayElement (',' ArrayElement)*)? ')'

/// `"..."` with fragments and interpolations.
InterpolatedString = '"' ('string_fragment' | StringInterpolation)* '"'

/// A heredoc or nowdoc, start to end label.
HeredocExpression = 'heredoc_start' ('string_fragment' | StringInterpolation)* 'heredoc_end'

/// A backtick string: shell execution.
ShellExecExpression = '`' ('string_fragment' | StringInterpolation)* '`'

/// `$name`, `$name->property`, `$name[offset]` inside a string.
SimpleInterpolation =
  name:'variable'
  ('->' | '?->')?
  '['? '-'? ('integer_literal' | 'identifier' | 'variable')? ']'?

/// `{ expression }` inside a string.
BraceInterpolation = '{' Expression '}'

/// `${ ... }` inside a string, the deprecated form.
DollarBraceInterpolation = '${' Expression '}'

/// `new` with a class reference and optional constructor arguments.
/// The reference is one of: a Name (`new Foo(1)`), the bare `static`
/// keyword, a variable or parenthesized expression, or an anonymous
/// ClassDeclaration (whose own constructor arguments sit inside it).
NewExpression =
  'new' static_keyword:'static'? Name? Expression? ClassDeclaration? ArgumentList?

/// `clone value` or the 8.5 function form `clone(...)`.
CloneExpression = 'clone' operand:Expression? ArgumentList?

/// `isset( arguments )`.
IssetExpression = 'isset' ArgumentList

/// `empty( argument )`.
EmptyExpression = 'empty' ArgumentList

/// `eval( argument )`.
EvalExpression = 'eval' ArgumentList

/// `exit` / `die`, with an optional argument list since 8.4.
ExitExpression = 'exit' ArgumentList?

/// `print operand`.
PrintExpression = 'print' operand:Expression

/// `throw operand`, an expression since PHP 8.0.
ThrowExpression = 'throw' operand:Expression

/// `yield`, `yield value`, `yield key => value`, `yield from source`.
/// `yield $v` puts the value first while `yield $k => $v` puts the key
/// first, so the accessors are hand-written.
YieldExpression = ('yield' | 'yield from') Expression? '=>'? Expression?

/// `include`, `include_once`, `require`, `require_once`; the
/// keyword token distinguishes them.
IncludeExpression =
  operator:('include' | 'include_once' | 'require' | 'require_once')
  operand:Expression

/// `match ( subject ) { arms }`.
MatchExpression = 'match' '(' subject:Expression ')' '{' (MatchArm (',' MatchArm)*)? '}'

/// One arm: a condition list (or `default`), `=>`, the body. The body
/// is the expression after the arrow; position alone cannot separate
/// it from the conditions, so the accessors are hand-written.
MatchArm = 'default'? (Expression (',' Expression)*)? '=>' Expression

/// `function (...) use (...) { ... }`, optionally `static`, with an
/// optional by-reference `&` and return type.
ClosureExpression =
  AttributeGroup* static_keyword:'static'? 'function' by_reference:'&'?
  ParameterList ClosureUseClause? (':' return_type:Type)? Block

/// `fn (...) => expression`, optionally `static`.
ArrowFunctionExpression =
  AttributeGroup* static_keyword:'static'? 'fn' by_reference:'&'?
  ParameterList (':' return_type:Type)? '=>' body:Expression

/// `( parameter, ... )`.
ParameterList = '(' (Parameter (',' Parameter)*)? ')'

/// One parameter: optional type, `&`, `...`, the variable, and an
/// optional default. Constructor promotion admits the full member
/// modifier set; which modifiers are legal here is semantic.
Parameter =
  AttributeGroup*
  ('public' | 'protected' | 'private' | 'static' | 'abstract' | 'final' | 'readonly' | 'var')*
  Type? by_reference:'&'? variadic:'...'? name:'variable'
  ('=' default_value:Expression)?
  PropertyHookList?

/// `use ( variables )` on a closure.
ClosureUseClause = 'use' '(' ('&'? VariableReference (',' '&'? VariableReference)*)? ')'

//**********************//
//      Statements      //
//**********************//

/// `{ statements }`.
Block = '{' Statement* '}'

/// A lone `;`.
EmptyStatement = ';'

/// `echo expression, expression;`.
EchoStatement = 'echo' (Expression (',' Expression)*)? ';'?

/// An expression used as a statement, terminator included.
ExpressionStatement = Expression ';'?

/// `return;` or `return expression;`.
ReturnStatement = 'return' Expression? ';'?

/// `break;` or `break level;`; level validity is semantic.
BreakStatement = 'break' Expression? ';'?

/// `continue;` or `continue level;`; level validity is semantic.
ContinueStatement = 'continue' Expression? ';'?

/// `global $a, $b;`.
GlobalStatement = 'global' (Expression (',' Expression)*)? ';'?

/// `static $a = 1, $b;`, the function-static declaration.
StaticStatement = 'static' (StaticVariable (',' StaticVariable)*)? ';'?

/// One declared static: the variable and its optional initializer.
StaticVariable = name:'variable' ('=' Expression)?

/// `unset( targets );`.
UnsetStatement = 'unset' ArgumentList ';'?

/// `goto label;`; whether the label exists is semantic.
GotoStatement = 'goto' label:'identifier' ';'?

/// `label:`, the target of a `goto`.
LabelStatement = name:'identifier' ':'

/// `if (condition) body`, with optional `ElseIfClause`s and one
/// optional `ElseClause`, in either classic or alternative syntax. The
/// classic body is a single statement child; the alternative body is
/// the statement list before the clauses.
IfStatement =
  'if' '(' condition:Expression ')'
  ':'? Statement* ElseIfClause* ElseClause? 'endif'? ';'?

/// `elseif (condition) body` (or its alternative-syntax form).
ElseIfClause = 'elseif' '(' condition:Expression ')' ':'? Statement*

/// `else body` (or its alternative-syntax form).
ElseClause = 'else' ':'? Statement*

/// `while (condition) body`, either syntax.
WhileStatement = 'while' '(' condition:Expression ')' ':'? Statement* 'endwhile'? ';'?

/// `do body while (condition);`.
DoWhileStatement = 'do' body:Statement? 'while' '(' condition:Expression ')' ';'?

/// `for (initializers; condition; updates) body`, either syntax.
ForStatement =
  'for' '('
    initializers:ForExpressionList? ';'
    condition:ForExpressionList? ';'
    updates:ForExpressionList?
  ')'
  ':'? Statement* 'endfor'? ';'?

/// One of `for`'s three sections: a possibly-empty comma-separated
/// expression list, always present as a node so the sections stay
/// addressable by position.
ForExpressionList = (Expression (',' Expression)*)?

/// `foreach (subject as key => value) body`, either syntax; the
/// `=>` separates the optional key target from the value target, so
/// position alone cannot assign roles and the accessors are
/// hand-written.
ForeachStatement =
  'foreach' '(' Expression 'as' '&'? Expression? '=>'? '&'? Expression? ')'
  ':'? Statement* 'endforeach'? ';'?

/// `switch (subject) { cases }`, either syntax.
SwitchStatement =
  'switch' '(' condition:Expression ')'
  ('{' | ':') ';'? SwitchCase* ('}' | 'endswitch') ';'?

/// One `case expression:` or `default:` section, its statements
/// included; the body ends where the next section (or the switch)
/// begins, so an empty body is a fallthrough.
SwitchCase = keyword:('case' | 'default') condition:Expression? (':' | ';') Statement*

/// `try block`, then catch clauses and an optional finally.
TryStatement = 'try' Block CatchClause* FinallyClause?

/// `catch (Type | Type $variable) block`; the variable is optional
/// since PHP 8.0.
CatchClause = 'catch' '(' (Name ('|' Name)*)? VariableReference? ')' Block

/// `finally block`.
FinallyClause = 'finally' Block

/// `declare( directives ) body`, either syntax; the body may be a
/// lone `;` (an empty statement). A classic body of `?>` leaves a bare
/// CloseTag token with no statement-node child, so the statement
/// accessor yields nothing there.
DeclareStatement =
  'declare' '(' (DeclareDirective (',' DeclareDirective)*)? ')'
  ':'? Statement* 'enddeclare'? ';'?

/// One `name = value` directive; which names and values are legal
/// is semantic.
DeclareDirective = name:'identifier' '=' value:Expression

//**********************//
//     Declarations     //
//**********************//

/// `function name(parameters): type { body }`, the top-level form;
/// methods are `MethodDeclaration`.
FunctionDeclaration =
  AttributeGroup* 'function' by_reference:'&'? name:'identifier'
  ParameterList (':' return_type:Type)? Block

/// One named type: a qualified `Name`, or a keyword type token
/// (`array`, `callable`, `static`, and permissively any keyword)
/// sitting bare. One concept in two shapes; the hand-written
/// `name_or_keyword()` extension unifies them.
NamedType = Name? ('array' | 'callable' | 'static')?

/// `?type`.
NullableType = '?' Type?

/// `A|B|C`, one flat node for the whole chain.
UnionType = Type ('|' Type)*

/// `A&B&C`, one flat node for the whole chain.
IntersectionType = Type ('&' Type)*

/// `( type )` inside a type: the DNF grouping form.
ParenthesizedType = '(' Type? ')'

/// `const FOO = 1, BAR = 2;`, optionally typed (8.3), at the top
/// level or as a class member (with modifiers).
ConstantDeclaration =
  AttributeGroup*
  ('public' | 'protected' | 'private' | 'static' | 'abstract' | 'final' | 'readonly' | 'var')*
  'const' Type? (ConstantElement (',' ConstantElement)*)? ';'?

/// One `name = value` element of a constant declaration. The name is
/// semi-reserved (`const FOR = 1;`), so its accessor is hand-written.
ConstantElement = 'identifier'? '=' value:Expression

/// `namespace A\B;` or `namespace A\B { ... }` or `namespace { ... }`.
NamespaceDeclaration = 'namespace' Name? Block? ';'?

/// `use A\B;` and every import shape: aliases, `function`/`const`
/// types, clause lists, group imports.
UseDeclaration =
  'use' import_type:('function' | 'const')? (UseClause (',' UseClause)*)? ';'?

/// One imported name: optional per-item `function`/`const` type
/// (inside groups), the name, an optional group or alias. The alias is
/// semi-reserved, so its accessor is hand-written.
UseClause = import_type:('function' | 'const')? Name UseGroup? 'as'? 'identifier'?

/// `\{ items }` of a grouped import.
UseGroup = 'backslash' '{' (UseClause (',' UseClause)*)? '}'

/// `class Name extends B implements C, D { members }`, with
/// optional `abstract` / `final` / `readonly` modifiers. Anonymous
/// classes (`new class(...) { ... }`) share this kind and simply
/// have no name; their constructor arguments sit before the
/// heritage clauses. The name is semi-reserved (`class List {}`
/// parses), so its accessor is hand-written.
ClassDeclaration =
  AttributeGroup* ('abstract' | 'final' | 'readonly')* 'class' 'identifier'?
  ArgumentList? ExtendsClause? ImplementsClause? MemberList

/// `interface Name extends A, B { members }`. The name is
/// semi-reserved, so its accessor is hand-written.
InterfaceDeclaration =
  AttributeGroup* 'interface' 'identifier'?
  ExtendsClause? ImplementsClause? MemberList

/// `trait Name { members }`. Heritage clauses parse permissively on
/// traits; their legality is semantic. The name is semi-reserved, so
/// its accessor is hand-written.
TraitDeclaration =
  AttributeGroup* 'trait' 'identifier'?
  ExtendsClause? ImplementsClause? MemberList

/// `extends` and its comma-separated names.
ExtendsClause = 'extends' (Name (',' Name)*)?

/// `implements` and its comma-separated names.
ImplementsClause = 'implements' (Name (',' Name)*)?

/// `{ members }` of a class-like body.
MemberList = '{' MemberDeclaration* '}'

/// `public int $a = 1, $b;`: modifiers, optional type, then the
/// declarator elements.
PropertyDeclaration =
  AttributeGroup*
  ('public' | 'protected' | 'private' | 'static' | 'abstract' | 'final' | 'readonly' | 'var')*
  Type? (PropertyElement (',' PropertyElement)*)? ';'?

/// One `$name [= initializer]` element; a hooked property carries
/// its `PropertyHookList` here.
PropertyElement = name:'variable' ('=' Expression)? PropertyHookList?

/// `function name(parameters): type { body }` (or `;` for the
/// abstract and interface forms) as a class member, modifiers
/// included. The name is semi-reserved (`public function list()`), so
/// its accessor is hand-written.
MethodDeclaration =
  AttributeGroup*
  ('public' | 'protected' | 'private' | 'static' | 'abstract' | 'final' | 'readonly' | 'var')*
  'function' by_reference:'&'? 'identifier'?
  ParameterList (':' return_type:Type)? Block? ';'?

/// `use TraitA, TraitB;` inside a class body, with an optional
/// adaptation list instead of the semicolon.
TraitUseClause = 'use' (Name (',' Name)*)? TraitAdaptationList? ';'?

/// `{ adaptations }` of a trait use.
TraitAdaptationList = '{' TraitAdaptation* '}'

/// `A::member insteadof B, C;`. The reference name, the semi-reserved
/// member, and the excluded names share types, so the accessors are
/// hand-written.
TraitPrecedence = Name? '::'? 'identifier'? 'insteadof' (Name (',' Name)*)? ';'?

/// `[A::]member as [visibility] [name];`. The member and the alias are
/// semi-reserved and position-dependent, so the accessors are
/// hand-written.
TraitAlias =
  Name? '::'? 'identifier'? 'as'
  ('public' | 'protected' | 'private')? 'identifier'? ';'?

/// `enum Name: BackingType implements A { cases and members }`.
EnumDeclaration =
  AttributeGroup* 'enum' name:'identifier' (':' backing_type:Type)?
  ExtendsClause? ImplementsClause? MemberList

/// `case Name;` or `case Name = expression;`. Case names are
/// semi-reserved, so the accessor is hand-written.
EnumCase = AttributeGroup* 'case' 'identifier'? ('=' value:Expression)? ';'?

/// `{ get; set(...) { ... } }` on a property or a promoted
/// parameter (8.4).
PropertyHookList = '{' PropertyHook* '}'

/// One hook: optional `final`, optional `&`, the name, an optional
/// parameter list, then `;`, `=> expression;`, or a block. Hook names
/// are semi-reserved in practice, so the accessor is hand-written.
PropertyHook =
  AttributeGroup* modifier:'final'? by_reference:'&'? 'identifier'?
  ParameterList? '=>'? body:Expression? Block? ';'?

/// `#[Attribute(arguments), Other]`: one bracketed group.
AttributeGroup = '#[' (Attribute (',' Attribute)*)? ']'

/// One attribute inside a group: a name and optional arguments.
Attribute = Name ArgumentList?
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p xtask`
Expected: PASS. If `the_grammar_covers_every_node_kind` disagrees on the count, list the loaded names against the enum's node section (`SourceFile` through `Attribute` in `syntax_kind.rs`, `ErrorNode` excluded) and fix the file, not the number, unless a rule was genuinely mistyped.

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax/php.ungram xtask/src/codegen.rs xtask/tests/grammar_file.rs
git commit -m "✨ feat(syntax): describe every node shape in php.ungram"
```

---

### Task 5: Generate `SyntaxKind` and migrate the hand-written enum

**Files:**
- Create: `xtask/src/codegen/emit_kinds.rs` (tests inline)
- Modify: `xtask/src/codegen.rs` (module, `artifacts()`, `reformat()`)
- Create: `xtask/tests/sourcegen.rs`
- Create: `crates/celerrate_syntax/src/syntax_kind/generated.rs` (by running the generator)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs`

**Interfaces:**
- Consumes: `grammar::{load, GrammarSource}`, `tokens::TOKEN_KINDS`, `codegen::php_ungram_source` (Tasks 2 to 4).
- Produces: `emit_kinds::syntax_kind_file(&GrammarSource) -> String`, `codegen::reformat(&str) -> Result<String>`, a populated `codegen::artifacts()`, and the committed `generated.rs`. The public `SyntaxKind` API is unchanged: same variants, same classifiers, `from_raw`/`into_raw` now live in the generated file. The **whole existing test suite passing unchanged is the migration proof**.

The one observable change: node-kind discriminants reorder (nodes now follow `php.ungram`'s interning order, and `ErrorNode` moves to the end). Nothing may depend on node discriminant values; the existing pins only require tokens before nodes and keyword contiguity, both preserved.

- [ ] **Step 1: Write the failing emission test**

Create `xtask/src/codegen/emit_kinds.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::syntax_kind_file;
    use crate::codegen::grammar::load;

    #[test]
    fn the_emission_contains_tokens_nodes_and_the_raw_conversion() {
        let grammar = load("/// The root.\nRoot = Item*\nItem = 'identifier'").expect("loads");
        let text = syntax_kind_file(&grammar);
        // Token section, from the table, docs included.
        assert!(text.contains("    Whitespace,\n"));
        assert!(text.contains("/// `exit` and its alias `die`."));
        // Node section, from the grammar, docs included, ErrorNode last.
        assert!(text.contains("/// The root.\n    Root,\n"));
        let root_position = text.find("    Root,").expect("Root emitted");
        let error_node_position = text.find("    ErrorNode,").expect("ErrorNode emitted");
        assert!(root_position < error_node_position);
        // The raw conversion and its backing list.
        assert!(text.contains("const ALL: &[SyntaxKind]"));
        assert!(text.contains("pub fn from_raw(raw: u16) -> Option<Self>"));
        assert!(text.contains("SyntaxKind::Root,"));
        // The do-not-edit banner.
        assert!(text.starts_with("//! Generated by `cargo xtask codegen`"));
    }
}
```

Add to `xtask/src/codegen.rs`, with the other modules:

```rust
pub mod emit_kinds;
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p xtask`
Expected: compile FAIL (`syntax_kind_file` does not exist).

- [ ] **Step 3: Implement the emission**

Production half of `xtask/src/codegen/emit_kinds.rs`:

```rust
//! Emits `crates/celerrate_syntax/src/syntax_kind/generated.rs`: the
//! whole `SyntaxKind` enum (tokens from the table, nodes from
//! `php.ungram`, `ErrorNode` appended) plus the raw `u16` conversion.
//! The single variant list drives both the enum and `ALL`, so the
//! discriminant order and the conversion can never drift apart.

use std::fmt::Write as _;

use super::grammar::GrammarSource;
use super::tokens::TOKEN_KINDS;

pub fn syntax_kind_file(grammar: &GrammarSource) -> String {
    let mut variants: Vec<(String, Vec<String>)> = Vec::new();
    for definition in TOKEN_KINDS {
        variants.push((
            definition.variant.to_owned(),
            definition
                .documentation
                .iter()
                .map(|line| (*line).to_owned())
                .collect(),
        ));
    }
    let first_node = variants.len();
    for node in &grammar.nodes {
        variants.push((node.name.clone(), node.documentation.clone()));
    }
    variants.push((
        "ErrorNode".to_owned(),
        vec!["Recovery wreckage: tokens no grammar rule accepted.".to_owned()],
    ));

    let mut text = String::new();
    text.push_str(
        "//! Generated by `cargo xtask codegen`; do not edit by hand.\n\
         //! Token kinds come from xtask's token table, node kinds from\n\
         //! `php.ungram` (`ErrorNode` appended by the generator).\n\n",
    );
    text.push_str(
        "/// Every kind of token and node in PHP syntax.\n\
         ///\n\
         /// One vocabulary shared by the whole syntax layer, `#[repr(u16)]`\n\
         /// so the rowan tree stores it directly. Token kinds first, node\n\
         /// kinds after them.\n\
         ///\n\
         /// Keywords each get their own kind, resolved case-insensitively by\n\
         /// the lexer. Semi-reserved uses (`$object->list()`, `const FOR = 1;`,\n\
         /// `enum` as a plain name) are the parser's business: it re-treats\n\
         /// keyword kinds as identifiers where the grammar allows. `true`,\n\
         /// `false`, `null`, `self`, `parent`, and the magic constants are\n\
         /// plain identifiers, resolved semantically.\n",
    );
    text.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]\n");
    text.push_str("#[repr(u16)]\n");
    text.push_str("pub enum SyntaxKind {\n");
    for (position, (variant, documentation)) in variants.iter().enumerate() {
        if position == first_node {
            text.push_str("    // Node kinds, owned by php.ungram.\n");
        }
        for line in documentation {
            let _ = writeln!(text, "    /// {line}");
        }
        let _ = writeln!(text, "    {variant},");
    }
    text.push_str("}\n\n");
    text.push_str("impl SyntaxKind {\n");
    text.push_str(
        "    /// Every kind, in declaration (and therefore discriminant) order.\n",
    );
    text.push_str("    const ALL: &[SyntaxKind] = &[\n");
    for (variant, _documentation) in &variants {
        let _ = writeln!(text, "        SyntaxKind::{variant},");
    }
    text.push_str("    ];\n\n");
    text.push_str(
        "    /// The inverse of [`SyntaxKind::into_raw`]. Total and panic-free:\n\
         \x20   /// out-of-range values return `None`.\n\
         \x20   pub fn from_raw(raw: u16) -> Option<Self> {\n\
         \x20       Self::ALL.get(usize::from(raw)).copied()\n\
         \x20   }\n\n\
         \x20   /// The `u16` the tree stores; the discriminant.\n\
         \x20   pub fn into_raw(self) -> u16 {\n\
         \x20       self as u16\n\
         \x20   }\n\
         }\n",
    );
    text
}
```

(The `\x20` escapes keep rustfmt from eating the source string's leading spaces; the *emitted* text has ordinary indentation. If the escaping fights you, build those lines with `writeln!` like the others; only the emitted bytes matter.)

- [ ] **Step 4: Wire `artifacts()` and `reformat`**

In `xtask/src/codegen.rs`, replace the empty `artifacts()` and add `reformat`:

```rust
/// Every artifact, in a fixed order, rustfmt-formatted.
pub fn artifacts() -> Result<Vec<Artifact>> {
    let text = php_ungram_source()?;
    let grammar = grammar::load(&text)?;
    Ok(vec![Artifact {
        relative_path: PathBuf::from("crates/celerrate_syntax/src/syntax_kind/generated.rs"),
        text: reformat(&emit_kinds::syntax_kind_file(&grammar))?,
    }])
}

/// Pipes generated text through rustfmt so the committed artifacts are
/// byte-stable under `cargo fmt --check` and the freshness comparison.
/// The toolchain pin (`rust-toolchain.toml`) fixes the rustfmt version.
fn reformat(text: &str) -> Result<String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("rustfmt is not runnable: {error}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!("rustfmt rejected generated code:\n{text}").into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
```

Run: `cargo test -p xtask`
Expected: PASS (the emission test and everything before it).

- [ ] **Step 5: Write the failing freshness test**

Create `xtask/tests/sourcegen.rs`:

```rust
#![allow(clippy::expect_used, clippy::unwrap_used)]

/// The committed generated files match what the generator produces
/// today. Regenerate with `cargo xtask codegen` when this fails.
#[test]
fn generated_sources_are_fresh() {
    let root = xtask::workspace_root().expect("workspace root");
    let artifacts = xtask::codegen::artifacts().expect("generation succeeds");
    assert!(!artifacts.is_empty());
    for artifact in artifacts {
        let path = root.join(&artifact.relative_path);
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk,
            artifact.text,
            "{} is stale: run `cargo xtask codegen` and commit the result",
            artifact.relative_path.display()
        );
    }
}
```

Run: `cargo test -p xtask --test sourcegen`
Expected: FAIL (the file on disk does not exist yet).

- [ ] **Step 6: Generate the file**

Run: `cargo xtask codegen`
Expected: `wrote crates/celerrate_syntax/src/syntax_kind/generated.rs`. The file is not yet referenced by any module, so the workspace still compiles against the old hand-written enum.

Run: `cargo test -p xtask --test sourcegen`
Expected: PASS.

- [ ] **Step 7: Swap `syntax_kind.rs` onto the generated enum**

Replace the macro, the `syntax_kinds! { ... }` invocation, and the `from_raw`/`into_raw` impl in `crates/celerrate_syntax/src/syntax_kind.rs`; keep the classifiers. The whole file becomes:

```rust
//! `SyntaxKind` and its classifiers. The enum itself (token and node
//! variants, the raw `u16` conversion) is generated: token kinds from
//! xtask's token table, node kinds from `php.ungram`. Regenerate with
//! `cargo xtask codegen`; a sourcegen test keeps the committed file
//! fresh. The classifiers below are hand-written because they encode
//! lexer policy (trivia, the keyword table), not grammar shape.

mod generated;

pub use generated::SyntaxKind;

/// The longest PHP keywords are `include_once` and `require_once`,
/// tied at twelve bytes.
const LONGEST_KEYWORD_LENGTH: usize = 12;

impl SyntaxKind {
    /// Whether this token carries no syntactic meaning (whitespace,
    /// comments, shebang). Trivia stay in the stream; this classifier is
    /// how upper layers skip them.
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace
                | Self::LineComment
                | Self::BlockComment
                | Self::DocComment
                | Self::Shebang
        )
    }

    /// Whether this kind is a PHP keyword. Relies on the keyword section
    /// being contiguous in the generated declaration, `Abstract` through
    /// `YieldFrom`; the token table preserves that layout and the
    /// classifier test pins it.
    pub fn is_keyword(self) -> bool {
        (Self::Abstract..=Self::YieldFrom).contains(&self)
    }

    /// Resolves a keyword case-insensitively, allocation-free. Returns
    /// `None` when the text is not a PHP keyword.
    pub fn from_keyword(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.is_empty() || bytes.len() > LONGEST_KEYWORD_LENGTH {
            return None;
        }
        let mut buffer = [0u8; LONGEST_KEYWORD_LENGTH];
        let slots = buffer.get_mut(..bytes.len())?;
        for (slot, byte) in slots.iter_mut().zip(bytes) {
            *slot = byte.to_ascii_lowercase();
        }
        let lowered = core::str::from_utf8(buffer.get(..bytes.len())?).ok()?;
        let kind = match lowered {
            "abstract" => Self::Abstract,
            "and" => Self::And,
            "array" => Self::Array,
            "as" => Self::As,
            "break" => Self::Break,
            "callable" => Self::Callable,
            "case" => Self::Case,
            "catch" => Self::Catch,
            "class" => Self::Class,
            "clone" => Self::Clone,
            "const" => Self::Const,
            "continue" => Self::Continue,
            "declare" => Self::Declare,
            "default" => Self::Default,
            "die" => Self::Exit,
            "do" => Self::Do,
            "echo" => Self::Echo,
            "else" => Self::Else,
            "elseif" => Self::ElseIf,
            "empty" => Self::Empty,
            "enddeclare" => Self::EndDeclare,
            "endfor" => Self::EndFor,
            "endforeach" => Self::EndForeach,
            "endif" => Self::EndIf,
            "endswitch" => Self::EndSwitch,
            "endwhile" => Self::EndWhile,
            "enum" => Self::Enum,
            "eval" => Self::Eval,
            "exit" => Self::Exit,
            "extends" => Self::Extends,
            "final" => Self::Final,
            "finally" => Self::Finally,
            "fn" => Self::Fn,
            "for" => Self::For,
            "foreach" => Self::Foreach,
            "function" => Self::Function,
            "global" => Self::Global,
            "goto" => Self::Goto,
            "if" => Self::If,
            "implements" => Self::Implements,
            "include" => Self::Include,
            "include_once" => Self::IncludeOnce,
            "instanceof" => Self::InstanceOf,
            "insteadof" => Self::InsteadOf,
            "interface" => Self::Interface,
            "isset" => Self::Isset,
            "list" => Self::List,
            "match" => Self::Match,
            "namespace" => Self::Namespace,
            "new" => Self::New,
            "or" => Self::Or,
            "print" => Self::Print,
            "private" => Self::Private,
            "protected" => Self::Protected,
            "public" => Self::Public,
            "readonly" => Self::Readonly,
            "require" => Self::Require,
            "require_once" => Self::RequireOnce,
            "return" => Self::Return,
            "static" => Self::Static,
            "switch" => Self::Switch,
            "throw" => Self::Throw,
            "trait" => Self::Trait,
            "try" => Self::Try,
            "unset" => Self::Unset,
            "use" => Self::Use,
            "var" => Self::Var,
            "while" => Self::While,
            "xor" => Self::Xor,
            "yield" => Self::Yield,
            _ => return None,
        };
        Some(kind)
    }
}
```

(The classifier bodies are today's, unchanged; only the module documentation, the `mod generated;` line, and the removal of the enum, the macro, `ALL`, `from_raw`, and `into_raw` are new.)

- [ ] **Step 8: Prove the migration with the whole suite**

Run: `cargo test --workspace`
Expected: PASS with **zero snapshot changes**. Every one of the existing tests (the syntax kind pins, all grammar snapshots, the corpus) compiling and passing against the generated enum is the proof that `php.ungram` and the token table own the exact same kind set the parser uses. A missing variant fails compilation; a renamed one fails compilation; a lost doc only shows in review.

Run: `cargo fmt --all --check` (the generated file must already be formatted; if this fails, `reformat` is not being applied), then `cargo clippy --workspace --all-targets -- -D warnings`.
Expected: all clean.

- [ ] **Step 9: Commit**

```bash
git add xtask/src/codegen.rs xtask/src/codegen/emit_kinds.rs xtask/tests/sourcegen.rs crates/celerrate_syntax/src/syntax_kind.rs crates/celerrate_syntax/src/syntax_kind/generated.rs
git commit -m "♻️ refactor(syntax): generate SyntaxKind from php.ungram and the token table"
```

---

### Task 6: The typed AST infrastructure

**Files:**
- Create: `crates/celerrate_syntax/src/ast.rs` (tests inline)
- Create: `crates/celerrate_syntax/src/ast/support.rs`
- Modify: `crates/celerrate_syntax/src/tree.rs` (the `SyntaxNodePtr` alias, plus a test)
- Modify: `crates/celerrate_syntax/src/lib.rs`

**Interfaces:**
- Consumes: the existing tree aliases and `parse`.
- Produces: `ast::AstNode` (`can_cast(SyntaxKind) -> bool`, `cast(SyntaxNode) -> Option<Self>`, `syntax(&self) -> &SyntaxNode`), `ast::AstChildren<N>` (an `Iterator<Item = N>`), `ast::support::{child, children, token}` (crate-private), and the public `SyntaxNodePtr` alias. Task 7's generated code calls exactly these.

- [ ] **Step 1: Write the failing infrastructure tests**

Create `crates/celerrate_syntax/src/ast.rs` with the test module first (the trait does not exist yet):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{AstNode, support};
    use crate::syntax_kind::SyntaxKind;
    use crate::tree::SyntaxNode;

    // Hand-rolled views: the generated code arrives in the next task;
    // the infrastructure must already carry any conforming type.
    struct EchoStatementView {
        syntax: SyntaxNode,
    }

    impl AstNode for EchoStatementView {
        fn can_cast(kind: SyntaxKind) -> bool {
            kind == SyntaxKind::EchoStatement
        }
        fn cast(syntax: SyntaxNode) -> Option<Self> {
            Self::can_cast(syntax.kind()).then(|| Self { syntax })
        }
        fn syntax(&self) -> &SyntaxNode {
            &self.syntax
        }
    }

    struct LiteralView {
        syntax: SyntaxNode,
    }

    impl AstNode for LiteralView {
        fn can_cast(kind: SyntaxKind) -> bool {
            kind == SyntaxKind::Literal
        }
        fn cast(syntax: SyntaxNode) -> Option<Self> {
            Self::can_cast(syntax.kind()).then(|| Self { syntax })
        }
        fn syntax(&self) -> &SyntaxNode {
            &self.syntax
        }
    }

    #[test]
    fn casting_children_and_tokens_work_over_a_real_parse() {
        let parse = crate::parse::parse("<?php echo 1, 2;");
        let root = parse.tree();
        let echo: EchoStatementView = support::child(&root, 0).expect("an echo statement");
        let literals: Vec<LiteralView> = support::children(echo.syntax()).collect();
        assert_eq!(literals.len(), 2, "typed children skip tokens and trivia");
        let second: LiteralView = support::child(echo.syntax(), 1).expect("a second literal");
        assert_eq!(second.syntax().text().to_string(), "2");
        let keyword =
            support::token(echo.syntax(), &[SyntaxKind::Echo]).expect("the echo keyword");
        assert_eq!(keyword.text(), "echo");
        assert!(
            LiteralView::cast(parse.tree()).is_none(),
            "a kind mismatch refuses the cast"
        );
    }
}
```

- [ ] **Step 2: Run and watch it fail**

The module is not declared yet, so first add to `crates/celerrate_syntax/src/lib.rs`, after the existing `mod` items:

```rust
pub mod ast;
```

Run: `cargo test -p celerrate_syntax`
Expected: compile FAIL (`AstNode`, `support` missing).

- [ ] **Step 3: Implement the trait, the iterator, and the support helpers**

Production half of `crates/celerrate_syntax/src/ast.rs` (above the test module):

```rust
//! The typed AST: typed, zero-cost views over the concrete syntax
//! tree. The structs, enums, and accessors are generated from
//! `php.ungram` (`generated.rs`, `cargo xtask codegen`); logic a
//! generator cannot express (semi-reserved names, position-dependent
//! roles) is hand-written in `extensions.rs`. Every accessor returns
//! `Option` or an iterator: the partial trees error recovery produces
//! are normal citizens, not special cases.

pub(crate) mod support;

use std::marker::PhantomData;

use crate::syntax_kind::SyntaxKind;
use crate::tree::{PhpLanguage, SyntaxNode};

/// A typed view over a syntax node of a known kind. Views are cheap:
/// one red-node handle, no copying.
pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(syntax: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

/// The typed children of one node, in source order; children of other
/// types (and wreckage) are skipped.
#[derive(Debug, Clone)]
pub struct AstChildren<N> {
    inner: rowan::SyntaxNodeChildren<PhpLanguage>,
    node_type: PhantomData<N>,
}

impl<N> AstChildren<N> {
    pub(crate) fn new(parent: &SyntaxNode) -> Self {
        AstChildren {
            inner: parent.children(),
            node_type: PhantomData,
        }
    }
}

impl<N: AstNode> Iterator for AstChildren<N> {
    type Item = N;

    fn next(&mut self) -> Option<N> {
        self.inner.by_ref().find_map(N::cast)
    }
}
```

Create `crates/celerrate_syntax/src/ast/support.rs`:

```rust
//! Accessor plumbing shared by the generated code and the extensions.

use super::{AstChildren, AstNode};
use crate::syntax_kind::SyntaxKind;
use crate::tree::{SyntaxNode, SyntaxToken};

/// The `index`th typed child of type `N`, counted among `N` children
/// only (this is what makes positional same-type accessors correct on
/// partial trees: a missing later child is `None`, never a shift of an
/// earlier one).
pub(crate) fn child<N: AstNode>(parent: &SyntaxNode, index: usize) -> Option<N> {
    parent.children().filter_map(N::cast).nth(index)
}

pub(crate) fn children<N: AstNode>(parent: &SyntaxNode) -> AstChildren<N> {
    AstChildren::new(parent)
}

/// The first direct token child whose kind is one of `kinds`.
pub(crate) fn token(parent: &SyntaxNode, kinds: &[SyntaxKind]) -> Option<SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| kinds.contains(&token.kind()))
}
```

- [ ] **Step 4: Add the `SyntaxNodePtr` alias**

In `crates/celerrate_syntax/src/tree.rs`, after the existing aliases:

```rust
/// A stable, lightweight pointer to a node: its kind and range, no
/// tree handle. Upper layers (the salsa database) key derived data
/// with it instead of holding red nodes.
pub type SyntaxNodePtr = rowan::ast::SyntaxNodePtr<PhpLanguage>;
```

And a test beside the existing tree tests:

```rust
    #[test]
    fn a_node_pointer_resolves_back_to_its_node() {
        let parse = crate::parse::parse("<?php echo 1;");
        let root = parse.tree();
        let statement = root.first_child().expect("a first statement");
        let pointer = super::SyntaxNodePtr::new(&statement);
        assert_eq!(pointer.to_node(&root), statement);
    }
```

In `crates/celerrate_syntax/src/lib.rs`, extend the tree re-export:

```rust
pub use tree::{PhpLanguage, SyntaxElement, SyntaxNode, SyntaxNodePtr, SyntaxToken};
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p celerrate_syntax`
Expected: PASS (the new infrastructure test, the pointer test, everything existing).

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_syntax/src/ast.rs crates/celerrate_syntax/src/ast/support.rs crates/celerrate_syntax/src/tree.rs crates/celerrate_syntax/src/lib.rs
git commit -m "✨ feat(syntax): add the typed AST infrastructure and SyntaxNodePtr"
```

---

### Task 7: Generate the typed nodes and enums

**Files:**
- Create: `xtask/src/codegen/emit_ast.rs` (tests inline)
- Modify: `xtask/src/codegen.rs` (module, second artifact)
- Create: `crates/celerrate_syntax/src/ast/generated.rs` (by running the generator)
- Modify: `crates/celerrate_syntax/src/ast.rs` (module declaration, re-export)
- Test: `crates/celerrate_syntax/tests/ast.rs`

**Interfaces:**
- Consumes: `grammar::GrammarSource` (Task 3), `ast::{AstNode, AstChildren, support}` (Task 6).
- Produces: `emit_ast::ast_file(&GrammarSource) -> String` and the committed `ast/generated.rs`: one struct per node (`pub struct ClassDeclaration { syntax: SyntaxNode }` with `AstNode` impl and accessors like `member_list() -> Option<MemberList>`, `statements() -> AstChildren<Statement>`, `operator_token() -> Option<SyntaxToken>`), one enum per alternation (`Expression`, `Statement`, `Type`, `MemberDeclaration`, `StringInterpolation`, `TraitAdaptation`), and a fieldless `ErrorNode` struct. Task 8 adds `impl` blocks beside these; Task 9 tests partial trees through them.

Two generated-code details decided here: every generated enum carries `#[allow(clippy::enum_variant_names)]` (variant names mirror node kind names on purpose, and `StringInterpolation`'s variants genuinely share a suffix), and token accessors append `_token` to the field name (`operator_token()`, `name_token()`), keeping them visually distinct from node accessors.

- [ ] **Step 1: Write the failing emission test**

Create `xtask/src/codegen/emit_ast.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::ast_file;
    use crate::codegen::grammar::load;

    const MINI: &str = "Root = Item*\nItem = name:'identifier' operator:('+' | '-') Root?\nExpression = Root | Item";

    #[test]
    fn the_emission_contains_structs_accessors_and_enums() {
        let grammar = load(MINI).expect("mini grammar loads");
        let text = ast_file(&grammar);
        assert!(text.starts_with("//! Generated by `cargo xtask codegen`"));
        assert!(text.contains("pub struct Root {"));
        assert!(text.contains("pub fn items(&self) -> AstChildren<Item> {"));
        assert!(text.contains("pub fn name_token(&self) -> Option<SyntaxToken> {"));
        assert!(text.contains(
            "support::token(self.syntax(), &[SyntaxKind::Plus, SyntaxKind::Minus])"
        ));
        assert!(text.contains("pub fn root(&self) -> Option<Root> {"));
        assert!(text.contains("support::child(self.syntax(), 0)"));
        assert!(text.contains("pub enum Expression {"));
        assert!(text.contains("#[allow(clippy::enum_variant_names)]"));
        assert!(text.contains("SyntaxKind::Item => Self::Item(Item { syntax }),"));
        assert!(text.contains("pub struct ErrorNode {"));
    }
}
```

Add to `xtask/src/codegen.rs`, with the other modules:

```rust
pub mod emit_ast;
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p xtask`
Expected: compile FAIL (`ast_file` does not exist).

- [ ] **Step 3: Implement the emission**

Production half of `xtask/src/codegen/emit_ast.rs`:

```rust
//! Emits `crates/celerrate_syntax/src/ast/generated.rs`: one struct per
//! node rule with its `AstNode` impl and accessors, one Rust enum per
//! alternation rule, and the fieldless `ErrorNode` struct. Everything
//! calls into the hand-written `ast::support` plumbing.

use std::fmt::Write as _;

use super::grammar::{AstEnumSource, AstNodeSource, Cardinality, Field, FieldKind, GrammarSource};

pub fn ast_file(grammar: &GrammarSource) -> String {
    let mut text = String::new();
    text.push_str(
        "//! Generated by `cargo xtask codegen` from `php.ungram`; do not\n\
         //! edit by hand. Accessors return `Option` or an iterator: the\n\
         //! partial trees error recovery produces are normal citizens.\n\n\
         use super::{AstChildren, AstNode, support};\n\
         use crate::syntax_kind::SyntaxKind;\n\
         use crate::tree::{SyntaxNode, SyntaxToken};\n\n",
    );
    for node in &grammar.nodes {
        emit_node(&mut text, node);
    }
    emit_node(
        &mut text,
        &AstNodeSource {
            name: "ErrorNode".to_owned(),
            documentation: vec![
                "Recovery wreckage: tokens no grammar rule accepted.".to_owned(),
            ],
            fields: Vec::new(),
        },
    );
    for enumeration in &grammar.enums {
        emit_enum(&mut text, enumeration);
    }
    text
}

fn emit_node(text: &mut String, node: &AstNodeSource) {
    let name = &node.name;
    for line in &node.documentation {
        let _ = writeln!(text, "/// {line}");
    }
    let _ = writeln!(text, "#[derive(Debug, Clone, PartialEq, Eq, Hash)]");
    let _ = writeln!(text, "pub struct {name} {{");
    let _ = writeln!(text, "    syntax: SyntaxNode,");
    let _ = writeln!(text, "}}\n");
    let _ = writeln!(text, "impl AstNode for {name} {{");
    let _ = writeln!(text, "    fn can_cast(kind: SyntaxKind) -> bool {{");
    let _ = writeln!(text, "        kind == SyntaxKind::{name}");
    let _ = writeln!(text, "    }}");
    let _ = writeln!(text, "    fn cast(syntax: SyntaxNode) -> Option<Self> {{");
    let _ = writeln!(
        text,
        "        Self::can_cast(syntax.kind()).then(|| Self {{ syntax }})"
    );
    let _ = writeln!(text, "    }}");
    let _ = writeln!(text, "    fn syntax(&self) -> &SyntaxNode {{");
    let _ = writeln!(text, "        &self.syntax");
    let _ = writeln!(text, "    }}");
    let _ = writeln!(text, "}}\n");
    if node.fields.is_empty() {
        return;
    }
    let _ = writeln!(text, "impl {name} {{");
    for field in &node.fields {
        emit_accessor(text, field);
    }
    let _ = writeln!(text, "}}\n");
}

fn emit_accessor(text: &mut String, field: &Field) {
    let name = &field.name;
    match &field.kind {
        FieldKind::Node {
            type_name,
            cardinality: Cardinality::Many,
            ..
        } => {
            let _ = writeln!(
                text,
                "    pub fn {name}(&self) -> AstChildren<{type_name}> {{"
            );
            let _ = writeln!(text, "        support::children(self.syntax())");
            let _ = writeln!(text, "    }}");
        }
        FieldKind::Node {
            type_name,
            cardinality: Cardinality::Optional,
            index,
        } => {
            let _ = writeln!(text, "    pub fn {name}(&self) -> Option<{type_name}> {{");
            let _ = writeln!(text, "        support::child(self.syntax(), {index})");
            let _ = writeln!(text, "    }}");
        }
        FieldKind::Token { variants } => {
            let kinds = variants
                .iter()
                .map(|variant| format!("SyntaxKind::{variant}"))
                .collect::<Vec<String>>()
                .join(", ");
            let _ = writeln!(
                text,
                "    pub fn {name}_token(&self) -> Option<SyntaxToken> {{"
            );
            let _ = writeln!(text, "        support::token(self.syntax(), &[{kinds}])");
            let _ = writeln!(text, "    }}");
        }
    }
}

fn emit_enum(text: &mut String, enumeration: &AstEnumSource) {
    let name = &enumeration.name;
    if enumeration.documentation.is_empty() {
        let _ = writeln!(text, "/// The `{name}` alternation of `php.ungram`.");
    }
    for line in &enumeration.documentation {
        let _ = writeln!(text, "/// {line}");
    }
    let _ = writeln!(
        text,
        "// Variant names mirror node kind names on purpose."
    );
    let _ = writeln!(text, "#[allow(clippy::enum_variant_names)]");
    let _ = writeln!(text, "#[derive(Debug, Clone, PartialEq, Eq, Hash)]");
    let _ = writeln!(text, "pub enum {name} {{");
    for variant in &enumeration.variants {
        let _ = writeln!(text, "    {variant}({variant}),");
    }
    let _ = writeln!(text, "}}\n");
    let _ = writeln!(text, "impl AstNode for {name} {{");
    let _ = writeln!(text, "    fn can_cast(kind: SyntaxKind) -> bool {{");
    let kinds = enumeration
        .variants
        .iter()
        .map(|variant| format!("SyntaxKind::{variant}"))
        .collect::<Vec<String>>()
        .join(" | ");
    let _ = writeln!(text, "        matches!(kind, {kinds})");
    let _ = writeln!(text, "    }}");
    let _ = writeln!(text, "    fn cast(syntax: SyntaxNode) -> Option<Self> {{");
    let _ = writeln!(text, "        let result = match syntax.kind() {{");
    for variant in &enumeration.variants {
        let _ = writeln!(
            text,
            "            SyntaxKind::{variant} => Self::{variant}({variant} {{ syntax }}),"
        );
    }
    let _ = writeln!(text, "            _ => return None,");
    let _ = writeln!(text, "        }};");
    let _ = writeln!(text, "        Some(result)");
    let _ = writeln!(text, "    }}");
    let _ = writeln!(text, "    fn syntax(&self) -> &SyntaxNode {{");
    let _ = writeln!(text, "        match self {{");
    for variant in &enumeration.variants {
        let _ = writeln!(text, "            Self::{variant}(node) => node.syntax(),");
    }
    let _ = writeln!(text, "        }}");
    let _ = writeln!(text, "    }}");
    let _ = writeln!(text, "}}\n");
}
```

Run: `cargo test -p xtask`
Expected: PASS.

- [ ] **Step 4: Wire the second artifact and regenerate**

In `xtask/src/codegen.rs`, extend `artifacts()`:

```rust
pub fn artifacts() -> Result<Vec<Artifact>> {
    let text = php_ungram_source()?;
    let grammar = grammar::load(&text)?;
    Ok(vec![
        Artifact {
            relative_path: PathBuf::from(
                "crates/celerrate_syntax/src/syntax_kind/generated.rs",
            ),
            text: reformat(&emit_kinds::syntax_kind_file(&grammar))?,
        },
        Artifact {
            relative_path: PathBuf::from("crates/celerrate_syntax/src/ast/generated.rs"),
            text: reformat(&emit_ast::ast_file(&grammar))?,
        },
    ])
}
```

Run: `cargo test -p xtask --test sourcegen`
Expected: FAIL (the new artifact is not on disk: the freshness test now demands it).

Run: `cargo xtask codegen`
Expected: both files written.

- [ ] **Step 5: Declare the generated module**

In `crates/celerrate_syntax/src/ast.rs`, next to `pub(crate) mod support;`:

```rust
mod generated;

pub use generated::*;
```

Run: `cargo build -p celerrate_syntax`
Expected: the generated file compiles under the workspace lints. If clippy later objects to something in the emitted text, fix the *emitter*, rerun `cargo xtask codegen`, never the file.

- [ ] **Step 6: Write the typed navigation tests**

Create `crates/celerrate_syntax/tests/ast.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::SyntaxKind;
use celerrate_syntax::ast::{
    AstNode, Expression, MemberDeclaration, SourceFile, Statement, Type,
};

#[test]
fn typed_navigation_reaches_a_method_through_a_class() {
    let parse = support::parse_verified(
        "<?php class Foo extends Bar { public function baz(int $a): void {} }",
    );
    let source_file = SourceFile::cast(parse.tree()).expect("the root casts");
    let class_declaration = source_file
        .statements()
        .find_map(|statement| match statement {
            Statement::ClassDeclaration(class_declaration) => Some(class_declaration),
            _ => None,
        })
        .expect("a class declaration");
    let extends_clause = class_declaration
        .extends_clause()
        .expect("an extends clause");
    assert_eq!(extends_clause.names().count(), 1);
    let member_list = class_declaration.member_list().expect("a member list");
    let method = member_list
        .member_declarations()
        .find_map(|member| match member {
            MemberDeclaration::MethodDeclaration(method) => Some(method),
            _ => None,
        })
        .expect("a method");
    let parameter = method
        .parameter_list()
        .expect("a parameter list")
        .parameters()
        .next()
        .expect("one parameter");
    assert_eq!(
        parameter.name_token().expect("the parameter name").text(),
        "$a"
    );
    assert!(matches!(parameter.ty(), Some(Type::NamedType(_))));
    assert!(matches!(method.return_type(), Some(Type::NamedType(_))));
}

#[test]
fn binary_operands_are_positional() {
    let parse = support::parse_verified("<?php 1 + 2;");
    let source_file = SourceFile::cast(parse.tree()).expect("the root casts");
    let statement = source_file.statements().next().expect("one statement");
    let Statement::ExpressionStatement(expression_statement) = statement else {
        panic!("an expression statement");
    };
    let Some(Expression::BinaryExpression(binary)) = expression_statement.expression() else {
        panic!("a binary expression");
    };
    assert_eq!(
        binary.operator_token().expect("the operator").kind(),
        SyntaxKind::Plus
    );
    assert_eq!(
        binary.lhs().expect("the left operand").syntax().text().to_string(),
        "1"
    );
    assert_eq!(
        binary.rhs().expect("the right operand").syntax().text().to_string(),
        "2"
    );
}
```

- [ ] **Step 7: Run everything**

Run: `cargo test --workspace`
Expected: PASS, including the freshness test over both artifacts and the two navigation tests. No snapshot changes (the parser did not change).

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all clean.

- [ ] **Step 8: Commit**

```bash
git add xtask/src/codegen.rs xtask/src/codegen/emit_ast.rs crates/celerrate_syntax/src/ast.rs crates/celerrate_syntax/src/ast/generated.rs crates/celerrate_syntax/tests/ast.rs
git commit -m "✨ feat(syntax): generate the typed AST nodes from php.ungram"
```

---

### Task 8: Hand-written extensions: semi-reserved names and position-dependent roles

**Files:**
- Create: `crates/celerrate_syntax/src/ast/extensions.rs`
- Modify: `crates/celerrate_syntax/src/ast.rs` (module declaration, `NamedTypeName` re-export)
- Test: `crates/celerrate_syntax/tests/ast.rs` (extend)

**Interfaces:**
- Consumes: the generated structs and enums (Task 7), `support::{child, children, token}` (Task 6).
- Produces: `ast::NamedTypeName` and, as inherent methods on generated types: `NamedType::name_or_keyword()`; `name_token()` on `ConstantElement`, `EnumCase`, `MemberName`, `MethodDeclaration`, `ClassDeclaration`, `InterfaceDeclaration`, `TraitDeclaration`, `PropertyHook`; `UseClause::alias_token()`; `Argument::label_token()`; `modifiers()` on `ClassDeclaration`, `PropertyDeclaration`, `MethodDeclaration`, `ConstantDeclaration`, `Parameter`; the full accessor sets of the override nodes `TernaryExpression`, `ArrayElement`, `YieldExpression`, `ForeachStatement`, `MatchArm`, `TraitPrecedence`, `TraitAlias`.

- [ ] **Step 1: Write the failing extension tests**

Append to `crates/celerrate_syntax/tests/ast.rs` (extend the `use celerrate_syntax::ast::{...}` list with what these tests name: `ArrayElement`, `ClassDeclaration`, `ConstantDeclaration`, `EnumCase`, `ForeachStatement`, `MatchArm`, `MatchExpression`, `MemberAccessExpression`, `MethodDeclaration`, `NamedType`, `NamedTypeName`, `TernaryExpression`, `TraitAlias`, `TraitPrecedence`, `TraitUseClause`, `UseClause`, `UseDeclaration`, `YieldExpression`):

```rust
/// The first typed descendant of the parse, found by walking every
/// node: the extension tests reach deep shapes without spelling the
/// whole path down.
fn first_descendant<N: AstNode>(parse: &celerrate_syntax::Parse) -> N {
    parse
        .tree()
        .descendants()
        .find_map(N::cast)
        .expect("the shape under test exists")
}

#[test]
fn semi_reserved_names_resolve_through_the_extensions() {
    let parse = support::parse_verified("<?php class List { const FOR = 1; public function match() {} }");
    let class_declaration: ClassDeclaration = first_descendant(&parse);
    assert_eq!(
        class_declaration.name_token().expect("a class name").text(),
        "List"
    );
    let constant: ConstantDeclaration = first_descendant(&parse);
    let element = constant.constant_elements().next().expect("one element");
    assert_eq!(element.name_token().expect("a constant name").text(), "FOR");
    let method: MethodDeclaration = first_descendant(&parse);
    assert_eq!(method.name_token().expect("a method name").text(), "match");

    let parse = support::parse_verified("<?php $object->list();");
    let access: MemberAccessExpression = first_descendant(&parse);
    let member_name = access.member_name().expect("a member name");
    assert_eq!(member_name.name_token().expect("a name token").text(), "list");

    let parse = support::parse_verified("<?php enum Suit { case Default; }");
    let case: EnumCase = first_descendant(&parse);
    assert_eq!(case.name_token().expect("a case name").text(), "Default");
}

#[test]
fn a_named_type_is_one_concept_in_two_shapes() {
    let parse = support::parse_verified("<?php function f(): Foo\\Bar {}");
    let named: NamedType = first_descendant(&parse);
    assert!(matches!(
        named.name_or_keyword(),
        Some(NamedTypeName::Name(_))
    ));

    let parse = support::parse_verified("<?php function f(): array {}");
    let named: NamedType = first_descendant(&parse);
    let Some(NamedTypeName::Keyword(keyword)) = named.name_or_keyword() else {
        panic!("a bare keyword type");
    };
    assert_eq!(keyword.text(), "array");
}

#[test]
fn position_dependent_roles_resolve_against_token_anchors() {
    // The short ternary has no middle.
    let parse = support::parse_verified("<?php $a ? $b : $c;");
    let ternary: TernaryExpression = first_descendant(&parse);
    assert_eq!(ternary.middle().expect("a middle").syntax().text(), "$b");
    assert_eq!(ternary.third().expect("a third").syntax().text(), "$c");
    let parse = support::parse_verified("<?php $a ?: $c;");
    let ternary: TernaryExpression = first_descendant(&parse);
    assert!(ternary.middle().is_none(), "the short form has no middle");
    assert_eq!(ternary.third().expect("a third").syntax().text(), "$c");

    // `[value]` versus `[key => value]`.
    let parse = support::parse_verified("<?php [1, 'k' => 2];");
    let elements: Vec<ArrayElement> = parse
        .tree()
        .descendants()
        .filter_map(ArrayElement::cast)
        .collect();
    assert!(elements[0].key().is_none());
    assert_eq!(elements[0].value().expect("a value").syntax().text(), "1");
    assert_eq!(elements[1].key().expect("a key").syntax().text(), "'k'");
    assert_eq!(elements[1].value().expect("a value").syntax().text(), "2");

    // foreach with and without a key target.
    let parse = support::parse_verified("<?php foreach ($all as $k => $v) {}");
    let foreach: ForeachStatement = first_descendant(&parse);
    assert_eq!(foreach.subject().expect("a subject").syntax().text(), "$all");
    assert_eq!(foreach.key().expect("a key").syntax().text(), "$k");
    assert_eq!(foreach.value().expect("a value").syntax().text(), "$v");
    let parse = support::parse_verified("<?php foreach ($all as $v) {}");
    let foreach: ForeachStatement = first_descendant(&parse);
    assert!(foreach.key().is_none());
    assert_eq!(foreach.value().expect("a value").syntax().text(), "$v");

    // Match arms: conditions before the arrow, the body after it.
    let parse = support::parse_verified("<?php match ($x) { 1, 2 => 'a', default => 'b' };");
    let expression: MatchExpression = first_descendant(&parse);
    let arms: Vec<MatchArm> = expression.match_arms().collect();
    assert_eq!(arms[0].conditions().count(), 2);
    assert!(!arms[0].is_default());
    assert_eq!(arms[0].body().expect("a body").syntax().text(), "'a'");
    assert!(arms[1].is_default());
    assert_eq!(arms[1].conditions().count(), 0);
    assert_eq!(arms[1].body().expect("a body").syntax().text(), "'b'");

    // yield in its three shapes.
    let parse = support::parse_verified("<?php function g() { yield $k => $v; }");
    let yielded: YieldExpression = first_descendant(&parse);
    assert_eq!(yielded.key().expect("a key").syntax().text(), "$k");
    assert_eq!(yielded.value().expect("a value").syntax().text(), "$v");
    let parse = support::parse_verified("<?php function g() { yield $v; }");
    let yielded: YieldExpression = first_descendant(&parse);
    assert!(yielded.key().is_none());
    assert_eq!(yielded.value().expect("a value").syntax().text(), "$v");
    let parse = support::parse_verified("<?php function g() { yield from $inner; }");
    let yielded: YieldExpression = first_descendant(&parse);
    assert!(yielded.yield_from_token().is_some());
    assert_eq!(yielded.value().expect("a value").syntax().text(), "$inner");
}

#[test]
fn trait_adaptations_and_import_aliases_resolve() {
    let parse = support::parse_verified(
        "<?php class C { use A, B { A::hello insteadof B; hello as protected h2; } }",
    );
    let trait_use: TraitUseClause = first_descendant(&parse);
    assert_eq!(trait_use.names().count(), 2);
    let precedence: TraitPrecedence = first_descendant(&parse);
    assert_eq!(
        precedence.reference_name().expect("a reference").syntax().text(),
        "A"
    );
    assert_eq!(
        precedence.member_token().expect("a member").text(),
        "hello"
    );
    assert_eq!(precedence.excluded_names().count(), 1);
    let alias: TraitAlias = first_descendant(&parse);
    assert_eq!(alias.member_token().expect("a member").text(), "hello");
    assert_eq!(
        alias.visibility_token().expect("a visibility").text(),
        "protected"
    );
    assert_eq!(alias.alias_token().expect("an alias").text(), "h2");

    let parse = support::parse_verified("<?php use Foo\\Bar as Baz;");
    let use_declaration: UseDeclaration = first_descendant(&parse);
    let clause: UseClause = use_declaration.use_clauses().next().expect("one clause");
    assert_eq!(clause.alias_token().expect("an alias").text(), "Baz");
}

#[test]
fn labels_hooks_and_modifiers_resolve() {
    let parse = support::parse_verified("<?php f(name: 1);");
    let argument = first_descendant::<celerrate_syntax::ast::Argument>(&parse);
    assert_eq!(argument.label_token().expect("a label").text(), "name");

    let parse = support::parse_verified(
        "<?php class C { public int $x { get => 1; final set($v) {} } }",
    );
    let hooks: Vec<celerrate_syntax::ast::PropertyHook> = parse
        .tree()
        .descendants()
        .filter_map(celerrate_syntax::ast::PropertyHook::cast)
        .collect();
    assert_eq!(hooks[0].name_token().expect("a hook name").text(), "get");
    assert_eq!(hooks[1].name_token().expect("a hook name").text(), "set");

    let parse = support::parse_verified("<?php class C { public static function f() {} }");
    let method: MethodDeclaration = first_descendant(&parse);
    let modifiers: Vec<String> = method
        .modifiers()
        .map(|token| token.text().to_owned())
        .collect();
    assert_eq!(modifiers, ["public", "static"]);
}
```

(`descendants()` is the rowan traversal on `SyntaxNode`; `elements[0]`/`arms[0]` indexing is fine under the test module's `#![allow(clippy::expect_used)]`; add `clippy::indexing_slicing` to that allow list at the top of `tests/ast.rs`.)

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p celerrate_syntax --test ast`
Expected: compile FAIL (none of the extension methods exist).

- [ ] **Step 3: Implement the extensions**

Create `crates/celerrate_syntax/src/ast/extensions.rs`:

```rust
//! Hand-written typed-AST accessors: the logic a generator cannot
//! express. Two families. Semi-reserved names: any keyword may stand
//! for the name (`const FOR = 1;`), so a generated Identifier accessor
//! would silently return `None`; these resolve identifier-or-keyword
//! tokens instead, anchored to the token that precedes the name.
//! Position-dependent roles: in `key => value` shapes, the short
//! ternary, and trait adaptations, the role of an expression depends on
//! where it sits relative to an anchor token, not on its position among
//! siblings.

use super::generated::{
    Argument, ArrayElement, ClassDeclaration, ConstantDeclaration, ConstantElement, EnumCase,
    Expression, ForeachStatement, InterfaceDeclaration, MatchArm, MemberName, MethodDeclaration,
    Name, NamedType, Parameter, PropertyDeclaration, PropertyHook, Statement, TernaryExpression,
    TraitAlias, TraitDeclaration, TraitPrecedence, UseClause, YieldExpression,
};
use super::{AstChildren, AstNode, support};
use crate::syntax_kind::SyntaxKind;
use crate::tree::{SyntaxNode, SyntaxToken};

/// A semi-reserved name position accepts an identifier or any keyword.
fn is_name_token(kind: SyntaxKind) -> bool {
    kind == SyntaxKind::Identifier || kind.is_keyword()
}

/// The direct token children of one node, trivia skipped. Tokens
/// inside child nodes are invisible here, which is what anchoring
/// relies on (a `Name`'s identifiers never leak into its parent).
fn tokens_of(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia())
}

/// The first identifier-or-keyword token after the anchor token.
fn name_after(node: &SyntaxNode, anchor: SyntaxKind) -> Option<SyntaxToken> {
    let mut seen_anchor = false;
    tokens_of(node).find(|token| {
        if seen_anchor && is_name_token(token.kind()) {
            return true;
        }
        if token.kind() == anchor {
            seen_anchor = true;
        }
        false
    })
}

/// The member-modifier tokens among a node's direct children.
fn modifier_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    tokens_of(node).filter(|token| {
        matches!(
            token.kind(),
            SyntaxKind::Public
                | SyntaxKind::Protected
                | SyntaxKind::Private
                | SyntaxKind::Static
                | SyntaxKind::Abstract
                | SyntaxKind::Final
                | SyntaxKind::Readonly
                | SyntaxKind::Var
        )
    })
}

/// The first `Expression` child strictly after `anchor`, by range.
fn expression_after(node: &SyntaxNode, anchor: &SyntaxToken) -> Option<Expression> {
    let minimum = anchor.text_range().end();
    support::children::<Expression>(node)
        .find(|expression| expression.syntax().text_range().start() >= minimum)
}

/// The first `Expression` child strictly between two anchors, by range.
fn expression_between(
    node: &SyntaxNode,
    start: &SyntaxToken,
    end: &SyntaxToken,
) -> Option<Expression> {
    let after = start.text_range().end();
    let before = end.text_range().start();
    support::children::<Expression>(node).find(|expression| {
        let range = expression.syntax().text_range();
        after <= range.start() && range.end() <= before
    })
}

/// The two shapes of a named type, unified: `Foo\Bar` is a `Name`
/// node, `array` / `callable` / `static` (and permissively any keyword)
/// is a bare token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedTypeName {
    Name(Name),
    Keyword(SyntaxToken),
}

impl NamedType {
    pub fn name_or_keyword(&self) -> Option<NamedTypeName> {
        if let Some(name) = self.name() {
            return Some(NamedTypeName::Name(name));
        }
        tokens_of(self.syntax())
            .find(|token| token.kind().is_keyword())
            .map(NamedTypeName::Keyword)
    }
}

impl ConstantElement {
    /// The declared name: semi-reserved, the first name token.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        tokens_of(self.syntax()).find(|token| is_name_token(token.kind()))
    }
}

impl EnumCase {
    /// The case name: semi-reserved, after `case`.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Case)
    }
}

impl MemberName {
    /// The plain-token form of the name: an identifier or any keyword
    /// (`$object->list()`). `None` for the variable and `{ ... }` forms.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        tokens_of(self.syntax()).find(|token| is_name_token(token.kind()))
    }
}

impl MethodDeclaration {
    /// The method name: semi-reserved, after `function` (skipping the
    /// by-reference `&`).
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Function)
    }

    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl ClassDeclaration {
    /// The declared name: semi-reserved (`class List {}` parses), after
    /// `class`. `None` for anonymous classes.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Class)
    }

    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl InterfaceDeclaration {
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Interface)
    }
}

impl TraitDeclaration {
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Trait)
    }
}

impl PropertyDeclaration {
    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl ConstantDeclaration {
    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl Parameter {
    /// The promotion modifiers of a constructor-promoted parameter.
    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl PropertyHook {
    /// The hook name (`get`, `set`; semi-reserved in practice): the
    /// first name token, except a leading `final`, which is the
    /// modifier unless it is the only candidate left.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        let mut names = tokens_of(self.syntax()).filter(|token| is_name_token(token.kind()));
        let first = names.next()?;
        if first.kind() == SyntaxKind::Final {
            return names.next();
        }
        Some(first)
    }
}

impl UseClause {
    /// The import alias: semi-reserved, after `as`.
    pub fn alias_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::As)
    }
}

impl Argument {
    /// The named-argument label: the name token directly before the
    /// `:` separator.
    pub fn label_token(&self) -> Option<SyntaxToken> {
        let mut previous: Option<SyntaxToken> = None;
        for token in tokens_of(self.syntax()) {
            if token.kind() == SyntaxKind::Colon {
                return previous.filter(|candidate| is_name_token(candidate.kind()));
            }
            previous = Some(token);
        }
        None
    }
}

impl TernaryExpression {
    pub fn condition(&self) -> Option<Expression> {
        support::child(self.syntax(), 0)
    }

    /// The middle operand; `None` for the short form `?:`.
    pub fn middle(&self) -> Option<Expression> {
        let question = support::token(self.syntax(), &[SyntaxKind::Question])?;
        let colon = support::token(self.syntax(), &[SyntaxKind::Colon])?;
        expression_between(self.syntax(), &question, &colon)
    }

    pub fn third(&self) -> Option<Expression> {
        let colon = support::token(self.syntax(), &[SyntaxKind::Colon])?;
        expression_after(self.syntax(), &colon)
    }
}

impl ArrayElement {
    /// The key: the expression before `=>`; `None` without an arrow.
    pub fn key(&self) -> Option<Expression> {
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        support::children::<Expression>(self.syntax()).find(|expression| {
            expression.syntax().text_range().end() <= arrow.text_range().start()
        })
    }

    pub fn value(&self) -> Option<Expression> {
        match support::token(self.syntax(), &[SyntaxKind::FatArrow]) {
            Some(arrow) => expression_after(self.syntax(), &arrow),
            None => support::child(self.syntax(), 0),
        }
    }

    pub fn spread_token(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), &[SyntaxKind::Ellipsis])
    }
}

impl YieldExpression {
    /// The `yield from` token, when this is the delegation form.
    pub fn yield_from_token(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), &[SyntaxKind::YieldFrom])
    }

    /// The key: the expression before `=>`; `None` without an arrow.
    pub fn key(&self) -> Option<Expression> {
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        support::children::<Expression>(self.syntax()).find(|expression| {
            expression.syntax().text_range().end() <= arrow.text_range().start()
        })
    }

    pub fn value(&self) -> Option<Expression> {
        match support::token(self.syntax(), &[SyntaxKind::FatArrow]) {
            Some(arrow) => expression_after(self.syntax(), &arrow),
            None => support::child(self.syntax(), 0),
        }
    }
}

impl ForeachStatement {
    pub fn subject(&self) -> Option<Expression> {
        support::child(self.syntax(), 0)
    }

    /// The key target: between `as` and `=>`; `None` without an arrow.
    pub fn key(&self) -> Option<Expression> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        expression_between(self.syntax(), &as_keyword, &arrow)
    }

    /// The value target: after `=>` when present, else after `as`.
    pub fn value(&self) -> Option<Expression> {
        let anchor = support::token(self.syntax(), &[SyntaxKind::FatArrow])
            .or_else(|| support::token(self.syntax(), &[SyntaxKind::As]))?;
        expression_after(self.syntax(), &anchor)
    }

    /// The body: one statement (classic syntax) or the list before
    /// `endforeach` (alternative syntax).
    pub fn statements(&self) -> AstChildren<Statement> {
        support::children(self.syntax())
    }
}

impl MatchArm {
    pub fn is_default(&self) -> bool {
        support::token(self.syntax(), &[SyntaxKind::Default]).is_some()
    }

    /// The conditions before the arrow; empty for a `default` arm.
    pub fn conditions(&self) -> impl Iterator<Item = Expression> {
        let arrow_start = support::token(self.syntax(), &[SyntaxKind::FatArrow])
            .map(|token| token.text_range().start());
        support::children::<Expression>(self.syntax()).take_while(move |expression| {
            arrow_start.is_none_or(|start| expression.syntax().text_range().end() <= start)
        })
    }

    /// The body: the expression after the arrow.
    pub fn body(&self) -> Option<Expression> {
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        expression_after(self.syntax(), &arrow)
    }
}

impl TraitPrecedence {
    /// The `Name` before `insteadof`: the qualified class half of
    /// `A::member`, or the bare member name itself when the reference
    /// is unqualified (`hello insteadof B;`).
    pub fn reference_name(&self) -> Option<Name> {
        let insteadof = support::token(self.syntax(), &[SyntaxKind::InsteadOf])?;
        support::children::<Name>(self.syntax())
            .find(|name| name.syntax().text_range().end() <= insteadof.text_range().start())
    }

    /// The member token after `::`; also the bare-keyword reference
    /// form (`list insteadof B;`), which the parser keeps as a token.
    pub fn member_token(&self) -> Option<SyntaxToken> {
        let limit = support::token(self.syntax(), &[SyntaxKind::InsteadOf])
            .map(|token| token.text_range().start());
        let after_separator = support::token(self.syntax(), &[SyntaxKind::ColonColon])
            .map(|token| token.text_range().end());
        tokens_of(self.syntax()).find(|token| {
            is_name_token(token.kind())
                && token.kind() != SyntaxKind::InsteadOf
                && after_separator.is_none_or(|start| token.text_range().start() >= start)
                && limit.is_none_or(|end| token.text_range().end() <= end)
        })
    }

    /// The excluded names after `insteadof`.
    pub fn excluded_names(&self) -> impl Iterator<Item = Name> {
        let minimum = support::token(self.syntax(), &[SyntaxKind::InsteadOf])
            .map(|token| token.text_range().end());
        support::children::<Name>(self.syntax()).filter(move |name| {
            minimum.is_some_and(|start| name.syntax().text_range().start() >= start)
        })
    }
}

impl TraitAlias {
    /// The `Name` before `as`: the qualified class half, or the bare
    /// member name itself when the reference is unqualified.
    pub fn reference_name(&self) -> Option<Name> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        support::children::<Name>(self.syntax())
            .find(|name| name.syntax().text_range().end() <= as_keyword.text_range().start())
    }

    /// The member token before `as` (after `::` when present; also the
    /// bare-keyword reference form).
    pub fn member_token(&self) -> Option<SyntaxToken> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        let limit = as_keyword.text_range().start();
        let after_separator = support::token(self.syntax(), &[SyntaxKind::ColonColon])
            .map(|token| token.text_range().end());
        tokens_of(self.syntax()).find(|token| {
            is_name_token(token.kind())
                && token.text_range().end() <= limit
                && after_separator.is_none_or(|start| token.text_range().start() >= start)
        })
    }

    /// The visibility after `as` (`hello as protected h2;`).
    pub fn visibility_token(&self) -> Option<SyntaxToken> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        let minimum = as_keyword.text_range().end();
        tokens_of(self.syntax()).find(|token| {
            token.text_range().start() >= minimum
                && matches!(
                    token.kind(),
                    SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private
                )
        })
    }

    /// The new name: the first name token after the visibility when
    /// one is present, else after `as`.
    pub fn alias_token(&self) -> Option<SyntaxToken> {
        let anchor = self
            .visibility_token()
            .or_else(|| support::token(self.syntax(), &[SyntaxKind::As]))?;
        let minimum = anchor.text_range().end();
        tokens_of(self.syntax()).find(|token| {
            token.text_range().start() >= minimum && is_name_token(token.kind())
        })
    }
}
```

In `crates/celerrate_syntax/src/ast.rs`, next to the other modules:

```rust
mod extensions;

pub use extensions::NamedTypeName;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p celerrate_syntax --test ast`
Expected: PASS (all six test functions). If `name_after` misbehaves on a shape, check the anchor against the real tree with `render_statement` before touching the logic: the parser's placement, not the extension, is authoritative.

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax/src/ast.rs crates/celerrate_syntax/src/ast/extensions.rs crates/celerrate_syntax/tests/ast.rs
git commit -m "✨ feat(syntax): hand-written accessors for names and anchored roles"
```

---

### Task 9: Partial-tree pins, corpus smoke, changelog, CI

**Files:**
- Test: `crates/celerrate_syntax/tests/ast.rs` (extend)
- Modify: `crates/celerrate_syntax/src/lib.rs` (module documentation)
- Modify: `.github/workflows/ci.yml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: everything from Tasks 5 to 8.
- Produces: the recorded guarantees as pins; the Foundations sub-project closes with this task.

- [ ] **Step 1: Write the partial-tree and recorded-note pins**

Append to `crates/celerrate_syntax/tests/ast.rs` (add `BinaryExpression` and `DeclareStatement` to the `ast::{...}` import list):

```rust
#[test]
fn partial_trees_are_normal_citizens() {
    // Broken input still yields typed nodes; the missing pieces are
    // `None`, never a panic and never a shifted role.
    let parse = support::parse_verified("<?php class {");
    let class_declaration: ClassDeclaration = first_descendant(&parse);
    assert!(class_declaration.name_token().is_none());
    assert!(
        class_declaration.member_list().is_some(),
        "the member list node completes even while broken"
    );
    assert!(!parse.diagnostics().is_empty());

    let parse = support::parse_verified("<?php $a + ;");
    let binary: BinaryExpression = first_descendant(&parse);
    assert!(binary.lhs().is_some());
    assert!(
        binary.rhs().is_none(),
        "a missing operand is a None, not a shift"
    );

    let parse = support::parse_verified("<?php function f(");
    let function: celerrate_syntax::ast::FunctionDeclaration = first_descendant(&parse);
    assert_eq!(function.name_token().expect("the name").text(), "f");
    assert!(function.parameter_list().is_some());
    assert!(function.block().is_none());
}

#[test]
fn a_close_tag_declare_body_yields_no_statement() {
    // Recorded in plan 3 and carried here: a classic `declare` body of
    // `?>` leaves a bare CloseTag token with no statement-node child,
    // so the typed statement list is empty.
    let parse = support::parse_verified("<?php declare(strict_types=1) ?>");
    let declare: DeclareStatement = first_descendant(&parse);
    assert_eq!(declare.declare_directives().count(), 1);
    assert_eq!(declare.statements().count(), 0);
}

#[test]
fn typed_casts_are_consistent_over_the_whole_corpus() {
    // For every corpus tree: lossless, and `can_cast` agrees with
    // `cast` on every node for the two big alternations. This is the
    // typed layer's zero-panic guarantee, exercised at corpus scale.
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parse_corpus");
    let mut checked = 0usize;
    for entry in std::fs::read_dir(corpus).expect("the corpus directory exists") {
        let path = entry.expect("a readable entry").path();
        if path.extension().is_none_or(|extension| extension != "php") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a corpus file reads");
        let parse = celerrate_syntax::parse(&source);
        assert_eq!(
            parse.tree().text().to_string(),
            source,
            "lossless: {}",
            path.display()
        );
        for node in parse.tree().descendants() {
            if Statement::can_cast(node.kind()) {
                let statement = Statement::cast(node.clone()).expect("can_cast implies cast");
                assert_eq!(statement.syntax(), &node);
            }
            if Expression::can_cast(node.kind()) {
                let expression = Expression::cast(node.clone()).expect("can_cast implies cast");
                assert_eq!(expression.syntax(), &node);
            }
        }
        checked += 1;
    }
    assert!(checked > 20, "the corpus was actually traversed");
}
```

Run: `cargo test -p celerrate_syntax --test ast`
Expected: PASS. If `function f(` does not produce a `FunctionDeclaration` (check what recovery actually built with `render_statement`), adjust the assertions to pin the real recovered shape; the point of the pin is the `Option` behavior, not one specific recovery.

- [ ] **Step 2: Mention the typed layer in the crate documentation**

In `crates/celerrate_syntax/src/lib.rs`, replace the module doc's opening with:

```rust
//! PHP syntax for the Celerrate toolchain: [`lex`] turns decoded source
//! text into a lossless token stream, [`parse`] builds the lossless
//! concrete syntax tree on top of it, plus structured diagnostics, and
//! the [`ast`] module gives typed, `Option`-everywhere access to that
//! tree, generated from `php.ungram`.
//! Nothing here ever fails: degenerate input yields error tokens,
//! `ErrorNode`s, and diagnostics, never a crash.
```

- [ ] **Step 3: Give the CI test job rustfmt**

The sourcegen freshness test shells out to `rustfmt`, so the test job's toolchain must carry the component. In `.github/workflows/ci.yml`, in the `test` job:

```yaml
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          components: rustfmt
```

(Only the `components` line is new; the `lint` job already has it.)

- [ ] **Step 4: Update the changelog**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, append:

```markdown
- `celerrate_syntax`: a typed AST layer generated from `php.ungram` by
  the new dev-only `xtask` workspace member: the `SyntaxKind` node
  kinds and the typed node structs (`Option`/iterator accessors
  everywhere, so partial trees from error recovery are normal
  citizens), plus hand-written accessors for semi-reserved names and
  position-dependent roles. A sourcegen test keeps the committed
  generated code fresh. This closes the Foundations sub-project.
```

- [ ] **Step 5: Full verification**

Run, in order, expecting every one clean:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo +nightly fuzz run parse -- -runs=200000
cargo +nightly fuzz run parse -- -runs=200000
```

The fuzz smoke covers Task 1's `new_expression` change (the only parser change in this plan); two runs, matching the previous plans' bar. No findings expected.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_syntax/tests/ast.rs crates/celerrate_syntax/src/lib.rs .github/workflows/ci.yml CHANGELOG.md
git commit -m "✅ test(syntax): pin partial trees and the typed corpus smoke"
```

---

## Completion checklist (for the finishing task)

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check` all clean; the fuzz smoke ran without findings.
- `cargo xtask codegen` immediately followed by `git status` shows a clean tree (the committed artifacts are fresh; the sourcegen test enforces the same thing in CI).
- Every deferred item from PR #8 is landed here (Tasks 1, 4, 5, 8) or recorded in this plan's header as a semantics-layer note (`var` in parameter position).
- Branch: this plan is developed on `foundations-5-typed-ast`; finish with superpowers:finishing-a-development-branch (PR to `main`, as the previous plans did).
- Items to record in the PR description for later sub-projects: the semantics layer's modifier-placement judgment list gains `var` in parameter position; `php.ungram` is now the single place a grammar change must touch alongside the parser (the freshness test arbitrates drift); the generated `SyntaxKind` node discriminants reordered, which nothing may depend on.
- The Foundations sub-project (parser design spec, section 7) is closed: all five plans landed.

## Self-review notes (performed while writing this plan)

- Spec coverage against `.claude/superpowers/specs/2026-07-10-foundations-4-parser-design.md` section 5: `php.ungram` (Task 4); the xtask binary generating both the kind variants and the typed structs (Tasks 2, 3, 5, 7); committed generated code with a freshness test and no `build.rs` (Tasks 5, 7); `Option`/iterator accessors everywhere (Tasks 6, 7); hand-written extensions beside the generated code, semi-reserved member names included (Task 8). Section 2's `SyntaxNodePtr` alias, previously missing from `tree.rs`, lands in Task 6. Section 6 item 6 (sourcegen freshness) lands in Task 5 and extends in Task 7.
- The PR #8 carry-overs and housekeeping items each map to a task (see the header section).
- Type consistency spot-checked across tasks: `Artifact { relative_path, text }` (Tasks 2, 5, 7); `GrammarSource` / `Field` / `FieldKind` / `Cardinality` (Tasks 3, 4, 5, 7); `AstNode` / `AstChildren` / `support::{child, children, token}` signatures (Tasks 6, 7, 8); the accessor names asserted by Task 4's spot checks match Task 7's emission rules and Task 8's imports.
- Judgment calls recorded rather than hidden: the whole `SyntaxKind` enum is generated (token table in xtask); the backslash token is spelled `backslash` in ungrammar; generated enums carry `#[allow(clippy::enum_variant_names)]`; node discriminants reorder; the override list is the ambiguity check's escape hatch, enforced by the loader.








