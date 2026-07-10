# Foundations Part 4, Plan 3: Statements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse the full PHP statement grammar: blocks, control flow (`if`, `while`, `do`, `for`, `foreach`, `switch`) in both classic and alternative (`endif`, ...) syntax, `try`/`catch`/`finally`, `goto` and labels, `global`/`static`/`unset`/`declare`, and simple top-level `function` declarations — with tested error recovery throughout.

**Architecture:** A new `grammar/statements.rs` module beside `grammar/expressions.rs`, dispatched from the existing `statement_list_step`. Statement lists share one rule: a nested list never consumes a list-terminator token (`}`, `endif`, `else`, `case`, ...); the construct that owns the terminator eats it, and an orphaned terminator unwinds to the source-file loop, which consumes anything — the same unwind-to-the-top shape the fuse recovery already uses. Design: `.claude/superpowers/specs/2026-07-10-foundations-4-parser-design.md` (section 3 "Statements", section 4). Statement productions are transcribed from php-src `zend_language_parser.y`, branch PHP-8.5 (`statement`, `if_stmt`, `alt_if_stmt`, `for_statement`, `foreach_statement`, `switch_case_list`, `catch_list`, `declare_statement`, `function_declaration_statement`).

**Tech Stack:** Rust 1.94 (edition 2024), `rowan` 0.16, `insta` (snapshots), `cargo-fuzz` (libFuzzer).

## Global Constraints

Copied from the parent spec and `CLAUDE.md`; every task's requirements include them.

- Zero panic, mechanically enforced: workspace denies `clippy::unwrap_used`, `clippy::expect_used`, `clippy::indexing_slicing`, `clippy::panic`; `unsafe_code` is forbidden. Production code returns totals (`Option`, fallbacks); test modules may locally `#[allow]` these lints (see existing test files for the idiom). `debug_assert!` is permitted (compiled out in release).
- TDD: every step of behavior starts from a failing test. No production code without a test that demanded it.
- Layering: `celerrate_syntax` depends only on `celerrate_source` (plus external `rowan`, `text-size`). No bare `rowan` type in any public signature.
- The lossless invariant: `parse(source).tree().text() == source` for every input, including degenerate input.
- Guaranteed progress: every parser loop consumes a token or terminates; every new loop states its termination argument in a comment when it is not obvious.
- The parser performs no version or semantic judgment and never fails: worst case is `ErrorNode` wreckage plus diagnostics. `readonly`-anything, `break 0;`, `goto` into a loop: all of it parses; validity is semantic.
- Everything in English, full words, no abbreviated names (standard acronyms fine). No em-dashes. Comments state constraints the code cannot show, never narration.
- Commits: gitmoji + Conventional Commits (`✨ feat(syntax): ...`), repository-configured identity, no AI attribution of any kind.
- Before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` must all pass.

## Deferred items from plan 2 absorbed here

Recorded in PR #6:

1. The blown-fuse block regression test drives the real grammar function directly instead of hand-mimicking its idiom (Task 1).
2. Consecutive duplicate diagnostics on exhausted nesting are collapsed (Task 2). Measured today: `<?php` + `(`×140 + `1;` emits 127 identical adjacent `Expected(CloseParenthesis)` diagnostics, zero-width at the same offset, one per unwound nesting level.
3. `static $x` statement dispatch (Task 5).

Explicitly NOT here: `qualified_type_name`/`name` unification (plan 4, with the full type grammar), the `$a ?? $b = $c` tree-shape note (semantics layer), `const`/`namespace`/`use`/class-likes (plan 4), `__halt_compiler` (out of scope for Foundations; recorded).

## Recorded permissiveness decisions (parse clean, judged upstairs)

- `case`/`default` bodies, `break`/`continue` levels, `goto` targets: any expression or placement parses; level validity, label existence, jump legality are semantic.
- `for ($i = 0,;;)` (trailing comma in a `for` section) parses without diagnostic.
- Mixing classic and alternative syntax (`if ($a): ... else { ... }` and the reverse) parses greedily; the mismatched closer is diagnosed as a missing expected token, never a crash.
- `foreach` keys by reference, arbitrary expressions as `foreach` targets: parse; assignability is semantic.

## File Structure

```
crates/celerrate_syntax/src/parser.rs                     modify: push_diagnostic dedupe; delete the superseded hand-driven fuse test
crates/celerrate_syntax/src/parser/grammar.rs             modify: shrink to source_file + statement_list_step + module declarations
crates/celerrate_syntax/src/parser/grammar/statements.rs  create: the whole statement grammar + its cfg(test) module
crates/celerrate_syntax/src/parser/grammar/expressions.rs modify: block moves out; pub(super) on shared rules
crates/celerrate_syntax/src/syntax_kind.rs                modify: statement node kinds (appended after Block)
crates/celerrate_syntax/src/diagnostic.rs                 modify: ExpectedStatement
crates/celerrate_syntax/tests/support/mod.rs              modify: render_statement, parser_diagnostics
crates/celerrate_syntax/tests/parse.rs                    modify: diagnostic dedupe test
crates/celerrate_syntax/tests/statements.rs               create: empty/block/return/break/continue/global/static/unset/goto/label
crates/celerrate_syntax/tests/statements_control_flow.rs  create: if/while/do/for/foreach, both syntaxes
crates/celerrate_syntax/tests/statements_switch_try.rs    create: switch, try/catch/finally
crates/celerrate_syntax/tests/statements_declarations.rs  create: declare, function declarations
crates/celerrate_syntax/tests/parse_corpus/statements_control_flow.php       create
crates/celerrate_syntax/tests/parse_corpus/statements_alternative_syntax.php create
crates/celerrate_syntax/tests/parse_corpus/statements_switch.php             create
crates/celerrate_syntax/tests/parse_corpus/statements_try.php                create
crates/celerrate_syntax/tests/parse_corpus/statements_declarations.php       create
crates/celerrate_syntax/tests/parse_corpus/statements_kitchen_sink.php       create
crates/celerrate_syntax/tests/parse_corpus/recovery_statements.php           create
fuzz/corpus/parse/seed_statements.php                     create
fuzz/corpus/parse/seed_statements_errors.php              create
```

Notes that apply to every task:

- **Inline snapshots** use `insta::assert_snapshot!(..., @r#"..."#)`. The expected trees below are authoritative for *shape and tokens*. If `cargo insta` reports an indentation-only mismatch, run `cargo insta accept` and verify the accepted snapshot matches the shape shown here node-for-node.
- **Corpus snapshots**: whenever a task changes how existing corpus files parse (a token that used to be `ErrorNode` wreckage becomes a real statement node), `cargo test` fails on `parse_corpus`. Inspect the diff: only ErrorNode-to-real-node improvements are acceptable; then `cargo insta accept`.
- **Test file preamble**: every new integration test file starts with

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_statement};
```

  (drop unused imports per file; `cargo clippy -- -D warnings` enforces it).

---

### Task 1: Extract the statements module; test the fuse on `statement_list` directly

Pure refactor plus a relocated regression test (deferred item 1). Statement rules move out of `grammar.rs`, `block` moves out of `expressions.rs`, and the shared statement-list loop becomes a named function a test can drive.

**Files:**
- Create: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (remove `block`)
- Modify: `crates/celerrate_syntax/src/parser.rs` (delete the superseded test)

**Interfaces:**
- Consumes: `Parser` (private to `parser`, visible to descendant modules), `super::statement_list_step`, `expressions::{error_element, expression, starts_expression}`.
- Produces: `statements::statement(parser: &mut Parser)`, `statements::statement_list(parser: &mut Parser)`, `statements::block(parser: &mut Parser)` — all `pub(super)`. Every later task adds rules to this module.

- [ ] **Step 1: Create `statements.rs` with the moved rules**

Move (verbatim bodies unless noted) `statement`, `echo_statement`, `expression_statement`, `error_statement`, `terminate_statement` from `grammar.rs`, and `block` from `expressions.rs`. Extract `block`'s loop as `statement_list`:

```rust
//! The statement grammar: dispatch over the leading token, one rule
//! per statement form. Every rule keeps the parser's guarantees: it
//! always makes progress and completes the node it opened.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::Parser;
use super::expressions::{error_element, expression, starts_expression};

pub(super) fn statement(parser: &mut Parser) {
    match parser.current() {
        Some(SyntaxKind::Echo) => echo_statement(parser),
        Some(kind) if starts_expression(kind) => expression_statement(parser),
        Some(_) => error_statement(parser),
        None => {}
    }
}

/// Statements until end of input or the list's closing brace. The
/// guard observes `current` (not `at_end`): once the fuse blows,
/// `current` reports `None` while real unconsumed tokens can still sit
/// past the raw position, and this loop must unwind, not spin. The
/// test module below drives exactly that state.
pub(super) fn statement_list(parser: &mut Parser) {
    while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
        super::statement_list_step(parser);
    }
}

