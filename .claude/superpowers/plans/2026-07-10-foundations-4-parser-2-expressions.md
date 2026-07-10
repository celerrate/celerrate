# Foundations Part 4, Plan 2: Expressions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse the full PHP expression grammar through 8.5: the complete Pratt precedence table, unary and cast operators, ternary and assignment, names, calls, member and scoped access chains, arrays, string interpolation nodes, `new`/`clone` (clone-with from 8.5), the intrinsic keyword expressions, `match`, closures, arrow functions, and the pipe operator `|>`.

**Architecture:** Everything builds on plan 1's event-based parser: `CompletedMarker::precede` (the forward-parent producer) powers a Pratt loop for binary operators and a postfix loop for access chains. One precedence table, transcribed from php-src's `zend_language_parser.y` (branch `PHP-8.5`), drives all binary operators. The parser stays permissive: it accepts what is structurally analyzable (semi-reserved keywords at name positions, version-gated syntax, Zend-rejected associativity chains with a diagnostic) and leaves validity judgments to upper layers. Spec: `.claude/superpowers/specs/2026-07-10-foundations-4-parser-design.md` (sections 3 and 4).

**Tech Stack:** Rust 1.94 (edition 2024), `rowan` 0.16, `text-size` 1, `insta` (snapshots, inline and file-based), `cargo-fuzz` (libFuzzer).

## Global Constraints

Copied from the parent spec and `CLAUDE.md`; every task's requirements include them.

- Zero panic, mechanically enforced: workspace denies `clippy::unwrap_used`, `clippy::expect_used`, `clippy::indexing_slicing`, `clippy::panic`; `unsafe_code` is forbidden. Production code returns totals (`Option`, fallbacks); test modules may locally `#[allow]` these lints (see existing test files for the idiom). `debug_assert!` is permitted (compiled out in release).
- TDD: every step of behavior starts from a failing test. No production code without a test that demanded it.
- Layering: `celerrate_syntax` depends only on `celerrate_source` (plus external `rowan`, `text-size`). No bare `rowan` type in any public signature.
- The lossless invariant: `parse(source).tree().text() == source` for every input, including degenerate input. Every test parse goes through `support::parse_verified`, which asserts it.
- Guaranteed progress: every parser loop iteration consumes a token or exits. Recovery wraps unexpected tokens in `ErrorNode` and continues; nothing is ever discarded.
- The parser performs no version or semantic judgment and never fails. `readonly` misuse, 8.4/8.5 syntax in older projects, arity errors: all of it parses. The only diagnostics are structural (`Expected…`, `UnexpectedToken`, `NonAssociativeOperator`, `NestingTooDeep`).
- Determinism: no wall-clock time, no randomness, no environment reads.
- Everything in English, full words, no abbreviated names (standard acronyms fine). No em-dashes. Comments state constraints the code cannot show, never narration.
- Commits: gitmoji + Conventional Commits (`✨ feat(syntax): ...`), repository-configured identity, no AI attribution of any kind.
- Before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` must all pass. This is "the gate" referenced by every task's commit step.

## The Precedence Table (authoritative)

Transcribed from php-src `Zend/zend_language_parser.y`, branch `PHP-8.5`, verified on 2026-07-10. Levels are loosest first; the parser encodes them as `const` levels and derives binding powers as `level * 2` (a right-associative operator recurses at `level * 2 - 1`, a left-associative or non-associative one at `level * 2 + 1`).

| Level | Operators | Associativity |
|---|---|---|
| 1 | `or` | left |
| 2 | `xor` | left |
| 3 | `and` | left |
| 4 | `print` | prefix |
| 5 | `yield` (and its `=>` key form) | prefix |
| 6 | `yield from` | prefix |
| 7 | `throw` | prefix |
| 8 | `include` `include_once` `require` `require_once` | prefix |
| 9 | `=` `+=` `-=` `*=` `/=` `.=` `%=` `**=` `&=` `\|=` `^=` `<<=` `>>=` `??=` (and `= &`) | right |
| 10 | `? :` (and short `?:`) | none (chain diagnosed) |
| 11 | `??` | right |
| 12 | `\|\|` | left |
| 13 | `&&` | left |
| 14 | `\|` | left |
| 15 | `^` | left |
| 16 | `&` | left |
| 17 | `==` `!=` `<>` `===` `!==` `<=>` | none (chain diagnosed) |
| 18 | `<` `<=` `>` `>=` | none (chain diagnosed) |
| 19 | `\|>` (PHP 8.5 pipe) | left |
| 20 | `.` | left |
| 21 | `<<` `>>` | left |
| 22 | `+` `-` (binary) | left |
| 23 | `*` `/` `%` | left |
| 24 | `!` | prefix |
| 25 | `instanceof` | none (chain diagnosed) |
| 26 | `~` casts `@` unary `+` `-` prefix `++` `--` | prefix |
| 27 | `**` | right |
| 28 | `clone` | prefix |
| tightest | `->` `?->` `::` `(...)` `[...]` postfix `++` `--` | postfix loop |

Notes pinned by tests in this plan:

- `.` sits **below** `<<`/`>>` and `+`/`-` (the PHP 8.0 change).
- `**` binds tighter than prefix minus: `-2 ** 3` is `-(2 ** 3)`.
- `|>` sits between the relational group and `.`: `'a' . 'b' |> $f` pipes the concatenation; `$x |> $f == 4` compares the pipe's result.
- Non-associative chains (`1 < 2 < 3`, `$a ? 1 : $b ? 2 : 3`, double `instanceof`) are compile errors in Zend. We parse them left-associatively and emit `NonAssociativeOperator`; resilience over rejection.
- Zend declares assignment with `%precedence` and restricts its left side grammatically; we wrap whatever expression precedes `=` and leave "is this assignable" to semantics.

## Deliberate Permissiveness (decisions, recorded)

These parse without diagnostics even though Zend rejects or version-gates them. Rejecting them needs information the parser does not have (version, adjacency, semantics), and a lossless tree carries everything an upper layer needs to judge:

- Qualified names with interior whitespace (`Foo \ Bar`): the trivia-free token view cannot see adjacency. Zend lexes names as single tokens; we assemble `Identifier`/`Backslash` runs.
- `new Foo->bar` and postfix chains on any `new` (8.4 gates chaining on argument parentheses).
- By-reference call-site arguments `f(&$x)` (removed in PHP 8, still structurally analyzable).
- Any keyword as a member name, constant name, or named-argument label (Zend's semi-reserved list, accepted wholesale; per-position validity is semantic).

Deferred to later plans, with recovery in the meantime:

- `new class ...` (anonymous classes): the `class` keyword after `new` becomes an `ErrorNode` with `UnexpectedToken`; plan 4 (declarations) parses it.
- `#[...]` attributes on closures and parameters: plan 4. An `AttributeOpen` token at expression position lands in statement-level recovery.
- Union, intersection, and DNF types: plan 4 extends `type_reference`, which parses exactly one optionally-nullable named type here.
- `function name() {}` declarations: plan 3/4 own statement-level dispatch of `function` followed by a name; until then it misparses as a closure with diagnostics (the corpus does not exercise it).

## File Structure

```
crates/celerrate_syntax/src/syntax_kind.rs           modify: PipeGreater + YieldFrom tokens, is_keyword, ~40 node kinds
crates/celerrate_syntax/src/lexer/scripting.rs       modify: |> operator entry, yield-from extension
crates/celerrate_syntax/src/diagnostic.rs            modify: Expected(SyntaxKind), ExpectedMemberName, NonAssociativeOperator, NestingTooDeep
crates/celerrate_syntax/src/parser.rs                modify: nth/eat/expect, nesting guard, CompletedMarker::precede
crates/celerrate_syntax/src/parser/grammar.rs        modify: expressions submodule, statement dispatch via starts_expression
crates/celerrate_syntax/src/parser/grammar/expressions.rs  create: the whole expression grammar (one module per spec section 3)
crates/celerrate_syntax/tests/support/mod.rs         modify: render_expression helper
crates/celerrate_syntax/tests/operators.rs           modify: |> and yield-from lexing tests
crates/celerrate_syntax/tests/expressions.rs         create: binary/unary/ternary/assignment tests
crates/celerrate_syntax/tests/expressions_postfix.rs create: names, calls, members, indexing tests
crates/celerrate_syntax/tests/expressions_literals.rs create: arrays and string interpolation tests
crates/celerrate_syntax/tests/expressions_keywords.rs create: new/clone/intrinsics/yield/match tests
crates/celerrate_syntax/tests/expressions_closures.rs create: closures and arrow function tests
crates/celerrate_syntax/tests/parse_corpus/*.php     create: one corpus file per grammar area
fuzz/corpus/parse/                                   modify: seed with the new corpus files
```

`grammar.rs` grows a `grammar/` directory (the crate's existing file-plus-directory convention). All expression rules live in `grammar/expressions.rs`; if a later plan finds it unwieldy, splitting is that plan's call.

### Node kinds added (appended after `ErrorNode`, hand-maintained until the ungrammar plan)

| Task | Kinds |
|---|---|
| 3 | `ParenthesizedExpression`, `BinaryExpression` |
| 4 | `PrefixExpression`, `PostfixExpression`, `CastExpression` |
| 5 | `TernaryExpression`, `AssignmentExpression` |
| 6 | `Name`, `NameExpression`, `DynamicVariableExpression` |
| 7 | `ArgumentList`, `Argument`, `CallExpression` |
| 8 | `MemberAccessExpression`, `ScopedAccessExpression`, `MemberName`, `IndexExpression` |
| 9 | `ArrayExpression`, `ArrayElement`, `ListExpression` |
| 10 | `InterpolatedString`, `HeredocExpression`, `ShellExecExpression`, `SimpleInterpolation`, `BraceInterpolation`, `DollarBraceInterpolation` |
| 11 | `NewExpression`, `CloneExpression` |
| 12 | `IssetExpression`, `EmptyExpression`, `EvalExpression`, `ExitExpression` |
| 13 | `PrintExpression`, `ThrowExpression`, `YieldExpression`, `IncludeExpression` |
| 14 | `MatchExpression`, `MatchArm` |
| 15 | `ClosureExpression`, `ArrowFunctionExpression`, `ParameterList`, `Parameter`, `TypeReference`, `ClosureUseClause`, `Block` |

### A note on inline snapshots

Expression tests use `insta::assert_snapshot!(value, @r#"..."#)` inline snapshots against `support::render_expression`, which renders node kinds and token text without offsets, so expectations are stable and readable. The expected trees written in this plan state the intent; if a run differs only in formatting (indentation, blank lines), inspect the actual output, confirm the tree shape matches the stated intent, then run `cargo insta accept` and re-run the tests. A shape difference is a real failure: fix the code, not the snapshot.

---

### Task 1: Lex the pipe operator and `yield from`

The parser plan needs two tokens the lexer does not produce yet. `|>` (PHP 8.5) currently lexes as `Pipe` then `Greater`. `yield from` is one token in Zend (`T_YIELD_FROM`, interior whitespace included) and the parser has no text access to detect `from` as an identifier, so the lexer must deliver it as one token, exactly like casts already contain interior whitespace. This task also adds the `is_keyword` classifier that later tasks (member names, named-argument labels) need.

**Files:**
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs`
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs`
- Test: `crates/celerrate_syntax/tests/operators.rs`, `crates/celerrate_syntax/tests/syntax_kind.rs`

**Interfaces:**
- Consumes: the existing `OPERATORS` table (longest-first matching), `lex_name`, `is_php_whitespace`, `SyntaxKind::from_keyword`.
- Produces: `SyntaxKind::PipeGreater` (token for `|>`), `SyntaxKind::YieldFrom` (token for `yield` + whitespace + `from`), `SyntaxKind::is_keyword(self) -> bool` (true exactly for the contiguous keyword section `Abstract..=YieldFrom`).

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/operators.rs` (this file already imports the `support` helpers and `SyntaxKind` variants; follow its existing `use` style):

```rust
#[test]
fn the_pipe_operator_is_one_token() {
    assert_eq!(
        texts("<?php 1 |> $f"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "1".to_owned()),
            (Whitespace, " ".to_owned()),
            (PipeGreater, "|>".to_owned()),
            (Whitespace, " ".to_owned()),
            (Variable, "$f".to_owned()),
        ]
    );
}

#[test]
fn pipe_pipe_still_wins_over_the_pipe_operator() {
    // Longest-first matching: `||` must not become `|` + `|`, and `|>`
    // must not shadow `|=`.
    assert_eq!(
        kinds("<?php $a || $b"),
        vec![OpenTag, Whitespace, Variable, Whitespace, PipePipe, Whitespace, Variable]
    );
    assert_eq!(
        kinds("<?php $a |= $b"),
        vec![OpenTag, Whitespace, Variable, Whitespace, PipeEquals, Whitespace, Variable]
    );
}

#[test]
fn yield_from_is_one_token_with_its_whitespace() {
    assert_eq!(
        texts("<?php yield  from $g"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (YieldFrom, "yield  from".to_owned()),
            (Whitespace, " ".to_owned()),
            (Variable, "$g".to_owned()),
        ]
    );
}

#[test]
fn yield_from_crosses_newlines_and_ignores_case() {
    assert_eq!(
        texts("<?php YIELD\nFROM $g"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (YieldFrom, "YIELD\nFROM".to_owned()),
            (Whitespace, " ".to_owned()),
            (Variable, "$g".to_owned()),
        ]
    );
}

#[test]
fn yield_needs_a_real_from_word_to_extend() {
    // "fromage" must not extend; neither may an adjacent "yieldfrom".
    assert_eq!(
        kinds("<?php yield fromage"),
        vec![OpenTag, Whitespace, Yield, Whitespace, Identifier]
    );
    assert_eq!(kinds("<?php yieldfrom"), vec![OpenTag, Whitespace, Identifier]);
}

