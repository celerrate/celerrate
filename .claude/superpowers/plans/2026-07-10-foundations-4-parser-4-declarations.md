# Foundations Part 4, Plan 4: Declarations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse the full PHP declaration grammar through 8.5: classes (anonymous included), interfaces, traits, enums, their members (properties, methods, constants, trait use with adaptations, enum cases), property hooks and asymmetric visibility (8.4), constructor promotion, the complete type grammar (nullable, union, intersection, DNF), attributes, and `const` / `namespace` / `use` declarations — with tested error recovery throughout.

**Architecture:** Three new grammar modules beside `statements.rs` and `expressions.rs`: `types.rs` (the full type grammar, replacing plan 2's provisional `type_reference`), `declarations.rs` (everything `class`-shaped plus `const`, `namespace`, and `use` imports), and `attributes.rs` (`#[...]` groups). Declaration rules take an already-open `Marker`, so attribute groups can open the node before the declaration dispatch runs — the one structural decision that makes the attributes task an insertion instead of a rewrite. The class member list implements the design's contextual recovery set (modifiers, `function`, `const`, `use`, `case`, attributes, `}`), backed by the same mechanical position guard the expression lists use. Design: `.claude/superpowers/specs/2026-07-10-foundations-4-parser-design.md` (section 3 "Declarations", "Types", "Attributes"; section 4). Productions are transcribed from php-src `zend_language_parser.y`, branch PHP-8.5 (`class_declaration_statement`, `class_statement_list`, `attributed_class_statement`, `property_hook_list`, `enum_declaration_statement`, `type_expr`, `union_type`, `intersection_type`, `attribute_group`, `use_declaration`, `group_use_declaration`, `trait_adaptation`).

**Tech Stack:** Rust 1.94 (edition 2024), `rowan` 0.16, `insta` (snapshots), `cargo-fuzz` (libFuzzer).

## Global Constraints

Copied from the parent spec and `CLAUDE.md`; every task's requirements include them.

- Zero panic, mechanically enforced: workspace denies `clippy::unwrap_used`, `clippy::expect_used`, `clippy::indexing_slicing`, `clippy::panic`; `unsafe_code` is forbidden. Production code returns totals (`Option`, fallbacks); test modules may locally `#[allow]` these lints (see existing test files for the idiom). `debug_assert!` is permitted (compiled out in release).
- TDD: every step of behavior starts from a failing test. No production code without a test that demanded it.
- Layering: `celerrate_syntax` depends only on `celerrate_source` (plus external `rowan`, `text-size`). No bare `rowan` type in any public signature.
- The lossless invariant: `parse(source).tree().text() == source` for every input, including degenerate input.
- Guaranteed progress: every parser loop consumes a token or terminates; every new loop states its termination argument in a comment when it is not obvious.
- The parser performs no version or semantic judgment and never fails: worst case is `ErrorNode` wreckage plus diagnostics. `readonly` on an interface, `abstract` on an enum, a hook in PHP 7 code: all of it parses; validity is semantic.
- Everything in English, full words, no abbreviated names (standard acronyms fine). No em-dashes. Comments state constraints the code cannot show, never narration.
- Commits: gitmoji + Conventional Commits (`✨ feat(syntax): ...`), repository-configured identity, no AI attribution of any kind.
- Before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` must all pass.

## Deferred items from plan 3 absorbed here

Recorded in PR #7:

1. Extract a shared `alternative_body` helper: the colon-form idiom (`statement_list`, `expect(End...)`, `terminate_statement`) repeats across `while`, `for`, `foreach`, and `declare` (Task 1).
2. Pin test for the recorded `for ($i = 0,;;)` trailing-comma permissiveness (Task 1).
3. `qualified_type_name` / `name` unification: the duplicated qualified-name walk disappears when the full type grammar replaces `type_reference` (Task 2).

Explicitly NOT here: the typed-AST note that a classic `declare` body of `?>` leaves a bare `CloseTag` token with no statement-node child (plan 5, recorded); the `$a ?? $b = $c` tree-shape note (semantics layer); `__halt_compiler` (out of scope for Foundations; recorded).

## Recorded permissiveness decisions (parse clean, judged upstairs)

- Every modifier sequence parses: order, repetition, combination (`final abstract`, `readonly` on a method, `static` on a constant, visibility on an interface member). Zend's modifier rules are semantic diagnostics.
- Keyword names for class-likes (`class List {}`) parse; reservation is semantic. Exception: `extends` and `implements` after the keyword read as heritage clauses with a missing-name diagnostic, because taking them as names would destroy the clause structure.
- Type compositions Zend rejects (`?A|B`, `(A|B)&C`, nested parenthesized types, `?` inside DNF) parse into the natural tree; composition legality is semantic. Keyword types beyond `array` / `callable` / `static` also parse (`function f(): match` is a `NamedType` over the keyword token).
- `enum(...)` and `readonly(...)` parse as calls: Zend keeps both words usable as function names for backward compatibility. `enum` declares only when directly followed by a name.
- `case` members parse in any class-like body; hooks parse on any property (interfaces, promoted parameters included); asymmetric visibility parses wherever a visibility parses. Placement is semantic.
- `private (set)` with interior whitespace parses as asymmetric visibility: Zend lexes `private(set)` as one token and forbids the space, but the parser's trivia-free view cannot see adjacency — the same trade already recorded on `name`.
- Attributed closures parse at expression position (`$f = #[A] function () {};` works through the expression statement). A `#[...]` prefix in front of a non-declaration statement (`#[A] echo 1;`) is wreckage: the groups keep their structure inside an `ErrorNode` and the statement parses on its own. Zend rejects that shape too.
- Trait adaptations parse permissively: a bare member name aliased (`hello as h;`), visibility-only aliases (`hello as protected;`), keyword member names.
- Top-level `use` accepts every import shape anywhere; nesting rules (`use` inside a block, group depth) are semantic.
- `?>` or inline HTML inside a member list is wreckage (`error_element`), as in Zend.

## File Structure

```
crates/celerrate_syntax/src/parser/grammar/types.rs         create (Task 2): the type grammar
crates/celerrate_syntax/src/parser/grammar/declarations.rs  create (Task 3), grows through Task 10
crates/celerrate_syntax/src/parser/grammar/attributes.rs    create (Task 10): attribute groups
crates/celerrate_syntax/src/parser/grammar.rs               modify: module declarations, Marker re-import
crates/celerrate_syntax/src/parser/grammar/statements.rs    modify: alternative_body (Task 1), declaration
                                                            dispatch arms, function rules move out,
                                                            pub(super) terminate_statement
crates/celerrate_syntax/src/parser/grammar/expressions.rs   modify: type call sites (Task 2), anonymous
                                                            classes (5), enum/readonly calls (5, 8),
                                                            parameter promotion + hooks (9), attributes
                                                            on closures and parameters (10)
crates/celerrate_syntax/src/syntax_kind.rs                  modify: TypeReference removed (Task 2);
                                                            30 declaration node kinds appended
crates/celerrate_syntax/src/diagnostic.rs                   modify: ExpectedType (2), ExpectedDeclaration (3)
crates/celerrate_syntax/tests/support/mod.rs                modify: render_type (2), render_member (6)
crates/celerrate_syntax/tests/statements_control_flow.rs    modify: for trailing-comma pin (Task 1)
crates/celerrate_syntax/tests/types.rs                      create (Task 2)
crates/celerrate_syntax/tests/declarations_top_level.rs     create (Tasks 3-4): const, namespace, use
crates/celerrate_syntax/tests/declarations_class_like.rs    create (Task 5)
crates/celerrate_syntax/tests/declarations_members.rs       create (Tasks 6-7)
crates/celerrate_syntax/tests/declarations_enums.rs         create (Task 8)
crates/celerrate_syntax/tests/declarations_hooks.rs         create (Task 9)
crates/celerrate_syntax/tests/declarations_attributes.rs    create (Task 10)
crates/celerrate_syntax/tests/parse_corpus/declarations_types.php          create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/declarations_top_level.php      create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/declarations_class_like.php     create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/declarations_members.php        create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/declarations_enums.php          create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/declarations_hooks.php          create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/declarations_attributes.php     create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/declarations_kitchen_sink.php   create (Task 11)
crates/celerrate_syntax/tests/parse_corpus/recovery_declarations.php       create (Task 11)
fuzz/corpus/parse/seed_declarations.php                     create (Task 11)
fuzz/corpus/parse/seed_declarations_errors.php              create (Task 11)
CHANGELOG.md                                                modify (Task 11)
```

Notes that apply to every task:

- **Inline snapshots** use `insta::assert_snapshot!(..., @r#"..."#)`. The expected trees below are authoritative for *shape and tokens*. If `cargo insta` reports an indentation-only mismatch, run `cargo insta accept` and verify the accepted snapshot matches the shape shown here node-for-node.
- **Corpus snapshots**: whenever a task changes how existing corpus files parse (a token that used to be `ErrorNode` wreckage becomes a real declaration node, or a `TypeReference` becomes a `NamedType`), `cargo test` fails on `parse_corpus`. Inspect the diff: only wreckage-to-real-node and type-node-reshaping improvements are acceptable; then `cargo insta accept`.
- **Test file preamble**: every new integration test file starts with

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_statement};
```

  (adjust the `support::` import list per file to what its tests use; `cargo clippy -- -D warnings` rejects unused imports).
- **Marker access**: `Marker` is private to the `parser` module and reachable from grammar submodules through ancestor-scope resolution. `declarations.rs` imports it as `use super::{Marker, Parser};` after Task 3 adds `Marker` to `grammar.rs`'s `use super::{...}` list.
- **Progress arguments**: every new loop either bumps a token on every iteration by construction, or wraps itself in the position-guard idiom (`let position_before = parser.position(); ...; if parser.position() == position_before { error_element(parser); }`) with a comment referencing `argument_list`'s rationale.

---

### Task 1: Plan-3 follow-ups: the shared alternative body, the `for` trailing-comma pin

Two deferred items from PR #7, both confined to `statements.rs` and its tests. Pure refactor plus one pin test; zero behavior change.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs`
- Modify: `crates/celerrate_syntax/tests/statements_control_flow.rs`

**Interfaces:**
- Consumes: `statement_list`, `terminate_statement`, `Parser::expect` — all already in `statements.rs`.
- Produces: `alternative_body(parser: &mut Parser, closing: SyntaxKind)` (private to `statements.rs`). No other task depends on it.

- [ ] **Step 1: Add the pin test**

In `tests/statements_control_flow.rs`, append:

```rust
#[test]
fn a_trailing_comma_in_a_for_section_parses_without_diagnostic() {
    // Recorded plan-3 permissiveness: Zend rejects `for ($i = 0,;;)`,
    // this parser accepts it clean. The pin keeps the divergence a
    // decision instead of an accident.
    assert_eq!(
        parser_diagnostics("<?php for ($i = 0,; $i < 3; $i++) {}"),
        vec![]
    );
}
```

- [ ] **Step 2: Run it — it passes already (a pin, not a change)**

Run: `cargo test --package celerrate_syntax --test statements_control_flow a_trailing_comma_in_a_for_section_parses_without_diagnostic`
Expected: PASS (the behavior exists; the test pins it).

- [ ] **Step 3: Commit the pin**

```bash
git add crates/celerrate_syntax/tests/statements_control_flow.rs
git commit -m "✅ test(syntax): pin the for-section trailing-comma permissiveness"
```

- [ ] **Step 4: Extract `alternative_body`**

In `statements.rs`, add below `embedded_statement`:

```rust
/// The alternative-syntax body shared by `while`, `for`, `foreach`,
/// and `declare` once their `:` is consumed: the statement list, the
/// closing keyword, the statement terminator. `if` and `switch` place
/// clauses or case sections between the list and the closer, so they
/// keep their own sequences.
fn alternative_body(parser: &mut Parser, closing: SyntaxKind) {
    statement_list(parser);
    parser.expect(closing);
    terminate_statement(parser);
}
```

Then replace the four colon branches:

In `while_statement`:

```rust
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndWhile);
    } else {
        embedded_statement(parser);
    }
```

In `for_statement`:

```rust
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndFor);
    } else {
        embedded_statement(parser);
    }
```

In `foreach_statement`:

```rust
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndForeach);
    } else {
        embedded_statement(parser);
    }
```

In `declare_statement`:

```rust
    if parser.eat(SyntaxKind::Colon) {
        alternative_body(parser, SyntaxKind::EndDeclare);
    } else {
        embedded_statement(parser);
    }
```

`if_statement` and `switch_statement` stay as they are.

- [ ] **Step 5: Run the full suite — behavior must be unchanged**

Run: `cargo test --workspace`
Expected: PASS, zero snapshot diffs (pure refactor).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_syntax/src/parser/grammar/statements.rs
git commit -m "♻️ refactor(syntax): extract the shared alternative-syntax body"
```

---

### Task 2: The full type grammar

Replaces plan 2's provisional `type_reference` (one optionally-nullable name) with the real grammar: `NamedType`, `NullableType`, `UnionType`, `IntersectionType`, `ParenthesizedType` (the DNF grouping). Deletes `qualified_type_name` — `NamedType` reuses `name()`, which closes the deferred unification. The one genuinely subtle point: in a parameter list, `&` after a type is the by-reference marker (`function f(A&$x)`), so the intersection loop only takes an ampersand whose next token can start a type.

**Files:**
- Create: `crates/celerrate_syntax/src/parser/grammar/types.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs` (declare `mod types;`)
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (delete `type_reference` and `qualified_type_name`; new call sites; `pub(super)` on `expect_list_separator`; `starts_parameter` gains `OpenParenthesis`)
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs` (`function_declaration` call site and import)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (remove `TypeReference`; append the five type node kinds)
- Modify: `crates/celerrate_syntax/src/diagnostic.rs` (`ExpectedType`)
- Modify: `crates/celerrate_syntax/tests/support/mod.rs` (`render_type`)
- Create: `crates/celerrate_syntax/tests/types.rs`