/// `{ statements }`.
pub(super) fn block(parser: &mut Parser) {
    let marker = parser.start();
    if parser.at(SyntaxKind::OpenBrace) {
        parser.bump();
        statement_list(parser);
        parser.expect(SyntaxKind::CloseBrace);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::Block);
}
```

Then `echo_statement`, `expression_statement`, `error_statement`, `terminate_statement` exactly as they read in `grammar.rs` today (same doc comments).

- [ ] **Step 2: Rewire `grammar.rs` and `expressions.rs`**

`grammar.rs` shrinks to:

```rust
//! The grammar's top level: the source file loop and the shared
//! statement-list step. Statements and expressions each own a module.

use crate::syntax_kind::SyntaxKind;

use super::{CompletedMarker, Parser};

mod expressions;
mod statements;

pub(super) fn source_file(parser: &mut Parser) {
    let marker = parser.start();
    while parser.current().is_some() {
        statement_list_step(parser);
    }
    marker.complete(parser, SyntaxKind::SourceFile);
}

/// One step of a statement list: inline HTML and tags stay tokens;
/// everything else is a statement. Shared by the source file and every
/// nested statement list.
pub(super) fn statement_list_step(parser: &mut Parser) {
    match parser.current() {
        Some(
            SyntaxKind::InlineHtml
            | SyntaxKind::OpenTag
            | SyntaxKind::OpenTagEcho
            | SyntaxKind::ShortOpenTag
            | SyntaxKind::CloseTag,
        ) => parser.bump(),
        _ => statements::statement(parser),
    }
}
```

(The `use super::{CompletedMarker, Parser};` line stays: child modules resolve `super::CompletedMarker` through it.)

In `expressions.rs`: delete `block` and its doc comment; in `closure_or_arrow_function`, replace the `block(parser);` call with `super::statements::block(parser);`.

- [ ] **Step 3: Run the full suite — behavior must be unchanged**

Run: `cargo test --workspace`
Expected: PASS, zero snapshot diffs (this is a pure move).

- [ ] **Step 4: Replace the hand-driven fuse test with a direct one**

In `parser.rs`, delete the whole test `a_blown_fuse_inside_a_block_context_still_terminates` (its comment says it hand-drives the idiom because `block` was unreachable; that excuse is gone). In `statements.rs`, append:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use crate::diagnostic::ParserDiagnosticKind;
    use crate::parser::Parser;
    use crate::parser::token_source::TokenSource;
    use crate::syntax_kind::SyntaxKind;
    use crate::tree::SyntaxNode;

    #[test]
    fn a_blown_fuse_terminates_a_statement_list_with_tokens_remaining() {
        // The historical bug: a statement-list loop gated on `at_end`
        // instead of observing `current` spins forever once the fuse
        // blows, because the fuse silences `current` without moving
        // the raw token position. This drives the real
        // `statement_list` in exactly that state; a regression hangs
        // this test rather than failing it.
        let source = "<?php { 1; 2; 3; }";
        let (tokens, _lexer_diagnostics) = crate::lexer::lex(source);
        let mut parser = Parser::new(TokenSource::new(&tokens));
        let root = parser.start();
        parser.bump(); // `<?php`
        parser.bump(); // `{`
        for _ in 0..=Parser::MAXIMUM_STEPS_WITHOUT_PROGRESS {
            parser.current();
        }
        assert_eq!(parser.current(), None, "the fuse must have blown");
        assert!(
            !parser.at_end(),
            "real tokens must still remain unconsumed"
        );
        super::statement_list(&mut parser);
        parser.expect(SyntaxKind::CloseBrace);
        root.complete(&mut parser, SyntaxKind::SourceFile);
        parser.recover_unconsumed_tail();
        let tree = SyntaxNode::new_root(crate::tree::builder::build_tree(
            source,
            &tokens,
            parser.events,
        ));
        assert_eq!(
            tree.text().to_string(),
            source,
            "the tree must stay lossless even after the fuse blows mid-list"
        );
        let no_progress_count = parser
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == ParserDiagnosticKind::NoProgress)
            .count();
        assert_eq!(no_progress_count, 1, "exactly one NoProgress diagnostic");
    }
}
```

(All those items are private to the `parser` module and visible here because `statements` is its descendant. If the compiler disagrees on any path, fix the path, not the visibility.)

- [ ] **Step 5: Run the new test and the full suite**

Run: `cargo test --package celerrate_syntax a_blown_fuse_terminates_a_statement_list` then `cargo test --workspace`
Expected: PASS. (No red step here: this task is a refactor plus a relocated regression pin whose protective value is termination — a deliberate TDD exception, recorded.)

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A && git commit -m "♻️ refactor(syntax): extract the statement grammar into its own module"
```

---

### Task 2: Collapse consecutive duplicate diagnostics

Deferred item 3. Unwinding out of an exhausted nesting budget re-diagnoses the same missing token at the same offset once per unwound level: 127 identical adjacent `Expected(CloseParenthesis)` on `(`×140. One report carries the same information as a hundred.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser.rs`
- Test: `crates/celerrate_syntax/tests/parse.rs`

**Interfaces:**
- Produces: `Parser::push_diagnostic(&mut self, diagnostic: ParserDiagnostic)` (private), routed through by `diagnose_current`, `diagnose_missing`, and `recover_unconsumed_tail`.

- [ ] **Step 1: Write the failing test** (append to `tests/parse.rs`)

```rust
#[test]
fn exhausted_nesting_reports_each_finding_once() {
    // Unwinding out of an exhausted nesting budget used to emit one
    // identical Expected(CloseParenthesis) per unwound level, all
    // zero-width at the same offset: 127 adjacent duplicates on this
    // input.
    let source = format!("<?php {}1;", "(".repeat(140));
    let parse = support::parse_verified(&source);
    assert!(
        parse
            .diagnostics()
            .windows(2)
            .all(|pair| pair[0] != pair[1]),
        "no diagnostic may repeat its immediate predecessor"
    );
}
```

- [ ] **Step 2: Run it — must fail**

Run: `cargo test --package celerrate_syntax --test parse exhausted_nesting_reports_each_finding_once`
Expected: FAIL on the assertion (127 duplicate pairs today).

- [ ] **Step 3: Implement `push_diagnostic`** (in `impl Parser`)

```rust
/// Appends a diagnostic unless it repeats the previous one exactly.
/// Recovery that unwinds through many levels at one spot (an
/// exhausted nesting budget above all) re-diagnoses the same missing
/// token at the same offset once per level; one report suffices.
fn push_diagnostic(&mut self, diagnostic: ParserDiagnostic) {
    if self.diagnostics.last() == Some(&diagnostic) {
        return;
    }
    self.diagnostics.push(diagnostic);
}
```

Replace every direct `self.diagnostics.push(...)` in `diagnose_current`, `diagnose_missing`, and `recover_unconsumed_tail` with `self.push_diagnostic(...)` (build the `ParserDiagnostic` value first where needed).

- [ ] **Step 4: Run the test, then the full suite**

Run: `cargo test --package celerrate_syntax --test parse exhausted_nesting_reports_each_finding_once && cargo test --workspace`
Expected: PASS. If a corpus snapshot footer loses lines, verify each lost line was an exact adjacent duplicate, then `cargo insta accept`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "🐛 fix(syntax): collapse consecutive duplicate parser diagnostics"
```

---

### Task 3: Empty statements, blocks as statements, and the statement nesting guard

`;` and `{ ... }` become statements. Statement dispatch gains the same recursion budget expressions already have: without it, `{`×100000 is a stack overflow the moment blocks recurse.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `EmptyStatement` after `Block`)
- Modify: `crates/celerrate_syntax/tests/support/mod.rs`
- Test: `crates/celerrate_syntax/tests/statements.rs` (create)

**Interfaces:**
- Produces: node kind `EmptyStatement`; support helpers `render_statement(statement_source: &str) -> String` and `parser_diagnostics(source: &str) -> Vec<ParserDiagnosticKind>`; the guarded `statement` entry point every later task's rules sit behind.

- [ ] **Step 1: Add the support helpers** (`tests/support/mod.rs`)

```rust
/// Renders the first statement of `<?php {statement_source}` as an
/// indented tree of node kinds and token texts, offsets and trivia
/// omitted: the workhorse assertion of the statement grammar tests.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_statement(statement_source: &str) -> String {
    let source = format!("<?php {statement_source}");
    let parse = parse_verified(&source);
    let statement = parse.tree().children().next().expect("a first statement");
    let mut output = String::new();
    render_element_without_offsets(&mut output, statement.into(), 0);
    output
}

/// The parser-side diagnostics of one parse, lexer findings filtered
/// out, for structural assertions.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn parser_diagnostics(source: &str) -> Vec<celerrate_syntax::ParserDiagnosticKind> {
    parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            celerrate_syntax::SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            celerrate_syntax::SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}
```

- [ ] **Step 2: Write the failing tests** (`tests/statements.rs`, with the standard preamble)

```rust
#[test]
fn an_empty_statement_is_a_node() {
    insta::assert_snapshot!(render_statement(";"), @r#"
    EmptyStatement
      Semicolon ";"
    "#);
}

#[test]
fn a_brace_block_is_a_statement() {
    insta::assert_snapshot!(render_statement("{ echo 1; }"), @r#"
    Block
      OpenBrace "{"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      CloseBrace "}"
    "#);
}

#[test]
fn an_unclosed_block_is_diagnosed_and_completes() {
    assert!(
        parser_diagnostics("<?php { echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace))
    );
}

#[test]
fn pathological_block_nesting_trips_the_guard_without_overflowing() {
    // 300 nested blocks: the statement guard must refuse past the
    // budget and keep consuming, never overflow the stack.
    let source = format!("<?php {}", "{".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        celerrate_syntax::SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}
```

- [ ] **Step 3: Run them — must fail**

Run: `cargo test --package celerrate_syntax --test statements`
Expected: FAIL — `;` and `{` currently parse as `ErrorNode`, and no `NestingTooDeep` appears for nested braces.

- [ ] **Step 4: Implement** (in `statements.rs`)

Add `EmptyStatement,` to `syntax_kind.rs` immediately after `Block,` with doc comment `/// A lone \`;\`.`. Then restructure `statement`:

```rust
pub(super) fn statement(parser: &mut Parser) {
    if !parser.enter_nesting() {
        // The budget refused the whole statement without consuming;
        // wrap one token so every enclosing statement list keeps
        // progressing (the same contract the expression lists use).
        if parser.current().is_some() {
            error_element(parser);
        }
        return;
    }
    dispatch_statement(parser);
    parser.leave_nesting();
}

fn dispatch_statement(parser: &mut Parser) {
    match parser.current() {
        Some(SyntaxKind::OpenBrace) => block(parser),
        Some(SyntaxKind::Semicolon) => empty_statement(parser),
        Some(SyntaxKind::Echo) => echo_statement(parser),
        Some(kind) if starts_expression(kind) => expression_statement(parser),
        Some(_) => error_statement(parser),
        None => {}
    }
}

/// A lone `;`: a complete statement in PHP.
fn empty_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump();
    marker.complete(parser, SyntaxKind::EmptyStatement);
}
```

- [ ] **Step 5: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test statements && cargo test --workspace`
Expected: PASS. Corpus snapshots for existing recovery files may improve (stray `;`/`{` were ErrorNodes); inspect and `cargo insta accept`.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse empty statements and brace blocks as statements"
```

---

### Task 4: `return`, `break`, `continue`

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `ReturnStatement`, `BreakStatement`, `ContinueStatement`)
- Test: `crates/celerrate_syntax/tests/statements.rs`

**Interfaces:**
- Consumes: `expression`, `starts_expression`, `terminate_statement`.
- Produces: the three node kinds; dispatch arms for `Return`, `Break`, `Continue`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_bare_return_terminates() {
    insta::assert_snapshot!(render_statement("return;"), @r#"
    ReturnStatement
      Return "return"
      Semicolon ";"
    "#);
}

#[test]
fn return_carries_an_optional_expression() {
    insta::assert_snapshot!(render_statement("return 1 + 2;"), @r#"
    ReturnStatement
      Return "return"
      BinaryExpression
        Literal
          IntegerLiteral "1"
        Plus "+"
        Literal
          IntegerLiteral "2"
      Semicolon ";"
    "#);
}

#[test]
fn break_and_continue_carry_an_optional_level() {
    insta::assert_snapshot!(render_statement("break 2;"), @r#"
    BreakStatement
      Break "break"
      Literal
        IntegerLiteral "2"
      Semicolon ";"
    "#);
    insta::assert_snapshot!(render_statement("continue;"), @r#"
    ContinueStatement
      Continue "continue"
      Semicolon ";"
    "#);
}

#[test]
fn a_missing_statement_terminator_is_diagnosed() {
    assert!(parser_diagnostics("<?php return 1").contains(&ParserDiagnosticKind::ExpectedSemicolon));
}
```

- [ ] **Step 2: Run — must fail** (`cargo test -p celerrate_syntax --test statements`; the keywords parse as ErrorNode today)

- [ ] **Step 3: Implement**

Node kinds after `EmptyStatement`:

```rust
    /// `return;` or `return expression;`.
    ReturnStatement,
    /// `break;` or `break level;`; level validity is semantic.
    BreakStatement,
    /// `continue;` or `continue level;`; level validity is semantic.
    ContinueStatement,
```

Dispatch arms (before the `starts_expression` arm):

```rust
        Some(SyntaxKind::Return) => return_statement(parser),
        Some(SyntaxKind::Break) => keyword_optional_expression_statement(parser, SyntaxKind::BreakStatement),
        Some(SyntaxKind::Continue) => keyword_optional_expression_statement(parser, SyntaxKind::ContinueStatement),
```

Rules:

```rust
fn return_statement(parser: &mut Parser) {
    keyword_optional_expression_statement(parser, SyntaxKind::ReturnStatement);
}

/// A keyword, an optional expression, the terminator: `return`,
/// `break`, and `continue` share the shape.
fn keyword_optional_expression_statement(parser: &mut Parser, kind: SyntaxKind) {
    let marker = parser.start();
    parser.bump();
    if parser.current().is_some_and(starts_expression) {
        expression(parser);
    }
    terminate_statement(parser);
    marker.complete(parser, kind);
}
```

(Fold `return_statement` away and dispatch all three through the shared helper directly — shown separately only for readability; pick one and be consistent.)

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse return, break, and continue statements"
```

---

### Task 5: `global`, `static` variables, `unset`

Deferred item 2 lands here: `static` directly followed by a variable is a statement; every other `static` stays an expression.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (`pub(super)` on `simple_variable` and `argument_list`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `GlobalStatement`, `StaticStatement`, `StaticVariable`, `UnsetStatement`)
- Test: `crates/celerrate_syntax/tests/statements.rs`

**Interfaces:**
- Consumes: `expressions::simple_variable(parser) -> Option<CompletedMarker>` (newly `pub(super)`), `expressions::argument_list(parser)` (newly `pub(super)`).
- Produces: the four node kinds; dispatch arms for `Global`, `Unset`, and guarded `Static`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn global_lists_variables() {
    insta::assert_snapshot!(render_statement("global $configuration, $$indirect;"), @r#"
    GlobalStatement
      Global "global"
      VariableReference
        Variable "$configuration"
      Comma ","
      DynamicVariableExpression
        Dollar "$"
        VariableReference
          Variable "$indirect"
      Semicolon ";"
    "#);
}

#[test]
fn global_without_a_variable_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php global;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Variable))
    );
}

#[test]
fn static_variables_declare_with_optional_initializers() {
    insta::assert_snapshot!(render_statement("static $count = 0, $names;"), @r#"
    StaticStatement
      Static "static"
      StaticVariable
        Variable "$count"
        Equals "="
        Literal
          IntegerLiteral "0"
      Comma ","
      StaticVariable
        Variable "$names"
      Semicolon ";"
    "#);
}

#[test]
fn static_scoped_access_stays_an_expression_statement() {
    insta::assert_snapshot!(render_statement("static::create();"), @r#"
    ExpressionStatement
      CallExpression
        ScopedAccessExpression
          NameExpression
            Static "static"
          ColonColon "::"
          MemberName
            Identifier "create"
        ArgumentList
          OpenParenthesis "("
          CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn a_static_closure_stays_an_expression_statement() {
    assert!(parser_diagnostics("<?php static fn () => 1;").is_empty());
}

#[test]
fn unset_takes_a_parenthesized_list() {
    insta::assert_snapshot!(render_statement("unset($map['key'], $other);"), @r#"
    UnsetStatement
      Unset "unset"
      ArgumentList
        OpenParenthesis "("
        Argument
          IndexExpression
            VariableReference
              Variable "$map"
            OpenBracket "["
            Literal
              SingleQuotedString "'key'"
            CloseBracket "]"
        Comma ","
        Argument
          VariableReference
            Variable "$other"
        CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn unset_without_parentheses_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php unset;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis))
    );
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

In `expressions.rs`, change `fn simple_variable` and `fn argument_list` to `pub(super) fn`. Node kinds after `ContinueStatement`:

```rust
    /// `global $a, $b;`.
    GlobalStatement,
    /// `static $a = 1, $b;`, the function-static declaration.
    StaticStatement,
    /// One declared static: the variable and its optional initializer.
    StaticVariable,
    /// `unset( targets );`.
    UnsetStatement,
```

Dispatch arms (`Static` guarded, placed before the `starts_expression` arm; `Static` is in `starts_expression`, so order is load-bearing):

```rust
        Some(SyntaxKind::Global) => global_statement(parser),
        Some(SyntaxKind::Unset) => unset_statement(parser),
        Some(SyntaxKind::Static) if parser.nth(1) == Some(SyntaxKind::Variable) => {
            static_statement(parser)
        }
```

Rules:

```rust
/// `global $a, $$b;`. Terminates: each iteration either consumed a
/// variable form or breaks; the comma is consumed before looping.
fn global_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `global`
    loop {
        if simple_variable(parser).is_none() {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
            break;
        }
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::GlobalStatement);
}

/// `static $a = 1, $b;`: dispatched only when `static` is directly
/// followed by a variable; `static::`, `static function`, and
/// `static fn` stay expressions.
fn static_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `static`
    loop {
        if !parser.at(SyntaxKind::Variable) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
            break;
        }
        let variable = parser.start();
        parser.bump();
        if parser.eat(SyntaxKind::Equals) {
            expression(parser);
        }
        variable.complete(parser, SyntaxKind::StaticVariable);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::StaticStatement);
}

/// `unset( targets );`. The shared argument list brings its recovery;
/// which targets are unsettable is semantic.
fn unset_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `unset`
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::UnsetStatement);
}
```

Update the `statements.rs` import line to pull the two new names from `super::expressions`.

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse global, static variable, and unset statements"
```

---

### Task 6: `goto` and labels

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `GotoStatement`, `LabelStatement`)
- Test: `crates/celerrate_syntax/tests/statements.rs`

**Interfaces:**
- Produces: the two node kinds; dispatch arms for `Goto` and for `Identifier` immediately followed by `Colon`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn goto_names_a_label() {
    insta::assert_snapshot!(render_statement("goto cleanup;"), @r#"
    GotoStatement
      Goto "goto"
      Identifier "cleanup"
      Semicolon ";"
    "#);
}

#[test]
fn an_identifier_followed_by_a_colon_is_a_label() {
    insta::assert_snapshot!(render_statement("cleanup: echo 1;"), @r#"
    LabelStatement
      Identifier "cleanup"
      Colon ":"
    "#);
}

#[test]
fn a_call_statement_is_not_mistaken_for_a_label() {
    assert!(parser_diagnostics("<?php cleanup();").is_empty());
}

#[test]
fn goto_without_a_label_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php goto ;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Identifier))
    );
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

Node kinds after `UnsetStatement`:

```rust
    /// `goto label;`; whether the label exists is semantic.
    GotoStatement,
    /// `label:`, the target of a `goto`.
    LabelStatement,
```

Dispatch arms — the label arm must sit before the `starts_expression` arm, since `Identifier` starts an expression:

```rust
        Some(SyntaxKind::Goto) => goto_statement(parser),
        Some(SyntaxKind::Identifier) if parser.nth(1) == Some(SyntaxKind::Colon) => {
            label_statement(parser)
        }
```

(`Foo::bar()` cannot collide: `::` lexes as one `ColonColon` token.)

```rust
fn goto_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `goto`
    parser.expect(SyntaxKind::Identifier);
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::GotoStatement);
}

/// The dispatcher guaranteed the identifier-colon shape.
fn label_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // the label
    parser.bump(); // `:`
    marker.complete(parser, SyntaxKind::LabelStatement);
}
```

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse goto statements and labels"
```

---

### Task 7: `if` / `elseif` / `else`, classic form

Brings the three pieces every control-flow statement reuses: the parenthesized condition, the embedded-statement body, and the statement-list terminator predicate.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `IfStatement`, `ElseIfClause`, `ElseClause`)
- Modify: `crates/celerrate_syntax/src/diagnostic.rs` (add `ExpectedStatement`)
- Test: `crates/celerrate_syntax/tests/statements_control_flow.rs` (create, standard preamble)

**Interfaces:**
- Produces: `parenthesized_condition(parser)`, `embedded_statement(parser)`, `at_statement_list_terminator(parser) -> bool` (all private to `statements`), diagnostic kind `ExpectedStatement`, three node kinds. Tasks 9-14 consume all three helpers.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_classic_if_wraps_condition_and_body() {
    insta::assert_snapshot!(render_statement("if ($x) echo 1;"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
    "#);
}

#[test]
fn elseif_and_else_are_clauses() {
    insta::assert_snapshot!(render_statement("if ($a) { } elseif ($b) { } else { }"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$a"
      CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
      ElseIfClause
        ElseIf "elseif"
        OpenParenthesis "("
        VariableReference
          Variable "$b"
        CloseParenthesis ")"
        Block
          OpenBrace "{"
          CloseBrace "}"
      ElseClause
        Else "else"
        Block
          OpenBrace "{"
          CloseBrace "}"
    "#);
}

#[test]
fn else_if_with_a_space_nests_an_if_inside_the_else() {
    insta::assert_snapshot!(render_statement("if ($a) echo 1; else if ($b) echo 2;"), @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$a"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      ElseClause
        Else "else"
        IfStatement
          If "if"
          OpenParenthesis "("
          VariableReference
            Variable "$b"
          CloseParenthesis ")"
          EchoStatement
            Echo "echo"
            Literal
              IntegerLiteral "2"
            Semicolon ";"
    "#);
}

#[test]
fn a_dangling_else_binds_to_the_innermost_if() {
    let rendered = render_statement("if ($a) if ($b) echo 1; else echo 2;");
    // The outer if has no ElseClause child of its own: the else sits
    // inside the inner IfStatement.
    let inner_holds_else = rendered
        .lines()
        .any(|line| line.trim() == "ElseClause" && line.starts_with("    "));
    assert!(inner_holds_else, "the else must nest inside the inner if:\n{rendered}");
}

#[test]
fn a_missing_condition_parenthesis_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php if $x echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis))
    );
}

#[test]
fn a_missing_body_is_diagnosed_without_consuming() {
    assert!(parser_diagnostics("<?php if ($x)").contains(&ParserDiagnosticKind::ExpectedStatement));
}
```

- [ ] **Step 2: Run — must fail** (also a compile fail: `ExpectedStatement` does not exist yet).

- [ ] **Step 3: Implement**

`diagnostic.rs`, after `ExpectedMemberName` in `ParserDiagnosticKind`:

```rust
    /// A control-flow body position holds no statement (`if ($x)` at
    /// end of input, or directly against a closing keyword).
    ExpectedStatement,
```

Node kinds after `LabelStatement`:

```rust
    /// `if (condition) body`, with optional `ElseIfClause`s and one
    /// optional `ElseClause`, in either classic or alternative syntax.
    IfStatement,
    /// `elseif (condition) body` (or its alternative-syntax form).
    ElseIfClause,
    /// `else body` (or its alternative-syntax form).
    ElseClause,
```

Helpers and rule in `statements.rs`:

```rust
/// The tokens that end a nested statement list. A nested list never
/// consumes one of these: the construct that owns it eats it, and an
/// orphan unwinds to the source-file loop, which consumes anything.
/// This is the plan's contextual recovery set, and the reason every
/// nested list terminates: unwinding consumes nothing, but the top
/// level always progresses.
fn at_statement_list_terminator(parser: &mut Parser) -> bool {
    matches!(
        parser.current(),
        Some(
            SyntaxKind::CloseBrace
                | SyntaxKind::EndIf
                | SyntaxKind::EndWhile
                | SyntaxKind::EndFor
                | SyntaxKind::EndForeach
                | SyntaxKind::EndSwitch
                | SyntaxKind::EndDeclare
                | SyntaxKind::Else
                | SyntaxKind::ElseIf
                | SyntaxKind::Case
                | SyntaxKind::Default
        )
    )
}

/// `( expression )` after a control-flow keyword. The tokens stay
/// flat under the statement node, like `match`'s subject.
fn parenthesized_condition(parser: &mut Parser) {
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        expression(parser);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
}

/// The single embedded statement of a control-flow body
/// (`if (c) body`). A statement-list terminator or end of input means
/// the body is missing: diagnosed, never consumed, so the enclosing
/// construct recovers its own closer.
fn embedded_statement(parser: &mut Parser) {
    if parser.current().is_none() || at_statement_list_terminator(parser) {
        parser.diagnose_missing(ParserDiagnosticKind::ExpectedStatement);
        return;
    }
    super::statement_list_step(parser);
}

fn if_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `if`
    parenthesized_condition(parser);
    embedded_statement(parser);
    while parser.at(SyntaxKind::ElseIf) {
        let clause = parser.start();
        parser.bump();
        parenthesized_condition(parser);
        embedded_statement(parser);
        clause.complete(parser, SyntaxKind::ElseIfClause);
    }
    if parser.at(SyntaxKind::Else) {
        let clause = parser.start();
        parser.bump();
        embedded_statement(parser);
        clause.complete(parser, SyntaxKind::ElseClause);
    }
    marker.complete(parser, SyntaxKind::IfStatement);
}
```

Dispatch arm: `Some(SyntaxKind::If) => if_statement(parser),`.

(Dangling else falls out for free: the innermost `if_statement` call checks `at(Else)` first.)

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse if statements"
```

---

### Task 8: Alternative syntax for `if`; the shared terminator set goes live

`statement_list` adopts `at_statement_list_terminator`, changing recovery for every nested list at once, and `if` gains its `: ... endif;` form — including inline HTML interruption, the templating idiom the design calls out.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Test: `crates/celerrate_syntax/tests/statements_control_flow.rs`

**Interfaces:**
- Consumes: Task 7's helpers.
- Produces: `statement_list` with the full terminator set — the contract every remaining task builds on.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn an_alternative_if_closes_with_endif() {
    insta::assert_snapshot!(
        render_statement("if ($x): echo 1; elseif ($y): echo 2; else: echo 3; endif;"),
        @r#"
    IfStatement
      If "if"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      Colon ":"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      ElseIfClause
        ElseIf "elseif"
        OpenParenthesis "("
        VariableReference
          Variable "$y"
        CloseParenthesis ")"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "2"
          Semicolon ";"
      ElseClause
        Else "else"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "3"
          Semicolon ";"
      EndIf "endif"
      Semicolon ";"
    "#);
}

#[test]
fn inline_html_interrupts_an_alternative_body() {
    // The templating idiom: the body of the colon form is raw HTML
    // between a close tag and the next open tag.
    assert!(parser_diagnostics("<?php if ($x): ?><p>yes</p><?php endif;").is_empty());
}

#[test]
fn a_missing_endif_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php if ($x): echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::EndIf))
    );
}

#[test]
fn a_block_does_not_swallow_an_alternative_closer() {
    // `endif` inside braces belongs to nobody: the block stops,
    // diagnoses its missing brace, and the orphan surfaces to the
    // source-file loop as an error element.
    let diagnostics = parser_diagnostics("<?php { endif; }");
    assert!(diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace)));
    assert!(diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken));
}
```

- [ ] **Step 2: Run — must fail** (the colon today lands in `embedded_statement` as an error element; `{ endif; }` consumes `endif` silently as wreckage inside the block — the last test's `Expected(CloseBrace)` assertion fails).

- [ ] **Step 3: Implement**

`statement_list` swaps its brace check for the shared predicate (doc comment updated to say nested lists stop at any terminator):

```rust
pub(super) fn statement_list(parser: &mut Parser) {
    while parser.current().is_some() && !at_statement_list_terminator(parser) {
        super::statement_list_step(parser);
    }
}
```

`if_statement` grows the alternative branch after `parenthesized_condition`:

```rust
fn if_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `if`
    parenthesized_condition(parser);
    if parser.eat(SyntaxKind::Colon) {
        statement_list(parser);
        while parser.at(SyntaxKind::ElseIf) {
            let clause = parser.start();
            parser.bump();
            parenthesized_condition(parser);
            parser.expect(SyntaxKind::Colon);
            statement_list(parser);
            clause.complete(parser, SyntaxKind::ElseIfClause);
        }
        if parser.at(SyntaxKind::Else) {
            let clause = parser.start();
            parser.bump();
            parser.expect(SyntaxKind::Colon);
            statement_list(parser);
            clause.complete(parser, SyntaxKind::ElseClause);
        }
        parser.expect(SyntaxKind::EndIf);
        terminate_statement(parser);
    } else {
        embedded_statement(parser);
        while parser.at(SyntaxKind::ElseIf) {
            let clause = parser.start();
            parser.bump();
            parenthesized_condition(parser);
            embedded_statement(parser);
            clause.complete(parser, SyntaxKind::ElseIfClause);
        }
        if parser.at(SyntaxKind::Else) {
            let clause = parser.start();
            parser.bump();
            embedded_statement(parser);
            clause.complete(parser, SyntaxKind::ElseClause);
        }
    }
    marker.complete(parser, SyntaxKind::IfStatement);
}
```

- [ ] **Step 4: Run tests + full suite.** Existing corpus/recovery snapshots may shift where a block used to swallow a closer; verify each diff moves wreckage out of the block (never loses a token), then `cargo insta accept`.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse the alternative if syntax behind shared statement-list terminators"
```