#[test]
fn yield_alone_stays_yield() {
    assert_eq!(kinds("<?php yield $x"), vec![OpenTag, Whitespace, Yield, Whitespace, Variable]);
}
```

Append to `crates/celerrate_syntax/tests/syntax_kind.rs`:

```rust
#[test]
fn keywords_are_classified_and_contiguous() {
    assert!(SyntaxKind::Abstract.is_keyword());
    assert!(SyntaxKind::Yield.is_keyword());
    assert!(SyntaxKind::YieldFrom.is_keyword());
    assert!(SyntaxKind::Exit.is_keyword());
    assert!(!SyntaxKind::Identifier.is_keyword());
    assert!(!SyntaxKind::PipeGreater.is_keyword());
    assert!(!SyntaxKind::IntCast.is_keyword());
    assert!(!SyntaxKind::SourceFile.is_keyword());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test operators --test syntax_kind`
Expected: FAIL to compile (`PipeGreater`, `YieldFrom`, `is_keyword` do not exist).

- [ ] **Step 3: Implement**

In `crates/celerrate_syntax/src/syntax_kind.rs`:

1. Add `YieldFrom` immediately after `Yield` at the end of the keyword section, keeping the keyword block contiguous:

```rust
    Yield,
    /// `yield from`, one token as in Zend, interior whitespace included.
    YieldFrom,
```

2. Add `PipeGreater` right after `Pipe` in the operator section:

```rust
    Pipe,
    /// `|>`, the PHP 8.5 pipe operator.
    PipeGreater,
```

3. Add the classifier next to `is_trivia`, with the constraint stated:

```rust
    /// Whether this kind is a PHP keyword. Relies on the keyword section
    /// being contiguous in the declaration, `Abstract` through
    /// `YieldFrom`; the classifier test pins that layout.
    pub fn is_keyword(self) -> bool {
        (Self::Abstract..=Self::YieldFrom).contains(&self)
    }
```

In `crates/celerrate_syntax/src/lexer/scripting.rs`:

1. Add the operator entry among the two-byte operators, next to `("|=", SyntaxKind::PipeEquals)` (the table is longest-first; both are two bytes and must precede `("|", SyntaxKind::Pipe)`):

```rust
    ("|>", SyntaxKind::PipeGreater),
```

2. Extend `lex_name` to attempt the `yield from` fusion:

```rust
    fn lex_name(&mut self) {
        self.cursor.eat_while(is_name_continue);
        let kind =
            SyntaxKind::from_keyword(self.cursor.pending_text()).unwrap_or(SyntaxKind::Identifier);
        if kind == SyntaxKind::Yield && self.try_extend_yield_from() {
            return;
        }
        self.emit(kind);
    }

    /// Zend lexes `yield` + whitespace + `from` as the single token
    /// T_YIELD_FROM (no comments allowed between the words); the
    /// whitespace stays inside the token, like casts. Consumes nothing
    /// and returns false when `from` does not follow.
    fn try_extend_yield_from(&mut self) -> bool {
        let rest = self.cursor.rest();
        let after_whitespace = rest.trim_start_matches(is_php_whitespace);
        let whitespace_length = rest.len() - after_whitespace.len();
        if whitespace_length == 0 {
            return false;
        }
        let word_matches = after_whitespace
            .get(..4)
            .is_some_and(|word| word.eq_ignore_ascii_case("from"));
        let word_continues = after_whitespace
            .get(4..)
            .unwrap_or_default()
            .starts_with(is_name_continue);
        if !word_matches || word_continues {
            return false;
        }
        self.cursor.bump_bytes(whitespace_length + 4);
        self.emit(SyntaxKind::YieldFrom);
        true
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, including every pre-existing lexer test (the corpus snapshots must not change: no existing corpus file contains `|>` or `yield from`; if one does, inspect the new snapshot and accept it only if the change is exactly the new fused token).

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): lex the pipe operator and yield from"
```

---

### Task 2: Pratt machinery: `precede`, lookahead, `expect`, and the nesting guard

Four parser capabilities every later task consumes. `CompletedMarker::precede` retroactively wraps a finished node (the forward-parent mechanism the builder already replays). `nth` gives bounded lookahead. `eat`/`expect` standardize optional and mandatory tokens. The nesting guard bounds recursion depth so degenerate input (`((((...`, `$$$$...`) yields a diagnostic, never a stack overflow; the fuzzer would find the overflow otherwise.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser.rs`
- Modify: `crates/celerrate_syntax/src/diagnostic.rs`

**Interfaces:**
- Consumes: `Parser`, `Marker`, `CompletedMarker`, `Event` from plan 1.
- Produces, all `pub(crate)`-internal to the parser module:
  - `Parser::nth(&self, offset: usize) -> Option<SyntaxKind>` (lookahead over significant tokens; `nth(0)` equals `current()`)
  - `Parser::eat(&mut self, kind: SyntaxKind) -> bool` (bump if at `kind`)
  - `Parser::expect(&mut self, kind: SyntaxKind)` (eat or diagnose `Expected(kind)` zero-width at the previous token's end)
  - `Parser::enter_nesting(&mut self) -> bool` / `Parser::leave_nesting(&mut self)` (depth counter against `MAXIMUM_NESTING_DEPTH = 128`; on refusal diagnoses `NestingTooDeep`)
  - `CompletedMarker { event_index: usize }` with `precede(self, parser) -> Marker` (no `kind` accessor: nothing in this plan reads it, YAGNI; the ungrammar plan can add it when a consumer exists)
  - `ParserDiagnosticKind::Expected(SyntaxKind)` and `ParserDiagnosticKind::NestingTooDeep`

Until task 3 wires the expression grammar in, these items have no production caller, and the lib target (compiled without `cfg(test)`) would fail the `-D warnings` gate on `dead_code`. Annotate each new item (`nth`, `eat`, `expect`, `enter_nesting`, `leave_nesting`, `precede`) with `#[allow(dead_code)] // Temporary: consumed by the expression grammar tasks of this plan.` — and delete each allow in the first task that uses the item. Task 16's closing gate confirms none remain.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module at the bottom of `crates/celerrate_syntax/src/parser.rs`:

```rust
    #[test]
    fn precede_wraps_a_completed_node_through_a_forward_parent() {
        let mut parser = parser_over("<?php 1");
        parser.bump(); // <?php
        let marker = parser.start();
        parser.bump(); // 1
        let completed = marker.complete(&mut parser, SyntaxKind::Literal);
        let wrapper = completed.precede(&mut parser);
        wrapper.complete(&mut parser, SyntaxKind::ExpressionStatement);
        assert_eq!(
            parser.events,
            vec![
                Event::Token,
                Event::Start {
                    kind: Some(SyntaxKind::Literal),
                    forward_parent: Some(4),
                },
                Event::Token,
                Event::Finish,
                Event::Start {
                    kind: Some(SyntaxKind::ExpressionStatement),
                    forward_parent: None,
                },
                Event::Finish,
            ],
        );
    }

    #[test]
    fn nth_looks_ahead_without_consuming() {
        let parser = parser_over("<?php echo 1;");
        assert_eq!(parser.nth(0), Some(SyntaxKind::OpenTag));
        assert_eq!(parser.nth(1), Some(SyntaxKind::Echo));
        assert_eq!(parser.nth(2), Some(SyntaxKind::IntegerLiteral));
        assert_eq!(parser.nth(4), None);
    }

    #[test]
    fn expect_bumps_or_diagnoses_a_missing_token() {
        let mut parser = parser_over("<?php ;");
        parser.expect(SyntaxKind::OpenTag);
        parser.expect(SyntaxKind::Semicolon);
        assert!(parser.diagnostics.is_empty());
        parser.expect(SyntaxKind::CloseParenthesis);
        assert!(matches!(
            parser.diagnostics.first(),
            Some(ParserDiagnostic {
                kind: ParserDiagnosticKind::Expected(SyntaxKind::CloseParenthesis),
                range,
            }) if range.is_empty()
        ));
    }

    #[test]
    fn the_nesting_guard_refuses_past_the_limit_and_recovers() {
        let mut parser = parser_over("<?php 1");
        let mut entered = 0usize;
        while parser.enter_nesting() {
            entered += 1;
            assert!(entered <= 1_000, "the guard must trip");
        }
        assert!(matches!(
            parser.diagnostics.first(),
            Some(ParserDiagnostic {
                kind: ParserDiagnosticKind::NestingTooDeep,
                ..
            })
        ));
        // Leaving frees capacity again: recovery paths keep parsing.
        parser.leave_nesting();
        assert!(parser.enter_nesting());
    }
```

Note: the wrapper's `forward_parent` is the **absolute event index** of the wrapper's own `Start`. It is 4 because `complete` already pushed the `Finish` at index 3 before `precede` ran: Token, Start, Token, Finish, Start, Finish.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --lib`
Expected: FAIL to compile (`precede`, `kind`, `nth`, `eat`, `expect`, `enter_nesting`, `Expected`, `NestingTooDeep` do not exist).

- [ ] **Step 3: Implement**

In `crates/celerrate_syntax/src/diagnostic.rs`, extend `ParserDiagnosticKind` (add `use crate::syntax_kind::SyntaxKind;` at the top; `diagnostic.rs` gains its first dependency on the kind enum):

```rust
/// What the parser expected or could not place, structurally. Rendering
/// into messages is an upper layer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserDiagnosticKind {
    /// An expression position holds no expression.
    ExpectedExpression,
    /// A statement misses its terminator (`;`, or `?>` / end of input
    /// only where PHP itself allows the omission).
    ExpectedSemicolon,
    /// A specific token is missing; the range is zero-width at the spot
    /// where it belongs.
    Expected(SyntaxKind),
    /// A token no grammar rule accepts; wrapped in an `ErrorNode`.
    UnexpectedToken,
    /// Expressions nest deeper than the parser's recursion budget; the
    /// innermost expression is missing from the tree, the tokens are
    /// preserved through recovery.
    NestingTooDeep,
}
```

In `crates/celerrate_syntax/src/parser.rs`:

1. Add the depth field to `Parser` and initialize it in `new`:

```rust
struct Parser {
    source: TokenSource,
    position: usize,
    events: Vec<Event>,
    diagnostics: Vec<ParserDiagnostic>,
    nesting_depth: u32,
}
```

2. Add the methods to `impl Parser`:

```rust
    /// Bounds recursive descent: degenerate nesting (`((((...`,
    /// `$$$$...`) must stay a diagnostic, never a stack overflow. 128
    /// levels is far beyond real code and well inside default stacks.
    const MAXIMUM_NESTING_DEPTH: u32 = 128;

    fn nth(&self, offset: usize) -> Option<SyntaxKind> {
        self.source.kind(self.position + offset)
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            return true;
        }
        false
    }

    fn expect(&mut self, kind: SyntaxKind) {
        if !self.eat(kind) {
            self.diagnose_missing(ParserDiagnosticKind::Expected(kind));
        }
    }

    /// Returns false (and diagnoses, once per trip) instead of
    /// recursing past the budget. Every recursive expression entry
    /// point pairs this with `leave_nesting`.
    fn enter_nesting(&mut self) -> bool {
        if self.nesting_depth >= Self::MAXIMUM_NESTING_DEPTH {
            self.diagnose_current(ParserDiagnosticKind::NestingTooDeep);
            return false;
        }
        self.nesting_depth += 1;
        true
    }

    fn leave_nesting(&mut self) {
        self.nesting_depth = self.nesting_depth.saturating_sub(1);
    }
```

The guard diagnoses on **every** refused entry, which can stack up on pathological input; that is acceptable (diagnostic dedup is a rendering concern) and keeps the guard stateless. If the noise bothers the corpus later, dedup there.

3. Replace the `CompletedMarker` placeholder and wire `Marker::complete`:

```rust
/// A finished node: remembers where its `Start` event lives so a
/// forward parent can wrap it retroactively.
struct CompletedMarker {
    event_index: usize,
}

impl CompletedMarker {
    /// Opens a node that will enclose this completed one: the new
    /// marker's `Start` is appended now, and this node's `Start` gains
    /// a forward parent pointing at it (absolute event index), which
    /// the builder replays outermost-first.
    #[allow(dead_code)] // Temporary: consumed by the expression grammar tasks of this plan.
    fn precede(self, parser: &mut Parser) -> Marker {
        let marker = parser.start();
        if let Some(Event::Start { forward_parent, .. }) =
            parser.events.get_mut(self.event_index)
        {
            *forward_parent = Some(marker.event_index);
        }
        marker
    }
}
```

In `Marker::complete`, build the value accordingly:

```rust
    fn complete(mut self, parser: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        self.defused = true;
        if let Some(Event::Start { kind: slot, .. }) = parser.events.get_mut(self.event_index) {
            *slot = Some(kind);
        }
        parser.events.push(Event::Finish);
        CompletedMarker {
            event_index: self.event_index,
        }
    }
```

`Marker.event_index` stays private; `precede` reads it within the module.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS (all existing suites included; behavior is unchanged so no snapshot moves).

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): grow the parser with precede, lookahead, and a nesting guard"
```

---

### Task 3: Binary expressions over the full precedence table, and parentheses

The Pratt loop, the whole binary operator table (including `|>`, `??`, the word operators, `instanceof`, and the non-associative groups with their chain diagnostic), and `ParenthesizedExpression`. The expression grammar moves into its own module; plan 1's minimal `expression` dissolves into it as the primary layer. Ternary and assignment are task 5; prefix operators are task 4; until then those branches simply do not exist in the loop.

**Files:**
- Create: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `ParenthesizedExpression`, `BinaryExpression` after `ErrorNode`)
- Modify: `crates/celerrate_syntax/src/diagnostic.rs` (add `NonAssociativeOperator`)
- Modify: `crates/celerrate_syntax/tests/support/mod.rs` (add `render_expression`)
- Test: `crates/celerrate_syntax/tests/expressions.rs` (new), corpus file `tests/parse_corpus/expressions_operators.php`

**Interfaces:**
- Consumes: `Parser` (`bump`, `at`, `eat`, `expect`, `enter_nesting`/`leave_nesting`, `diagnose_current`, `diagnose_missing`), `Marker`/`CompletedMarker` (`precede`, `kind`).
- Produces (module `grammar::expressions`):
  - `pub(super) fn expression(parser: &mut Parser) -> Option<CompletedMarker>` (same contract as plan 1: diagnoses `ExpectedExpression` and returns `None` when nothing starts an expression)
  - `pub(super) fn starts_expression(kind: SyntaxKind) -> bool` (grows in almost every later task; the statement dispatcher uses it)
  - internal: `expression_with_minimum_power(parser, minimum_power: u8)`, `prefix_expression`, `postfix_expression`, `primary_expression`, `parenthesized_expression`, the level constants, `Associativity`, `binary_operator`
- Produces (test support): `support::render_expression(expression_source: &str) -> String` wrapping the source as `<?php {source};`, asserting losslessness, and rendering the first statement's expression as indented node kinds and token texts, offsets and trivia omitted.

- [ ] **Step 1: Add `render_expression` to the test support**

In `crates/celerrate_syntax/tests/support/mod.rs`, widen the file-level allow to `#![allow(clippy::indexing_slicing, clippy::expect_used)]` and append:

```rust
/// Renders the first statement's expression as an indented tree of node
/// kinds and token texts, offsets and trivia omitted: the workhorse
/// assertion of the expression grammar tests. Wraps the fragment as one
/// PHP statement, so the fragment must be a single valid-ish expression.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_expression(expression_source: &str) -> String {
    let source = format!("<?php {expression_source};");
    let parse = parse_verified(&source);
    let statement = parse.tree().children().next().expect("a first statement");
    let expression = statement
        .children()
        .next()
        .expect("an expression inside the statement");
    let mut output = String::new();
    render_element_without_offsets(&mut output, expression.into(), 0);
    output
}

#[allow(dead_code)]
fn render_element_without_offsets(
    output: &mut String,
    element: celerrate_syntax::SyntaxElement,
    depth: usize,
) {
    use std::fmt::Write as _;

    let indent = "  ".repeat(depth);
    match element {
        celerrate_syntax::SyntaxElement::Node(node) => {
            let _ = writeln!(output, "{indent}{:?}", node.kind());
            for child in node.children_with_tokens() {
                render_element_without_offsets(output, child, depth + 1);
            }
        }
        celerrate_syntax::SyntaxElement::Token(token) => {
            if token.kind().is_trivia() {
                return;
            }
            let _ = writeln!(output, "{indent}{:?} {:?}", token.kind(), token.text());
        }
    }
}
```

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_syntax/tests/expressions.rs`:

```rust
//! Expression grammar tests: tree shapes through
//! `support::render_expression` (offsets omitted), diagnostics asserted
//! structurally. Every parse asserts the lossless invariant on the way.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxDiagnosticKind, SyntaxKind};

fn parser_diagnostics(source: &str) -> Vec<ParserDiagnosticKind> {
    support::parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    insta::assert_snapshot!(support::render_expression("1 + 2 * 3"), @r#"
    BinaryExpression
      Literal
        IntegerLiteral "1"
      Plus "+"
      BinaryExpression
        Literal
          IntegerLiteral "2"
        Star "*"
        Literal
          IntegerLiteral "3"
    "#);
}

#[test]
fn same_level_operators_associate_left() {
    insta::assert_snapshot!(support::render_expression("1 - 2 - 3"), @r#"
    BinaryExpression
      BinaryExpression
        Literal
          IntegerLiteral "1"
        Minus "-"
        Literal
          IntegerLiteral "2"
      Minus "-"
      Literal
        IntegerLiteral "3"
    "#);
}

#[test]
fn power_associates_right() {
    insta::assert_snapshot!(support::render_expression("2 ** 3 ** 4"), @r#"
    BinaryExpression
      Literal
        IntegerLiteral "2"
      StarStar "**"
      BinaryExpression
        Literal
          IntegerLiteral "3"
        StarStar "**"
        Literal
          IntegerLiteral "4"
    "#);
}

#[test]
fn concatenation_sits_below_addition() {
    // The PHP 8.0 precedence change: `.` is looser than `+`.
    insta::assert_snapshot!(support::render_expression("'a' . 1 + 2"), @r#"
    BinaryExpression
      Literal
        SingleQuotedString "'a'"
      Dot "."
      BinaryExpression
        Literal
          IntegerLiteral "1"
        Plus "+"
        Literal
          IntegerLiteral "2"
    "#);
}

#[test]
fn the_pipe_operator_sits_between_comparison_and_concatenation() {
    insta::assert_snapshot!(support::render_expression("$x . 'a' |> $f == 4"), @r#"
    BinaryExpression
      BinaryExpression
        BinaryExpression
          VariableReference
            Variable "$x"
          Dot "."
          Literal
            SingleQuotedString "'a'"
        PipeGreater "|>"
        VariableReference
          Variable "$f"
      EqualsEquals "=="
      Literal
        IntegerLiteral "4"
    "#);
}

#[test]
fn coalesce_associates_right() {
    insta::assert_snapshot!(support::render_expression("$a ?? $b ?? $c"), @r#"
    BinaryExpression
      VariableReference
        Variable "$a"
      QuestionQuestion "??"
      BinaryExpression
        VariableReference
          Variable "$b"
        QuestionQuestion "??"
        VariableReference
          Variable "$c"
    "#);
}

#[test]
fn word_logical_operators_bind_loosest() {
    insta::assert_snapshot!(support::render_expression("$a && $b or $c && $d"), @r#"
    BinaryExpression
      BinaryExpression
        VariableReference
          Variable "$a"
        AmpersandAmpersand "&&"
        VariableReference
          Variable "$b"
      Or "or"
      BinaryExpression
        VariableReference
          Variable "$c"
        AmpersandAmpersand "&&"
        VariableReference
          Variable "$d"
    "#);
}

#[test]
fn instanceof_is_a_binary_operator() {
    insta::assert_snapshot!(support::render_expression("$a instanceof $class"), @r#"
    BinaryExpression
      VariableReference
        Variable "$a"
      InstanceOf "instanceof"
      VariableReference
        Variable "$class"
    "#);
}

#[test]
fn parentheses_regroup() {
    insta::assert_snapshot!(support::render_expression("(1 + 2) * 3"), @r#"
    BinaryExpression
      ParenthesizedExpression
        OpenParenthesis "("
        BinaryExpression
          Literal
            IntegerLiteral "1"
          Plus "+"
          Literal
            IntegerLiteral "2"
        CloseParenthesis ")"
      Star "*"
      Literal
        IntegerLiteral "3"
    "#);
}

#[test]
fn a_comparison_chain_is_diagnosed_and_still_parses() {
    // Zend rejects `1 < 2 < 3`; we parse it left-associatively and say so.
    let parse = support::parse_verified("<?php 1 < 2 < 3;");
    let kinds = parse
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![SyntaxDiagnosticKind::Parser(
            ParserDiagnosticKind::NonAssociativeOperator
        )]
    );
}

#[test]
fn equality_and_relational_are_different_levels() {
    assert!(parser_diagnostics("<?php $a == $b < $c;").is_empty());
}

#[test]
fn a_missing_right_operand_is_diagnosed_and_the_node_completes() {
    let diagnostics = parser_diagnostics("<?php 1 +;");
    assert_eq!(diagnostics, vec![ParserDiagnosticKind::ExpectedExpression]);
    let parse = support::parse_verified("<?php 1 +;");
    let statement = parse.tree().children().next().expect("a statement");
    assert_eq!(
        statement.children().next().map(|node| node.kind()),
        Some(SyntaxKind::BinaryExpression)
    );
}

#[test]
fn an_unclosed_parenthesis_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php (1 + 2;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseParenthesis))
    );
}

#[test]
fn pathological_nesting_trips_the_guard_without_panicking() {
    let source = format!("<?php {}1;", "(".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions`
Expected: FAIL to compile (`render_expression` exists but `ParenthesizedExpression`, `BinaryExpression`, `NonAssociativeOperator` do not), or failing assertions once kinds exist.

- [ ] **Step 4: Implement**

1. `syntax_kind.rs`: append after `ErrorNode`:

```rust
    /// `( expression )`.
    ParenthesizedExpression,
    /// One binary operation: left operand, operator token, right operand.
    /// The operator token distinguishes `+` from `instanceof` from `|>`.
    BinaryExpression,
```

2. `diagnostic.rs`: add to `ParserDiagnosticKind`:

```rust
    /// A non-associative operator chained at the same level, which Zend
    /// rejects (`1 < 2 < 3`, unparenthesized ternary chains, double
    /// `instanceof`). Parsed left-associatively anyway.
    NonAssociativeOperator,
```

3. `grammar.rs`: declare the submodule and delegate. Delete the old `expression` and `is_expression_start` functions, keep everything else:

```rust
mod expressions;

use expressions::{expression, starts_expression};
```

In `statement`, the dispatch arm becomes:

```rust
        Some(kind) if starts_expression(kind) => expression_statement(parser),
```

`echo_statement` and `expression_statement` keep calling `expression(parser)` unchanged (it now resolves to the submodule's function).

4. Create `crates/celerrate_syntax/src/parser/grammar/expressions.rs`:

```rust
//! The expression grammar: a Pratt loop over the precedence table
//! transcribed from php-src's `zend_language_parser.y` (branch
//! PHP-8.5), a prefix dispatch, a postfix loop, and the primary
//! expressions. Levels are loosest first; binding powers are
//! `level * 2`, so right-associative operators can recurse at
//! `level * 2 - 1` and everything stays in one `u8` space.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::{CompletedMarker, Parser};

// One constant per level of the precedence table (see the plan's
// authoritative table). Declared as each task consumes them, so the
// dead-code lint stays green at every commit: this task declares the
// binary levels; task 4 adds 24 and 26, task 5 adds 9 and 10, task 11
// adds 28, task 13 adds 4 through 8.
const LOGICAL_OR_LEVEL: u8 = 1;
const LOGICAL_XOR_LEVEL: u8 = 2;
const LOGICAL_AND_LEVEL: u8 = 3;
const COALESCE_LEVEL: u8 = 11;
const BOOLEAN_OR_LEVEL: u8 = 12;
const BOOLEAN_AND_LEVEL: u8 = 13;
const BITWISE_OR_LEVEL: u8 = 14;
const BITWISE_XOR_LEVEL: u8 = 15;
const BITWISE_AND_LEVEL: u8 = 16;
const EQUALITY_LEVEL: u8 = 17;
const RELATIONAL_LEVEL: u8 = 18;
const PIPE_LEVEL: u8 = 19;
const CONCATENATION_LEVEL: u8 = 20;
const SHIFT_LEVEL: u8 = 21;
const ADDITIVE_LEVEL: u8 = 22;
const MULTIPLICATIVE_LEVEL: u8 = 23;
const INSTANCEOF_LEVEL: u8 = 25;
const POWER_LEVEL: u8 = 27;

fn left_binding_power(level: u8) -> u8 {
    level * 2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Associativity {
    Left,
    Right,
    /// Zend rejects same-level chains; we parse them left-associatively
    /// and diagnose.
    NonAssociative,
}

/// The binary operator table. Returns the level and associativity, or
/// `None` for any token that is not a binary operator.
fn binary_operator(kind: SyntaxKind) -> Option<(u8, Associativity)> {
    use Associativity::{Left, NonAssociative, Right};
    let entry = match kind {
        SyntaxKind::Or => (LOGICAL_OR_LEVEL, Left),
        SyntaxKind::Xor => (LOGICAL_XOR_LEVEL, Left),
        SyntaxKind::And => (LOGICAL_AND_LEVEL, Left),
        SyntaxKind::QuestionQuestion => (COALESCE_LEVEL, Right),
        SyntaxKind::PipePipe => (BOOLEAN_OR_LEVEL, Left),
        SyntaxKind::AmpersandAmpersand => (BOOLEAN_AND_LEVEL, Left),
        SyntaxKind::Pipe => (BITWISE_OR_LEVEL, Left),
        SyntaxKind::Caret => (BITWISE_XOR_LEVEL, Left),
        SyntaxKind::Ampersand => (BITWISE_AND_LEVEL, Left),
        SyntaxKind::EqualsEquals
        | SyntaxKind::BangEquals
        | SyntaxKind::EqualsEqualsEquals
        | SyntaxKind::BangEqualsEquals
        | SyntaxKind::Spaceship => (EQUALITY_LEVEL, NonAssociative),
        SyntaxKind::Less
        | SyntaxKind::LessEquals
        | SyntaxKind::Greater
        | SyntaxKind::GreaterEquals => (RELATIONAL_LEVEL, NonAssociative),
        SyntaxKind::PipeGreater => (PIPE_LEVEL, Left),
        SyntaxKind::Dot => (CONCATENATION_LEVEL, Left),
        SyntaxKind::LessLess | SyntaxKind::GreaterGreater => (SHIFT_LEVEL, Left),
        SyntaxKind::Plus | SyntaxKind::Minus => (ADDITIVE_LEVEL, Left),
        SyntaxKind::Star | SyntaxKind::Slash | SyntaxKind::Percent => {
            (MULTIPLICATIVE_LEVEL, Left)
        }
        SyntaxKind::InstanceOf => (INSTANCEOF_LEVEL, NonAssociative),
        SyntaxKind::StarStar => (POWER_LEVEL, Right),
        _ => return None,
    };
    Some(entry)
}

/// Which tokens can start an expression. Grows with every task of this
/// plan; the statement dispatcher keys off it.
pub(super) fn starts_expression(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IntegerLiteral
            | SyntaxKind::FloatLiteral
            | SyntaxKind::SingleQuotedString
            | SyntaxKind::Variable
            | SyntaxKind::OpenParenthesis
    )
}

pub(super) fn expression(parser: &mut Parser) -> Option<CompletedMarker> {
    expression_with_minimum_power(parser, 0)
}

fn expression_with_minimum_power(
    parser: &mut Parser,
    minimum_power: u8,
) -> Option<CompletedMarker> {
    if !parser.enter_nesting() {
        return None;
    }
    let result = binary_loop(parser, minimum_power);
    parser.leave_nesting();
    result
}

fn binary_loop(parser: &mut Parser, minimum_power: u8) -> Option<CompletedMarker> {
    let mut left = prefix_expression(parser)?;
    // The previous wrap's binding power, for the non-associativity
    // diagnostic: a same-level wrap right after a non-associative one
    // is a chain Zend rejects.
    let mut previous_power: Option<u8> = None;
    while let Some(kind) = parser.current() {
        let Some((level, associativity)) = binary_operator(kind) else {
            break;
        };
        let power = left_binding_power(level);
        if power < minimum_power {
            break;
        }
        if associativity == Associativity::NonAssociative && previous_power == Some(power) {
            parser.diagnose_current(ParserDiagnosticKind::NonAssociativeOperator);
        }
        let marker = left.precede(parser);
        parser.bump();
        let next_minimum = match associativity {
            Associativity::Right => power - 1,
            Associativity::Left | Associativity::NonAssociative => power + 1,
        };
        // A missing right operand was already diagnosed downstream; the
        // node still completes so the tree stays full-coverage.
        expression_with_minimum_power(parser, next_minimum);
        left = marker.complete(parser, SyntaxKind::BinaryExpression);
        previous_power = Some(power);
    }
    Some(left)
}

/// Prefix operators land here in task 4; until then this layer passes
/// through.
fn prefix_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    postfix_expression(parser)
}

/// Postfix chains land here (tasks 4, 7, 8); until then this layer
/// passes through.
fn postfix_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    primary_expression(parser)
}

fn primary_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    match parser.current() {
        Some(
            SyntaxKind::IntegerLiteral
            | SyntaxKind::FloatLiteral
            | SyntaxKind::SingleQuotedString,
        ) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::Literal))
        }
        Some(SyntaxKind::Variable) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::VariableReference))
        }
        Some(SyntaxKind::OpenParenthesis) => Some(parenthesized_expression(parser)),
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
            None
        }
    }
}