**Interfaces:**
- Consumes: `expressions::name` (`pub(super)`, exists), `Parser::{enter_nesting, leave_nesting, current, nth, at, eat, bump, expect, start, diagnose_current}`.
- Produces: `types::type_expression(parser: &mut Parser) -> Option<CompletedMarker>` and `types::starts_type(kind: SyntaxKind) -> bool`, both `pub(super)`. Every later task that parses a type position (constants, properties, methods, enum backing, promotion) calls exactly these two.

- [ ] **Step 1: Add the `render_type` helper**

In `tests/support/mod.rs`, append:

```rust
/// Renders the return type of `<?php function fixture(): {type_source} {}`
/// as an indented tree, offsets and trivia omitted: the workhorse
/// assertion of the type grammar tests.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_type(type_source: &str) -> String {
    let source = format!("<?php function fixture(): {type_source} {{}}");
    let parse = parse_verified(&source);
    let function = parse
        .tree()
        .children()
        .next()
        .expect("a function declaration");
    let type_node = function
        .children()
        .find(|node| {
            !matches!(
                node.kind(),
                celerrate_syntax::SyntaxKind::ParameterList | celerrate_syntax::SyntaxKind::Block
            )
        })
        .expect("a return type node");
    let mut output = String::new();
    render_element_without_offsets(&mut output, type_node.into(), 0);
    output
}
```

- [ ] **Step 2: Write the failing tests**

Create `tests/types.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_statement, render_type};

#[test]
fn a_named_type_wraps_one_name() {
    insta::assert_snapshot!(render_type("int"), @r#"
    NamedType
      Name
        Identifier "int"
    "#);
}

#[test]
fn a_qualified_type_is_one_name_node() {
    insta::assert_snapshot!(render_type("\\App\\Value"), @r#"
    NamedType
      Name
        Backslash "\\"
        Identifier "App"
        Backslash "\\"
        Identifier "Value"
    "#);
}

#[test]
fn keyword_types_stay_bare_tokens() {
    insta::assert_snapshot!(render_type("static"), @r#"
    NamedType
      Static "static"
    "#);
}

#[test]
fn a_nullable_type_prefixes_its_question_mark() {
    insta::assert_snapshot!(render_type("?Foo"), @r#"
    NullableType
      Question "?"
      NamedType
        Name
          Identifier "Foo"
    "#);
}

#[test]
fn union_types_are_one_flat_node() {
    insta::assert_snapshot!(render_type("int|string|null"), @r#"
    UnionType
      NamedType
        Name
          Identifier "int"
      Pipe "|"
      NamedType
        Name
          Identifier "string"
      Pipe "|"
      NamedType
        Name
          Identifier "null"
    "#);
}

#[test]
fn intersections_bind_tighter_than_unions() {
    insta::assert_snapshot!(render_type("A&B|C"), @r#"
    UnionType
      IntersectionType
        NamedType
          Name
            Identifier "A"
        Ampersand "&"
        NamedType
          Name
            Identifier "B"
      Pipe "|"
      NamedType
        Name
          Identifier "C"
    "#);
}

#[test]
fn dnf_types_keep_their_parentheses_as_a_node() {
    insta::assert_snapshot!(render_type("(A&B)|C"), @r#"
    UnionType
      ParenthesizedType
        OpenParenthesis "("
        IntersectionType
          NamedType
            Name
              Identifier "A"
          Ampersand "&"
          NamedType
            Name
              Identifier "B"
        CloseParenthesis ")"
      Pipe "|"
      NamedType
        Name
          Identifier "C"
    "#);
}

#[test]
fn a_parameter_ampersand_before_a_variable_is_by_reference_not_intersection() {
    insta::assert_snapshot!(render_statement("function f(A&$x) {}"), @r#"
    FunctionDeclaration
      Function "function"
      Identifier "f"
      ParameterList
        OpenParenthesis "("
        Parameter
          NamedType
            Name
              Identifier "A"
          Ampersand "&"
          Variable "$x"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_parameter_ampersand_before_a_variadic_stays_the_parameter_marker() {
    assert_eq!(parser_diagnostics("<?php function f(A&...$rest) {}"), vec![]);
}

#[test]
fn a_dnf_parameter_type_parses() {
    assert_eq!(
        parser_diagnostics("<?php function f((Countable&ArrayAccess)|null $x) {}"),
        vec![]
    );
}

#[test]
fn a_missing_union_member_is_diagnosed() {
    assert_eq!(
        parser_diagnostics("<?php function f(): int| {}"),
        vec![ParserDiagnosticKind::ExpectedType]
    );
}

#[test]
fn pathological_nullable_nesting_stays_a_diagnostic() {
    // `??` lexes as one coalesce token, so the chain must be spaced to
    // reach the parser as repeated `?` tokens.
    let source = format!("<?php function f(): {}int {{}}", "? ".repeat(300));
    let diagnostics = parser_diagnostics(&source);
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::NestingTooDeep),
        "the nesting guard must refuse, got {diagnostics:?}"
    );
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test types`
Expected: FAIL — compile error first (`render_type` exists but `ExpectedType` does not, `NamedType` does not).

- [ ] **Step 4: Add the node kinds and the diagnostic**

In `syntax_kind.rs`: delete the `TypeReference` variant and its doc comment. Append after `FunctionDeclaration`:

```rust
    /// One named type: a qualified `Name`, or a keyword type token
    /// (`array`, `callable`, `static`) sitting bare.
    NamedType,
    /// `?type`.
    NullableType,
    /// `A|B|C`, one flat node for the whole chain.
    UnionType,
    /// `A&B&C`, one flat node for the whole chain.
    IntersectionType,
    /// `( type )` inside a type: the DNF grouping form.
    ParenthesizedType,
```

In `diagnostic.rs`, append to `ParserDiagnosticKind`:

```rust
    /// A type position holds no type (`function f(): {}`).
    ExpectedType,
```

- [ ] **Step 5: Write `types.rs`**

Create `crates/celerrate_syntax/src/parser/grammar/types.rs`:

```rust
//! The type grammar: nullable, union, intersection, and DNF forms,
//! transcribed from php-src's `zend_language_parser.y` (branch
//! PHP-8.5: `type_expr`, `union_type`, `intersection_type`). Which
//! compositions Zend accepts is a semantic judgment (`?A|B` and
//! `(A|B)&C` it rejects); the parser accepts every composition.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::expressions::name;
use super::{CompletedMarker, Parser};

/// Whether `kind` can start a type. `array`, `callable`, and `static`
/// are Zend's keyword types; `int`, `string`, `null`, `self`, and the
/// rest are plain identifiers.
pub(super) fn starts_type(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Question
            | SyntaxKind::OpenParenthesis
            | SyntaxKind::Identifier
            | SyntaxKind::Backslash
            | SyntaxKind::Namespace
            | SyntaxKind::Array
            | SyntaxKind::Callable
            | SyntaxKind::Static
    )
}

/// A full type expression: `?T`, `A|B`, `A&B`, `(A&B)|C`. Returns
/// `None` without consuming when no type can start here (diagnosed) or
/// when the nesting guard refuses; callers with loops position-guard
/// against the latter.
pub(super) fn type_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    if !parser.enter_nesting() {
        return None;
    }
    let result = union_type(parser);
    parser.leave_nesting();
    result
}

/// `A|B|C`, one flat node however long the chain. Terminates: every
/// iteration consumes its `|`.
fn union_type(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = intersection_type(parser)?;
    if parser.at(SyntaxKind::Pipe) {
        let marker = left.precede(parser);
        while parser.eat(SyntaxKind::Pipe) {
            // A missing member (`int|`) is diagnosed downstream; the
            // node still completes.
            intersection_type(parser);
        }
        left = marker.complete(parser, SyntaxKind::UnionType);
    }
    Some(left)
}

/// `A&B&C`, one flat node. Terminates: every iteration consumes its
/// `&`.
fn intersection_type(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = atomic_type(parser)?;
    if at_intersection_ampersand(parser) {
        let marker = left.precede(parser);
        while at_intersection_ampersand(parser) {
            parser.bump();
            atomic_type(parser);
        }
        left = marker.complete(parser, SyntaxKind::IntersectionType);
    }
    Some(left)
}

/// `&` continues an intersection only when a type can follow. In a
/// parameter list, the ampersand after a type is the by-reference
/// marker (`function f(A&$x)`, `function f(A&...$xs)`): it precedes a
/// variable or `...`, never a type start, so one token of lookahead
/// separates the readings.
fn at_intersection_ampersand(parser: &mut Parser) -> bool {
    parser.at(SyntaxKind::Ampersand) && parser.nth(1).is_some_and(starts_type)
}

fn atomic_type(parser: &mut Parser) -> Option<CompletedMarker> {
    match parser.current() {
        Some(SyntaxKind::Question) => {
            // `? ? T` recurses one level per token (`??` lexes as one
            // coalesce token, but spaced chains reach here), so the
            // nesting guard bounds the recursion like any other.
            if !parser.enter_nesting() {
                return None;
            }
            let marker = parser.start();
            parser.bump();
            atomic_type(parser);
            let completed = marker.complete(parser, SyntaxKind::NullableType);
            parser.leave_nesting();
            Some(completed)
        }
        Some(SyntaxKind::OpenParenthesis) => {
            if !parser.enter_nesting() {
                return None;
            }
            let marker = parser.start();
            parser.bump();
            type_expression(parser);
            parser.expect(SyntaxKind::CloseParenthesis);
            let completed = marker.complete(parser, SyntaxKind::ParenthesizedType);
            parser.leave_nesting();
            Some(completed)
        }
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace) => {
            let marker = parser.start();
            name(parser);
            Some(marker.complete(parser, SyntaxKind::NamedType))
        }
        Some(kind) if kind.is_keyword() => {
            // `array`, `callable`, `static` are Zend's keyword types;
            // any other keyword parses too and is judged upstairs.
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::NamedType))
        }
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedType);
            None
        }
    }
}
```

- [ ] **Step 6: Wire the call sites and delete the provisional rule**

In `grammar.rs`, add `mod types;` beside the other module declarations.