---

### Task 9: `while` and `do`-`while`

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `WhileStatement`, `DoWhileStatement`)
- Test: `crates/celerrate_syntax/tests/statements_control_flow.rs`

**Interfaces:**
- Consumes: `parenthesized_condition`, `embedded_statement`, `statement_list`, `terminate_statement`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn while_takes_both_syntaxes() {
    insta::assert_snapshot!(render_statement("while ($x) echo 1;"), @r#"
    WhileStatement
      While "while"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
    "#);
    insta::assert_snapshot!(render_statement("while ($x): echo 1; endwhile;"), @r#"
    WhileStatement
      While "while"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      Colon ":"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      EndWhile "endwhile"
      Semicolon ";"
    "#);
}

#[test]
fn do_while_puts_the_condition_after_the_body() {
    insta::assert_snapshot!(render_statement("do echo 1; while ($x);"), @r#"
    DoWhileStatement
      Do "do"
      EchoStatement
        Echo "echo"
        Literal
          IntegerLiteral "1"
        Semicolon ";"
      While "while"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn do_without_while_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php do echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::While))
    );
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

Node kinds after `ElseClause`:

```rust
    /// `while (condition) body`, either syntax.
    WhileStatement,
    /// `do body while (condition);`.
    DoWhileStatement,
```

```rust
fn while_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `while`
    parenthesized_condition(parser);
    if parser.eat(SyntaxKind::Colon) {
        statement_list(parser);
        parser.expect(SyntaxKind::EndWhile);
        terminate_statement(parser);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::WhileStatement);
}

fn do_while_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `do`
    embedded_statement(parser);
    parser.expect(SyntaxKind::While);
    parenthesized_condition(parser);
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::DoWhileStatement);
}
```

Dispatch arms: `Some(SyntaxKind::While) => while_statement(parser),` and `Some(SyntaxKind::Do) => do_while_statement(parser),`.

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse while and do-while statements"
```

---

### Task 10: `for`

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `ForStatement`, `ForExpressionList`)
- Test: `crates/celerrate_syntax/tests/statements_control_flow.rs`

**Interfaces:**
- Produces: `ForExpressionList` — always three per well-formed `for`, in initializer/condition/update order, so the future typed AST addresses sections positionally.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn for_holds_three_sections_and_a_body() {
    insta::assert_snapshot!(render_statement("for ($i = 0; $i < 3; $i++) echo $i;"), @r#"
    ForStatement
      For "for"
      OpenParenthesis "("
      ForExpressionList
        AssignmentExpression
          VariableReference
            Variable "$i"
          Equals "="
          Literal
            IntegerLiteral "0"
      Semicolon ";"
      ForExpressionList
        BinaryExpression
          VariableReference
            Variable "$i"
          Less "<"
          Literal
            IntegerLiteral "3"
      Semicolon ";"
      ForExpressionList
        PostfixExpression
          VariableReference
            Variable "$i"
          PlusPlus "++"
      CloseParenthesis ")"
      EchoStatement
        Echo "echo"
        VariableReference
          Variable "$i"
        Semicolon ";"
    "#);
}

#[test]
fn for_sections_may_be_empty_or_lists() {
    insta::assert_snapshot!(render_statement("for (;;) ;"), @r#"
    ForStatement
      For "for"
      OpenParenthesis "("
      ForExpressionList
      Semicolon ";"
      ForExpressionList
      Semicolon ";"
      ForExpressionList
      CloseParenthesis ")"
      EmptyStatement
        Semicolon ";"
    "#);
    assert!(parser_diagnostics("<?php for ($i = 0, $j = 9; $i < $j; $i++, $j--) ;").is_empty());
}

#[test]
fn an_alternative_for_closes_with_endfor() {
    assert!(parser_diagnostics("<?php for (;;): echo 1; endfor;").is_empty());
    assert!(
        parser_diagnostics("<?php for (;;): echo 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::EndFor))
    );
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

Node kinds after `DoWhileStatement`:

```rust
    /// `for (initializers; condition; updates) body`, either syntax.
    ForStatement,
    /// One of `for`'s three sections: a possibly-empty comma-separated
    /// expression list, always present as a node so the sections stay
    /// addressable by position.
    ForExpressionList,
```

```rust
fn for_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `for`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        for_expression_list(parser);
        parser.expect(SyntaxKind::Semicolon);
        for_expression_list(parser);
        parser.expect(SyntaxKind::Semicolon);
        for_expression_list(parser);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.eat(SyntaxKind::Colon) {
        statement_list(parser);
        parser.expect(SyntaxKind::EndFor);
        terminate_statement(parser);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::ForStatement);
}