fn parenthesized_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump();
    expression(parser);
    parser.expect(SyntaxKind::CloseParenthesis);
    marker.complete(parser, SyntaxKind::ParenthesizedExpression)
}
```

5. Add the corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_operators.php`:

```php
<?php
1 + 2 * 3 - 4 / 5 % 6;
2 ** 3 ** 4;
'a' . 'b' . 1 + 2;
1 << 2 >> 3;
$a < $b || $c >= $d && $e == $f;
$a === $b xor $c !== $d;
$a <=> $b;
$a ?? $b ?? $c;
$a | $b ^ $c & $d;
1 |> $f |> $g;
$a instanceof $class;
$a and $b or $c;
(1 + 2) * (3 - 4);
```

(Every operand is a literal or a variable on purpose: names only parse in task 6, and corpus files stay diagnostic-free.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS. The new corpus file produces a pending snapshot: run `cargo insta test -p celerrate_syntax --review` (or `--accept` after inspecting) and check the snapshot in. Existing corpus snapshots must not change.

- [ ] **Step 6: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse binary expressions over the Zend precedence table"
```

---

### Task 4: Prefix operators, casts, and increment/decrement

`!`, `~`, unary `+`/`-`, `@`, prefix and postfix `++`/`--`, and the seven cast tokens. Two node kinds for symmetry with how consumers read them (`PrefixExpression`, `PostfixExpression`) plus `CastExpression`. The pinned subtlety: `**` binds tighter than prefix minus.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `PrefixExpression`, `PostfixExpression`, `CastExpression`)
- Test: `crates/celerrate_syntax/tests/expressions.rs`, corpus file `tests/parse_corpus/expressions_unary.php`

**Interfaces:**
- Consumes: task 3's layer structure (`prefix_expression`, `postfix_expression` pass-throughs).
- Produces: `prefix_expression` handling the operator tokens above; `postfix_expression` gaining its wrap loop (only `++`/`--` arms for now; tasks 7 and 8 add more arms to the same loop).

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions.rs`:

```rust
#[test]
fn power_binds_tighter_than_prefix_minus() {
    insta::assert_snapshot!(support::render_expression("-2 ** 3"), @r#"
    PrefixExpression
      Minus "-"
      BinaryExpression
        Literal
          IntegerLiteral "2"
        StarStar "**"
        Literal
          IntegerLiteral "3"
    "#);
}

#[test]
fn logical_not_binds_tighter_than_boolean_and() {
    insta::assert_snapshot!(support::render_expression("!$a && $b"), @r#"
    BinaryExpression
      PrefixExpression
        Bang "!"
        VariableReference
          Variable "$a"
      AmpersandAmpersand "&&"
      VariableReference
        Variable "$b"
    "#);
}

#[test]
fn a_cast_is_a_single_token_prefix() {
    insta::assert_snapshot!(support::render_expression("(int) $x + 1"), @r#"
    BinaryExpression
      CastExpression
        IntCast "(int)"
        VariableReference
          Variable "$x"
      Plus "+"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn increment_works_prefix_and_postfix() {
    insta::assert_snapshot!(support::render_expression("++$i"), @r#"
    PrefixExpression
      PlusPlus "++"
      VariableReference
        Variable "$i"
    "#);
    insta::assert_snapshot!(support::render_expression("$i++"), @r#"
    PostfixExpression
      VariableReference
        Variable "$i"
      PlusPlus "++"
    "#);
}

#[test]
fn error_suppression_wraps_its_operand() {
    insta::assert_snapshot!(support::render_expression("@$x + 1"), @r#"
    BinaryExpression
      PrefixExpression
        At "@"
        VariableReference
          Variable "$x"
      Plus "+"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn prefix_operators_nest() {
    insta::assert_snapshot!(support::render_expression("- -$x"), @r#"
    PrefixExpression
      Minus "-"
      PrefixExpression
        Minus "-"
        VariableReference
          Variable "$x"
    "#);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions`
Expected: FAIL (kinds missing, then wrong shapes: `-2 ** 3` currently diagnoses `ExpectedExpression` at `-`).

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append after `BinaryExpression`:

```rust
    /// A prefix operation: `!`, `~`, unary `+`/`-`, `@`, `++`, `--`.
    PrefixExpression,
    /// A postfix operation: `++`, `--`.
    PostfixExpression,
    /// A cast: the single cast token, then the operand.
    CastExpression,
```

2. `expressions.rs`: add this task's level constants to the block from task 3, in table position:

```rust
const LOGICAL_NOT_LEVEL: u8 = 24;
const UNARY_LEVEL: u8 = 26;
```

3. `expressions.rs`: replace the pass-through `prefix_expression` with the dispatch. Later tasks add keyword arms to this match; the `_` arm stays the postfix fallthrough:

```rust
fn prefix_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    let (node_kind, operand_power) = match parser.current()? {
        SyntaxKind::Bang => (
            SyntaxKind::PrefixExpression,
            left_binding_power(LOGICAL_NOT_LEVEL),
        ),
        SyntaxKind::Plus
        | SyntaxKind::Minus
        | SyntaxKind::Tilde
        | SyntaxKind::At
        | SyntaxKind::PlusPlus
        | SyntaxKind::MinusMinus => (
            SyntaxKind::PrefixExpression,
            left_binding_power(UNARY_LEVEL),
        ),
        SyntaxKind::IntCast
        | SyntaxKind::BoolCast
        | SyntaxKind::FloatCast
        | SyntaxKind::StringCast
        | SyntaxKind::BinaryCast
        | SyntaxKind::ArrayCast
        | SyntaxKind::ObjectCast => (
            SyntaxKind::CastExpression,
            left_binding_power(UNARY_LEVEL),
        ),
        _ => return postfix_expression(parser),
    };
    let marker = parser.start();
    parser.bump();
    // A missing operand is diagnosed downstream; the node completes
    // regardless, partial trees are normal citizens.
    expression_with_minimum_power(parser, operand_power);
    Some(marker.complete(parser, node_kind))
}
```

Note `parser.current()?`: at end of input there is no expression; the statement loop already stops on `None`, and `expression`'s callers treat `None` uniformly. The `ExpectedExpression` diagnostic for "nothing here" stays in `primary_expression`, reached through the `_` arm. Wait: at end of input the `_` arm is unreachable (`current()` returned `None` and `?` exited early) and no diagnostic fires; plan 1's `expression` diagnosed in that case. Preserve the old contract: change the first line to

```rust
    let Some(kind) = parser.current() else {
        parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
        return None;
    };
    let (node_kind, operand_power) = match kind {
```

(`diagnose_current` at end of input points zero-width after the last token, which is what plan 1's tests already pin for `<?php echo`.)

4. `expressions.rs`: replace the pass-through `postfix_expression` with the wrap loop:

```rust
/// The tightest tier: postfix wraps applied greedily around a primary.
/// Tasks 7 and 8 add call, member, scoped, and index arms to this loop.
fn postfix_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = primary_expression(parser)?;
    loop {
        left = match parser.current() {
            Some(SyntaxKind::PlusPlus | SyntaxKind::MinusMinus) => {
                let marker = left.precede(parser);
                parser.bump();
                marker.complete(parser, SyntaxKind::PostfixExpression)
            }
            _ => break,
        };
    }
    Some(left)
}
```

5. `starts_expression` grows:

```rust
            | SyntaxKind::Bang
            | SyntaxKind::Tilde
            | SyntaxKind::Plus
            | SyntaxKind::Minus
            | SyntaxKind::PlusPlus
            | SyntaxKind::MinusMinus
            | SyntaxKind::At
            | SyntaxKind::IntCast
            | SyntaxKind::BoolCast
            | SyntaxKind::FloatCast
            | SyntaxKind::StringCast
            | SyntaxKind::BinaryCast
            | SyntaxKind::ArrayCast
            | SyntaxKind::ObjectCast
```

Two knock-on effects of `+` becoming an expression start, both correct behavior rather than regressions:

- Plan 1's test `an_unexpected_token_becomes_an_error_node_and_parsing_continues` uses `<?php + echo 1;`, which now parses `+` as a prefix expression with a missing operand instead of an error node. Keep the test's name and intent by switching the stray token to one that still has no rule: `<?php ) echo 1;`.
- The existing corpus snapshot for `recovery.php` changes on its lone `+` line (now a `PrefixExpression` with an `ExpectedExpression` diagnostic). Review that this is the only change, then accept it.

6. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_unary.php`:

```php
<?php
-2 ** 3;
!$a && !$b;
(int) '42' + (float) '1.5';
(bool) $flag;
(string) 42 . (binary) 'raw';
(array) $value;
(object) $map;
~$mask | $bits;
@$maybe;
++$i;
--$i;
$i++ + $i--;
- -$x;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS after updating the plan 1 test noted above and accepting the reviewed corpus snapshot (`cargo insta test -p celerrate_syntax --review`).

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse unary, cast, and increment expressions"
```

---

### Task 5: Ternary and assignment

The two special forms of the Pratt loop: `? :` (level 10, non-associative, short form included) and the assignment family (level 9, right-associative, `= &` included). Both live as new branches **before** the `binary_operator` lookup inside `binary_loop`.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `TernaryExpression`, `AssignmentExpression`)
- Test: `crates/celerrate_syntax/tests/expressions.rs`, corpus file `tests/parse_corpus/expressions_assignment.php`

**Interfaces:**
- Consumes: `binary_loop`, `previous_power` tracking.
- Produces: `TernaryExpression` (condition, `?`, optional middle, `:`, third operand), `AssignmentExpression` (target, operator token, optional `&`, value). Later tasks rely on assignment parsing for destructuring tests (`[$a, $b] = $pair`).

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions.rs`:

```rust
#[test]
fn assignment_associates_right() {
    insta::assert_snapshot!(support::render_expression("$a = $b = 1"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$a"
      Equals "="
      AssignmentExpression
        VariableReference
          Variable "$b"
        Equals "="
        Literal
          IntegerLiteral "1"
    "#);
}

#[test]
fn compound_assignment_operators_parse() {
    insta::assert_snapshot!(support::render_expression("$x ??= 1"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$x"
      QuestionQuestionEquals "??="
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn assignment_by_reference_keeps_the_ampersand() {
    insta::assert_snapshot!(support::render_expression("$a = &$b"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$a"
      Equals "="
      Ampersand "&"
      VariableReference
        Variable "$b"
    "#);
}

#[test]
fn the_ternary_parses_long_and_short_forms() {
    insta::assert_snapshot!(support::render_expression("$a ? 'y' : 'n'"), @r#"
    TernaryExpression
      VariableReference
        Variable "$a"
      Question "?"
      Literal
        SingleQuotedString "'y'"
      Colon ":"
      Literal
        SingleQuotedString "'n'"
    "#);
    insta::assert_snapshot!(support::render_expression("$a ?: 'n'"), @r#"
    TernaryExpression
      VariableReference
        Variable "$a"
      Question "?"
      Colon ":"
      Literal
        SingleQuotedString "'n'"
    "#);
}

#[test]
fn coalesce_binds_tighter_than_the_ternary() {
    insta::assert_snapshot!(support::render_expression("$a ?? $b ? 1 : 2"), @r#"
    TernaryExpression
      BinaryExpression
        VariableReference
          Variable "$a"
        QuestionQuestion "??"
        VariableReference
          Variable "$b"
      Question "?"
      Literal
        IntegerLiteral "1"
      Colon ":"
      Literal
        IntegerLiteral "2"
    "#);
}

#[test]
fn assignment_binds_looser_than_the_ternary() {
    insta::assert_snapshot!(support::render_expression("$a = $b ? 1 : 2"), @r#"
    AssignmentExpression
      VariableReference
        Variable "$a"
      Equals "="
      TernaryExpression
        VariableReference
          Variable "$b"
        Question "?"
        Literal
          IntegerLiteral "1"
        Colon ":"
        Literal
          IntegerLiteral "2"
    "#);
}

#[test]
fn an_unparenthesized_ternary_chain_is_diagnosed() {
    // A compile error in Zend since 8.0; parsed left-associatively here.
    let diagnostics = parser_diagnostics("<?php $a ? 1 : $b ? 2 : 3;");
    assert_eq!(
        diagnostics,
        vec![ParserDiagnosticKind::NonAssociativeOperator]
    );
}

#[test]
fn a_ternary_missing_its_colon_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php $a ? 1;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Colon))
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions`
Expected: FAIL (kinds missing; `$a = 1` currently leaves `= 1` unparsed and the statement terminator diagnoses).

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `condition ? middle : third`; the short form `?:` has no middle.
    TernaryExpression,
    /// `target = value` and the compound forms; `= &value` keeps its
    /// ampersand as a token child. Whether the target is assignable is
    /// a semantic judgment.
    AssignmentExpression,
```

2. `expressions.rs`: add this task's level constants to the block, in table position:

```rust
const ASSIGNMENT_LEVEL: u8 = 9;
const TERNARY_LEVEL: u8 = 10;
```

3. `expressions.rs`: add the assignment classifier near `binary_operator`:

```rust
fn is_assignment_operator(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Equals
            | SyntaxKind::PlusEquals
            | SyntaxKind::MinusEquals
            | SyntaxKind::StarEquals
            | SyntaxKind::SlashEquals
            | SyntaxKind::DotEquals
            | SyntaxKind::PercentEquals
            | SyntaxKind::StarStarEquals
            | SyntaxKind::AmpersandEquals
            | SyntaxKind::PipeEquals
            | SyntaxKind::CaretEquals
            | SyntaxKind::LessLessEquals
            | SyntaxKind::GreaterGreaterEquals
            | SyntaxKind::QuestionQuestionEquals
    )
}
```

4. In `binary_loop`, insert the two special forms at the top of the `while` body, before the `binary_operator` lookup:

```rust
    while let Some(kind) = parser.current() {
        if is_assignment_operator(kind) {
            let power = left_binding_power(ASSIGNMENT_LEVEL);
            if power < minimum_power {
                break;
            }
            let marker = left.precede(parser);
            parser.bump();
            if kind == SyntaxKind::Equals {
                // `= &$value` assigns by reference.
                parser.eat(SyntaxKind::Ampersand);
            }
            expression_with_minimum_power(parser, power - 1);
            left = marker.complete(parser, SyntaxKind::AssignmentExpression);
            previous_power = Some(power);
            continue;
        }
        if kind == SyntaxKind::Question {
            let power = left_binding_power(TERNARY_LEVEL);
            if power < minimum_power {
                break;
            }
            // Zend rejects unparenthesized ternary chains since 8.0.
            if previous_power == Some(power) {
                parser.diagnose_current(ParserDiagnosticKind::NonAssociativeOperator);
            }
            let marker = left.precede(parser);
            parser.bump();
            if !parser.at(SyntaxKind::Colon) {
                // The middle operand is a full expression, as in Zend's
                // `expr '?' expr ':' expr`.
                expression(parser);
            }
            parser.expect(SyntaxKind::Colon);
            expression_with_minimum_power(parser, power + 1);
            left = marker.complete(parser, SyntaxKind::TernaryExpression);
            previous_power = Some(power);
            continue;
        }
        let Some((level, associativity)) = binary_operator(kind) else {
            break;
        };
        // ... the existing binary handling, unchanged ...
    }
```

The chain diagnostic works because the third operand recurses at `power + 1`, so a following `?` falls back to this loop with `previous_power == Some(power)`.

5. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_assignment.php`:

```php
<?php
$a = $b = $c;
$total += 1;
$path .= '/etc';
$mask <<= 2;
$value ??= 'default';
$reference = &$original;
$grade = $score >= 90 ? 'a' : 'b';
$display = $name ?: 'anonymous';
$pick = $first ?? $second ? 'found' : 'missing';
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse ternary and assignment expressions"
```

---

### Task 6: Names, name expressions, and dynamic variables

Qualified names (`Foo\Bar`, `\Foo`, `namespace\Foo`) as `Name` nodes wrapped in `NameExpression` when used as expressions (constant fetches, future callees), `static` as a scoped-access subject, and the dynamic variable forms `$$x` / `${expression}`. This task also hardens the statement dispatcher: `starts_expression` now admits tokens the expression grammar can still refuse (`namespace` without `\`), so `expression_statement` must guarantee progress on refusal.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs` (refusal recovery in `expression_statement`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `Name`, `NameExpression`, `DynamicVariableExpression`)
- Test: `crates/celerrate_syntax/tests/expressions_postfix.rs` (new), corpus file `tests/parse_corpus/expressions_names.php`

**Interfaces:**
- Consumes: `simple_variable` does not exist yet; this task creates it. `enter_nesting`/`leave_nesting` from task 2.
- Produces:
  - `fn name(parser) -> CompletedMarker` (a `Name` node; leading `\`, `namespace\` prefix, `Identifier (\ Identifier)*`)
  - `fn simple_variable(parser) -> Option<CompletedMarker>` (`VariableReference` for `$x`; `DynamicVariableExpression` for `$$x`, `${expression}`; recursion guarded) — tasks 8 and 10 reuse it for member names and interpolation
  - `starts_expression` grows: `Identifier`, `Backslash`, `Namespace`, `Static`, `Dollar`

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/expressions_postfix.rs`:

```rust
//! Names, calls, member access, scoped access, and indexing.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxDiagnosticKind, SyntaxKind};

fn parser_diagnostics(source: &str) -> Vec<ParserDiagnosticKind> {
    support::parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}

#[test]
fn a_bare_identifier_is_a_name_expression() {
    insta::assert_snapshot!(support::render_expression("PHP_EOL"), @r#"
    NameExpression
      Name
        Identifier "PHP_EOL"
    "#);
}

#[test]
fn qualified_and_fully_qualified_names_parse() {
    insta::assert_snapshot!(support::render_expression("\\Foo\\Bar"), @r#"
    NameExpression
      Name
        Backslash "\\"
        Identifier "Foo"
        Backslash "\\"
        Identifier "Bar"
    "#);
    insta::assert_snapshot!(support::render_expression("namespace\\Foo"), @r#"
    NameExpression
      Name
        Namespace "namespace"
        Backslash "\\"
        Identifier "Foo"
    "#);
}

#[test]
fn true_false_null_are_plain_names() {
    // The design routes them through semantic resolution, not the lexer.
    insta::assert_snapshot!(support::render_expression("true"), @r#"
    NameExpression
      Name
        Identifier "true"
    "#);
}

#[test]
fn instanceof_accepts_a_name_on_the_right() {
    insta::assert_snapshot!(support::render_expression("$a instanceof Foo\\Bar"), @r#"
    BinaryExpression
      VariableReference
        Variable "$a"
      InstanceOf "instanceof"
      NameExpression
        Name
          Identifier "Foo"
          Backslash "\\"
          Identifier "Bar"
    "#);
}

#[test]
fn dynamic_variables_parse_recursively() {
    insta::assert_snapshot!(support::render_expression("$$x"), @r#"
    DynamicVariableExpression
      Dollar "$"
      VariableReference
        Variable "$x"
    "#);
    insta::assert_snapshot!(support::render_expression("${'a' . 'b'}"), @r#"
    DynamicVariableExpression
      Dollar "$"
      OpenBrace "{"
      BinaryExpression
        Literal
          SingleQuotedString "'a'"
        Dot "."
        Literal
          SingleQuotedString "'b'"
      CloseBrace "}"
    "#);
}

#[test]
fn a_lone_dollar_is_diagnosed_but_wrapped() {
    assert!(parser_diagnostics("<?php $ + 1;").contains(&ParserDiagnosticKind::ExpectedExpression));
}

#[test]
fn pathological_dollar_chains_trip_the_guard_without_panicking() {
    let source = format!("<?php {}x;", "$".repeat(300));
    let parse = support::parse_verified(&source);
    assert!(parse.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        SyntaxDiagnosticKind::Parser(ParserDiagnosticKind::NestingTooDeep)
    )));
}

#[test]
fn a_refused_expression_start_still_advances() {
    // `namespace` not followed by `\` is a declaration keyword this
    // plan does not parse; the statement loop must not livelock on it.
    let parse = support::parse_verified("<?php namespace Foo; $x;");
    assert!(!parse.diagnostics().is_empty());
    assert!(
        parse
            .tree()
            .children()
            .any(|node| node.kind() == SyntaxKind::ExpressionStatement)
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_postfix`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// A possibly-qualified name: `Foo`, `Foo\Bar`, `\Foo`, `namespace\Foo`.
    Name,
    /// A name used as an expression: a constant fetch or a callee.
    NameExpression,
    /// `$$name` and `${expression}`.
    DynamicVariableExpression,
```

2. `expressions.rs`, add the two rules:

```rust
/// `Foo`, `Foo\Bar`, `\Foo`, `namespace\Foo`: one `Name` node. Zend
/// lexes each qualified form as a single token, which forbids interior
/// whitespace; the trivia-free token view cannot see adjacency, so
/// spaced segments parse here and adjacency is judged upstairs.
fn name(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    if parser.eat(SyntaxKind::Namespace) {
        parser.expect(SyntaxKind::Backslash);
    } else {
        parser.eat(SyntaxKind::Backslash);
    }
    parser.expect(SyntaxKind::Identifier);
    while parser.at(SyntaxKind::Backslash) && parser.nth(1) == Some(SyntaxKind::Identifier) {
        parser.bump();
        parser.bump();
    }
    marker.complete(parser, SyntaxKind::Name)
}

/// `$name`, `$$name`, `${expression}`: the dynamic forms recurse, so
/// the nesting guard applies. Returns `None` without consuming when
/// the current token is not a variable form.
fn simple_variable(parser: &mut Parser) -> Option<CompletedMarker> {
    match parser.current() {
        Some(SyntaxKind::Variable) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::VariableReference))
        }
        Some(SyntaxKind::Dollar) => {
            if !parser.enter_nesting() {
                return None;
            }
            let marker = parser.start();
            parser.bump();
            match parser.current() {
                Some(SyntaxKind::OpenBrace) => {
                    parser.bump();
                    expression(parser);
                    parser.expect(SyntaxKind::CloseBrace);
                }
                Some(SyntaxKind::Variable | SyntaxKind::Dollar) => {
                    simple_variable(parser);
                }
                _ => parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression),
            }
            let completed = marker.complete(parser, SyntaxKind::DynamicVariableExpression);
            parser.leave_nesting();
            Some(completed)
        }
        _ => None,
    }
}
```

3. In `primary_expression`, replace the `Variable` arm and add the new arms (keep the `_` fallback last):

```rust
        Some(SyntaxKind::Variable | SyntaxKind::Dollar) => simple_variable(parser),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash) => {
            let marker = parser.start();
            name(parser);
            Some(marker.complete(parser, SyntaxKind::NameExpression))
        }
        Some(SyntaxKind::Namespace) if parser.nth(1) == Some(SyntaxKind::Backslash) => {
            let marker = parser.start();
            name(parser);
            Some(marker.complete(parser, SyntaxKind::NameExpression))
        }
        // `static` as a scoped-access subject (`static::create()`).
        // Static closures take a different arm in task 15.
        Some(SyntaxKind::Static) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::NameExpression))
        }