In `expressions.rs`:
- Delete `type_reference` and `qualified_type_name` entirely (their doc comments included).
- In `parameter`, replace `type_reference(parser);` with `super::types::type_expression(parser);`.
- In `closure_or_arrow_function`, replace both `type_reference(parser);` calls with `super::types::type_expression(parser);`.
- In `starts_parameter`, add `| SyntaxKind::OpenParenthesis` to the matched kinds (a DNF parameter type starts with `(`).
- Change `fn expect_list_separator` to `pub(super) fn expect_list_separator` (Task 4's group-use list reuses it).

In `statements.rs`:
- In the `use super::expressions::{...}` list, remove `type_reference`.
- In `function_declaration`, replace `type_reference(parser);` with `super::types::type_expression(parser);`.

- [ ] **Step 7: Run the type tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test types`
Expected: PASS.

Run: `cargo test --workspace`
Expected: every snapshot that contained a `TypeReference` fails — the closure tests, the statement-declaration tests, and corpus files with typed functions. Review each diff: the only acceptable change is `TypeReference` reshaping into `NamedType`/`NullableType` trees (same tokens, new node names). Then `cargo insta accept` and re-run to green.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse union, intersection, and DNF types"
```

---

### Task 3: The declarations module; `const` and `namespace` declarations

Creates `grammar/declarations.rs` with the plan's structural convention: every declaration rule takes an already-open `Marker`, and one `declaration` dispatcher owns the marker so Task 10 can slide attribute groups in front of every declaration with a single insertion. `function_declaration` moves here from `statements.rs` unchanged in behavior. Then the two smallest new declarations land: `const` (typed since 8.3, semi-reserved names) and `namespace` (both forms).

**Files:**
- Create: `crates/celerrate_syntax/src/parser/grammar/declarations.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs` (declare `mod declarations;`, re-import `Marker`)
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs` (dispatch arms; `function_declaration` and `at_function_declaration` move out; `pub(super)` on `terminate_statement`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (three node kinds)
- Modify: `crates/celerrate_syntax/src/diagnostic.rs` (`ExpectedDeclaration`)
- Create: `crates/celerrate_syntax/tests/declarations_top_level.rs`

**Interfaces:**
- Consumes: `statements::{block, terminate_statement}` (`terminate_statement` becomes `pub(super)`), `expressions::{expression, name, parameter_list}`, `types::type_expression`, `types::starts_type`, `Marker` (via `grammar.rs`'s `use super::{CompletedMarker, Marker, Parser};`).
- Produces: `declarations::declaration(parser: &mut Parser)` (`pub(super)`, the single entry the statement dispatcher calls for every declaration kind), `declarations::at_function_declaration(parser: &mut Parser) -> bool` (`pub(super)`), and the internal marker-taking rules (`function_declaration`, `constant_declaration`, `namespace_declaration`) later tasks extend. `constant_declaration(parser, marker)` is reused verbatim by Task 6 for class constants.

- [ ] **Step 1: Write the failing tests**

Create `tests/declarations_top_level.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn a_constant_declaration_lists_its_elements() {
    insta::assert_snapshot!(render_statement("const GREETING = 'hello', ANSWER = 42;"), @r#"
    ConstantDeclaration
      Const "const"
      ConstantElement
        Identifier "GREETING"
        Equals "="
        Literal
          SingleQuotedString "'hello'"
      Comma ","
      ConstantElement
        Identifier "ANSWER"
        Equals "="
        Literal
          IntegerLiteral "42"
      Semicolon ";"
    "#);
}

#[test]
fn a_typed_constant_takes_a_type_before_its_name() {
    insta::assert_snapshot!(render_statement("const int LIMIT = 10;"), @r#"
    ConstantDeclaration
      Const "const"
      NamedType
        Name
          Identifier "int"
      ConstantElement
        Identifier "LIMIT"
        Equals "="
        Literal
          IntegerLiteral "10"
      Semicolon ";"
    "#);
}

#[test]
fn a_semi_reserved_constant_name_parses_clean() {
    // `const FOR = 1;`: keyword names are accepted wholesale; which
    // positions allow them is semantic.
    assert_eq!(parser_diagnostics("<?php const FOR = 1;"), vec![]);
}

#[test]
fn a_missing_constant_value_is_diagnosed_and_the_statement_recovers() {
    assert_eq!(
        parser_diagnostics("<?php const A = ; echo 1;"),
        vec![ParserDiagnosticKind::ExpectedExpression]
    );
}

#[test]
fn a_namespace_declaration_takes_a_name_and_a_terminator() {
    insta::assert_snapshot!(render_statement("namespace App\\Domain;"), @r#"
    NamespaceDeclaration
      Namespace "namespace"
      Name
        Identifier "App"
        Backslash "\\"
        Identifier "Domain"
      Semicolon ";"
    "#);
}

#[test]
fn a_braced_namespace_wraps_its_statements_in_a_block() {
    insta::assert_snapshot!(render_statement("namespace App { echo 1; }"), @r#"
    NamespaceDeclaration
      Namespace "namespace"
      Name
        Identifier "App"
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
fn a_global_namespace_block_has_no_name() {
    assert_eq!(parser_diagnostics("<?php namespace { echo 1; }"), vec![]);
}

#[test]
fn namespace_backslash_stays_a_name_expression() {
    // `namespace\helper()` is the relative-name call, not a namespace
    // declaration; the dispatcher separates the two on one lookahead.
    insta::assert_snapshot!(render_statement("namespace\\helper();"), @r#"
    ExpressionStatement
      CallExpression
        NameExpression
          Name
            Namespace "namespace"
            Backslash "\\"
            Identifier "helper"
        ArgumentList
          OpenParenthesis "("
          CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn function_declarations_still_parse_after_the_move() {
    assert_eq!(
        parser_diagnostics("<?php function f(int $x): void { return; }"),
        vec![]
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_top_level`
Expected: FAIL — `ConstantDeclaration` does not exist; `const` and `namespace` currently parse as wreckage.

- [ ] **Step 3: Add the node kinds and the diagnostic**

In `syntax_kind.rs`, append after `ParenthesizedType`:

```rust
    /// `const FOO = 1, BAR = 2;`, optionally typed (8.3), at the top
    /// level or as a class member (with modifiers).
    ConstantDeclaration,
    /// One `name = value` element of a constant declaration.
    ConstantElement,
    /// `namespace A\B;` or `namespace A\B { ... }` or `namespace { ... }`.
    NamespaceDeclaration,
```

In `diagnostic.rs`, append to `ParserDiagnosticKind`:

```rust
    /// A position that requires a declaration holds none (attribute
    /// groups in front of a non-declaration, modifiers with nothing to
    /// modify).
    ExpectedDeclaration,
```

- [ ] **Step 4: Create `declarations.rs`**

```rust
//! The declaration grammar: `const`, `namespace`, `use` imports,
//! functions, and the class-likes with their members. Every rule takes
//! an already-open [`Marker`], and the [`declaration`] dispatcher owns
//! opening it, so attribute groups (a later task of this plan) can
//! open the node before the dispatch runs.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::expressions::{expression, name, parameter_list};
use super::statements::{block, terminate_statement};
use super::{Marker, Parser};

/// Dispatch for declaration statements. The statement dispatcher only
/// routes here on tokens it has already vetted (with lookahead where a
/// keyword is overloaded), so the fallback arm is defensive: it is
/// reachable only behind consumed tokens (attribute groups, from a
/// later task), never at a standstill.
pub(super) fn declaration(parser: &mut Parser) {
    let marker = parser.start();
    match parser.current() {
        Some(SyntaxKind::Function) if at_function_declaration(parser) => {
            function_declaration(parser, marker);
        }
        Some(SyntaxKind::Const) => constant_declaration(parser, marker),
        Some(SyntaxKind::Namespace) if parser.nth(1) != Some(SyntaxKind::Backslash) => {
            namespace_declaration(parser, marker);
        }
        Some(_) => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedDeclaration);
            marker.complete(parser, SyntaxKind::ErrorNode);
        }
        None => marker.abandon(parser),
    }
}

/// `function` declares only when a name follows, with an optional `&`
/// between: `function (` and `function &(` are closure expressions.
pub(super) fn at_function_declaration(parser: &mut Parser) -> bool {
    match parser.nth(1) {
        Some(SyntaxKind::Identifier) => true,
        Some(SyntaxKind::Ampersand) => parser.nth(2) == Some(SyntaxKind::Identifier),
        _ => false,
    }
}

fn function_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `function`
    parser.eat(SyntaxKind::Ampersand); // by-reference return
    parser.expect(SyntaxKind::Identifier);
    parameter_list(parser);
    if parser.eat(SyntaxKind::Colon) {
        super::types::type_expression(parser);
    }
    block(parser);
    marker.complete(parser, SyntaxKind::FunctionDeclaration);
}

/// `const FOO = 1, BAR = 2;`, optionally typed since 8.3
/// (`const int FOO = 1;`). The type is absent exactly when the next
/// token is a name directly followed by `=`. Constant names accept any
/// keyword (semi-reserved: `const FOR = 1;`); which names and types
/// are legal where is semantic. Also the class-constant rule: the
/// member path parses its modifiers into `marker` first.
///
/// Terminates: every iteration bumps a name or breaks.
pub(super) fn constant_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `const`
    let at_untyped_name = parser
        .current()
        .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword())
        && parser.nth(1) == Some(SyntaxKind::Equals);
    if !at_untyped_name && parser.current().is_some_and(super::types::starts_type) {
        super::types::type_expression(parser);
    }
    loop {
        let at_name = parser
            .current()
            .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword());
        if !at_name {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
            break;
        }
        let element = parser.start();
        parser.bump();
        parser.expect(SyntaxKind::Equals);
        expression(parser);
        element.complete(parser, SyntaxKind::ConstantElement);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::ConstantDeclaration);
}

/// `namespace A\B;`, `namespace A\B { ... }`, `namespace { ... }`.
/// Only dispatched when the next token is not `\` (`namespace\Foo` is
/// a name expression). Where namespaces may appear and nest is
/// semantic.
fn namespace_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `namespace`
    if parser.at(SyntaxKind::Identifier) {
        name(parser);
    }
    if parser.at(SyntaxKind::OpenBrace) {
        block(parser);
    } else {
        terminate_statement(parser);
    }
    marker.complete(parser, SyntaxKind::NamespaceDeclaration);
}
```

- [ ] **Step 5: Rewire `grammar.rs` and `statements.rs`**

In `grammar.rs`:
- Change the import line to `use super::{CompletedMarker, Marker, Parser};` (children resolve `super::Marker` through it).
- Add `mod declarations;` beside the other module declarations.

In `statements.rs`:
- Delete `function_declaration` and `at_function_declaration` (they moved).
- Change `fn terminate_statement` to `pub(super) fn terminate_statement`.
- In `dispatch_statement`, replace the `Function` arm and add the new declaration arms. All of them sit **before** the `starts_expression` arm (the `Namespace` guard matters: `namespace` is in `starts_expression`):

```rust
        Some(SyntaxKind::Function) if super::declarations::at_function_declaration(parser) => {
            super::declarations::declaration(parser)
        }
        Some(SyntaxKind::Const) => super::declarations::declaration(parser),
        Some(SyntaxKind::Namespace) if parser.nth(1) != Some(SyntaxKind::Backslash) => {
            super::declarations::declaration(parser)
        }
```

- [ ] **Step 6: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_top_level`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS. Corpus files where `const`/`namespace` used to be wreckage improve; verify only wreckage-to-real-node diffs, then `cargo insta accept`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse const and namespace declarations"
```

---

### Task 4: `use` import declarations

Simple imports, aliases, `function`/`const` import types, comma-separated clause lists, and the group form `use A\{B, function c as d};`. Statement-level `use` never collides with closure `use` (expression context) or trait `use` (member context, Task 7): the contexts are disjoint.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/declarations.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs` (one dispatch arm)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (three node kinds)
- Modify: `crates/celerrate_syntax/tests/declarations_top_level.rs`

**Interfaces:**
- Consumes: `expressions::{error_element, expect_list_separator, name}` (the latter made `pub(super)` in Task 2), `terminate_statement`.
- Produces: `use_declaration(parser, marker)` (private to `declarations.rs`, dispatched through `declaration`). Task 7's trait `use` is a different rule and does not reuse this one.

- [ ] **Step 1: Write the failing tests**

Append to `tests/declarations_top_level.rs`:

```rust
#[test]
fn a_use_import_with_an_alias() {
    insta::assert_snapshot!(render_statement("use App\\Service as Servicing;"), @r#"
    UseDeclaration
      Use "use"
      UseClause
        Name
          Identifier "App"
          Backslash "\\"
          Identifier "Service"
        As "as"
        Identifier "Servicing"
      Semicolon ";"
    "#);
}

#[test]
fn a_function_import_types_the_whole_clause_list() {
    insta::assert_snapshot!(render_statement("use function strlen, strrev;"), @r#"
    UseDeclaration
      Use "use"
      Function "function"
      UseClause
        Name
          Identifier "strlen"
      Comma ","
      UseClause
        Name
          Identifier "strrev"
      Semicolon ";"
    "#);
}

#[test]
fn a_group_import_nests_typed_and_aliased_items() {
    insta::assert_snapshot!(render_statement("use App\\{Service, function helper as aid, const LIMIT};"), @r#"
    UseDeclaration
      Use "use"
      UseClause
        Name
          Identifier "App"
        UseGroup
          Backslash "\\"
          OpenBrace "{"
          UseClause
            Name
              Identifier "Service"
          Comma ","
          UseClause
            Function "function"
            Name
              Identifier "helper"
            As "as"
            Identifier "aid"
          Comma ","
          UseClause
            Const "const"
            Name
              Identifier "LIMIT"
          CloseBrace "}"
      Semicolon ";"
    "#);
}

#[test]
fn a_trailing_comma_inside_a_group_parses_clean() {
    // Zend allows the trailing comma in group imports.
    assert_eq!(
        parser_diagnostics("<?php use App\\{Service,};"),
        vec![]
    );
}

#[test]
fn an_unclosed_group_recovers_at_the_semicolon() {
    assert_eq!(
        parser_diagnostics("<?php use App\\{Service; echo 1;"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace)]
    );
}

#[test]
fn a_use_without_a_name_is_diagnosed_and_recovers() {
    assert_eq!(
        parser_diagnostics("<?php use ; echo 1;"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Identifier)]
    );
}
```

(`SyntaxKind` is already imported by the file preamble.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_top_level`
Expected: FAIL — `UseDeclaration` does not exist; `use` parses as wreckage.

- [ ] **Step 3: Add the node kinds**

In `syntax_kind.rs`, append after `NamespaceDeclaration`:

```rust
    /// `use A\B;` and every import shape: aliases, `function`/`const`
    /// types, clause lists, group imports.
    UseDeclaration,
    /// One imported name: optional per-item `function`/`const` type
    /// (inside groups), the name, an optional group or alias.
    UseClause,
    /// `\{ items }` of a grouped import.
    UseGroup,
```

- [ ] **Step 4: Implement in `declarations.rs`**

Extend the import list: `use super::expressions::{error_element, expect_list_separator, expression, name, parameter_list};`.

Add the `Use` arm to `declaration`'s match, before the fallback:

```rust
        Some(SyntaxKind::Use) => use_declaration(parser, marker),
```

Append the rules:

```rust
/// `use A\B;`, `use A\B as C;`, `use function a\b;`,
/// `use const A\B;`, comma-separated clause lists, and the group form
/// `use A\{B, function c as d};`. Collisions and resolution are
/// semantic. Terminates: every iteration parses a clause that consumed
/// at least one name token, or breaks.
fn use_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `use`
    if matches!(
        parser.current(),
        Some(SyntaxKind::Function | SyntaxKind::Const)
    ) {
        parser.bump(); // the import type, for the whole clause list
    }
    loop {
        if !use_clause(parser) {
            break;
        }
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::UseDeclaration);
}

/// One top-level import clause: a name, then a group (`\{ ... }`) or
/// an optional alias. Returns false without consuming when no name can
/// start here.
fn use_clause(parser: &mut Parser) -> bool {
    if !matches!(
        parser.current(),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
    ) {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
        return false;
    }
    let marker = parser.start();
    name(parser);
    if parser.at(SyntaxKind::Backslash) && parser.nth(1) == Some(SyntaxKind::OpenBrace) {
        use_group(parser);
    } else {
        use_alias(parser);
    }
    marker.complete(parser, SyntaxKind::UseClause);
    true
}

/// `\{ B, function c as d, }`: the group of a grouped import. Same
/// recovery contract as the expression lists: unexpected tokens are
/// wrapped and consumed; `;`, `?>`, and end of input abort. The shared
/// separator helper tolerates the trailing comma Zend allows here.
/// Terminates: every iteration consumes through `use_group_item` (its
/// dispatch admits only kinds that item always bumps) or through
/// `error_element`.
fn use_group(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `\`
    parser.bump(); // `{`
    while !parser.at(SyntaxKind::CloseBrace) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        if !matches!(
            parser.current(),
            Some(
                SyntaxKind::Function
                    | SyntaxKind::Const
                    | SyntaxKind::Identifier
                    | SyntaxKind::Backslash
                    | SyntaxKind::Namespace
            )
        ) {
            error_element(parser);
            continue;
        }
        use_group_item(parser);
        expect_list_separator(parser, SyntaxKind::CloseBrace);
    }
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::UseGroup);
}

/// One item of a group: an optional per-item `function`/`const` type,
/// the name, an optional alias. Always consumes at least one token:
/// the group loop admitted only its leading kinds.
fn use_group_item(parser: &mut Parser) {
    let marker = parser.start();
    if matches!(
        parser.current(),
        Some(SyntaxKind::Function | SyntaxKind::Const)
    ) {
        parser.bump();
    }
    if matches!(
        parser.current(),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
    ) {
        name(parser);
        use_alias(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
    }
    marker.complete(parser, SyntaxKind::UseClause);
}

/// `as Alias`. Any keyword is accepted as the alias; validity is
/// semantic.
fn use_alias(parser: &mut Parser) {
    if !parser.eat(SyntaxKind::As) {
        return;
    }
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => parser.bump(),
        _ => parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
}
```

In `statements.rs`, add the dispatch arm beside the other declaration arms:

```rust
        Some(SyntaxKind::Use) => super::declarations::declaration(parser),
```

- [ ] **Step 5: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_top_level`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS (accept wreckage-to-real-node corpus improvements only).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse use import declarations"
```

---

### Task 5: Class, interface, and trait declarations; anonymous classes

The class-like shells: leading modifiers, the name, heritage clauses, and a member list whose recovery loop is the backbone every member task builds on. Anonymous classes replace plan 2's `new class` wreckage arm and reuse the same tail. `readonly` joins the dispatch with its Zend backward-compatibility exception: `readonly(...)` stays a call.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/declarations.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs` (dispatch arms)
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (`new` arm, `readonly(...)` primary, `starts_expression`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (six node kinds)
- Create: `crates/celerrate_syntax/tests/declarations_class_like.rs`

**Interfaces:**
- Consumes: `expressions::{argument_list, error_element, name}`, `types` (unused yet in this task's rules), the `declaration` dispatcher.
- Produces: `anonymous_class(parser: &mut Parser, marker: Marker)` (`pub(super)`, called from `expressions::new_expression`), and the internals every member task extends: `member_list`, `member`, `heritage_clauses`, `name_list`, `class_like_name`, `class_modifiers`. `name_list` is reused by Task 7's trait adaptations.

- [ ] **Step 1: Write the failing tests**

Create `tests/declarations_class_like.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_expression, render_statement};

#[test]
fn a_class_declaration_with_modifiers_and_heritage() {
    insta::assert_snapshot!(render_statement("abstract class A extends B implements C, D {}"), @r#"
    ClassDeclaration
      Abstract "abstract"
      Class "class"
      Identifier "A"
      ExtendsClause
        Extends "extends"
        Name
          Identifier "B"
      ImplementsClause
        Implements "implements"
        Name
          Identifier "C"
        Comma ","
        Name
          Identifier "D"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_readonly_class_parses() {
    assert_eq!(parser_diagnostics("<?php final readonly class Value {}"), vec![]);
}

#[test]
fn an_interface_extends_a_list() {
    insta::assert_snapshot!(render_statement("interface Shape extends HasArea, HasPerimeter {}"), @r#"
    InterfaceDeclaration
      Interface "interface"
      Identifier "Shape"
      ExtendsClause
        Extends "extends"
        Name
          Identifier "HasArea"
        Comma ","
        Name
          Identifier "HasPerimeter"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_trait_declaration_parses() {
    insta::assert_snapshot!(render_statement("trait Greets {}"), @r#"
    TraitDeclaration
      Trait "trait"
      Identifier "Greets"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn a_keyword_class_name_parses_and_is_judged_upstairs() {
    // Zend rejects `class List {}` (reserved word); structurally it is
    // one analyzable declaration, so it parses clean here.
    assert_eq!(parser_diagnostics("<?php class List {}"), vec![]);
}

#[test]
fn a_missing_class_name_before_extends_is_diagnosed_not_eaten() {
    // `extends` must stay the clause keyword: taking it as the name
    // would destroy the heritage structure.
    assert_eq!(
        parser_diagnostics("<?php class extends B {}"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Identifier)]
    );
}

#[test]
fn an_anonymous_class_parses_inside_new() {
    insta::assert_snapshot!(render_expression("new class(1) extends Base {}"), @r#"
    NewExpression
      New "new"
      ClassDeclaration
        Class "class"
        ArgumentList
          OpenParenthesis "("
          Argument
            Literal
              IntegerLiteral "1"
          CloseParenthesis ")"
        ExtendsClause
          Extends "extends"
          Name
            Identifier "Base"
        MemberList
          OpenBrace "{"
          CloseBrace "}"
    "#);
}

#[test]
fn a_readonly_anonymous_class_parses() {
    // `new readonly class {}` is 8.3; availability is semantic.
    assert_eq!(parser_diagnostics("<?php $o = new readonly class {};"), vec![]);
}

#[test]
fn readonly_stays_callable_as_a_function_name() {
    // Zend backward compatibility: `readonly` is not reserved as a
    // function name.
    insta::assert_snapshot!(render_statement("readonly($flag);"), @r#"
    ExpressionStatement
      CallExpression
        NameExpression
          Name
            Readonly "readonly"
        ArgumentList
          OpenParenthesis "("
          Argument
            VariableReference
              Variable "$flag"
          CloseParenthesis ")"
      Semicolon ";"
    "#);
}

#[test]
fn modifiers_without_a_class_become_wreckage_and_the_rest_recovers() {
    assert_eq!(
        parser_diagnostics("<?php abstract 1;"),
        vec![ParserDiagnosticKind::Expected(SyntaxKind::Class)]
    );
}

#[test]
fn junk_inside_a_member_list_is_swept_and_the_list_recovers() {
    assert_eq!(
        parser_diagnostics("<?php class A { 1 + 2; } echo 3;"),
        vec![
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::UnexpectedToken,
        ]
    );
}

#[test]
fn an_unclosed_member_list_is_diagnosed() {
    let diagnostics = parser_diagnostics("<?php class A {");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::CloseBrace)),
        "got {diagnostics:?}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_class_like`
Expected: FAIL — the node kinds do not exist.

- [ ] **Step 3: Add the node kinds**

In `syntax_kind.rs`, append after `UseGroup`:

```rust
    /// `class Name extends B implements C, D { members }`, with
    /// optional `abstract` / `final` / `readonly` modifiers. Anonymous
    /// classes (`new class(...) { ... }`) share this kind and simply
    /// have no name; their constructor arguments sit before the
    /// heritage clauses.
    ClassDeclaration,
    /// `interface Name extends A, B { members }`.
    InterfaceDeclaration,
    /// `trait Name { members }`.
    TraitDeclaration,
    /// `extends` and its comma-separated names.
    ExtendsClause,
    /// `implements` and its comma-separated names.
    ImplementsClause,
    /// `{ members }` of a class-like body.
    MemberList,
```

- [ ] **Step 4: Implement the class-like rules in `declarations.rs`**

Extend the imports: `use super::expressions::{argument_list, error_element, expect_list_separator, expression, name, parameter_list};`.

Add the dispatch arms to `declaration`'s match, before the fallback:

```rust
        Some(
            SyntaxKind::Class | SyntaxKind::Abstract | SyntaxKind::Final | SyntaxKind::Readonly,
        ) => class_declaration(parser, marker),
        Some(SyntaxKind::Interface) => interface_declaration(parser, marker),
        Some(SyntaxKind::Trait) => trait_declaration(parser, marker),
```

Append the rules:

```rust
/// `abstract`, `final`, `readonly` before `class`, in any order and
/// multiplicity; validity is semantic.
fn class_modifiers(parser: &mut Parser) {
    while matches!(
        parser.current(),
        Some(SyntaxKind::Abstract | SyntaxKind::Final | SyntaxKind::Readonly)
    ) {
        parser.bump();
    }
}

fn class_declaration(parser: &mut Parser, marker: Marker) {
    class_modifiers(parser);
    if !parser.eat(SyntaxKind::Class) {
        // Modifiers with no `class` behind them (`abstract 1;`): the
        // modifiers become wreckage and the rest parses on its own.
        // Progress holds: this path is only reachable behind at least
        // one consumed token (a modifier, or attribute groups).
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Class));
        marker.complete(parser, SyntaxKind::ErrorNode);
        return;
    }
    class_like_name(parser);
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::ClassDeclaration);
}

fn interface_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `interface`
    class_like_name(parser);
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::InterfaceDeclaration);
}

fn trait_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `trait`
    class_like_name(parser);
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::TraitDeclaration);
}

/// The declaration half of `new class(...) extends ... { ... }`: no
/// name, optional constructor arguments between the keyword and the
/// heritage clauses. Which modifiers an anonymous class allows
/// (`readonly` since 8.3) is semantic.
pub(super) fn anonymous_class(parser: &mut Parser, marker: Marker) {
    class_modifiers(parser);
    parser.expect(SyntaxKind::Class);
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    }
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::ClassDeclaration);
}

/// The declared name. Zend rejects keywords here; every keyword parses
/// as the name anyway (`class List {}` stays one analyzable
/// declaration) and reservation is judged upstairs — except `extends`
/// and `implements`, which stay heritage clauses so a missing name
/// cannot swallow them.
fn class_like_name(parser: &mut Parser) {
    match parser.current() {
        Some(kind)
            if (kind == SyntaxKind::Identifier || kind.is_keyword())
                && !matches!(kind, SyntaxKind::Extends | SyntaxKind::Implements) =>
        {
            parser.bump();
        }
        _ => parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
}

/// `extends ...` then `implements ...`, each optional. One shared rule
/// for every class-like: an interface with `implements` or a trait
/// with either clause parses, and the misplacement is semantic.
fn heritage_clauses(parser: &mut Parser) {
    if parser.at(SyntaxKind::Extends) {
        let clause = parser.start();
        parser.bump();
        name_list(parser);
        clause.complete(parser, SyntaxKind::ExtendsClause);
    }
    if parser.at(SyntaxKind::Implements) {
        let clause = parser.start();
        parser.bump();
        name_list(parser);
        clause.complete(parser, SyntaxKind::ImplementsClause);
    }
}

/// Comma-separated qualified names. Arity (single inheritance) is
/// semantic. Terminates: every iteration parses a name (which always
/// bumps) or breaks.
fn name_list(parser: &mut Parser) {
    loop {
        if matches!(
            parser.current(),
            Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
        ) {
            name(parser);
        } else {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
            break;
        }
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
}

/// `{ members }`. The loop is position-guarded like `argument_list`:
/// a member rule can refuse without consuming (the nesting guard can
/// veto a type or initializer sub-parse), and the guard then forces an
/// `error_element` bump, so the list always progresses. The guard on
/// `current` (not `at_end`) makes a blown fuse unwind instead of spin.
fn member_list(parser: &mut Parser) {
    let marker = parser.start();
    if parser.eat(SyntaxKind::OpenBrace) {
        while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
            let position_before_member = parser.position();
            member(parser);
            if parser.position() == position_before_member {
                error_element(parser);
            }
        }
        parser.expect(SyntaxKind::CloseBrace);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::MemberList);
}

/// One class-body member. Tasks 6 through 10 grow this dispatch
/// (properties, constants, methods, trait use, enum cases,
/// attributes); until then everything is swept as wreckage.
fn member(parser: &mut Parser) {
    if parser.current().is_none() {
        return;
    }
    error_element(parser);
}
```

- [ ] **Step 5: Wire the statement dispatch and the expression arms**

In `statements.rs`, add beside the other declaration arms (order matters: the guarded `Readonly` arm precedes nothing else that matches `Readonly`, and all sit before the `starts_expression` arm):

```rust
        Some(
            SyntaxKind::Class
                | SyntaxKind::Interface
                | SyntaxKind::Trait
                | SyntaxKind::Abstract
                | SyntaxKind::Final,
        ) => super::declarations::declaration(parser),
        Some(SyntaxKind::Readonly) if parser.nth(1) != Some(SyntaxKind::OpenParenthesis) => {
            super::declarations::declaration(parser)
        }
```

In `expressions.rs`:

1. In `starts_expression`, add `| SyntaxKind::Readonly` (the statement dispatcher routes `readonly(` here; everything else `readonly` goes to the declaration path).

2. In `primary_expression`, add an arm (beside the `static` arms):

```rust
        // Zend keeps `readonly` callable as a plain function name for
        // backward compatibility: directly followed by `(` it is a
        // call target, never a modifier.
        Some(SyntaxKind::Readonly) if parser.nth(1) == Some(SyntaxKind::OpenParenthesis) => {
            let marker = parser.start();
            let name_marker = parser.start();
            parser.bump();
            name_marker.complete(parser, SyntaxKind::Name);
            Some(marker.complete(parser, SyntaxKind::NameExpression))
        }
```

(A bare `readonly` in expression position falls through to `ExpectedExpression`, same recovery as before.)

3. In `new_expression`, replace the wreckage arm

```rust
        // `new class { ... }` (anonymous classes) belongs to the
        // declarations plan; recovery keeps the tokens until then.
        Some(SyntaxKind::Class) => error_element(parser),
```

with

```rust
        Some(
            SyntaxKind::Class
                | SyntaxKind::Readonly
                | SyntaxKind::Final
                | SyntaxKind::Abstract,
        ) => {
            let class_marker = parser.start();
            super::declarations::anonymous_class(parser, class_marker);
        }
```

- [ ] **Step 6: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_class_like`
Expected: PASS.

Run: `cargo test --workspace`
Expected: corpus files where `class` bodies were wreckage improve; `expressions_new_clone` snapshots change from the `new class` `ErrorNode` to a real `ClassDeclaration`. Accept those diffs only.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse class, interface, and trait declarations"
```

---

### Task 6: Properties and class constants

The first real members: modifier sequences (asymmetric visibility included — it is five lines here, not a separate feature), typed and untyped properties with initializer lists, class constants through the marker-taking `constant_declaration` from Task 3, and the `render_member` test helper the remaining member tasks lean on.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/declarations.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (two node kinds)
- Modify: `crates/celerrate_syntax/tests/support/mod.rs` (`render_member`)
- Create: `crates/celerrate_syntax/tests/declarations_members.rs`

**Interfaces:**
- Consumes: `constant_declaration(parser, marker)` (Task 3), `types::{starts_type, type_expression}`, `expression`, `terminate_statement`.
- Produces: `member_modifiers(parser)`, `asymmetric_visibility_suffix(parser)`, `starts_member(kind) -> bool`, `modified_member(parser, marker)`, `property_declaration(parser, marker)` — all private; Tasks 7-10 extend `starts_member` and `modified_member`, Task 9 modifies `property_declaration`.

- [ ] **Step 1: Add the `render_member` helper**

In `tests/support/mod.rs`, append:

```rust
/// Renders the first member of `<?php class Fixture { {member_source} }`
/// as an indented tree, offsets and trivia omitted: the workhorse
/// assertion of the member grammar tests.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_member(member_source: &str) -> String {
    let source = format!("<?php class Fixture {{ {member_source} }}");
    let parse = parse_verified(&source);
    let class_declaration = parse
        .tree()
        .children()
        .next()
        .expect("a class declaration");
    let member_list = class_declaration
        .children()
        .find(|node| node.kind() == SyntaxKind::MemberList)
        .expect("a member list");
    let member = member_list.children().next().expect("a first member");
    let mut output = String::new();
    render_element_without_offsets(&mut output, member.into(), 0);
    output
}
```

- [ ] **Step 2: Write the failing tests**

Create `tests/declarations_members.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_member};

#[test]
fn a_typed_property_lists_its_declarators() {
    insta::assert_snapshot!(render_member("public int $balance = 0, $pending;"), @r#"
    PropertyDeclaration
      Public "public"
      NamedType
        Name
          Identifier "int"
      PropertyElement
        Variable "$balance"
        Equals "="
        Literal
          IntegerLiteral "0"
      Comma ","
      PropertyElement
        Variable "$pending"
      Semicolon ";"
    "#);
}

#[test]
fn a_var_property_parses() {
    insta::assert_snapshot!(render_member("var $legacy;"), @r#"
    PropertyDeclaration
      Var "var"
      PropertyElement
        Variable "$legacy"
      Semicolon ";"
    "#);
}

#[test]
fn asymmetric_visibility_stays_flat_modifier_tokens() {
    insta::assert_snapshot!(render_member("public private(set) string $name;"), @r#"
    PropertyDeclaration
      Public "public"
      Private "private"
      OpenParenthesis "("
      Identifier "set"
      CloseParenthesis ")"
      NamedType
        Name
          Identifier "string"
      PropertyElement
        Variable "$name"
      Semicolon ";"
    "#);
}

#[test]
fn a_nullable_static_property_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { protected static ?self $instance = null; }"),
        vec![]
    );
}

#[test]
fn a_class_constant_carries_modifiers_and_a_type() {
    insta::assert_snapshot!(render_member("final protected const int LIMIT = 10;"), @r#"
    ConstantDeclaration
      Final "final"
      Protected "protected"
      Const "const"
      NamedType
        Name
          Identifier "int"
      ConstantElement
        Identifier "LIMIT"
        Equals "="
        Literal
          IntegerLiteral "10"
      Semicolon ";"
    "#);
}

#[test]
fn a_semi_reserved_class_constant_name_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { const FOR = 'semi-reserved'; }"),
        vec![]
    );
}

#[test]
fn dangling_modifiers_become_wreckage_and_the_list_recovers() {
    // `public` wraps into an ErrorNode (ExpectedDeclaration); the `;`
    // it dangled on is then swept by the member list (UnexpectedToken);
    // the constant after it parses clean.
    assert_eq!(
        parser_diagnostics("<?php class A { public; const OK = 1; }"),
        vec![
            ParserDiagnosticKind::ExpectedDeclaration,
            ParserDiagnosticKind::UnexpectedToken,
        ]
    );
}

#[test]
fn a_property_without_a_variable_is_diagnosed() {
    let diagnostics = parser_diagnostics("<?php class A { public int = 5; }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::Variable)),
        "got {diagnostics:?}"
    );
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_members`
Expected: FAIL — members are still wreckage.

- [ ] **Step 4: Add the node kinds**

In `syntax_kind.rs`, append after `MemberList`:

```rust
    /// `public int $a = 1, $b;`: modifiers, optional type, then the
    /// declarator elements.
    PropertyDeclaration,
    /// One `$name [= initializer]` element; a hooked property carries
    /// its `PropertyHookList` here (a later task of this plan).
    PropertyElement,
```

- [ ] **Step 5: Implement the member dispatch**

In `declarations.rs`, replace the Task 5 `member` placeholder with the dispatch, and add the rules:

```rust
/// One class-body member. Tasks of this plan keep growing this
/// dispatch; `member_list`'s position guard backstops any
/// zero-consumption refusal path, so the arms may refuse freely.
fn member(parser: &mut Parser) {
    match parser.current() {
        Some(kind) if starts_member(kind) => {
            let marker = parser.start();
            modified_member(parser, marker);
        }
        Some(_) => error_element(parser),
        None => {}
    }
}

/// Whether `kind` can start a modified member (a property, a
/// constant, or — from the methods task on — a method).
fn starts_member(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Public
            | SyntaxKind::Protected
            | SyntaxKind::Private
            | SyntaxKind::Static
            | SyntaxKind::Abstract
            | SyntaxKind::Final
            | SyntaxKind::Readonly
            | SyntaxKind::Var
            | SyntaxKind::Const
            | SyntaxKind::Variable
    ) || super::types::starts_type(kind)
}

/// Modifiers, then the member they modify: a constant, a method
/// (next task), or a property (optionally typed). The member's kind
/// is decided by the first token after the modifiers.
fn modified_member(parser: &mut Parser, marker: Marker) {
    member_modifiers(parser);
    match parser.current() {
        Some(SyntaxKind::Const) => constant_declaration(parser, marker),
        // A bare `$x;` member is a Zend parse error; parsing it as an
        // unmodified property keeps it one analyzable unit.
        Some(SyntaxKind::Variable) => property_declaration(parser, marker),
        Some(kind) if super::types::starts_type(kind) => {
            types_then_property(parser, marker);
        }
        _ => {
            // Modifiers with nothing to modify (`public;`). At least
            // one modifier token was consumed on this path, so the
            // member list progresses.
            parser.diagnose_current(ParserDiagnosticKind::ExpectedDeclaration);
            marker.complete(parser, SyntaxKind::ErrorNode);
        }
    }
}

/// The typed-property tail. The type parse can be refused by the
/// nesting guard without consuming; `member_list`'s position guard
/// covers that case.
fn types_then_property(parser: &mut Parser, marker: Marker) {
    super::types::type_expression(parser);
    property_declaration(parser, marker);
}

/// `public`, `protected`, `private` (each optionally asymmetric:
/// `private(set)`), `static`, `abstract`, `final`, `readonly`, `var`.
/// Order, repetition, and combination are all judged upstairs; the
/// parser accepts any sequence.
fn member_modifiers(parser: &mut Parser) {
    loop {
        match parser.current() {
            Some(SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private) => {
                parser.bump();
                asymmetric_visibility_suffix(parser);
            }
            Some(
                SyntaxKind::Static
                    | SyntaxKind::Abstract
                    | SyntaxKind::Final
                    | SyntaxKind::Readonly
                    | SyntaxKind::Var,
            ) => parser.bump(),
            _ => break,
        }
    }
}

/// The `(set)` of 8.4's asymmetric visibility, three flat tokens.
/// Zend lexes `private(set)` as one token and thereby forbids interior
/// whitespace; the trivia-free view cannot see adjacency, so spaced
/// forms parse here and adjacency is judged upstairs (the same trade
/// recorded on `name`). Only the exact `( identifier )` shape is
/// taken: anything else belongs to the member that follows.
fn asymmetric_visibility_suffix(parser: &mut Parser) {
    if parser.at(SyntaxKind::OpenParenthesis)
        && parser.nth(1) == Some(SyntaxKind::Identifier)
        && parser.nth(2) == Some(SyntaxKind::CloseParenthesis)
    {
        parser.bump();
        parser.bump();
        parser.bump();
    }
}

/// The declarators of one property: `$a = 1, $b;`. The caller consumed
/// the modifiers and the optional type into `marker`. Terminates:
/// every iteration bumps a variable or breaks.
fn property_declaration(parser: &mut Parser, marker: Marker) {
    loop {
        if !parser.at(SyntaxKind::Variable) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
            break;
        }
        let element = parser.start();
        parser.bump();
        if parser.eat(SyntaxKind::Equals) {
            expression(parser);
        }
        element.complete(parser, SyntaxKind::PropertyElement);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::PropertyDeclaration);
}
```

- [ ] **Step 6: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_members`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS (accept wreckage-to-member corpus improvements only).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse property and class constant members"
```

---

### Task 7: Methods and trait use with adaptations

Methods close over everything the expression plan already built (`parameter_list`, blocks, return types) and add the semi-reserved name rule. Trait `use` gets its adaptation braces: precedences (`A::b insteadof C;`) and aliases (`b as protected c;`).

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/declarations.rs`
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (five node kinds)
- Modify: `crates/celerrate_syntax/tests/declarations_members.rs`

**Interfaces:**
- Consumes: `parameter_list`, `block`, `types::type_expression`, `name`, `name_list` (Task 5), `terminate_statement`, `error_element`.
- Produces: `method_declaration(parser, marker)` (private, reached through `modified_member`), `trait_use(parser)` (private, reached through `member`). Task 9 does not touch these; Task 10 only adds attribute routing in front.

- [ ] **Step 1: Write the failing tests**

Append to `tests/declarations_members.rs`:

```rust
#[test]
fn a_method_with_modifiers_a_return_type_and_a_body() {
    insta::assert_snapshot!(render_member("public function list(): array { return []; }"), @r#"
    MethodDeclaration
      Public "public"
      Function "function"
      List "list"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      NamedType
        Array "array"
      Block
        OpenBrace "{"
        ReturnStatement
          Return "return"
          ArrayExpression
            OpenBracket "["
            CloseBracket "]"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn an_abstract_method_ends_at_its_semicolon() {
    insta::assert_snapshot!(render_member("abstract protected function close(): void;"), @r#"
    MethodDeclaration
      Abstract "abstract"
      Protected "protected"
      Function "function"
      Identifier "close"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      NamedType
        Name
          Identifier "void"
      Semicolon ";"
    "#);
}

#[test]
fn a_by_reference_method_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { public function &reference(): int { return $this->x; } }"),
        vec![]
    );
}

#[test]
fn interface_method_signatures_parse() {
    assert_eq!(
        parser_diagnostics("<?php interface Shape { public function area(): float; }"),
        vec![]
    );
}

#[test]
fn a_simple_trait_use_ends_at_its_semicolon() {
    insta::assert_snapshot!(render_member("use Greets, Counts;"), @r#"
    TraitUseClause
      Use "use"
      Name
        Identifier "Greets"
      Comma ","
      Name
        Identifier "Counts"
      Semicolon ";"
    "#);
}

#[test]
fn trait_adaptations_parse_precedences_and_aliases() {
    insta::assert_snapshot!(render_member("use Greets, Counts { Greets::hello insteadof Counts; Counts::hello as protected countedHello; }"), @r#"
    TraitUseClause
      Use "use"
      Name
        Identifier "Greets"
      Comma ","
      Name
        Identifier "Counts"
      TraitAdaptationList
        OpenBrace "{"
        TraitPrecedence
          Name
            Identifier "Greets"
          ColonColon "::"
          Identifier "hello"
          InsteadOf "insteadof"
          Name
            Identifier "Counts"
          Semicolon ";"
        TraitAlias
          Name
            Identifier "Counts"
          ColonColon "::"
          Identifier "hello"
          As "as"
          Protected "protected"
          Identifier "countedHello"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn a_bare_alias_with_a_keyword_member_name_parses() {
    // `list as unreserved;`: a bare semi-reserved member name, no
    // class qualifier.
    assert_eq!(
        parser_diagnostics("<?php class A { use B { list as unreserved; } }"),
        vec![]
    );
}

#[test]
fn a_visibility_only_alias_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { use B { hello as protected; } }"),
        vec![]
    );
}

#[test]
fn junk_inside_an_adaptation_list_is_swept() {
    let diagnostics = parser_diagnostics("<?php class A { use B { 42; hello as h; } }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken),
        "got {diagnostics:?}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_members`
Expected: FAIL — the node kinds do not exist.

- [ ] **Step 3: Add the node kinds**

In `syntax_kind.rs`, append after `PropertyElement`:

```rust
    /// `function name(parameters): type { body }` (or `;` for the
    /// abstract and interface forms) as a class member, modifiers
    /// included.
    MethodDeclaration,
    /// `use TraitA, TraitB;` inside a class body, with an optional
    /// adaptation list instead of the semicolon.
    TraitUseClause,
    /// `{ adaptations }` of a trait use.
    TraitAdaptationList,
    /// `A::member insteadof B, C;`.
    TraitPrecedence,
    /// `[A::]member as [visibility] [name];`.
    TraitAlias,
```

- [ ] **Step 4: Implement in `declarations.rs`**

Extend the imports to include `block` if not already there (it is, from Task 3).

In `starts_member`, add `| SyntaxKind::Function` to the matched kinds.

In `member`, add the trait-use arm before the `starts_member` arm:

```rust
        Some(SyntaxKind::Use) => trait_use(parser),
```

In `modified_member`, add the method arm after the `Const` arm:

```rust
        Some(SyntaxKind::Function) => method_declaration(parser, marker),
```

Append the rules:

```rust
/// `function name(parameters): type` ending in a block or, for the
/// abstract and interface forms, `;`. Method names are semi-reserved:
/// any keyword parses (`public function list() {}`). The caller
/// consumed the modifiers into `marker`; whether a body is required
/// is semantic.
fn method_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `function`
    parser.eat(SyntaxKind::Ampersand); // by-reference return
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => parser.bump(),
        _ => parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
    parameter_list(parser);
    if parser.eat(SyntaxKind::Colon) {
        super::types::type_expression(parser);
    }
    if parser.at(SyntaxKind::OpenBrace) {
        block(parser);
    } else {
        parser.expect(SyntaxKind::Semicolon);
    }
    marker.complete(parser, SyntaxKind::MethodDeclaration);
}

/// `use A, B;` or `use A, B { adaptations }` inside a class body.
/// Distinct from the import `use` (statement level) and the closure
/// `use` (expression level); the contexts are disjoint.
fn trait_use(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `use`
    name_list(parser);
    if parser.at(SyntaxKind::OpenBrace) {
        trait_adaptation_list(parser);
    } else {
        terminate_statement(parser);
    }
    marker.complete(parser, SyntaxKind::TraitUseClause);
}

/// `{ A::b insteadof C; b as protected c; }`. Position-guarded like
/// `member_list`, and for the same reason.
fn trait_adaptation_list(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `{`
    while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
        let position_before_adaptation = parser.position();
        trait_adaptation(parser);
        if parser.position() == position_before_adaptation {
            error_element(parser);
        }
    }
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::TraitAdaptationList);
}

/// One adaptation. The method reference is `A\B::member` or a bare
/// `member` (semi-reserved: keywords parse). `insteadof` makes it a
/// precedence; otherwise `as` with an optional visibility and an
/// optional new name makes it an alias. Whether the reference resolves
/// is semantic.
fn trait_adaptation(parser: &mut Parser) {
    let marker = parser.start();
    match parser.current() {
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace) => {
            name(parser);
            if parser.eat(SyntaxKind::ColonColon) {
                adaptation_member_name(parser);
            }
        }
        Some(kind) if kind.is_keyword() => parser.bump(),
        _ => {
            marker.abandon(parser);
            error_element(parser);
            return;
        }
    }
    if parser.eat(SyntaxKind::InsteadOf) {
        name_list(parser);
        terminate_statement(parser);
        marker.complete(parser, SyntaxKind::TraitPrecedence);
    } else {
        parser.expect(SyntaxKind::As);
        if matches!(
            parser.current(),
            Some(SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private)
        ) {
            parser.bump();
        }
        if parser
            .current()
            .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword())
        {
            parser.bump();
        }
        terminate_statement(parser);
        marker.complete(parser, SyntaxKind::TraitAlias);
    }
}

/// The member half of `A::member` in an adaptation: an identifier or
/// any keyword (semi-reserved).
fn adaptation_member_name(parser: &mut Parser) {
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => parser.bump(),
        _ => parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
}
```

- [ ] **Step 5: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_members`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS (accept wreckage-to-member corpus improvements only).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse methods and trait use adaptations"
```

---

### Task 8: Enum declarations and cases

Enums reuse the whole member machine: only the declaration head (`enum Name: BackingType`) and the `case` member are new. `enum` is not a reserved word, so both dispatch sites look one token ahead, and `enum(...)` joins `readonly(...)` in the call-permissiveness arm.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/declarations.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs` (one dispatch arm)
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (the call-permissiveness arm widens; `starts_expression`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (two node kinds)
- Create: `crates/celerrate_syntax/tests/declarations_enums.rs`

**Interfaces:**
- Consumes: `heritage_clauses`, `member_list`, `types::type_expression`, `expression`, `terminate_statement`.
- Produces: `enum_declaration(parser, marker)` and `enum_case(parser, marker)` (private; the latter's marker-taking shape is what lets Task 10 attach attributes to cases).

- [ ] **Step 1: Write the failing tests**

Create `tests/declarations_enums.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_statement};

#[test]
fn a_backed_enum_with_heritage_and_cases() {
    insta::assert_snapshot!(render_statement("enum Suit: string implements HasColor { case Hearts = 'H'; }"), @r#"
    EnumDeclaration
      Enum "enum"
      Identifier "Suit"
      Colon ":"
      NamedType
        Name
          Identifier "string"
      ImplementsClause
        Implements "implements"
        Name
          Identifier "HasColor"
      MemberList
        OpenBrace "{"
        EnumCase
          Case "case"
          Identifier "Hearts"
          Equals "="
          Literal
            SingleQuotedString "'H'"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn a_pure_enum_case_has_no_value() {
    insta::assert_snapshot!(render_statement("enum Direction { case North; }"), @r#"
    EnumDeclaration
      Enum "enum"
      Identifier "Direction"
      MemberList
        OpenBrace "{"
        EnumCase
          Case "case"
          Identifier "North"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn enums_carry_ordinary_members_too() {
    assert_eq!(
        parser_diagnostics(
            "<?php enum Suit: string { case Hearts = 'H'; const WILD = '*'; public function color(): string { return 'red'; } }"
        ),
        vec![]
    );
}

#[test]
fn enum_stays_callable_as_a_function_name() {
    // Zend backward compatibility: `enum` is not reserved; it only
    // declares when a name follows.
    assert_eq!(parser_diagnostics("<?php enum(1);"), vec![]);
}

#[test]
fn a_case_member_in_a_class_parses_and_is_judged_upstairs() {
    // Structurally fine anywhere a member is; enums-only is semantic.
    assert_eq!(parser_diagnostics("<?php class A { case North; }"), vec![]);
}

#[test]
fn a_semi_reserved_case_name_parses() {
    assert_eq!(parser_diagnostics("<?php enum Ops { case List; }"), vec![]);
}

#[test]
fn a_case_without_a_name_is_diagnosed_and_the_enum_recovers() {
    let diagnostics = parser_diagnostics("<?php enum Broken { case = 1; case Ok; }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(
            celerrate_syntax::SyntaxKind::Identifier
        )),
        "got {diagnostics:?}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_enums`
Expected: FAIL.

- [ ] **Step 3: Add the node kinds**

In `syntax_kind.rs`, append after `TraitAlias`:

```rust
    /// `enum Name: BackingType implements A { cases and members }`.
    EnumDeclaration,
    /// `case Name;` or `case Name = expression;`.
    EnumCase,
```

- [ ] **Step 4: Implement**

In `declarations.rs`, add the dispatch arm to `declaration`'s match:

```rust
        Some(SyntaxKind::Enum) if parser.nth(1) == Some(SyntaxKind::Identifier) => {
            enum_declaration(parser, marker);
        }
```

In `member`, add the case arm beside the trait-use arm:

```rust
        Some(SyntaxKind::Case) => {
            let marker = parser.start();
            enum_case(parser, marker);
        }
```

Append the rules:

```rust
/// `enum Name: BackingType implements A { ... }`. `enum` is not
/// reserved: both dispatch sites verified a name follows, so
/// `enum(...)` stays a call. Backing types other than `int` and
/// `string` parse; arity is semantic.
fn enum_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `enum`
    parser.expect(SyntaxKind::Identifier);
    if parser.eat(SyntaxKind::Colon) {
        super::types::type_expression(parser);
    }
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::EnumDeclaration);
}

/// `case Name;` or `case Name = expression;`. Case names are
/// semi-reserved; whether a case belongs here (enums only) and whether
/// the value is required (backed enums) are semantic.
fn enum_case(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `case`
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => parser.bump(),
        _ => parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
    if parser.eat(SyntaxKind::Equals) {
        expression(parser);
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::EnumCase);
}
```

In `statements.rs`, add the dispatch arm beside the other declaration arms (before the `starts_expression` arm):

```rust
        Some(SyntaxKind::Enum) if parser.nth(1) == Some(SyntaxKind::Identifier) => {
            super::declarations::declaration(parser)
        }
```

In `expressions.rs`:
- In `starts_expression`, add `| SyntaxKind::Enum`.
- Widen the Task 5 call-permissiveness arm in `primary_expression` from `Some(SyntaxKind::Readonly)` to:

```rust
        // Zend keeps `enum` and `readonly` callable as plain function
        // names for backward compatibility: directly followed by `(`
        // they are call targets, never declaration keywords.
        Some(SyntaxKind::Enum | SyntaxKind::Readonly)
            if parser.nth(1) == Some(SyntaxKind::OpenParenthesis) =>
```

(body unchanged).

Note on `case = 1;` recovery: `enum_case` diagnoses the missing name, then `eat(Equals)` and the expression still run, so the case completes partially and the list moves on — the behavior the last test pins.

- [ ] **Step 5: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_enums`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS (accept wreckage-to-enum corpus improvements only).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse enum declarations and cases"
```

---

### Task 9: Property hooks and constructor promotion

The 8.4 property machinery: hook lists (`{ get; set(...) {...} }`) hanging off a `PropertyElement`, and promotion modifiers (visibility, asymmetric forms, `readonly`) plus hook lists on parameters. A hooked property has no terminating `;` — the hook list's closing brace ends it.

**Files:**
- Modify: `crates/celerrate_syntax/src/parser/grammar/declarations.rs` (the hook rules; `promotion_modifiers`, which reuses the private `asymmetric_visibility_suffix` from Task 6)
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (`parameter`, `starts_parameter`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (two node kinds)
- Create: `crates/celerrate_syntax/tests/declarations_hooks.rs`

**Interfaces:**
- Consumes: `parameter_list` (for `set(string $value)`), `block`, `expression`, `terminate_statement`, `error_element`.
- Produces: `property_hook_list(parser)` and `promotion_modifiers(parser)`, both `pub(super)` in `declarations.rs` because `expressions::parameter` calls them across the module boundary.

- [ ] **Step 1: Write the failing tests**

Create `tests/declarations_hooks.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_member};

#[test]
fn abstract_hooks_are_name_and_semicolon() {
    insta::assert_snapshot!(render_member("public string $name { get; set; }"), @r#"
    PropertyDeclaration
      Public "public"
      NamedType
        Name
          Identifier "string"
      PropertyElement
        Variable "$name"
        PropertyHookList
          OpenBrace "{"
          PropertyHook
            Identifier "get"
            Semicolon ";"
          PropertyHook
            Identifier "set"
            Semicolon ";"
          CloseBrace "}"
    "#);
}

#[test]
fn hook_bodies_take_arrow_expressions_parameters_and_blocks() {
    insta::assert_snapshot!(render_member("public string $name { get => $this->raw; set(string $value) { $this->raw = $value; } }"), @r#"
    PropertyDeclaration
      Public "public"
      NamedType
        Name
          Identifier "string"
      PropertyElement
        Variable "$name"
        PropertyHookList
          OpenBrace "{"
          PropertyHook
            Identifier "get"
            FatArrow "=>"
            MemberAccessExpression
              VariableReference
                Variable "$this"
              Arrow "->"
              MemberName
                Identifier "raw"
            Semicolon ";"
          PropertyHook
            Identifier "set"
            ParameterList
              OpenParenthesis "("
              Parameter
                NamedType
                  Name
                    Identifier "string"
                Variable "$value"
              CloseParenthesis ")"
            Block
              OpenBrace "{"
              ExpressionStatement
                AssignmentExpression
                  MemberAccessExpression
                    VariableReference
                      Variable "$this"
                    Arrow "->"
                    MemberName
                      Identifier "raw"
                  Equals "="
                  VariableReference
                    Variable "$value"
                Semicolon ";"
              CloseBrace "}"
          CloseBrace "}"
    "#);
}

#[test]
fn a_by_reference_final_hook_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { public array $items { final &get { return $this->items; } } }"),
        vec![]
    );
}

#[test]
fn a_hooked_property_needs_no_semicolon() {
    assert_eq!(
        parser_diagnostics("<?php class A { public string $x { get; } public int $y = 1; }"),
        vec![]
    );
}

#[test]
fn hooks_in_an_interface_parse() {
    assert_eq!(
        parser_diagnostics("<?php interface Named { public string $name { get; set; } }"),
        vec![]
    );
}

#[test]
fn promoted_constructor_parameters_take_modifiers() {
    insta::assert_snapshot!(render_member("public function __construct(public readonly int $x, private(set) string $y = 'a') {}"), @r#"
    MethodDeclaration
      Public "public"
      Function "function"
      Identifier "__construct"
      ParameterList
        OpenParenthesis "("
        Parameter
          Public "public"
          Readonly "readonly"
          NamedType
            Name
              Identifier "int"
          Variable "$x"
        Comma ","
        Parameter
          Private "private"
          OpenParenthesis "("
          Identifier "set"
          CloseParenthesis ")"
          NamedType
            Name
              Identifier "string"
          Variable "$y"
          Equals "="
          Literal
            SingleQuotedString "'a'"
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn hooks_on_a_promoted_parameter_parse() {
    // 8.4 allows hooks in constructor promotion; availability is
    // semantic.
    assert_eq!(
        parser_diagnostics(
            "<?php class A { public function __construct(public string $full { get => $this->first; }) {} }"
        ),
        vec![]
    );
}

#[test]
fn junk_inside_a_hook_list_is_swept_and_the_list_recovers() {
    let diagnostics = parser_diagnostics("<?php class A { public int $x { 42 get; } }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken),
        "got {diagnostics:?}"
    );
}

#[test]
fn an_unclosed_hook_list_terminates() {
    let diagnostics = parser_diagnostics("<?php class A { public int $x { get;");
    assert!(!diagnostics.is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_hooks`
Expected: FAIL.

- [ ] **Step 3: Add the node kinds**

In `syntax_kind.rs`, append after `EnumCase`:

```rust
    /// `{ get; set(...) { ... } }` on a property or a promoted
    /// parameter (8.4).
    PropertyHookList,
    /// One hook: optional `final`, optional `&`, the name, an optional
    /// parameter list, then `;`, `=> expression;`, or a block.
    PropertyHook,
```

- [ ] **Step 4: Implement the hooks in `declarations.rs`**

Replace `property_declaration` with the hook-aware version:

```rust
/// The declarators of one property: `$a = 1, $b;`, or the hooked form
/// `$name { get; ... }`, which ends at its closing brace with no `;`.
/// The caller consumed the modifiers and the optional type into
/// `marker`. Terminates: every iteration bumps a variable or breaks.
fn property_declaration(parser: &mut Parser, marker: Marker) {
    let mut hooked = false;
    loop {
        if !parser.at(SyntaxKind::Variable) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
            break;
        }
        let element = parser.start();
        parser.bump();
        if parser.eat(SyntaxKind::Equals) {
            expression(parser);
        }
        if parser.at(SyntaxKind::OpenBrace) {
            property_hook_list(parser);
            hooked = true;
        }
        element.complete(parser, SyntaxKind::PropertyElement);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    if !hooked {
        terminate_statement(parser);
    }
    marker.complete(parser, SyntaxKind::PropertyDeclaration);
}
```

Append the hook rules and the promotion modifiers:

```rust
/// `{ get; set => ...; &get { ... } }` (8.4 property hooks).
/// Position-guarded like `member_list`, and for the same reason.
pub(super) fn property_hook_list(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `{`
    while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
        let position_before_hook = parser.position();
        property_hook(parser);
        if parser.position() == position_before_hook {
            error_element(parser);
        }
    }
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::PropertyHookList);
}

/// One hook: `get;` (abstract), `get => expression;`,
/// `set(string $value) { ... }`, `final &get { ... }`. Hook names are
/// plain identifiers; which names exist (`get`, `set`) and which
/// combinations are legal is semantic.
fn property_hook(parser: &mut Parser) {
    let marker = parser.start();
    parser.eat(SyntaxKind::Final); // the one modifier hooks admit today
    parser.eat(SyntaxKind::Ampersand); // by-reference `get`
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => {
            parser.bump();
            if parser.at(SyntaxKind::OpenParenthesis) {
                parameter_list(parser); // `set(string $value)`
            }
            if parser.eat(SyntaxKind::FatArrow) {
                expression(parser);
                terminate_statement(parser);
            } else if parser.at(SyntaxKind::OpenBrace) {
                block(parser);
            } else {
                terminate_statement(parser); // the abstract form `get;`
            }
            marker.complete(parser, SyntaxKind::PropertyHook);
        }
        _ => {
            // Nothing hook-shaped. Tokens may already be consumed
            // (`final`, `&`), so the node completes partially; a
            // zero-consumption trip is swept by the list's position
            // guard.
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
            marker.complete(parser, SyntaxKind::PropertyHook);
        }
    }
}

/// Constructor promotion (8.0) and its 8.4 extensions on a parameter:
/// visibility (optionally asymmetric) and `readonly`. Which parameters
/// admit them (constructors only) is semantic.
pub(super) fn promotion_modifiers(parser: &mut Parser) {
    loop {
        match parser.current() {
            Some(SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private) => {
                parser.bump();
                asymmetric_visibility_suffix(parser);
            }
            Some(SyntaxKind::Readonly) => parser.bump(),
            _ => break,
        }
    }
}
```

- [ ] **Step 5: Extend `parameter` in `expressions.rs`**

Replace `parameter` with:

```rust
fn parameter(parser: &mut Parser) {
    let marker = parser.start();
    super::declarations::promotion_modifiers(parser);
    if !matches!(
        parser.current(),
        Some(SyntaxKind::Variable | SyntaxKind::Ampersand | SyntaxKind::Ellipsis)
    ) {
        super::types::type_expression(parser);
    }
    parser.eat(SyntaxKind::Ampersand);
    parser.eat(SyntaxKind::Ellipsis);
    parser.expect(SyntaxKind::Variable);
    if parser.eat(SyntaxKind::Equals) {
        expression(parser);
    }
    if parser.at(SyntaxKind::OpenBrace) {
        // Hooks on a promoted constructor property (8.4); legality is
        // semantic. Unreachable for an ordinary closing parameter: the
        // list already stopped at `)`.
        super::declarations::property_hook_list(parser);
    }
    marker.complete(parser, SyntaxKind::Parameter);
}
```

In `starts_parameter`, add `| SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private | SyntaxKind::Readonly` to the matched kinds.

- [ ] **Step 6: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_hooks`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS, no snapshot churn outside the new file.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse property hooks and constructor promotion"
```

---

### Task 10: Attributes

`#[...]` groups everywhere Zend allows them: declarations, members, enum cases, parameters, property hooks, closures and arrow functions, anonymous classes. The marker-taking convention pays off here: the declaration and member paths gain one `attribute_groups` call each; only the closure entry changes signature.

**Files:**
- Create: `crates/celerrate_syntax/src/parser/grammar/attributes.rs`
- Modify: `crates/celerrate_syntax/src/parser/grammar.rs` (declare `mod attributes;`)
- Modify: `crates/celerrate_syntax/src/parser/grammar/declarations.rs` (`declaration` and `member` route attributes; `property_hook`)
- Modify: `crates/celerrate_syntax/src/parser/grammar/statements.rs` (one dispatch arm)
- Modify: `crates/celerrate_syntax/src/parser/grammar/expressions.rs` (closures, parameters, `new`, `starts_expression`, `starts_parameter`)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (two node kinds)
- Create: `crates/celerrate_syntax/tests/declarations_attributes.rs`

**Interfaces:**
- Consumes: `expressions::{argument_list, error_element, expect_list_separator, name}`, the marker-taking declaration rules.
- Produces: `attributes::attribute_groups(parser: &mut Parser)` (`pub(super)`): consumes zero or more `#[...]` groups, always leaving the cursor on whatever follows.

- [ ] **Step 1: Write the failing tests**

Create `tests/declarations_attributes.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use support::{parser_diagnostics, render_expression, render_statement};

#[test]
fn an_attributed_function_keeps_its_groups_inside_the_declaration() {
    insta::assert_snapshot!(render_statement("#[Route('/home')] function home() {}"), @r#"
    FunctionDeclaration
      AttributeGroup
        AttributeOpen "#["
        Attribute
          Name
            Identifier "Route"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                SingleQuotedString "'/home'"
            CloseParenthesis ")"
        CloseBracket "]"
      Function "function"
      Identifier "home"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Block
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn one_group_carries_several_attributes() {
    insta::assert_snapshot!(render_statement("#[First, Second(1)] class A {}"), @r#"
    ClassDeclaration
      AttributeGroup
        AttributeOpen "#["
        Attribute
          Name
            Identifier "First"
        Comma ","
        Attribute
          Name
            Identifier "Second"
          ArgumentList
            OpenParenthesis "("
            Argument
              Literal
                IntegerLiteral "1"
            CloseParenthesis ")"
        CloseBracket "]"
      Class "class"
      Identifier "A"
      MemberList
        OpenBrace "{"
        CloseBrace "}"
    "#);
}

#[test]
fn stacked_groups_parse_on_every_declaration_kind() {
    for source in [
        "<?php #[A] #[B] final class C {}",
        "<?php #[A] interface I {}",
        "<?php #[A] trait T {}",
        "<?php #[A] enum E { case One; }",
        "<?php #[A] const X = 1;",
    ] {
        assert_eq!(parser_diagnostics(source), vec![], "source: {source}");
    }
}

#[test]
fn members_cases_and_parameters_take_attributes() {
    for source in [
        "<?php class A { #[Override] public function handle(#[SensitiveParameter] string $token): void {} }",
        "<?php class A { #[Marker] public int $x = 1; }",
        "<?php class A { #[Marker] const X = 1; }",
        "<?php enum E { #[Marker] case One; }",
        "<?php class A { public int $x { #[Marker] get => 1; } }",
    ] {
        assert_eq!(parser_diagnostics(source), vec![], "source: {source}");
    }
}

#[test]
fn closures_and_arrow_functions_take_attributes() {
    insta::assert_snapshot!(render_expression("#[Pure] static fn (int $x): int => $x"), @r#"
    ArrowFunctionExpression
      AttributeGroup
        AttributeOpen "#["
        Attribute
          Name
            Identifier "Pure"
        CloseBracket "]"
      Static "static"
      Fn "fn"
      ParameterList
        OpenParenthesis "("
        Parameter
          NamedType
            Name
              Identifier "int"
          Variable "$x"
        CloseParenthesis ")"
      Colon ":"
      NamedType
        Name
          Identifier "int"
      FatArrow "=>"
      VariableReference
        Variable "$x"
    "#);
}

#[test]
fn an_anonymous_class_takes_attributes_after_new() {
    assert_eq!(
        parser_diagnostics("<?php $o = new #[Marker] class {};"),
        vec![]
    );
}

#[test]
fn attributes_before_a_non_declaration_become_wreckage() {
    // Zend rejects `#[A] echo 1;` too; the groups keep their structure
    // inside an ErrorNode and the statement parses on its own.
    assert_eq!(
        parser_diagnostics("<?php #[Marker] echo 1;"),
        vec![ParserDiagnosticKind::ExpectedDeclaration]
    );
}

#[test]
fn an_unterminated_group_is_diagnosed_and_recovers() {
    let diagnostics = parser_diagnostics("<?php #[Marker function f() {}");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(
            celerrate_syntax::SyntaxKind::CloseBracket
        )),
        "got {diagnostics:?}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test declarations_attributes`
Expected: FAIL.

- [ ] **Step 3: Add the node kinds**

In `syntax_kind.rs`, append after `PropertyHook`:

```rust
    /// `#[Attribute(arguments), Other]`: one bracketed group.
    AttributeGroup,
    /// One attribute inside a group: a name and optional arguments.
    Attribute,
```

- [ ] **Step 4: Create `attributes.rs`**

```rust
//! Attribute groups: `#[Name(arguments), Other\Name]`. Groups precede
//! declarations, members, enum cases, parameters, property hooks,
//! closures, and anonymous classes; each host rule calls
//! [`attribute_groups`] with its node marker already open, so the
//! groups become leading children of the declaration they decorate.

use crate::syntax_kind::SyntaxKind;

use super::Parser;
use super::expressions::{argument_list, error_element, expect_list_separator, name};

/// Zero or more `#[...]` groups. Progress: every group bumps its `#[`.
pub(super) fn attribute_groups(parser: &mut Parser) {
    while parser.at(SyntaxKind::AttributeOpen) {
        attribute_group(parser);
    }
}

/// One `#[ ... ]` group. Same recovery contract as the expression
/// lists: unexpected tokens are wrapped and consumed; `;`, `?>`, and
/// end of input abort so a runaway group cannot swallow the file.
fn attribute_group(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `#[`
    while !parser.at(SyntaxKind::CloseBracket) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        if !matches!(
            parser.current(),
            Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
        ) {
            error_element(parser);
            continue;
        }
        attribute(parser);
        expect_list_separator(parser, SyntaxKind::CloseBracket);
    }
    parser.expect(SyntaxKind::CloseBracket);
    marker.complete(parser, SyntaxKind::AttributeGroup);
}

/// One attribute: a qualified name and optional arguments. Attribute
/// names are class names, never keywords.
fn attribute(parser: &mut Parser) {
    let marker = parser.start();
    name(parser);
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    }
    marker.complete(parser, SyntaxKind::Attribute);
}
```

In `grammar.rs`, add `mod attributes;`.

- [ ] **Step 5: Route attributes through declarations and members**

In `declarations.rs`:

1. In `declaration`, insert the groups right after the marker opens:

```rust
pub(super) fn declaration(parser: &mut Parser) {
    let marker = parser.start();
    super::attributes::attribute_groups(parser);
    match parser.current() {
        // ... arms unchanged ...
```

The existing fallback arm is what turns `#[A] echo 1;` into diagnosed wreckage; its progress argument (at least the groups were consumed) now becomes real.

2. In `member`, add the attribute arm before the others:

```rust
        Some(SyntaxKind::AttributeOpen) => {
            let marker = parser.start();
            super::attributes::attribute_groups(parser);
            match parser.current() {
                Some(SyntaxKind::Case) => enum_case(parser, marker),
                Some(kind) if starts_member(kind) => modified_member(parser, marker),
                _ => {
                    // Attribute groups with no member behind them: the
                    // groups (consumed, so the list progresses) become
                    // wreckage.
                    parser.diagnose_current(ParserDiagnosticKind::ExpectedDeclaration);
                    marker.complete(parser, SyntaxKind::ErrorNode);
                }
            }
        }
```

3. In `property_hook`, insert `super::attributes::attribute_groups(parser);` as the first line of the function body (before the `final` eat).

In `statements.rs`, add the dispatch arm beside the other declaration arms:

```rust
        Some(SyntaxKind::AttributeOpen) => super::declarations::declaration(parser),
```

- [ ] **Step 6: Route attributes through expressions**

In `expressions.rs`:

1. `closure_or_arrow_function` takes the marker from its callers. Change the signature to

```rust
fn closure_or_arrow_function(parser: &mut Parser, marker: Marker) -> CompletedMarker {
```

delete its `let marker = parser.start();` line, and update the two existing `primary_expression` call sites to `Some(closure_or_arrow_function(parser, parser.start()))`. Add `Marker` to the imports: `use super::{CompletedMarker, Marker, Parser};`.

2. Add the attributed-closure arm to `primary_expression`:

```rust
        // Attributes at expression position decorate a closure or an
        // arrow function; anything else behind them is wreckage (Zend
        // rejects it too).
        Some(SyntaxKind::AttributeOpen) => {
            let marker = parser.start();
            super::attributes::attribute_groups(parser);
            match parser.current() {
                Some(SyntaxKind::Function | SyntaxKind::Fn | SyntaxKind::Static) => {
                    Some(closure_or_arrow_function(parser, marker))
                }
                _ => {
                    parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
                    Some(marker.complete(parser, SyntaxKind::ErrorNode))
                }
            }
        }
```

3. In `starts_expression`, add `| SyntaxKind::AttributeOpen`.

4. In `parameter`, insert `super::attributes::attribute_groups(parser);` as the first line of the body (before `promotion_modifiers`). In `starts_parameter`, add `| SyntaxKind::AttributeOpen`.

5. In `new_expression`, widen the anonymous-class arm to include `SyntaxKind::AttributeOpen` and parse the groups first:

```rust
        Some(
            SyntaxKind::Class
                | SyntaxKind::Readonly
                | SyntaxKind::Final
                | SyntaxKind::Abstract
                | SyntaxKind::AttributeOpen,
        ) => {
            let class_marker = parser.start();
            super::attributes::attribute_groups(parser);
            super::declarations::anonymous_class(parser, class_marker);
        }
```

- [ ] **Step 7: Run the tests, then the full suite**

Run: `cargo test --package celerrate_syntax --test declarations_attributes`
Expected: PASS.

Run: `cargo test --workspace`
Expected: corpus files where `#[...]` lexed into wreckage improve; accept those diffs only.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "✨ feat(syntax): parse attributes"
```

---

### Task 11: Corpus, error corpus, fuzz seeds, changelog

The whole-pipeline safety net: nine corpus files exercising every rule of this plan (the broken one deliberately), two fuzz seeds, a fuzz smoke run, and the changelog entry the statements plan skipped.

**Files:**
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_types.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_top_level.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_class_like.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_members.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_enums.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_hooks.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_attributes.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/declarations_kitchen_sink.php`
- Create: `crates/celerrate_syntax/tests/parse_corpus/recovery_declarations.php`
- Create: `fuzz/corpus/parse/seed_declarations.php`
- Create: `fuzz/corpus/parse/seed_declarations_errors.php`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: the `parse_corpus` glob test (picks new files up automatically) and the existing `parse` fuzz target.
- Produces: nothing for later tasks; this closes the plan.

- [ ] **Step 1: Write the corpus files**

`declarations_types.php`:

```php
<?php

function scalars(int $a, ?string $b, float|bool $c): void {}
function unions(int|string|null $x): int|false {}
function intersections(Countable&ArrayAccess $x): static {}
function dnf((Countable&ArrayAccess)|null $x): (Traversable&Countable)|false {}
function references(A&$x, B &...$rest): never {}
function relative(namespace\Kind $x, \Fully\Qualified $y): parent {}
```

`declarations_top_level.php`:

```php
<?php

namespace App\Domain;

use App\Service;
use App\{Helper as Aid, function helper, const LIMIT,};
use function strlen as length;
use const PHP_EOL, PHP_INT_MAX;

const GREETING = 'hello', ANSWER = 42;
const int CEILING = 10;
const FOR = 'semi-reserved';

namespace App\Other {
    const NESTED = true;
}
```

`declarations_class_like.php`:

```php
<?php

abstract class Base extends Root implements Countable, Stringable {}

final readonly class Value {}

interface Shape extends HasArea, HasPerimeter {}

trait Greets {}

class List {}

$instance = new class(1) extends Base {
    public int $inline = 0;
};

$flag = new readonly class {};

readonly($flag);
```

`declarations_members.php`:

```php
<?php

class Account {
    var $legacy;
    public int $balance = 0, $pending = 0;
    protected static ?self $instance = null;
    final protected const int CEILING = 10;
    const FOR = 'semi-reserved';

    public function __construct() {}
    abstract public function close(): void;
    public function list(): array { return []; }
    public function &reference(): int { return $this->balance; }

    use Greets, Counts {
        Greets::hello insteadof Counts;
        Counts::hello as protected countedHello;
        list as unreserved;
        rename as private;
    }
}
```

`declarations_enums.php`:

```php
<?php

enum Direction {
    case North;
    case South;
}

enum Suit: string implements HasColor {
    case Hearts = 'H';
    case Spades = 'S';

    const WILDCARD = '*';

    public function color(): string {
        return match ($this) {
            Suit::Hearts => 'red',
            Suit::Spades => 'black',
        };
    }
}

enum(1);
```

`declarations_hooks.php`:

```php
<?php

class Person {
    public string $name {
        get => strtoupper($this->name);
        set(string $value) {
            $this->name = trim($value);
        }
    }

    public private(set) DateTimeImmutable $created;

    public array $items {
        final &get { return $this->items; }
    }

    public function __construct(
        public readonly string $first,
        private(set) string $last = 'unknown',
        public string $full { get => $this->first . ' ' . $this->last; },
    ) {}
}

interface Named {
    public string $name { get; set; }
}
```

`declarations_attributes.php`:

```php
<?php

#[Attribute(Attribute::TARGET_CLASS)]
class Route {}

#[Route('/home', methods: ['GET'])]
#[Deprecated]
final class HomeController {
    #[Override]
    public function handle(#[SensitiveParameter] string $token): void {}

    #[Marker]
    const MAPPED = 1;

    #[Marker]
    public int $count = 0;
}

enum Level {
    #[Description('lowest')]
    case Low;
}

$handler = #[Pure] static fn (int $x): int => $x * 2;
$instance = new #[Marker] class {};
```

`declarations_kitchen_sink.php`:

```php
<?php

namespace App;

use App\Contracts\{Countable as Sized, function assert_positive};

#[Entity(table: 'accounts')]
final class Account extends Base implements Sized, \Stringable
{
    use Auditable, Timestamps {
        Auditable::record insteadof Timestamps;
        Timestamps::record as protected recordTime;
    }

    public private(set) int $balance = 0 {
        get => $this->balance;
        set(int $value) { $this->balance = max(0, $value); }
    }

    final public const (Countable&Traversable)|null REGISTRY = null;

    public function __construct(
        #[Id] public readonly string $identifier,
        protected ?self $parent = null,
    ) {}

    abstract protected function audit(): void;

    public function count(): int { return $this->balance; }
}

enum Currency: string
{
    case Euro = 'EUR';

    public function symbol(): string
    {
        return match ($this) { Currency::Euro => '€' };
    }
}
```

`recovery_declarations.php` (deliberately broken; the snapshot is the point):

```php
<?php

class Broken {
    public int = 5;
    function () {}
    const = 3;
    public;
}

interface Half {
    public function signature(): ;
}

enum Unfinished {
    case
}

use App\{Unclosed;
class extends Base {}
abstract 1;
#[Dangling] echo 'still parses';
```

- [ ] **Step 2: Run the corpus test and review the new snapshots**

Run: `cargo test --package celerrate_syntax --test parse_corpus`
Expected: FAIL with new-snapshot messages. Review each `.snap.new` (`cargo insta review`, or read the files): every construct of this plan must appear as its real node kind, the recovery file must show partial nodes plus `ErrorNode`s (never a missing region), and every diagnostic in the footer must point at a plausible offset. Then accept.

- [ ] **Step 3: Add the fuzz seeds**

`fuzz/corpus/parse/seed_declarations.php`:

```php
<?php
namespace A; use B\{C as D, function e}; const int F = 1;
#[G(h: 1)] final readonly class I extends J implements K {
    use L { M::n insteadof O; n as private p; }
    public private(set) ?Q $r = null { get => $this->r; set(R $v) {} }
    final protected const (S&T)|U V = 1;
    abstract public function w(#[X] public readonly Y&Z ...$a): (B&C)|null;
}
enum D2: string { case E2 = 'f'; }
$g = new #[H2] class(1) {}; readonly(enum(1));
```

`fuzz/corpus/parse/seed_declarations_errors.php`:

```php
<?php
class { public int = ; function
interface I { const
enum { case = , }
use A\{ ; #[ class extends { abstract
```

- [ ] **Step 4: Fuzz smoke run**

Run: `cargo +nightly fuzz run parse -- -runs=200000 -max_total_time=120`
Expected: no crash, no hang. If a crash surfaces, minimize (`cargo +nightly fuzz tmin parse <artifact>`), fix under a new test, and re-run before continuing.

- [ ] **Step 5: Run the full suite and the whole gate**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check`
Expected: all PASS.

- [ ] **Step 6: Commit the corpus and seeds**

```bash
git add crates/celerrate_syntax/tests/parse_corpus fuzz/corpus/parse
git add crates/celerrate_syntax/tests/snapshots
git commit -m "✅ test(syntax): extend the parse corpus and fuzz seeds for declarations"
```

- [ ] **Step 7: Update the changelog**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, replace the parser bullet with one covering the now-complete grammar (the statement grammar from plan 3 was never recorded; record both):

```markdown
- The parser covers the full PHP 8.5 grammar: the complete expression
  grammar (Zend precedence table, calls and access chains, `match`,
  closures, the pipe operator), the complete statement grammar (control
  flow in classic and alternative syntax, `try`/`catch`/`finally`,
  inline HTML interruption), and the complete declaration grammar
  (classes with anonymous forms, interfaces, traits, enums, property
  hooks and asymmetric visibility, constructor promotion, union /
  intersection / DNF types, attributes, `const`/`namespace`/`use`).
```

Keep the existing expression bullet's deletion in the same edit (this bullet replaces it).

- [ ] **Step 8: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the statement and declaration grammars"
```

---

## Completion checklist (for the finishing task)

- `cargo fmt --all` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green, `cargo deny check` clean.
- The fuzz smoke run completed without findings.
- Every deferred item from PR #7 is either landed (Tasks 1 and 2) or explicitly recorded as out of scope in this plan's header.
- Branch: this plan was developed on `foundations-4-parser-4-declarations`; finish with superpowers:finishing-a-development-branch (PR to `main`, as the previous plans did).
- Items to carry to plan 5 (typed AST), to record in the PR description: the node-kind list in `syntax_kind.rs` is now 30 kinds richer and fully hand-maintained — plan 5's `php.ungram` takes ownership of all of them; the bare-`CloseTag` typed-AST note from plan 3 still stands; `NamedType` holds either a `Name` node or a bare keyword token, which the generated accessors must expose as one concept.