/// One `for` section. Progress is enforced mechanically: the nesting
/// guard can refuse an expression without consuming (this loop can be
/// entered while the budget is exhausted, through a pathological
/// condition chain); an unmoved position breaks out and leaves the
/// token to the section's caller.
fn for_expression_list(parser: &mut Parser) {
    let marker = parser.start();
    while parser.current().is_some_and(starts_expression) {
        let position_before_expression = parser.position();
        expression(parser);
        if parser.position() == position_before_expression {
            break;
        }
        if !parser.eat(SyntaxKind::Comma) && parser.current().is_some_and(starts_expression) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Comma));
        }
    }
    marker.complete(parser, SyntaxKind::ForExpressionList);
}
```

Dispatch arm: `Some(SyntaxKind::For) => for_statement(parser),`.

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse for statements"
```

---

### Task 11: `foreach`

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `ForeachStatement`)
- Test: `crates/celerrate_syntax/tests/statements_control_flow.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn foreach_binds_key_and_value() {
    insta::assert_snapshot!(render_statement("foreach ($map as $key => $value) ;"), @r#"
    ForeachStatement
      Foreach "foreach"
      OpenParenthesis "("
      VariableReference
        Variable "$map"
      As "as"
      VariableReference
        Variable "$key"
      FatArrow "=>"
      VariableReference
        Variable "$value"
      CloseParenthesis ")"
      EmptyStatement
        Semicolon ";"
    "#);
}

#[test]
fn foreach_values_may_be_by_reference_or_destructured() {
    assert!(parser_diagnostics("<?php foreach ($queue as &$task) ;").is_empty());
    assert!(parser_diagnostics("<?php foreach ($pairs as [$a, $b]) ;").is_empty());
    assert!(parser_diagnostics("<?php foreach ($pairs as $k => list($a, $b)) ;").is_empty());
}

#[test]
fn an_alternative_foreach_closes_with_endforeach() {
    assert!(parser_diagnostics("<?php foreach ($items as $item): echo $item; endforeach;").is_empty());
}

#[test]
fn foreach_without_as_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php foreach ($items $item) ;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::As))
    );
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

Node kind after `ForExpressionList`:

```rust
    /// `foreach (subject as key => value) body`, either syntax; the
    /// `=>` separates the optional key target from the value target.
    ForeachStatement,