```

4. `starts_expression` grows:

```rust
            | SyntaxKind::Identifier
            | SyntaxKind::Backslash
            | SyntaxKind::Namespace
            | SyntaxKind::Static
            | SyntaxKind::Dollar
```

5. `grammar.rs`: guarantee progress when the dispatcher admits a token the grammar refuses. Replace `expression_statement`:

```rust
fn expression_statement(parser: &mut Parser) {
    let marker = parser.start();
    if expression(parser).is_none() {
        // The dispatcher saw an expression start but the grammar
        // refused (for example `namespace` without `\`). The refusal
        // already carried its diagnostic; consume the token so the
        // statement loop always advances.
        if parser.at_end() {
            marker.abandon(parser);
            return;
        }
        parser.bump();
        marker.complete(parser, SyntaxKind::ErrorNode);
        return;
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::ExpressionStatement);
}
```

6. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_names.php`:

```php
<?php
PHP_EOL;
\Foo\Bar;
namespace\helpers;
Foo\Bar\BAZ;
true;
false;
null;
$a instanceof Foo\Bar;
$$indirect;
$$$doubly_indirect;
${'computed' . $suffix};
```

Note: `namespace\helpers;` needs the identifier after `namespace\`; the corpus keeps every line a valid expression statement so this file stays diagnostic-free.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse names and dynamic variables"
```

---

### Task 7: Call expressions and argument lists