```

```rust
fn foreach_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `foreach`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        expression(parser);
        parser.expect(SyntaxKind::As);
        foreach_target(parser);
        if parser.eat(SyntaxKind::FatArrow) {
            foreach_target(parser);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.eat(SyntaxKind::Colon) {
        statement_list(parser);
        parser.expect(SyntaxKind::EndForeach);
        terminate_statement(parser);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::ForeachStatement);
}

/// One binding target: optional `&`, then an expression (a variable,
/// `[...]`/`list(...)` destructuring, a member chain). `=>` is not a
/// binary operator, so the expression stops before it. Assignability
/// is semantic.
fn foreach_target(parser: &mut Parser) {
    parser.eat(SyntaxKind::Ampersand);
    expression(parser);
}
```

Dispatch arm: `Some(SyntaxKind::Foreach) => foreach_statement(parser),`.

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse foreach statements"
```

---

### Task 12: `switch`

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `SwitchStatement`, `SwitchCase`)
- Test: `crates/celerrate_syntax/tests/statements_switch_try.rs` (create, standard preamble)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn switch_holds_cases_and_a_default() {
    insta::assert_snapshot!(
        render_statement("switch ($x) { case 1: echo 1; break; default: echo 0; }"),
        @r#"
    SwitchStatement
      Switch "switch"
      OpenParenthesis "("
      VariableReference
        Variable "$x"
      CloseParenthesis ")"
      OpenBrace "{"
      SwitchCase
        Case "case"
        Literal
          IntegerLiteral "1"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "1"
          Semicolon ";"
        BreakStatement
          Break "break"
          Semicolon ";"
      SwitchCase
        Default "default"
        Colon ":"
        EchoStatement
          Echo "echo"
          Literal
            IntegerLiteral "0"
          Semicolon ";"
      CloseBrace "}"
    "#);
}

#[test]
fn cases_fall_through_and_accept_semicolon_terminators() {
    // `case 1;` is Zend-legal; an empty case body falls through.
    assert!(parser_diagnostics("<?php switch ($x) { case 1: case 2; echo 1; }").is_empty());
}

#[test]
fn switch_tolerates_one_leading_semicolon() {
    assert!(parser_diagnostics("<?php switch ($x) { ; case 1: echo 1; }").is_empty());
}

#[test]
fn an_alternative_switch_closes_with_endswitch() {
    assert!(parser_diagnostics("<?php switch ($x): case 1: echo 1; endswitch;").is_empty());
}

#[test]
fn junk_between_cases_is_wrapped_and_consumed() {
    let diagnostics = parser_diagnostics("<?php switch ($x) { junk case 1: echo 1; }");
    assert!(diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken));
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

Node kinds after `ForeachStatement`:

```rust
    /// `switch (subject) { cases }`, either syntax.
    SwitchStatement,
    /// One `case expression:` or `default:` section, its statements
    /// included; the body ends where the next section (or the switch)
    /// begins, so an empty body is a fallthrough.
    SwitchCase,
```

```rust
fn switch_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `switch`
    parenthesized_condition(parser);
    if parser.eat(SyntaxKind::OpenBrace) {
        // Zend tolerates one stray `;` before the first case.
        parser.eat(SyntaxKind::Semicolon);
        switch_case_list(parser, SyntaxKind::CloseBrace);
        parser.expect(SyntaxKind::CloseBrace);
    } else if parser.eat(SyntaxKind::Colon) {
        parser.eat(SyntaxKind::Semicolon);
        switch_case_list(parser, SyntaxKind::EndSwitch);
        parser.expect(SyntaxKind::EndSwitch);
        terminate_statement(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::SwitchStatement);
}

/// Sections until `closing`. Terminates: the case and default arms
/// always bump their keyword before anything refusable, and the
/// fallback arm is an error element, which always bumps.
fn switch_case_list(parser: &mut Parser, closing: SyntaxKind) {
    while parser.current().is_some() && !parser.at(closing) {
        match parser.current() {
            Some(SyntaxKind::Case) => {
                let case = parser.start();
                parser.bump();
                expression(parser);
                switch_case_separator(parser);
                statement_list(parser);
                case.complete(parser, SyntaxKind::SwitchCase);
            }
            Some(SyntaxKind::Default) => {
                let case = parser.start();
                parser.bump();
                switch_case_separator(parser);
                statement_list(parser);
                case.complete(parser, SyntaxKind::SwitchCase);
            }
            _ => error_element(parser),
        }
    }
}

/// `:` or the Zend-legal `;` after a case label.
fn switch_case_separator(parser: &mut Parser) {
    if !parser.eat(SyntaxKind::Colon) && !parser.eat(SyntaxKind::Semicolon) {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Colon));
    }
}
```

Dispatch arm: `Some(SyntaxKind::Switch) => switch_statement(parser),`.

(Case bodies stop at `Case`/`Default`/`CloseBrace`/`EndSwitch` through the shared terminator set — that is what makes fallthrough shapes come out right. A body statement that hits a foreign closer like `endwhile` unwinds to `switch_case_list`, whose error-element arm consumes it: progress holds.)

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse switch statements"
```

---

### Task 13: `try` / `catch` / `finally`

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (`pub(super)` on `name`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `TryStatement`, `CatchClause`, `FinallyClause`)
- Test: `crates/celerrate_syntax/tests/statements_switch_try.rs`

**Interfaces:**
- Consumes: `expressions::name(parser) -> CompletedMarker` (newly `pub(super)`); `block`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn try_catch_finally_holds_clauses() {
    insta::assert_snapshot!(
        render_statement("try { } catch (LogicException | \\RuntimeException $error) { } finally { }"),
        @r#"
    TryStatement
      Try "try"
      Block
        OpenBrace "{"
        CloseBrace "}"
      CatchClause
        Catch "catch"
        OpenParenthesis "("
        Name
          Identifier "LogicException"
        Pipe "|"
        Name
          Backslash "\\"
          Identifier "RuntimeException"
        VariableReference
          Variable "$error"
        CloseParenthesis ")"
        Block
          OpenBrace "{"
          CloseBrace "}"
      FinallyClause
        Finally "finally"
        Block
          OpenBrace "{"
          CloseBrace "}"
    "#);
}

#[test]
fn catch_variables_are_optional_since_php_8() {
    assert!(parser_diagnostics("<?php try { } catch (Throwable) { }").is_empty());
}

#[test]
fn finally_alone_satisfies_a_try() {
    assert!(parser_diagnostics("<?php try { } finally { }").is_empty());
}

#[test]
fn a_bare_try_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php try { }")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Catch))
    );
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

In `expressions.rs`, change `fn name` to `pub(super) fn name`. Node kinds after `SwitchCase`:

```rust
    /// `try block`, then catch clauses and an optional finally.
    TryStatement,
    /// `catch (Type | Type $variable) block`; the variable is optional
    /// since PHP 8.0.
    CatchClause,
    /// `finally block`.
    FinallyClause,
```

```rust
fn try_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `try`
    block(parser);
    let mut caught = false;
    while parser.at(SyntaxKind::Catch) {
        caught = true;
        catch_clause(parser);
    }
    if parser.at(SyntaxKind::Finally) {
        let clause = parser.start();
        parser.bump();
        block(parser);
        clause.complete(parser, SyntaxKind::FinallyClause);
    } else if !caught {
        // Zend rejects `try` without a single catch or finally.
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Catch));
    }
    marker.complete(parser, SyntaxKind::TryStatement);
}

fn catch_clause(parser: &mut Parser) {
    let clause = parser.start();
    parser.bump(); // `catch`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        // `A | B\C`: qualified names separated by pipes. `name`
        // self-recovers (diagnoses a missing identifier) on absence,
        // and each loop iteration consumed its pipe: progress holds.
        name(parser);
        while parser.eat(SyntaxKind::Pipe) {
            name(parser);
        }
        if parser.at(SyntaxKind::Variable) {
            let variable = parser.start();
            parser.bump();
            variable.complete(parser, SyntaxKind::VariableReference);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    block(parser);
    clause.complete(parser, SyntaxKind::CatchClause);
}
```

Dispatch arm: `Some(SyntaxKind::Try) => try_statement(parser),`.

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse try, catch, and finally"
```

---

### Task 14: `declare`

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `DeclareStatement`, `DeclareDirective`)
- Test: `crates/celerrate_syntax/tests/statements_declarations.rs` (create, standard preamble)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn declare_takes_directives_and_a_body() {
    insta::assert_snapshot!(render_statement("declare(strict_types=1);"), @r#"
    DeclareStatement
      Declare "declare"
      OpenParenthesis "("
      DeclareDirective
        Identifier "strict_types"
        Equals "="
        Literal
          IntegerLiteral "1"
      CloseParenthesis ")"
      EmptyStatement
        Semicolon ";"
    "#);
}

#[test]
fn declare_accepts_every_body_form() {
    assert!(parser_diagnostics("<?php declare(ticks=1) { echo 1; }").is_empty());
    assert!(parser_diagnostics("<?php declare(ticks=1): echo 1; enddeclare;").is_empty());
    assert!(parser_diagnostics("<?php declare(encoding='UTF-8', ticks=1);").is_empty());
}

#[test]
fn a_directive_without_a_value_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php declare(strict_types);")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Equals))
    );
}
```

- [ ] **Step 2: Run — must fail.**

- [ ] **Step 3: Implement**

Node kinds after `FinallyClause`:

```rust
    /// `declare( directives ) body`, either syntax; the body may be a
    /// lone `;` (an empty statement).
    DeclareStatement,
    /// One `name = value` directive; which names and values are legal
    /// is semantic.
    DeclareDirective,