Calls as a postfix wrap on any callee, with the full 8.x argument grammar: positional, named (`label:`, keywords allowed), spread (`...$arguments`), the first-class callable form `f(...)`, legacy call-site by-reference, trailing commas, and list recovery that never swallows past `;`, `?>`, or end of input. `argument_list` becomes the shared engine that `isset`/`exit`/`clone(...)`/`new` reuse in later tasks.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs` (delegate `error_statement`'s body to the shared `error_element`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `ArgumentList`, `Argument`, `CallExpression`)
- Test: `crates/celerrate_syntax/tests/expressions_postfix.rs`, corpus file `tests/parse_corpus/expressions_calls.php`

**Interfaces:**
- Consumes: the postfix loop (task 4), `name`/`simple_variable` (task 6), `is_keyword` (task 1).
- Produces:
  - `fn argument_list(parser)` (an `ArgumentList` node: `(`, arguments and comma tokens, `)`) — reused by tasks 11, 12
  - `fn error_element(parser)` (wrap-diagnose-bump one token; shared by every delimited-list recovery, and by `grammar::error_statement`)
  - `fn expect_list_separator(parser, closing: SyntaxKind)` (comma-or-diagnose after a list element; reused by tasks 9, 14, 15)
  - the `OpenParenthesis` arm of the postfix loop producing `CallExpression`

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions_postfix.rs`:

```rust
#[test]
fn a_call_wraps_its_callee_and_arguments() {
    insta::assert_snapshot!(support::render_expression("strlen('x')"), @r#"
    CallExpression
      NameExpression
        Name
          Identifier "strlen"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            SingleQuotedString "'x'"
        CloseParenthesis ")"
    "#);
}

#[test]
fn named_and_spread_arguments_parse() {
    insta::assert_snapshot!(support::render_expression("f(name: 1, ...$rest)"), @r#"
    CallExpression
      NameExpression
        Name
          Identifier "f"
      ArgumentList
        OpenParenthesis "("
        Argument
          Identifier "name"
          Colon ":"
          Literal
            IntegerLiteral "1"
        Comma ","
        Argument
          Ellipsis "..."
          VariableReference
            Variable "$rest"
        CloseParenthesis ")"
    "#);
}

#[test]
fn a_keyword_works_as_a_named_argument_label() {
    let parse = support::parse_verified("<?php f(default: 1);");
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
}

#[test]
fn the_first_class_callable_form_is_a_lone_ellipsis() {
    insta::assert_snapshot!(support::render_expression("f(...)"), @r#"
    CallExpression
      NameExpression
        Name
          Identifier "f"
      ArgumentList
        OpenParenthesis "("
        Ellipsis "..."
        CloseParenthesis ")"
    "#);
}

#[test]
fn calls_chain_and_take_any_callee() {
    insta::assert_snapshot!(support::render_expression("$f(1)(2)"), @r#"
    CallExpression
      CallExpression
        VariableReference
          Variable "$f"
        ArgumentList
          OpenParenthesis "("
          Argument
            Literal
              IntegerLiteral "1"
          CloseParenthesis ")"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "2"
        CloseParenthesis ")"
    "#);
}

#[test]
fn trailing_commas_and_by_reference_arguments_parse() {
    assert!(parser_diagnostics("<?php f($a, &$b,);").is_empty());
}

#[test]
fn a_missing_argument_separator_is_diagnosed_and_both_arguments_survive() {
    let diagnostics = parser_diagnostics("<?php f(1 2);");
    assert_eq!(
        diagnostics,
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Comma)]
    );
}

#[test]
fn an_unclosed_call_stops_at_the_statement_boundary() {
    let diagnostics = parser_diagnostics("<?php f(1; $x;");
    assert!(diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseParenthesis)));
    let parse = support::parse_verified("<?php f(1; $x;");
    assert!(
        parse
            .tree()
            .children()
            .filter(|node| node.kind() == SyntaxKind::ExpressionStatement)
            .count()
            >= 2,
        "the call must not swallow the next statement"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_postfix`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `( argument, ... )`, including the lone `...` of a first-class
    /// callable.
    ArgumentList,
    /// One argument: optional `label:`, optional `...`, optional `&`,
    /// then the expression.
    Argument,
    /// A call: the callee expression, then its argument list.
    CallExpression,
```

2. `expressions.rs`, the shared recovery and the list:

```rust
/// One token no rule accepts inside a delimited construct: wrapped,
/// diagnosed, consumed, so every list loop makes progress.
pub(super) fn error_element(parser: &mut Parser) {
    let marker = parser.start();
    parser.diagnose_current(ParserDiagnosticKind::UnexpectedToken);
    parser.bump();
    marker.complete(parser, SyntaxKind::ErrorNode);
}

fn starts_argument(parser: &Parser) -> bool {
    parser.current().is_some_and(|kind| {
        starts_expression(kind)
            || kind == SyntaxKind::Ellipsis
            || kind == SyntaxKind::Ampersand
            || (kind.is_keyword() && parser.nth(1) == Some(SyntaxKind::Colon))
    })
}

/// After a list element: eat the separating comma, or diagnose one
/// missing unless the list sits at a legitimate boundary (its closer,
/// a statement boundary, end of input). Shared by every
/// comma-separated list of this plan.
fn expect_list_separator(parser: &mut Parser, closing: SyntaxKind) {
    if parser.eat(SyntaxKind::Comma) {
        return;
    }
    if parser.at(closing)
        || parser.at(SyntaxKind::Semicolon)
        || parser.at(SyntaxKind::CloseTag)
        || parser.at_end()
    {
        return;
    }
    parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Comma));
}

/// `( argument, ... )`. The caller has already checked the opening
/// parenthesis is there (or wants its absence diagnosed). Recovery: an
/// unexpected token is wrapped and consumed; `;`, `?>`, and end of
/// input abort the list so a runaway call cannot swallow the file.
fn argument_list(parser: &mut Parser) {
    let marker = parser.start();
    parser.expect(SyntaxKind::OpenParenthesis);
    while !parser.at(SyntaxKind::CloseParenthesis) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        // `f(...)`: the first-class callable form, a lone ellipsis.
        if parser.at(SyntaxKind::Ellipsis) && parser.nth(1) == Some(SyntaxKind::CloseParenthesis)
        {
            parser.bump();
            break;
        }
        if !starts_argument(parser) {
            error_element(parser);
            continue;
        }
        argument(parser);
        expect_list_separator(parser, SyntaxKind::CloseParenthesis);
    }
    parser.expect(SyntaxKind::CloseParenthesis);
    marker.complete(parser, SyntaxKind::ArgumentList);
}

fn argument(parser: &mut Parser) {
    let marker = parser.start();
    // A named argument: `label:` where the label is an identifier or
    // any keyword (semi-reserved, accepted wholesale). `::` cannot be
    // confused with the label colon because it lexes as one token.
    let at_label = parser
        .current()
        .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword())
        && parser.nth(1) == Some(SyntaxKind::Colon);
    if at_label {
        parser.bump();
        parser.bump();
    }
    parser.eat(SyntaxKind::Ellipsis);
    // Call-site by-reference: removed from PHP, still analyzable.
    parser.eat(SyntaxKind::Ampersand);
    expression(parser);
    marker.complete(parser, SyntaxKind::Argument);
}
```

Termination argument, stated because clippy cannot check it: every loop iteration either consumes at least one token (`error_element`, and `argument` always consumes because `starts_argument` held) or breaks.

3. Add the call arm to the postfix loop in `postfix_expression`, alongside the increment arm:

```rust
            Some(SyntaxKind::OpenParenthesis) => {
                let marker = left.precede(parser);
                argument_list(parser);
                marker.complete(parser, SyntaxKind::CallExpression)
            }
```

4. `grammar.rs`: `error_statement`'s body is now duplicated by `error_element`; delegate it:

```rust
/// One token no rule accepts, wrapped and reported; the guaranteed
/// progress of the statement loop.
fn error_statement(parser: &mut Parser) {
    expressions::error_element(parser);
}
```

(Adjust the `use` line to `use expressions::{error_element, expression, starts_expression};` and call it directly if that reads better; either way, one definition.)

5. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_calls.php`:

```php
<?php
strlen('hello');
array_map($callback, $items);
f(name: 'value', ...$rest,);
strlen(...);
$callable(1)(2);
\Foo\bar($x);
g(&$legacy);
h(default: 1, list: 2);
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse call expressions and argument lists"
```

---

### Task 8: Member access, scoped access, and indexing

The remaining postfix arms: `->` / `?->` (member access), `::` (scoped access), `[...]` (indexing, empty form included). Member names accept identifiers, **every** keyword (Zend's semi-reserved list wholesale, `::class` included), variables (dynamic members, static properties), and `{expression}`. This completes the spec's motivating chain `$a->b[0]() + $c`.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `MemberAccessExpression`, `ScopedAccessExpression`, `MemberName`, `IndexExpression`)
- Modify: `crates/celerrate_syntax/src/diagnostic.rs` (add `ExpectedMemberName`)
- Test: `crates/celerrate_syntax/tests/expressions_postfix.rs`, corpus file `tests/parse_corpus/expressions_members.php`

**Interfaces:**
- Consumes: the postfix loop, `simple_variable`, `is_keyword`.
- Produces: `fn member_name(parser)` (a `MemberName` node) — task 10's simple interpolation does **not** reuse it (the string grammar is narrower); tasks 11 reuses the postfix arms transitively.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions_postfix.rs`:

```rust
#[test]
fn member_access_wraps_left_to_right() {
    insta::assert_snapshot!(support::render_expression("$user->name"), @r#"
    MemberAccessExpression
      VariableReference
        Variable "$user"
      Arrow "->"
      MemberName
        Identifier "name"
    "#);
}

#[test]
fn nullsafe_access_and_semi_reserved_members_parse() {
    insta::assert_snapshot!(support::render_expression("$user?->list()"), @r#"
    CallExpression
      MemberAccessExpression
        VariableReference
          Variable "$user"
        NullsafeArrow "?->"
        MemberName
          List "list"
      ArgumentList
        OpenParenthesis "("
        CloseParenthesis ")"
    "#);
}

#[test]
fn dynamic_and_computed_member_names_parse() {
    insta::assert_snapshot!(support::render_expression("$object->$property"), @r#"
    MemberAccessExpression
      VariableReference
        Variable "$object"
      Arrow "->"
      MemberName
        VariableReference
          Variable "$property"
    "#);
    insta::assert_snapshot!(support::render_expression("$object->{$prefix . 'x'}"), @r#"
    MemberAccessExpression
      VariableReference
        Variable "$object"
      Arrow "->"
      MemberName
        OpenBrace "{"
        BinaryExpression
          VariableReference
            Variable "$prefix"
          Dot "."
          Literal
            SingleQuotedString "'x'"
        CloseBrace "}"
    "#);
}

#[test]
fn scoped_access_covers_constants_methods_properties_and_class() {
    insta::assert_snapshot!(support::render_expression("Foo::class"), @r#"
    ScopedAccessExpression
      NameExpression
        Name
          Identifier "Foo"
      ColonColon "::"
      MemberName
        Class "class"
    "#);
    insta::assert_snapshot!(support::render_expression("Foo::$instance"), @r#"
    ScopedAccessExpression
      NameExpression
        Name
          Identifier "Foo"
      ColonColon "::"
      MemberName
        VariableReference
          Variable "$instance"
    "#);
    assert!(parser_diagnostics("<?php static::create();").is_empty());
    assert!(parser_diagnostics("<?php \\Foo\\Bar::BAZ;").is_empty());
}

#[test]
fn indexing_chains_and_the_empty_index_parse() {
    insta::assert_snapshot!(support::render_expression("$matrix[0][1]"), @r#"
    IndexExpression
      IndexExpression
        VariableReference
          Variable "$matrix"
        OpenBracket "["
        Literal
          IntegerLiteral "0"
        CloseBracket "]"
      OpenBracket "["
      Literal
        IntegerLiteral "1"
      CloseBracket "]"
    "#);
    assert!(parser_diagnostics("<?php $queue[] = 1;").is_empty());
}

#[test]
fn the_spec_chain_parses_with_forward_parents() {
    insta::assert_snapshot!(support::render_expression("$a->b[0]() + $c"), @r#"
    BinaryExpression
      CallExpression
        IndexExpression
          MemberAccessExpression
            VariableReference
              Variable "$a"
            Arrow "->"
            MemberName
              Identifier "b"
          OpenBracket "["
          Literal
            IntegerLiteral "0"
          CloseBracket "]"
        ArgumentList
          OpenParenthesis "("
          CloseParenthesis ")"
      Plus "+"
      VariableReference
        Variable "$c"
    "#);
}

#[test]
fn a_missing_member_name_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php $object->;").contains(&ParserDiagnosticKind::ExpectedMemberName)
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_postfix`
Expected: FAIL to compile (missing kinds and diagnostic), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `subject->name` and `subject?->name`.
    MemberAccessExpression,
    /// `subject::name`: constants, methods, static properties, `::class`.
    ScopedAccessExpression,
    /// The name after `->`, `?->`, or `::`: identifier, any keyword,
    /// variable, or `{ expression }`.
    MemberName,
    /// `subject[index]`; the index is absent in the push form `$a[]`.
    IndexExpression,
```

2. `diagnostic.rs`, add to `ParserDiagnosticKind`:

```rust
    /// `->`, `?->`, or `::` with nothing usable after it.
    ExpectedMemberName,
```

3. `expressions.rs`, the member name rule:

```rust
/// The name after `->`, `?->`, or `::`: an identifier, any keyword
/// (Zend's semi-reserved list accepted wholesale, `::class` included;
/// per-position validity is semantic), a variable form, or
/// `{ expression }`.
fn member_name(parser: &mut Parser) {
    let marker = parser.start();
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => parser.bump(),
        Some(SyntaxKind::Variable | SyntaxKind::Dollar) => {
            simple_variable(parser);
        }
        Some(SyntaxKind::OpenBrace) => {
            parser.bump();
            expression(parser);
            parser.expect(SyntaxKind::CloseBrace);
        }
        _ => parser.diagnose_current(ParserDiagnosticKind::ExpectedMemberName),
    }
    marker.complete(parser, SyntaxKind::MemberName);
}
```

4. Add the three arms to the postfix loop in `postfix_expression`:

```rust
            Some(SyntaxKind::Arrow | SyntaxKind::NullsafeArrow) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::MemberAccessExpression)
            }
            Some(SyntaxKind::ColonColon) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::ScopedAccessExpression)
            }
            Some(SyntaxKind::OpenBracket) => {
                let marker = left.precede(parser);
                parser.bump();
                if !parser.at(SyntaxKind::CloseBracket) {
                    expression(parser);
                }
                parser.expect(SyntaxKind::CloseBracket);
                marker.complete(parser, SyntaxKind::IndexExpression)
            }
```

5. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_members.php`:

```php
<?php
$user->name;
$user?->profile?->avatar;
$object->list();
$object->$dynamic;
$object->{'computed' . $name};
Foo::CONSTANT;
Foo::class;
Foo::$staticProperty;
Foo::create($x);
static::instance();
\Fully\Qualified::method();
$matrix[0][1];
$map['key']->value[2]();
$queue[] = 'item';
$a->b[0]() + $c;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse member access, scoped access, and indexing"
```

---

### Task 9: Arrays, array elements, and `list()`

`[...]` and `array(...)` as one `ArrayExpression` (the delimiter tokens tell the forms apart), elements with keys (`=>`), by-reference values (`&`), spread (`...`), trailing commas, and the destructuring shapes: empty slots (`[, $b]`) and `list(...)`. Destructuring targets are just arrays on the left of `=`; the parser does not distinguish.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `ArrayExpression`, `ArrayElement`, `ListExpression`)
- Test: `crates/celerrate_syntax/tests/expressions_literals.rs` (new), corpus file `tests/parse_corpus/expressions_arrays.php`

**Interfaces:**
- Consumes: `expression`, `error_element`, the separator-recovery pattern from `argument_list`.
- Produces: `fn array_expression(parser) -> CompletedMarker`, `fn list_expression(parser) -> CompletedMarker`, and `fn array_element_list(parser, closing: SyntaxKind)` (shared by both, and the pattern model for later delimited lists). `starts_expression` grows: `OpenBracket`, `Array`, `List`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/expressions_literals.rs`:

```rust
//! Arrays, list destructuring, and string interpolation nodes.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxDiagnosticKind, SyntaxKind};

fn parser_diagnostics(source: &str) -> Vec<ParserDiagnosticKind> {
    support::parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}

#[test]
fn short_arrays_hold_keyed_and_positional_elements() {
    insta::assert_snapshot!(support::render_expression("[1, 'k' => 2]"), @r#"
    ArrayExpression
      OpenBracket "["
      ArrayElement
        Literal
          IntegerLiteral "1"
      Comma ","
      ArrayElement
        Literal
          SingleQuotedString "'k'"
        FatArrow "=>"
        Literal
          IntegerLiteral "2"
      CloseBracket "]"
    "#);
}

#[test]
fn the_long_array_form_shares_the_node_kind() {
    insta::assert_snapshot!(support::render_expression("array(1)"), @r#"
    ArrayExpression
      Array "array"
      OpenParenthesis "("
      ArrayElement
        Literal
          IntegerLiteral "1"
      CloseParenthesis ")"
    "#);
}

#[test]
fn spread_and_by_reference_elements_parse() {
    insta::assert_snapshot!(support::render_expression("[...$xs, &$y]"), @r#"
    ArrayExpression
      OpenBracket "["
      ArrayElement
        Ellipsis "..."
        VariableReference
          Variable "$xs"
      Comma ","
      ArrayElement
        Ampersand "&"
        VariableReference
          Variable "$y"
      CloseBracket "]"
    "#);
}

#[test]
fn destructuring_shapes_parse() {
    assert!(parser_diagnostics("<?php [$a, [$b, $c]] = $nested;").is_empty());
    assert!(parser_diagnostics("<?php [, $second] = $pair;").is_empty());
    assert!(parser_diagnostics("<?php ['k' => $v] = $map;").is_empty());
    assert!(parser_diagnostics("<?php [1 => &$byReference] = $source;").is_empty());
}

#[test]
fn list_destructuring_parses() {
    insta::assert_snapshot!(support::render_expression("list($a, $b) = $pair"), @r#"
    AssignmentExpression
      ListExpression
        List "list"
        OpenParenthesis "("
        ArrayElement
          VariableReference
            Variable "$a"
        Comma ","
        ArrayElement
          VariableReference
            Variable "$b"
        CloseParenthesis ")"
      Equals "="
      VariableReference
        Variable "$pair"
    "#);
}

#[test]
fn trailing_commas_and_nested_arrays_parse() {
    assert!(parser_diagnostics("<?php [[1, 2], [3, 4],];").is_empty());
}

#[test]
fn an_array_literal_indexes_directly() {
    // `[` after a primary is indexing; at expression start it is an array.
    assert!(parser_diagnostics("<?php [1, 2][0];").is_empty());
}

#[test]
fn an_unclosed_array_stops_at_the_statement_boundary() {
    let diagnostics = parser_diagnostics("<?php [1, 2; $x;");
    assert!(diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBracket)));
    let parse = support::parse_verified("<?php [1, 2; $x;");
    assert!(
        parse
            .tree()
            .children()
            .filter(|node| node.kind() == SyntaxKind::ExpressionStatement)
            .count()
            >= 2
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_literals`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `[ elements ]` or `array( elements )`; also the destructuring
    /// target shape. Empty destructuring slots keep their commas as
    /// direct children.
    ArrayExpression,
    /// One element: optional `...`, optional `&`, expression, then
    /// optionally `=>` (optional `&`) expression.
    ArrayElement,
    /// `list( elements )`, the keyword destructuring form.
    ListExpression,
```

2. `expressions.rs`:

```rust
fn array_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    if parser.eat(SyntaxKind::Array) {
        if parser.at(SyntaxKind::OpenParenthesis) {
            parser.bump();
            array_element_list(parser, SyntaxKind::CloseParenthesis);
            parser.expect(SyntaxKind::CloseParenthesis);
        } else {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
        }
    } else {
        parser.bump(); // `[`
        array_element_list(parser, SyntaxKind::CloseBracket);
        parser.expect(SyntaxKind::CloseBracket);
    }
    marker.complete(parser, SyntaxKind::ArrayExpression)
}

fn list_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `list`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        array_element_list(parser, SyntaxKind::CloseParenthesis);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, SyntaxKind::ListExpression)
}

/// Elements until `closing`. Same recovery contract as the argument
/// list: unexpected tokens are wrapped and consumed; `;`, `?>`, and
/// end of input abort.
fn array_element_list(parser: &mut Parser, closing: SyntaxKind) {
    while !parser.at(closing) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        if parser.at(SyntaxKind::Comma) {
            // An empty destructuring slot: the comma stands alone.
            parser.bump();
            continue;
        }
        if !starts_array_element(parser) {
            error_element(parser);
            continue;
        }
        array_element(parser);
        expect_list_separator(parser, closing);
    }
}

fn starts_array_element(parser: &Parser) -> bool {
    parser.current().is_some_and(|kind| {
        starts_expression(kind) || kind == SyntaxKind::Ellipsis || kind == SyntaxKind::Ampersand
    })
}

fn array_element(parser: &mut Parser) {
    let marker = parser.start();
    parser.eat(SyntaxKind::Ellipsis);
    parser.eat(SyntaxKind::Ampersand);
    expression(parser);
    if parser.eat(SyntaxKind::FatArrow) {
        parser.eat(SyntaxKind::Ampersand);
        expression(parser);
    }
    marker.complete(parser, SyntaxKind::ArrayElement);
}
```

3. `primary_expression` gains the arms:

```rust
        Some(SyntaxKind::OpenBracket | SyntaxKind::Array) => Some(array_expression(parser)),
        Some(SyntaxKind::List) => Some(list_expression(parser)),
```

4. `starts_expression` grows: `OpenBracket`, `Array`, `List`.

5. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_arrays.php`:

```php
<?php
[];
[1, 2, 3,];
['one' => 1, 'two' => 2];
array('legacy' => true);
[...$defaults, 'override' => 1];
[&$first, 1 => &$second];
[[1, 2], [3, 4]][0][1];
[$a, [, $c]] = $nested;
list($x, list($y)) = $pairs;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse array and list expressions"
```

---

### Task 10: String interpolation nodes

The lexer already delivers interpolated strings as structured tokens (delimiters, fragments, variables, operators, braces); the parser builds nodes over them: `InterpolatedString` (`"..."`), `HeredocExpression` (heredocs and nowdocs), `ShellExecExpression` (backticks), with three interpolation part kinds. Zend's "simple" interpolation grammar (one property hop or one offset) is narrower than the expression grammar, so it gets its own rule rather than reusing `member_name`.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add the six kinds)
- Test: `crates/celerrate_syntax/tests/expressions_literals.rs`, corpus file `tests/parse_corpus/expressions_strings.php`

**Interfaces:**
- Consumes: the lexer's string token shapes (see `tests/strings.rs` for the exact streams), `expression`, `error_element`.
- Produces: `fn interpolated_string(parser, closing: SyntaxKind, node_kind: SyntaxKind) -> CompletedMarker` shared by the three delimiter forms. `starts_expression` grows: `DoubleQuote`, `Backtick`, `HeredocStart`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions_literals.rs`:

```rust
#[test]
fn a_double_quoted_string_holds_fragments_and_interpolations() {
    insta::assert_snapshot!(support::render_expression(r#""a $name b""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      StringFragment "a "
      SimpleInterpolation
        Variable "$name"
      StringFragment " b"
      DoubleQuote "\""
    "#);
}

#[test]
fn simple_interpolation_takes_one_property_hop_or_one_offset() {
    insta::assert_snapshot!(support::render_expression(r#""$user->name""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      SimpleInterpolation
        Variable "$user"
        Arrow "->"
        Identifier "name"
      DoubleQuote "\""
    "#);
    insta::assert_snapshot!(support::render_expression(r#""$items[0] $list[-1]""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      SimpleInterpolation
        Variable "$items"
        OpenBracket "["
        IntegerLiteral "0"
        CloseBracket "]"
      StringFragment " "
      SimpleInterpolation
        Variable "$list"
        OpenBracket "["
        Minus "-"
        IntegerLiteral "1"
        CloseBracket "]"
      DoubleQuote "\""
    "#);
}

#[test]
fn brace_interpolation_holds_a_full_expression() {
    insta::assert_snapshot!(support::render_expression(r#""x {$a->b(1)} y""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      StringFragment "x "
      BraceInterpolation
        OpenBrace "{"
        CallExpression
          MemberAccessExpression
            VariableReference
              Variable "$a"
            Arrow "->"
            MemberName
              Identifier "b"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                IntegerLiteral "1"
            CloseParenthesis ")"
        CloseBrace "}"
      StringFragment " y"
      DoubleQuote "\""
    "#);
}

#[test]
fn the_deprecated_dollar_brace_form_still_parses() {
    insta::assert_snapshot!(support::render_expression(r#""${name}""#), @r#"
    InterpolatedString
      DoubleQuote "\""
      DollarBraceInterpolation
        DollarOpenBrace "${"
        NameExpression
          Name
            Identifier "name"
        CloseBrace "}"
      DoubleQuote "\""
    "#);
}

#[test]
fn backticks_are_shell_executions() {
    insta::assert_snapshot!(support::render_expression("`ls $dir`"), @r#"
    ShellExecExpression
      Backtick "`"
      StringFragment "ls "
      SimpleInterpolation
        Variable "$dir"
      Backtick "`"
    "#);
}

#[test]
fn heredocs_interpolate_and_nowdocs_do_not() {
    let heredoc = support::parse_verified("<?php <<<TXT\nHello $name\nTXT;");
    let interpolations = heredoc
        .tree()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::SimpleInterpolation)
        .count();
    assert_eq!(interpolations, 1);
    assert!(
        heredoc
            .tree()
            .descendants()
            .any(|node| node.kind() == SyntaxKind::HeredocExpression)
    );

    let nowdoc = support::parse_verified("<?php <<<'TXT'\nHello $name\nTXT;");
    let interpolation_nodes = nowdoc
        .tree()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::SimpleInterpolation)
        .count();
    assert_eq!(interpolation_nodes, 0);
}

#[test]
fn an_unterminated_string_adds_no_parser_diagnostic_of_its_own() {
    // The lexer already reported the unterminated string; the parser
    // contributes only the ordinary missing terminator, nothing about
    // the missing quote.
    assert_eq!(
        parser_diagnostics("<?php \"open"),
        vec![ParserDiagnosticKind::ExpectedSemicolon]
    );
}
```

(`descendants()` is rowan's standard node iterator on `SyntaxNode`; it is available through the crate's alias.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_literals`
Expected: FAIL to compile (missing kinds), then failing shapes (a `"` currently diagnoses `ExpectedExpression`).

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `"..."` with fragments and interpolations.
    InterpolatedString,
    /// A heredoc or nowdoc, start to end label.
    HeredocExpression,
    /// A backtick string: shell execution.
    ShellExecExpression,
    /// `$name`, `$name->property`, `$name[offset]` inside a string.
    SimpleInterpolation,
    /// `{ expression }` inside a string.
    BraceInterpolation,
    /// `${ ... }` inside a string, the deprecated form.
    DollarBraceInterpolation,
```

2. `expressions.rs`:

```rust
/// `"..."`, heredocs, and backticks share one loop: fragments stay
/// tokens, interpolations become nodes. The lexer already diagnosed an
/// unterminated string, so the missing closer is eaten silently here.
fn interpolated_string(
    parser: &mut Parser,
    closing: SyntaxKind,
    node_kind: SyntaxKind,
) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // the opening delimiter
    while !parser.at(closing) && !parser.at_end() {
        match parser.current() {
            Some(SyntaxKind::StringFragment) => parser.bump(),
            Some(SyntaxKind::Variable) => simple_interpolation(parser),
            Some(SyntaxKind::OpenBrace) => brace_interpolation(parser),
            Some(SyntaxKind::DollarOpenBrace) => dollar_brace_interpolation(parser),
            // The lexer does not produce anything else between string
            // delimiters; this arm is fuzz armor, not grammar.
            _ => error_element(parser),
        }
    }
    parser.eat(closing);
    marker.complete(parser, node_kind)
}

/// Zend's "simple" interpolation: the variable, then at most one
/// property hop or one offset. The lexer only emits the arrow or the
/// bracket when the full form is present; the diagnostics are
/// defensive, for fuzzed streams.
fn simple_interpolation(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // the variable
    if parser.at(SyntaxKind::Arrow) || parser.at(SyntaxKind::NullsafeArrow) {
        parser.bump();
        if !parser.eat(SyntaxKind::Identifier) {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedMemberName);
        }
    } else if parser.eat(SyntaxKind::OpenBracket) {
        parser.eat(SyntaxKind::Minus);
        if matches!(
            parser.current(),
            Some(SyntaxKind::IntegerLiteral | SyntaxKind::Identifier | SyntaxKind::Variable)
        ) {
            parser.bump();
        } else {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
        }
        parser.expect(SyntaxKind::CloseBracket);
    }
    marker.complete(parser, SyntaxKind::SimpleInterpolation);
}

fn brace_interpolation(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `{`
    expression(parser);
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::BraceInterpolation);
}

fn dollar_brace_interpolation(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `${`
    expression(parser);
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::DollarBraceInterpolation);
}
```

3. `primary_expression` gains the arms:

```rust
        Some(SyntaxKind::DoubleQuote) => Some(interpolated_string(
            parser,
            SyntaxKind::DoubleQuote,
            SyntaxKind::InterpolatedString,
        )),
        Some(SyntaxKind::Backtick) => Some(interpolated_string(
            parser,
            SyntaxKind::Backtick,
            SyntaxKind::ShellExecExpression,
        )),
        Some(SyntaxKind::HeredocStart) => Some(interpolated_string(
            parser,
            SyntaxKind::HeredocEnd,
            SyntaxKind::HeredocExpression,
        )),
```

4. `starts_expression` grows: `DoubleQuote`, `Backtick`, `HeredocStart`.

5. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_strings.php` (heredoc bodies must keep real newlines):

```php
<?php
"plain";
"a $name b";
"$user->name and $user?->name";
"$items[0] $map[key] $grid[$x] $list[-1]";
"x {$a->b(1)} y";
"{$f(['k' => 1])}";
"${legacy}";
`ls $directory`;
<<<TXT
Hello $name, total {$cart->total()}
TXT;
<<<'RAW'
No $interpolation here
RAW;
b"binary $x";
```

The exact fragment boundaries inside heredocs (where newlines attach) are the lexer's business and already pinned by `tests/heredoc.rs`; review the corpus snapshot against those shapes before accepting.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse string interpolation nodes"
```

---

### Task 11: `new` and `clone`

`new` is a primary (so the 8.4 chain `new Foo()->bar()` falls out of the postfix loop) with a deliberately narrow class-reference rule: names, `static`, variable forms with member and index chains (Zend's `new_variable`), or a parenthesized expression; calls are excluded so the constructor's argument list stays the `new`'s. `clone` has two forms: the classic prefix (level 28) and the 8.5 function form `clone(...)` that clone-with rides on; the parenthesis decides, and the function form is a primary so it chains.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `NewExpression`, `CloneExpression`)
- Test: `crates/celerrate_syntax/tests/expressions_keywords.rs` (new), corpus file `tests/parse_corpus/expressions_new_clone.php`

**Interfaces:**
- Consumes: `name`, `simple_variable`, `member_name`, `argument_list`, `parenthesized_expression`, `error_element`, `CLONE_LEVEL`.
- Produces: `fn new_expression(parser) -> CompletedMarker` (primary), the two `clone` arms. `starts_expression` grows: `New`, `Clone`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/expressions_keywords.rs`:

```rust
//! Keyword-headed expressions: new, clone, the intrinsics, the
//! low-precedence prefixes, and match.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxDiagnosticKind, SyntaxKind};

fn parser_diagnostics(source: &str) -> Vec<ParserDiagnosticKind> {
    support::parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}

#[test]
fn new_takes_a_name_and_arguments() {
    insta::assert_snapshot!(support::render_expression("new Foo(1)"), @r#"
    NewExpression
      New "new"
      Name
        Identifier "Foo"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
    "#);
}

#[test]
fn new_accepts_every_class_reference_form() {
    assert!(parser_diagnostics("<?php new \\Foo\\Bar();").is_empty());
    assert!(parser_diagnostics("<?php new static();").is_empty());
    assert!(parser_diagnostics("<?php new $class;").is_empty());
    assert!(parser_diagnostics("<?php new $factory->product();").is_empty());
    assert!(parser_diagnostics("<?php new ($resolver->pick())($x);").is_empty());
}

#[test]
fn member_access_chains_on_new_since_php_84() {
    insta::assert_snapshot!(support::render_expression("new Foo()->bar()"), @r#"
    CallExpression
      MemberAccessExpression
        NewExpression
          New "new"
          Name
            Identifier "Foo"
          ArgumentList
            OpenParenthesis "("
            CloseParenthesis ")"
        Arrow "->"
        MemberName
          Identifier "bar"
      ArgumentList
        OpenParenthesis "("
        CloseParenthesis ")"
    "#);
}

#[test]
fn an_anonymous_class_is_deferred_with_recovery() {
    // `new class {}` belongs to the declarations plan; until then the
    // tokens survive through recovery.
    assert!(parser_diagnostics("<?php new class;").contains(&ParserDiagnosticKind::UnexpectedToken));
}

#[test]
fn clone_keeps_its_prefix_form_and_precedence() {
    insta::assert_snapshot!(support::render_expression("clone $entity + 1"), @r#"
    BinaryExpression
      CloneExpression
        Clone "clone"
        VariableReference
          Variable "$entity"
      Plus "+"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn the_php_85_clone_function_form_parses_and_chains() {
    insta::assert_snapshot!(support::render_expression("clone($entity, ['id' => null])"), @r#"
    CloneExpression
      Clone "clone"
      ArgumentList
        OpenParenthesis "("
        Argument
          VariableReference
            Variable "$entity"
        Comma ","
        Argument
          ArrayExpression
            OpenBracket "["
            ArrayElement
              Literal
                SingleQuotedString "'id'"
              FatArrow "=>"
              NameExpression
                Name
                  Identifier "null"
            CloseBracket "]"
        CloseParenthesis ")"
    "#);
    assert!(parser_diagnostics("<?php clone($entity)->touch();").is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_keywords`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `new` with a class reference and optional constructor arguments.
    NewExpression,
    /// `clone value` or the 8.5 function form `clone(...)`.
    CloneExpression,