```

```rust
fn declare_statement(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `declare`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        // Terminates: each iteration bumps the directive's identifier
        // before anything refusable, or breaks.
        loop {
            if !parser.at(SyntaxKind::Identifier) {
                parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
                break;
            }
            let directive = parser.start();
            parser.bump();
            parser.expect(SyntaxKind::Equals);
            expression(parser);
            directive.complete(parser, SyntaxKind::DeclareDirective);
            if !parser.eat(SyntaxKind::Comma) {
                break;
            }
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.eat(SyntaxKind::Colon) {
        statement_list(parser);
        parser.expect(SyntaxKind::EndDeclare);
        terminate_statement(parser);
    } else {
        embedded_statement(parser);
    }
    marker.complete(parser, SyntaxKind::DeclareStatement);
}
```

Dispatch arm: `Some(SyntaxKind::Declare) => declare_statement(parser),`.

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse declare statements"
```

---

### Task 15: Top-level `function` declarations

The "simple top-level declarations" of the design's plan list. `function` followed by a name (optionally through `&`) declares; `function (` and `function &(` stay closure expressions. Classes, interfaces, traits, enums, `const`, `namespace`, `use` remain plan 4.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (`pub(super)` on `parameter_list` and `type_reference`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (append `FunctionDeclaration`)
- Test: `crates/celerrate_syntax/tests/statements_declarations.rs`

**Interfaces:**
- Consumes: `expressions::parameter_list(parser)`, `expressions::type_reference(parser)` (both newly `pub(super)`); `block`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_named_function_declares() {
    insta::assert_snapshot!(render_statement("function add(int $a, int $b = 0) { return $a + $b; }"), @r#"
    FunctionDeclaration
      Function "function"
      Identifier "add"
      ParameterList
        OpenParenthesis "("
        Parameter
          TypeReference
            Identifier "int"
          Variable "$a"
        Comma ","
        Parameter
          TypeReference
            Identifier "int"
          Variable "$b"
          Equals "="
          Literal
            IntegerLiteral "0"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        ReturnStatement
          Return "return"
          BinaryExpression
            VariableReference
              Variable "$a"
            Plus "+"
            VariableReference
              Variable "$b"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn by_reference_returns_and_return_types_parse() {
    insta::assert_snapshot!(render_statement("function &all(): ?iterable { }"), @r#"
    FunctionDeclaration
      Function "function"
      Ampersand "&"
      Identifier "all"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      TypeReference
        Question "?"
        Identifier "iterable"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn an_anonymous_function_stays_a_closure_expression() {
    insta::assert_snapshot!(render_statement("function () { };"), @r#"
    ExpressionStatement
      ClosureExpression
        Function "function"
        ParameterList
          OpenParenthesis "("
          CloseParenthesis ")"
        Block
          OpenBrace "{"
          CloseBrace "}"
      Semicolon ";"
    "#);
}

#[test]
fn a_by_reference_closure_also_stays_an_expression() {
    assert!(parser_diagnostics("<?php $f = function &() { return $x; };").is_empty());
}
```

- [ ] **Step 2: Run — must fail** (`function add` currently parses as a closure expression, then chokes on the name).

- [ ] **Step 3: Implement**

In `expressions.rs`, change `fn parameter_list` and `fn type_reference` to `pub(super) fn`. Node kind after `DeclareDirective`:

```rust
    /// `function name(parameters): type { body }`, the top-level form;
    /// methods arrive with the declarations plan.
    FunctionDeclaration,
```

Dispatch arm — before the `starts_expression` arm (`Function` starts closures):

```rust
        Some(SyntaxKind::Function) if at_function_declaration(parser) => {
            function_declaration(parser)
        }
```

```rust
/// `function` declares only when a name follows, with an optional `&`
/// between: `function (` and `function &(` are closure expressions.
fn at_function_declaration(parser: &mut Parser) -> bool {
    match parser.nth(1) {
        Some(SyntaxKind::Identifier) => true,
        Some(SyntaxKind::Ampersand) => parser.nth(2) == Some(SyntaxKind::Identifier),
        _ => false,
    }
}

fn function_declaration(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `function`
    parser.eat(SyntaxKind::Ampersand); // by-reference return
    parser.expect(SyntaxKind::Identifier);
    parameter_list(parser);
    if parser.eat(SyntaxKind::Colon) {
        type_reference(parser);
    }
    block(parser);
    marker.complete(parser, SyntaxKind::FunctionDeclaration);
}
```

Update the `statements.rs` import line to pull `parameter_list` and `type_reference` from `super::expressions`.

- [ ] **Step 4: Run tests + full suite** — PASS expected.
- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✨ feat(syntax): parse top-level function declarations"
```

---

### Task 16: Corpus, error corpus, fuzz seeds

**Files:**
- Create: `crates/celerrate_syntax/tests/parse_corpus/statements_control_flow.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/statements_alternative_syntax.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/statements_switch.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/statements_try.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/statements_declarations.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/statements_kitchen_sink.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/recovery_statements.php`
- Create: `fuzz/corpus/parse/seed_statements.php`, `fuzz/corpus/parse/seed_statements_errors.php`

- [ ] **Step 1: Write the corpus files**

`statements_control_flow.php`:

```php
<?php

if ($ready) {
    echo 'go';
} elseif ($waiting) {
    echo 'hold';
} else {
    echo 'stop';
}

if ($a) if ($b) echo 1; else echo 2;

$i = 0;
while ($i < 3) {
    $i++;
}

do {
    $i--;
} while ($i > 0);

for ($j = 0, $k = 9; $j < $k; $j++, $k--) {
    echo $j;
}

foreach ($items as $item) {
    echo $item;
}

foreach ($map as $key => [$first, &$second]) {
    echo $key;
}

foreach ($queue as &$task) {
    $task = null;
}
```

`statements_alternative_syntax.php`:

```php
<?php if ($mode === 'header'): ?>
<h1>Header</h1>
<?php elseif ($mode === 'footer'): ?>
<footer>Bye</footer>
<?php else: ?>
<p>Body</p>
<?php endif;

while ($row): echo $row; endwhile;

for ($i = 0; $i < 2; $i++): echo $i; endfor;

foreach ($links as $link): echo $link; endforeach;

declare(ticks=1): echo 'tick'; enddeclare;
```

`statements_switch.php`:

```php
<?php

switch ($signal) {
    case 'red':
    case 'amber':
        $action = 'stop';
        break;
    case 'green':
        $action = 'go';
        break;
    default:
        $action = 'wait';
}

switch ($tight) { ; case 1: echo 1; }

switch ($state):
    case 'on':
        echo 1;
        break;
    default:
        echo 0;
endswitch;
```

`statements_try.php`:

```php
<?php

try {
    risky();
} catch (LogicException | \RuntimeException $error) {
    report($error);
} catch (Throwable) {
    recover();
} finally {
    cleanup();
}

try {
    once();
} finally {
    always();
}
```

`statements_declarations.php`:

```php
<?php
declare(strict_types=1);

function add(int $first, int $second = 0): int
{
    return $first + $second;
}

function &finder(callable $predicate): ?object
{
    static $cache = [], $hits = 0;
    global $registry;
    foreach ($registry as $entry) {
        if ($predicate($entry)) {
            $hits++;
            return $entry;
        }
    }
    unset($cache['stale']);
    goto missing;
    missing:
    return null;
}
```

`statements_kitchen_sink.php`:

```php
<?php
declare(strict_types=1);

function process(array $jobs, ?callable $notify = null): int
{
    $done = 0;
    foreach ($jobs as $id => $job) {
        switch (true) {
            case $job === null:
                continue 2;
            default:
                break;
        }
        try {
            if ($job->run()) {
                $done++;
            } else {
                throw new RuntimeException('failed');
            }
        } catch (RuntimeException $error) {
            if ($notify !== null) {
                $notify($id, $error);
            }
        } finally {
            unset($jobs[$id]);
        }
    }
    do {
        $done--;
    } while ($done > 100);
    for ($i = 0; $i < 2; $i++) {
        echo $i;
    }
    return $done;
}
```

`recovery_statements.php` (deliberately broken; every token must survive into the tree):

```php
<?php
if ($broken {
    echo 1;
}
do echo 3;
switch ($y) { junk case 1: echo 4; }
foreach ($items as) { echo 6; }
try { echo 5; }
goto ;
while ($x): echo 2;
{
```

- [ ] **Step 2: Run the corpus test and review the new snapshots**

Run: `cargo test --package celerrate_syntax --test parse_corpus`
Expected: FAIL with new-snapshot messages. Review each `.snap.new` (`cargo insta review` or read the files): trees must match the grammar built in Tasks 3-15, diagnostics only on `recovery_statements.php`, then accept. Re-run: PASS.

- [ ] **Step 3: Seed the fuzzer and smoke-run**

Copy `statements_kitchen_sink.php` to `fuzz/corpus/parse/seed_statements.php` and `recovery_statements.php` to `fuzz/corpus/parse/seed_statements_errors.php`. Then:

Run: `cargo +nightly fuzz run parse -- -runs=200000 -max_total_time=120`
Expected: no crash, no timeout. Record the execution count for the PR description.

- [ ] **Step 4: Full gate**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "✅ test(syntax): extend the parse corpus and fuzz seeds for statements"
```

---

## Self-review notes (performed while writing)

- **Spec coverage** (design section 3 "Statements" + plan list item 3): control flow ✓ (Tasks 7-12), loops ✓ (9-11), `try`/`catch`/`finally` ✓ (13), `switch` ✓ (12), `goto` ✓ (6), alternative syntax ✓ (8-12, 14), inline HTML interrupting statement lists ✓ (Task 8 test + alternative-syntax corpus), simple top-level declarations ✓ (15), error recovery shipped per task ✓. `echo`, expression statements, inline HTML dispatch existed from plan 1. `global`/`static`/`unset`/`declare`/empty statements are Zend `statement` productions and land here (5, 14, 3).
- **Type consistency**: all statement rules take `&mut Parser` and return `()`; `statement_list`, `block`, `statement` are the module's only `pub(super)` items; expression-module exposures are exactly `expression`, `starts_expression`, `error_element` (pre-existing) plus `simple_variable`, `argument_list` (Task 5), `name` (Task 13), `parameter_list`, `type_reference` (Task 15).
- **Termination**: every new loop either bumps before anything refusable (`switch_case_list`, `declare` directives, `global`/`static` lists, catch types), carries a position guard (`for_expression_list`), or never consumes terminators while the top level consumes everything (`statement_list` unwinding). The statement nesting guard (Task 3) converts unbounded statement recursion into diagnostics plus single-token progress.
- **Node kinds** are appended strictly after `Block` in task order; the hand-maintained list stays the ungrammar migration's input (plan 5).