```

2. `expressions.rs`: add this task's level constant to the block, in table position:

```rust
const CLONE_LEVEL: u8 = 28;
```

3. `expressions.rs`:

```rust
/// `new` with a class reference and optional arguments. The class
/// reference is narrower than an expression: calls are excluded so
/// the argument list stays the `new`'s. Postfix chains wrap the
/// completed node afterwards, which is how 8.4's `new Foo()->bar()`
/// parses; version gating is semantic.
fn new_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `new`
    match parser.current() {
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash) => {
            name(parser);
        }
        Some(SyntaxKind::Namespace) if parser.nth(1) == Some(SyntaxKind::Backslash) => {
            name(parser);
        }
        Some(SyntaxKind::Static) => parser.bump(),
        Some(SyntaxKind::Variable | SyntaxKind::Dollar) => {
            new_class_reference_chain(parser);
        }
        Some(SyntaxKind::OpenParenthesis) => {
            parenthesized_expression(parser);
        }
        // `new class { ... }` (anonymous classes) belongs to the
        // declarations plan; recovery keeps the tokens until then.
        Some(SyntaxKind::Class) => error_element(parser),
        _ => parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression),
    }
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    }
    marker.complete(parser, SyntaxKind::NewExpression)
}

/// The variable form of a class reference: member, scoped, and index
/// wraps, calls excluded (Zend's new_variable). Deliberately repeats
/// three postfix arms; folding them together would thread an
/// allow-calls flag through the hot loop for one caller.
fn new_class_reference_chain(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = simple_variable(parser)?;
    loop {
        left = match parser.current() {
            Some(SyntaxKind::Arrow | SyntaxKind::NullsafeArrow) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::MemberAccessExpression)
            }
            Some(SyntaxKind::ColonColon) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::ScopedAccessExpression)
            }
            Some(SyntaxKind::OpenBracket) => {
                let marker = left.precede(parser);
                parser.bump();
                if !parser.at(SyntaxKind::CloseBracket) {
                    expression(parser);
                }
                parser.expect(SyntaxKind::CloseBracket);
                marker.complete(parser, SyntaxKind::IndexExpression)
            }
            _ => break,
        };
    }
    Some(left)
}
```

4. `primary_expression` gains:

```rust
        Some(SyntaxKind::New) => Some(new_expression(parser)),
        // The 8.5 function form; a primary, so postfix chains wrap it.
        Some(SyntaxKind::Clone) if parser.nth(1) == Some(SyntaxKind::OpenParenthesis) => {
            let marker = parser.start();
            parser.bump();
            argument_list(parser);
            Some(marker.complete(parser, SyntaxKind::CloneExpression))
        }
```

5. `prefix_expression` gains the classic form, as an early arm of its match (before the tuple arms; it returns directly):

```rust
        SyntaxKind::Clone if parser.nth(1) != Some(SyntaxKind::OpenParenthesis) => {
            let marker = parser.start();
            parser.bump();
            expression_with_minimum_power(parser, left_binding_power(CLONE_LEVEL));
            return Some(marker.complete(parser, SyntaxKind::CloneExpression));
        }
```

6. `starts_expression` grows: `New`, `Clone`.

7. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_new_clone.php`:

```php
<?php
new Foo;
new Foo(1, 2);
new \Fully\Qualified($x);
new static;
new $class;
new $factory->product(1);
new ($resolver->pick())($x);
new Foo()->bar()->baz;
clone $entity;
clone $entity->child;
(clone $prototype)->mutate();
clone($entity, ['id' => null]);
clone($entity)->touch();
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse new and clone expressions"
```

---

### Task 12: The parenthesized intrinsics: `isset`, `empty`, `eval`, `exit`

Keyword-headed primaries that reuse `argument_list` wholesale: `isset(...)` and `empty(...)` and `eval(...)` require their parentheses (missing ones are diagnosed), `exit`/`die` take them optionally (8.4 made `exit` a function; bare `exit` stays valid). Arity and argument validity are semantic; the permissive list gives recovery for free.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `IssetExpression`, `EmptyExpression`, `EvalExpression`, `ExitExpression`)
- Test: `crates/celerrate_syntax/tests/expressions_keywords.rs`, corpus file `tests/parse_corpus/expressions_intrinsics.php`

**Interfaces:**
- Consumes: `argument_list`.
- Produces: `fn keyword_call(parser, node_kind) -> CompletedMarker` (keyword + mandatory argument list). `starts_expression` grows: `Isset`, `Empty`, `Eval`, `Exit`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions_keywords.rs`:

```rust
#[test]
fn isset_takes_a_variable_list() {
    insta::assert_snapshot!(support::render_expression("isset($a, $b->c)"), @r#"
    IssetExpression
      Isset "isset"
      ArgumentList
        OpenParenthesis "("
        Argument
          VariableReference
            Variable "$a"
        Comma ","
        Argument
          MemberAccessExpression
            VariableReference
              Variable "$b"
            Arrow "->"
            MemberName
              Identifier "c"
        CloseParenthesis ")"
    "#);
}

#[test]
fn empty_and_eval_require_their_parentheses() {
    assert!(parser_diagnostics("<?php empty($x);").is_empty());
    assert!(parser_diagnostics("<?php eval($code);").is_empty());
    assert!(
        parser_diagnostics("<?php isset;")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis))
    );
}

#[test]
fn exit_and_die_take_optional_arguments() {
    insta::assert_snapshot!(support::render_expression("exit(1)"), @r#"
    ExitExpression
      Exit "exit"
      ArgumentList
        OpenParenthesis "("
        Argument
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
    "#);
    assert!(parser_diagnostics("<?php exit;").is_empty());
    assert!(parser_diagnostics("<?php die;").is_empty());
    assert!(parser_diagnostics("<?php $code = $failed ? exit(1) : 0;").is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_keywords`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `isset( arguments )`.
    IssetExpression,
    /// `empty( argument )`.
    EmptyExpression,
    /// `eval( argument )`.
    EvalExpression,
    /// `exit` / `die`, with an optional argument list since 8.4.
    ExitExpression,
```

2. `expressions.rs`:

```rust
/// A keyword followed by a mandatory argument list (`isset`, `empty`,
/// `eval`). Arity and argument validity are semantic; the shared list
/// brings its recovery along.
fn keyword_call(parser: &mut Parser, node_kind: SyntaxKind) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // the keyword
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, node_kind)
}
```

3. `primary_expression` gains:

```rust
        Some(SyntaxKind::Isset) => Some(keyword_call(parser, SyntaxKind::IssetExpression)),
        Some(SyntaxKind::Empty) => Some(keyword_call(parser, SyntaxKind::EmptyExpression)),
        Some(SyntaxKind::Eval) => Some(keyword_call(parser, SyntaxKind::EvalExpression)),
        Some(SyntaxKind::Exit) => {
            let marker = parser.start();
            parser.bump();
            if parser.at(SyntaxKind::OpenParenthesis) {
                argument_list(parser);
            }
            Some(marker.complete(parser, SyntaxKind::ExitExpression))
        }
```

4. `starts_expression` grows: `Isset`, `Empty`, `Eval`, `Exit`.

5. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_intrinsics.php`:

```php
<?php
isset($a);
isset($a, $b[0], $c->d);
empty($value);
eval('return 1;');
exit;
exit(0);
die('goodbye');
$status = $broken ? exit(1) : 'ok';
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse isset, empty, eval, and exit expressions"
```

---

### Task 13: The low-precedence keyword prefixes: `print`, `throw`, `yield`, `include`

The prefix operators below assignment: `print` (level 4), `yield` and `yield from` (5 and 6, with the optional operand and the `key => value` form), `throw` (7, an expression since 8.0, so `$x ?? throw new E()` works), and the four `include`/`require` keywords (8, one node kind, the keyword token distinguishes them).

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `PrintExpression`, `ThrowExpression`, `YieldExpression`, `IncludeExpression`)
- Test: `crates/celerrate_syntax/tests/expressions_keywords.rs`, corpus file `tests/parse_corpus/expressions_yield_throw.php`

**Interfaces:**
- Consumes: `prefix_expression`'s tuple dispatch (task 4).
- Produces: `fn yield_expression(parser) -> CompletedMarker`. `starts_expression` grows: `Print`, `Throw`, `Yield`, `YieldFrom`, `Include`, `IncludeOnce`, `Require`, `RequireOnce`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions_keywords.rs`:

```rust
#[test]
fn print_is_a_low_prefix_expression() {
    insta::assert_snapshot!(support::render_expression("print 'x' . 'y'"), @r#"
    PrintExpression
      Print "print"
      BinaryExpression
        Literal
          SingleQuotedString "'x'"
        Dot "."
        Literal
          SingleQuotedString "'y'"
    "#);
    assert!(parser_diagnostics("<?php $ok = print 'x';").is_empty());
}

#[test]
fn throw_works_as_a_coalesce_fallback() {
    insta::assert_snapshot!(support::render_expression("$x ?? throw new Error('missing')"), @r#"
    BinaryExpression
      VariableReference
        Variable "$x"
      QuestionQuestion "??"
      ThrowExpression
        Throw "throw"
        NewExpression
          New "new"
          Name
            Identifier "Error"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                SingleQuotedString "'missing'"
            CloseParenthesis ")"
    "#);
}

#[test]
fn yield_covers_bare_value_and_keyed_forms() {
    assert!(parser_diagnostics("<?php yield;").is_empty());
    assert!(parser_diagnostics("<?php yield $value;").is_empty());
    insta::assert_snapshot!(support::render_expression("yield $key => $value"), @r#"
    YieldExpression
      Yield "yield"
      VariableReference
        Variable "$key"
      FatArrow "=>"
      VariableReference
        Variable "$value"
    "#);
}

#[test]
fn yield_from_delegates_a_whole_generator() {
    insta::assert_snapshot!(support::render_expression("yield from $generator"), @r#"
    YieldExpression
      YieldFrom "yield from"
      VariableReference
        Variable "$generator"
    "#);
}

#[test]
fn yield_binds_tighter_than_the_word_operators() {
    insta::assert_snapshot!(support::render_expression("yield $a and $b"), @r#"
    BinaryExpression
      YieldExpression
        Yield "yield"
        VariableReference
          Variable "$a"
      And "and"
      VariableReference
        Variable "$b"
    "#);
}

#[test]
fn include_swallows_its_whole_operand() {
    insta::assert_snapshot!(support::render_expression("include $path . '.php'"), @r#"
    IncludeExpression
      Include "include"
      BinaryExpression
        VariableReference
          Variable "$path"
        Dot "."
        Literal
          SingleQuotedString "'.php'"
    "#);
    assert!(parser_diagnostics("<?php require_once __DIR__ . '/bootstrap.php';").is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_keywords`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `print operand`.
    PrintExpression,
    /// `throw operand`, an expression since PHP 8.0.
    ThrowExpression,
    /// `yield`, `yield value`, `yield key => value`, `yield from source`.
    YieldExpression,
    /// `include`, `include_once`, `require`, `require_once`; the
    /// keyword token distinguishes them.
    IncludeExpression,
```

2. `expressions.rs`: add this task's level constants to the block, in table position:

```rust
const PRINT_LEVEL: u8 = 4;
const YIELD_LEVEL: u8 = 5;
const YIELD_FROM_LEVEL: u8 = 6;
const THROW_LEVEL: u8 = 7;
const INCLUDE_LEVEL: u8 = 8;
```

3. `expressions.rs`, extend `prefix_expression`'s tuple match with the straightforward prefixes:

```rust
        SyntaxKind::Print => (
            SyntaxKind::PrintExpression,
            left_binding_power(PRINT_LEVEL),
        ),
        SyntaxKind::Throw => (
            SyntaxKind::ThrowExpression,
            left_binding_power(THROW_LEVEL),
        ),
        SyntaxKind::Include
        | SyntaxKind::IncludeOnce
        | SyntaxKind::Require
        | SyntaxKind::RequireOnce => (
            SyntaxKind::IncludeExpression,
            left_binding_power(INCLUDE_LEVEL),
        ),
        SyntaxKind::YieldFrom => (
            SyntaxKind::YieldExpression,
            left_binding_power(YIELD_FROM_LEVEL),
        ),
```

and the `yield` arm as an early return (its operand is optional, the tuple shape does not fit):

```rust
        SyntaxKind::Yield => return Some(yield_expression(parser)),
```

4. `expressions.rs`, the yield rule:

```rust
/// `yield`, `yield value`, `yield key => value`. The operand is
/// optional: a bare `yield` is a complete expression.
fn yield_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `yield`
    if parser.current().is_some_and(starts_expression) {
        expression_with_minimum_power(parser, left_binding_power(YIELD_LEVEL));
        if parser.eat(SyntaxKind::FatArrow) {
            expression_with_minimum_power(parser, left_binding_power(YIELD_LEVEL));
        }
    }
    marker.complete(parser, SyntaxKind::YieldExpression)
}
```

5. `starts_expression` grows: `Print`, `Throw`, `Yield`, `YieldFrom`, `Include`, `IncludeOnce`, `Require`, `RequireOnce`.

6. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_yield_throw.php`:

```php
<?php
print 'hello';
$ok = print 'logged';
$value = $cache ?? throw new RuntimeException('cold');
yield;
yield $item;
yield $key => $item;
yield from $inner;
$sum = 1 + yield;
include $path;
include_once $path . '.php';
require $bootstrap;
require_once __DIR__ . '/vendor/autoload.php';
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse print, throw, yield, and include expressions"
```

---

### Task 14: `match` expressions

`match (subject) { conditions => body, ... }`: arms comma-separated, conditions comma-separated full expressions ended by `=>`, `default` standing alone, trailing commas everywhere, and the same delimited-list recovery contract as arguments and arrays.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `MatchExpression`, `MatchArm`)
- Test: `crates/celerrate_syntax/tests/expressions_keywords.rs`, corpus file `tests/parse_corpus/expressions_match.php`

**Interfaces:**
- Consumes: `expression`, `error_element`, `starts_expression`.
- Produces: `fn match_expression(parser) -> CompletedMarker` (a primary: postfix chains apply, `match(...)  {...}->x` parses). `starts_expression` grows: `Match`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/expressions_keywords.rs`:

```rust
#[test]
fn match_holds_arms_with_condition_lists_and_default() {
    insta::assert_snapshot!(
        support::render_expression("match ($status) { 1, 2 => 'low', default => 'other' }"),
        @r#"
    MatchExpression
      Match "match"
      OpenParenthesis "("
      VariableReference
        Variable "$status"
      CloseParenthesis ")"
      OpenBrace "{"
      MatchArm
        Literal
          IntegerLiteral "1"
        Comma ","
        Literal
          IntegerLiteral "2"
        FatArrow "=>"
        Literal
          SingleQuotedString "'low'"
      Comma ","
      MatchArm
        Default "default"
        FatArrow "=>"
        Literal
          SingleQuotedString "'other'"
      CloseBrace "}"
    "#);
}

#[test]
fn match_accepts_empty_bodies_and_trailing_commas() {
    assert!(parser_diagnostics("<?php match ($x) {};").is_empty());
    assert!(parser_diagnostics("<?php match ($x) { 1 => 'a', };").is_empty());
    assert!(parser_diagnostics("<?php match ($x) { 1, => 'a' };").is_empty());
}

#[test]
fn match_conditions_are_full_expressions() {
    assert!(parser_diagnostics("<?php match (true) { $age >= 18 => 'adult', default => 'minor' };").is_empty());
    assert!(parser_diagnostics("<?php $r = match ($x) { f($x) => g($x) };").is_empty());
}

#[test]
fn a_match_arm_missing_its_arrow_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php match ($x) { 1 'a' };")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::FatArrow))
    );
}

#[test]
fn garbage_inside_match_is_wrapped_and_the_tree_survives() {
    let parse = support::parse_verified("<?php match ($x) { => 'a', 2 => 'b' };");
    assert!(!parse.diagnostics().is_empty());
    assert!(
        parse
            .tree()
            .descendants()
            .any(|node| node.kind() == SyntaxKind::MatchExpression)
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_keywords`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `match ( subject ) { arms }`.
    MatchExpression,
    /// One arm: a condition list (or `default`), `=>`, the body.
    MatchArm,
```

2. `expressions.rs`:

```rust
/// `match (subject) { arms }`. Same recovery contract as the other
/// delimited lists: unexpected tokens are wrapped and consumed; `;`,
/// `?>`, and end of input abort.
fn match_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `match`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        expression(parser);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.at(SyntaxKind::OpenBrace) {
        parser.bump();
        while !parser.at(SyntaxKind::CloseBrace) && !parser.at_end() {
            if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
                break;
            }
            if parser.at(SyntaxKind::Default) || parser.current().is_some_and(starts_expression) {
                match_arm(parser);
            } else {
                error_element(parser);
                continue;
            }
            expect_list_separator(parser, SyntaxKind::CloseBrace);
        }
        parser.expect(SyntaxKind::CloseBrace);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::MatchExpression)
}

fn match_arm(parser: &mut Parser) {
    let marker = parser.start();
    if !parser.eat(SyntaxKind::Default) {
        // Conditions: comma-separated full expressions, up to the `=>`.
        // Any comma before the arrow separates conditions; arm-level
        // commas only occur after the body.
        expression(parser);
        while parser.at(SyntaxKind::Comma) {
            parser.bump();
            if parser.at(SyntaxKind::FatArrow) {
                // A trailing comma in the condition list.
                break;
            }
            expression(parser);
        }
    }
    parser.expect(SyntaxKind::FatArrow);
    expression(parser);
    marker.complete(parser, SyntaxKind::MatchArm);
}
```

3. `primary_expression` gains:

```rust
        Some(SyntaxKind::Match) => Some(match_expression(parser)),
```

4. `starts_expression` grows: `Match`.

5. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_match.php`:

```php
<?php
match ($status) {
    200, 204 => 'success',
    301, 302 => 'redirect',
    default => 'other',
};
$label = match (true) {
    $age >= 65 => 'senior',
    $age >= 18 => 'adult',
    default => 'minor',
};
match ($x) {};
$nested = match ($outer) {
    1 => match ($inner) {
        default => 'deep',
    },
    default => 'shallow',
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse match expressions"
```

---

### Task 15: Closures and arrow functions

`function (...) use (...) {...}` and `fn (...) => ...`, both with an optional leading `static`, by-reference returns (`&`), parameters (optional minimal type, by-reference, variadic, defaults), an optional return type, and for closures a `use` clause and a `Block` of statements. Blocks reuse the statement dispatch, so closures can nest anything the grammar knows, inline HTML included. `type_reference` parses exactly one optionally-nullable named type; plan 4 replaces it with the full union/intersection/DNF grammar.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs` (extract `statement_list_step`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (add `ClosureExpression`, `ArrowFunctionExpression`, `ParameterList`, `Parameter`, `TypeReference`, `ClosureUseClause`, `Block`)
- Test: `crates/celerrate_syntax/tests/expressions_closures.rs` (new), corpus file `tests/parse_corpus/expressions_closures.php`

**Interfaces:**
- Consumes: the statement rules from `grammar.rs`, `name`, `expression`, `error_element`.
- Produces:
  - `grammar.rs`: `pub(super) fn statement_list_step(parser)` (one iteration of a statement list: inline HTML and tag tokens bumped, everything else a statement) — the `source_file` loop and `block` both call it; plan 3's compound statements will too
  - `expressions.rs`: `closure_or_arrow_function`, `parameter_list`, `parameter`, `type_reference`, `closure_use_clause`, `block`
  - `starts_expression` grows: `Function`, `Fn` (`Static` is already in since task 6)
  - the task 6 `Static` primary arm gains a guard arm above it dispatching to closures when `nth(1)` is `Function` or `Fn`

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/expressions_closures.rs`:

```rust
//! Closures and arrow functions.
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::{ParserDiagnosticKind, SyntaxDiagnosticKind, SyntaxKind};

fn parser_diagnostics(source: &str) -> Vec<ParserDiagnosticKind> {
    support::parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}

#[test]
fn a_closure_holds_parameters_use_clause_and_block() {
    insta::assert_snapshot!(
        support::render_expression("function ($a, $b = 1) use (&$total) { echo $a; }"),
        @r#"
    ClosureExpression
      Function "function"
      ParameterList
        OpenParenthesis "("
        Parameter
          Variable "$a"
        Comma ","
        Parameter
          Variable "$b"
          Equals "="
          Literal
            IntegerLiteral "1"
        CloseParenthesis ")"
      ClosureUseClause
        Use "use"
        OpenParenthesis "("
        Ampersand "&"
        VariableReference
          Variable "$total"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        EchoStatement
          Echo "echo"
          VariableReference
            Variable "$a"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn typed_variadic_and_by_reference_parameters_parse() {
    assert!(parser_diagnostics("<?php function (int $x, ?\\Foo\\Bar $y, callable ...$rest) {};").is_empty());
    assert!(parser_diagnostics("<?php function (&$byReference) {};").is_empty());
    assert!(parser_diagnostics("<?php function &($x) { return $x; };").is_empty());
}

#[test]
fn return_types_parse_on_both_forms() {
    assert!(parser_diagnostics("<?php function (): int {};").is_empty());
    insta::assert_snapshot!(support::render_expression("static fn (): int => 1"), @r#"
    ArrowFunctionExpression
      Static "static"
      Fn "fn"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      TypeReference
        Identifier "int"
      FatArrow "=>"
      Literal
        IntegerLiteral "1"
    "#);
}

#[test]
fn an_arrow_function_body_extends_as_far_as_possible() {
    insta::assert_snapshot!(support::render_expression("fn ($x) => $x * 2"), @r#"
    ArrowFunctionExpression
      Fn "fn"
      ParameterList
        OpenParenthesis "("
        Parameter
          Variable "$x"
        CloseParenthesis ")"
      FatArrow "=>"
      BinaryExpression
        VariableReference
          Variable "$x"
        Star "*"
        Literal
          IntegerLiteral "2"
    "#);
}

#[test]
fn arrow_functions_nest_and_stop_at_argument_commas() {
    assert!(parser_diagnostics("<?php $add = fn ($x) => fn ($y) => $x + $y;").is_empty());
    assert!(parser_diagnostics("<?php usort($list, fn ($a, $b) => $a <=> $b);").is_empty());
}

#[test]
fn static_closures_and_immediate_calls_parse() {
    assert!(parser_diagnostics("<?php static function () {};").is_empty());
    assert!(parser_diagnostics("<?php (function () { return 1; })();").is_empty());
    // `static` alone keeps its scoped-access meaning.
    assert!(parser_diagnostics("<?php static::helper();").is_empty());
}

#[test]
fn closures_nest_statements_and_inline_html() {
    assert!(parser_diagnostics("<?php function () { echo 1; ?>raw<?php echo 2; };").is_empty());
}

#[test]
fn a_parameter_missing_its_variable_is_diagnosed() {
    assert!(
        parser_diagnostics("<?php function (int) {};")
            .contains(&ParserDiagnosticKind::Expected(SyntaxKind::Variable))
    );
}

#[test]
fn an_arrow_function_missing_its_body_is_diagnosed() {
    assert!(parser_diagnostics("<?php fn () => ;").contains(&ParserDiagnosticKind::ExpectedExpression));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_syntax --test expressions_closures`
Expected: FAIL to compile (missing kinds), then failing shapes.

- [ ] **Step 3: Implement**

1. `syntax_kind.rs`, append:

```rust
    /// `function (...) use (...) { ... }`, optionally `static`, with an
    /// optional by-reference `&` and return type.
    ClosureExpression,
    /// `fn (...) => expression`, optionally `static`.
    ArrowFunctionExpression,
    /// `( parameter, ... )`.
    ParameterList,
    /// One parameter: optional type, `&`, `...`, the variable, and an
    /// optional default.
    Parameter,
    /// One optionally-nullable named type. The declarations plan
    /// replaces this with the full union/intersection/DNF grammar.
    TypeReference,
    /// `use ( variables )` on a closure.
    ClosureUseClause,
    /// `{ statements }`.
    Block,
```

2. `grammar.rs`: extract the shared list step and rewire `source_file`:

```rust
pub(super) fn source_file(parser: &mut Parser) {
    let marker = parser.start();
    while parser.current().is_some() {
        statement_list_step(parser);
    }
    marker.complete(parser, SyntaxKind::SourceFile);
}

/// One step of a statement list: inline HTML and tags stay tokens;
/// everything else is a statement. Shared by the source file and every
/// brace-delimited body (closure blocks now, compound statements in
/// the statements plan).
pub(super) fn statement_list_step(parser: &mut Parser) {
    match parser.current() {
        Some(
            SyntaxKind::InlineHtml
            | SyntaxKind::OpenTag
            | SyntaxKind::OpenTagEcho
            | SyntaxKind::ShortOpenTag
            | SyntaxKind::CloseTag,
        ) => parser.bump(),
        _ => statement(parser),
    }
}
```

3. `expressions.rs`:

```rust
/// `function`/`fn` at expression position, optionally preceded by
/// `static`; the caller checked the shape.
fn closure_or_arrow_function(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.eat(SyntaxKind::Static);
    if parser.at(SyntaxKind::Function) {
        parser.bump();
        parser.eat(SyntaxKind::Ampersand); // by-reference return
        parameter_list(parser);
        if parser.at(SyntaxKind::Use) {
            closure_use_clause(parser);
        }
        if parser.eat(SyntaxKind::Colon) {
            type_reference(parser);
        }
        block(parser);
        marker.complete(parser, SyntaxKind::ClosureExpression)
    } else {
        parser.expect(SyntaxKind::Fn);
        parser.eat(SyntaxKind::Ampersand); // by-reference return
        parameter_list(parser);
        if parser.eat(SyntaxKind::Colon) {
            type_reference(parser);
        }
        parser.expect(SyntaxKind::FatArrow);
        expression(parser);
        marker.complete(parser, SyntaxKind::ArrowFunctionExpression)
    }
}

fn parameter_list(parser: &mut Parser) {
    let marker = parser.start();
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        while !parser.at(SyntaxKind::CloseParenthesis) && !parser.at_end() {
            if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
                break;
            }
            if !starts_parameter(parser) {
                error_element(parser);
                continue;
            }
            parameter(parser);
            expect_list_separator(parser, SyntaxKind::CloseParenthesis);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, SyntaxKind::ParameterList);
}

fn starts_parameter(parser: &Parser) -> bool {
    matches!(
        parser.current(),
        Some(
            SyntaxKind::Variable
                | SyntaxKind::Ampersand
                | SyntaxKind::Ellipsis
                | SyntaxKind::Question
                | SyntaxKind::Identifier
                | SyntaxKind::Backslash
                | SyntaxKind::Namespace
                | SyntaxKind::Array
                | SyntaxKind::Callable
                | SyntaxKind::Static
        )
    )
}

fn parameter(parser: &mut Parser) {
    let marker = parser.start();
    if !matches!(
        parser.current(),
        Some(SyntaxKind::Variable | SyntaxKind::Ampersand | SyntaxKind::Ellipsis)
    ) {
        type_reference(parser);
    }
    parser.eat(SyntaxKind::Ampersand);
    parser.eat(SyntaxKind::Ellipsis);
    parser.expect(SyntaxKind::Variable);
    if parser.eat(SyntaxKind::Equals) {
        expression(parser);
    }
    marker.complete(parser, SyntaxKind::Parameter);
}

/// One optionally-nullable named type (`int`, `?\Foo\Bar`, `callable`,
/// `array`, `static`). Union, intersection, and DNF forms arrive with
/// the declarations plan, which replaces this rule.
fn type_reference(parser: &mut Parser) {
    let marker = parser.start();
    parser.eat(SyntaxKind::Question);
    match parser.current() {
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace) => {
            name(parser);
        }
        Some(kind) if kind.is_keyword() => parser.bump(),
        _ => parser.diagnose_current(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
    marker.complete(parser, SyntaxKind::TypeReference);
}

fn closure_use_clause(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `use`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        while !parser.at(SyntaxKind::CloseParenthesis) && !parser.at_end() {
            if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
                break;
            }
            if parser.at(SyntaxKind::Ampersand) || parser.at(SyntaxKind::Variable) {
                parser.eat(SyntaxKind::Ampersand);
                if parser.at(SyntaxKind::Variable) {
                    let variable = parser.start();
                    parser.bump();
                    variable.complete(parser, SyntaxKind::VariableReference);
                } else {
                    parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
                }
            } else {
                error_element(parser);
                continue;
            }
            expect_list_separator(parser, SyntaxKind::CloseParenthesis);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, SyntaxKind::ClosureUseClause);
}

fn block(parser: &mut Parser) {
    let marker = parser.start();
    if parser.at(SyntaxKind::OpenBrace) {
        parser.bump();
        while !parser.at(SyntaxKind::CloseBrace) && !parser.at_end() {
            super::statement_list_step(parser);
        }
        parser.expect(SyntaxKind::CloseBrace);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::Block);
}
```

4. `primary_expression`: add the closure arms, and put the guarded `static` arm **above** the plain one from task 6:

```rust
        Some(SyntaxKind::Function | SyntaxKind::Fn) => {
            Some(closure_or_arrow_function(parser))
        }
        Some(SyntaxKind::Static)
            if matches!(parser.nth(1), Some(SyntaxKind::Function | SyntaxKind::Fn)) =>
        {
            Some(closure_or_arrow_function(parser))
        }
```

5. `starts_expression` grows: `Function`, `Fn`.

6. Corpus file `crates/celerrate_syntax/tests/parse_corpus/expressions_closures.php`:

```php
<?php
$closure = function ($a, $b = 1) use (&$total, $rate) {
    echo $a;
};
$typed = function (int $x, ?\Foo\Bar $y = null, callable ...$rest): ?int {
    return $x;
};
$byReference = function &(&$target) {
    return $target;
};
$immediate = (function () { return 'now'; })();
$arrow = fn ($x) => $x * 2;
$curried = static fn (int $x): callable => fn (int $y): int => $x + $y;
usort($items, fn ($a, $b) => $a->weight <=> $b->weight);
$mixed = function () { ?>chunk<?php echo 'back'; };
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_syntax`
Expected: PASS, new corpus snapshot reviewed and accepted.

- [ ] **Step 5: Gate, then commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -A
git commit -m "✨ feat(syntax): parse closures and arrow functions"
```

---

### Task 16: Error corpus, kitchen sink, fuzz seeds, and the closing gate

Resilience tested as a feature: an error corpus of deliberately broken expressions snapshotting the partial trees, a kitchen-sink corpus file combining every grammar area of this plan, fuzz seeds refreshed from the corpus, a fuzz smoke run, and the CHANGELOG entry.

**Files:**
- Create: `crates/celerrate_syntax/tests/parse_corpus/recovery_expressions.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/expressions_kitchen_sink.php`
- Modify: `fuzz/corpus/parse/` (seed copies)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: everything above.
- Produces: the closed plan; the parser handles the full expression grammar with tested recovery.

- [ ] **Step 1: Add the error corpus**

Create `crates/celerrate_syntax/tests/parse_corpus/recovery_expressions.php`. Every construct here is broken on purpose; the snapshot must show partial nodes and diagnostics, never dropped text (the lossless assertion in the corpus test enforces that mechanically):

```php
<?php
f(1, 2;
[1, 'k' =>;
$object->;
Foo::;
match ($x) { 1 'a' };
new;
new class;
$a ? 1;
$a instanceof;
function (int) {};
fn () => ;
clone;
yield from;
"unterminated {$x
```

The last line leaves an interpolation and the string open at end of file: lexer diagnostics and parser recovery must both appear, and the file must still round-trip.

- [ ] **Step 2: Add the kitchen sink**

Create `crates/celerrate_syntax/tests/parse_corpus/expressions_kitchen_sink.php`:

```php
<?php
$result = [1, 2, 3]
    |> fn (array $list) => array_map(fn ($item) => $item ** 2, $list)
    |> array_filter(...);
$total = match (true) {
    $result === [] => throw new RuntimeException('empty'),
    default => array_sum($result),
};
echo "Total: {$total} for $user->name", PHP_EOL;
$copy = clone($order, ['id' => null, 'lines' => [...$order->lines]]);
$counter = static function (?int $seed = null) use (&$state): int {
    $state ??= $seed ?? 0;
    return $state++;
};
$label = $count > 1 ? "$count items" : ($count === 1 ? 'one item' : 'empty');
[$first, [, $third]] = $matrix[0];
$dispatcher?->events[static::class][] = fn () => yield from $queue;
```

Run `cargo test -p celerrate_syntax --test parse_corpus`, review both snapshots line by line (recovery shapes are the deliverable here), then accept.

- [ ] **Step 3: Refresh the fuzz seeds and smoke-run the fuzzer**

```bash
cp crates/celerrate_syntax/tests/parse_corpus/*.php fuzz/corpus/parse/
cargo +nightly fuzz run parse -- -max_total_time=60
```

(Match the invocation to whatever `.github/workflows/ci.yml` already uses for the `parse` target; the point is a local smoke run over the new seeds.)
Expected: no crash, no timeout. Any finding is a bug in this plan's code: minimize, fix at the responsible task's site, and add the minimized case to the error corpus before continuing.

- [ ] **Step 4: Update the CHANGELOG**

Add under the unreleased section of `CHANGELOG.md`, following the file's existing format:

```markdown
- The parser covers the full PHP 8.5 expression grammar: the Zend
  precedence table, calls and access chains, arrays, string
  interpolation, `new`/`clone` (clone-with), intrinsics, `match`,
  closures and arrow functions, and the pipe operator.
```

- [ ] **Step 5: The closing gate**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Expected: all green. Also confirm none of task 2's temporary `#[allow(dead_code)]` annotations remain in `parser.rs`: every helper (`nth`, `eat`, `expect`, `enter_nesting`, `leave_nesting`, `precede`) has production callers by now.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "✅ test(syntax): extend the parse corpus and fuzz seeds for expressions"
```

---

## Plan Completion Criteria

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo deny check` all pass.
- Every construct of spec section 3's "Expressions" and "Types" (minimal form) areas parses; the corpus files demonstrate each.
- The error corpus shows partial trees with diagnostics for every recovery path added in this plan.
- The fuzz target runs clean on the refreshed seed corpus.
- Plans 3 (statements) and 4 (declarations) inherit: `statement_list_step`, `argument_list`, `type_reference` (to be replaced), `parameter_list`, `member_name`, `name`, and the deferrals recorded in "Deliberate Permissiveness".
