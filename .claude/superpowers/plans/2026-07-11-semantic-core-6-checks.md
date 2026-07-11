# Semantic Core Part 6: Checks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The preview's two diagnostic families — unknown top-level symbols
and version gating (syntax constructs and stub symbols) — as per-file salsa
queries in `celerrate_semantics`, with their permanent `CEL####`
identifiers and the rendered message joining the shared diagnostic model.

**Architecture:** Per the spec
`.claude/superpowers/specs/2026-07-11-semantic-core-design.md` (section 7)
and the parent spec's version range rule (availability at **min**, removal
and deprecation at **max**). Decisions fixed here:

- **The message joins the shared model now.** `Diagnostic` gains a
  `message: String` field (section 7: the parameterized detail a renderer
  needs joins the shared model when the preview renderer consumes it,
  parts 6 and 7). `Diagnostic` loses `Copy` (keeps `Clone`); the manual
  `Ord` gains `message` as the final tie-break. Lexer and parser kinds get
  short literal messages in `celerrate_syntax`; `CEL0001` gets its message
  in `celerrate_db`. The rich anatomy (annotated spans, notes,
  suggestions) remains sub-project 4.
- **Identifier allocation, permanent once merged:** `CEL0018` unknown
  class, `CEL0019` unknown function, `CEL0020` unknown constant,
  `CEL0021` symbol not available at the range minimum, `CEL0022` symbol
  removed within the range, `CEL0023` symbol deprecated (severity
  Warning; all others Error), `CEL0024` syntax construct not available at
  the range minimum. `CEL0001`–`CEL0017` are already taken by
  `celerrate_db` and `celerrate_syntax`.
- **The checks live in `celerrate_semantics`.** They need `resolve_name`,
  the stub availability metadata, and `ProjectConfiguration` — all already
  dependencies of this crate. The rule framework (`celerrate_rules`) is
  later sub-projects; the preview queries are plain tracked functions the
  CLI (part 7) will fan out over.
- **One resolution pass, two families.** `reference_diagnostics` collects
  the file's statically named references once and resolves each: no
  resolution produces the unknown-symbol family; a
  `SymbolResolution::Stub` answer carries `StubAvailability`, which
  produces the symbol version-gating family in the same pass. A
  `SymbolResolution::Source` answer is never gated (project code has no
  availability metadata).
- **Reference collection is a pure function, not a query.**
  `collect_references(root)` walks the file's own syntax tree — the
  design's deliberate boundary exception: a per-file output may read its
  own tree, never another file's. It carries ranges, so it re-runs on
  every edit of its file; the per-name lookup firewall (part 5) is what
  spares other files.
- **Skip lists are engine semantics, not scope workarounds** (the
  anti-false-positive policy applied at collection time):
  - Relative class names `self`, `parent`, `static` (ASCII
    case-insensitive) are never class references. (`static` is a keyword
    and never surfaces as a `Name`; it stays in the list as cheap armor.)
  - Built-in type names in type positions (`array` and `callable` are
    keywords and never reach the check; the rest lex as identifiers):
    `bool`, `false`, `float`, `int`, `iterable`, `mixed`, `never`,
    `null`, `object`, `parent`, `self`, `string`, `true`, `void`.
  - Language constants `true`, `false`, `null` and the magic constants
    `__LINE__`, `__FILE__`, `__DIR__`, `__FUNCTION__`, `__CLASS__`,
    `__TRAIT__`, `__METHOD__`, `__NAMESPACE__`, `__PROPERTY__` (all ASCII
    case-insensitive, compared after trimming one leading `\` and only
    when no other `\` remains) are never constant references.
  - Dynamic references (`new $class`, `$x::`, call-by-string, `new
    (expression)`) are out of scope — documented engine semantics.
- **The `stubs_in_range` interplay is documented behavior:** a stub
  symbol that exists nowhere in the project's range is filtered out of
  the stub table (part 5), so a reference to it reports *unknown symbol*,
  not a gating diagnostic. Gating fires only for symbols that exist
  somewhere in the range but not everywhere (`introduced > minimum`,
  `removed <= maximum`) or are deprecated by `maximum`.
- **`clone($x)` with one positional argument is not gated.** Pre-8.5 PHP
  reads it as `clone` of a parenthesized expression, so gating it would
  be a false positive. Only two-or-more arguments or a named argument
  (`clone($object, [...])`, `clone(object: $o)`) mark the 8.5 clone-with
  form.
- **The construct table is data-shaped for growth:** one detection walk
  produces `(label, required version, range)` triples; adding a construct
  is one match arm. All gated constructs share `CEL0024` — the identifier
  names the problem class, the message carries the construct.

Out of scope (later parts): the CLI, rendering, `--watch`, panic
isolation (part 7); the persistent cache, the Symfony corpus in CI, LRU
eviction (part 8); member existence, reachability of conditional
declarations, dynamic references, `celerrate explain`, suggestions
(later sub-projects).

**Tech Stack:** Rust edition 2024, salsa 0.27 (existing workspace
dependency). Existing crates and APIs consumed:
`celerrate_diagnostics` (`Diagnostic`, `DiagnosticId`, `Severity`),
`celerrate_db` (`SourceFile`, `AnalyzedFileSet`, `parse`,
`file_diagnostics`, instrumented `testing::TestDatabase`),
`celerrate_syntax` (`ast::*`, `SyntaxKind`, `SyntaxNode`, `parse`),
`celerrate_project` (`ProjectConfiguration`, `PhpVersion`,
`PhpVersionRange`), `celerrate_stubs` (`StubIndexInput`,
`StubAvailability`, `StubSymbol`, `StubSymbolKind`, `StubIndex`,
`embedded_stub_index`), `celerrate_semantics` (`item_tree`, `UseTables`,
`SymbolSources`, `resolve_name`, `SymbolResolution`, `SymbolSpace`).
No new external dependencies anywhere.

## Global Constraints

- Zero panic, mechanically enforced: the workspace denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` is forbidden.
  Only test modules may `#[allow]` / `#![allow]` these lints. Production
  code uses `.get()`, iterator adapters, `unwrap_or_default`, never
  indexing.
- Strict layering, DAG with no upward edges: `celerrate_semantics` gains
  one dependency, `celerrate_diagnostics` (below it in the parent spec's
  layout). No other crate gains dependencies.
- Error resilience: malformed input produces whatever references and
  constructs the tree carries; empty or wreckage names are skipped, never
  failures.
- Determinism: every query output is sorted by the `Diagnostic` `Ord`;
  no wall-clock time, no randomness, no environment reads inside queries.
- Diagnostic identifiers are permanent once merged: never renumber
  `CEL0018`–`CEL0024`.
- TDD throughout: every step of behavior starts from a failing test.
- Everything in English, full words, no abbreviated names (standard
  acronyms fine).
- Commits: gitmoji + Conventional Commits, repository-configured
  identity, no Claude attribution anywhere.
- Local commands that must stay green after every task:
  `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `cargo deny check`.

## File Structure

```
crates/celerrate_diagnostics/src/diagnostic.rs    message field + Ord (task 1)
crates/celerrate_syntax/src/diagnostic.rs         kind messages, to_diagnostic (task 1)
crates/celerrate_db/src/queries.rs                CEL0001 message (task 1)
crates/celerrate_semantics/Cargo.toml             + celerrate_diagnostics (task 5)
crates/celerrate_semantics/src/references.rs      Reference, collect_references (tasks 2-4)
crates/celerrate_semantics/src/reference_checks.rs CEL0018-0023, reference_diagnostics (tasks 5-6)
crates/celerrate_semantics/src/syntax_gating.rs   CEL0024, syntax_version_diagnostics (tasks 7-8)
crates/celerrate_semantics/src/queries.rs         semantic_diagnostics (task 9)
crates/celerrate_semantics/src/lib.rs             module + export additions (tasks 2, 5, 7, 9)
crates/celerrate_semantics/tests/invalidation_scope.rs      checks scope tests (task 10)
crates/celerrate_semantics/tests/incremental_consistency.rs checks in the harness (task 11)
crates/celerrate_semantics/tests/false_positives.rs         smoke corpus (task 11)
```

---

### Task 1: The message joins the shared diagnostic model

**Files:**
- Modify: `crates/celerrate_diagnostics/src/diagnostic.rs`
- Modify: `crates/celerrate_syntax/src/diagnostic.rs`
- Modify: `crates/celerrate_db/src/queries.rs`
- Fix fallout in: `crates/celerrate_db/src/testing.rs`, any test
  constructing a `Diagnostic` literal or copying one (compiler-guided).

**Interfaces:**
- Consumes: the existing `Diagnostic { id, severity, file, range }`,
  `SyntaxDiagnostic::to_diagnostic(file)`, `SOURCE_TOO_LARGE`.
- Produces: `Diagnostic { id, severity, file, range, message: String }`
  (`Clone`, **not** `Copy`; `Ord` tie-break ends on `message`), and
  `SyntaxDiagnostic::message(&self) -> String`. Every later task
  constructs diagnostics with all five fields.

- [ ] **Step 1: Write the failing tests**

In `crates/celerrate_syntax/src/diagnostic.rs`, extend the existing test
module (or create one following the crate's test style):

```rust
#[test]
fn a_missing_semicolon_projects_its_message() {
    let parse = crate::parse("<?php $x = 1");
    let messages: Vec<String> = parse
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.to_diagnostic(FileId::new(0)).message)
        .collect();
    assert!(
        messages.contains(&"expected `;`".to_owned()),
        "messages: {messages:?}",
    );
}

#[test]
fn every_kind_has_a_non_empty_message() {
    let parse = crate::parse("<?php class { $");
    for diagnostic in parse.diagnostics() {
        assert!(!diagnostic.to_diagnostic(FileId::new(0)).message.is_empty());
    }
}
```

In `crates/celerrate_diagnostics/src/diagnostic.rs`, add to the test
module a deterministic-ordering check:

```rust
#[test]
fn the_message_is_the_final_ordering_tie_break() {
    let first = Diagnostic {
        id: DiagnosticId::new("CEL9999"),
        severity: Severity::Error,
        file: FileId::new(0),
        range: TextRange::empty(0.into()),
        message: "alpha".to_owned(),
    };
    let second = Diagnostic { message: "beta".to_owned(), ..first.clone() };
    assert!(first < second);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --package celerrate_diagnostics`
Expected: FAIL to compile — no field `message` on `Diagnostic`.

- [ ] **Step 3: Implement**

`crates/celerrate_diagnostics/src/diagnostic.rs` — add the field (keep
existing doc-comment style; drop `Copy` from the derive; keep the rest):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: DiagnosticId,
    pub severity: Severity,
    pub file: FileId,
    pub range: TextRange,
    /// The rendered one-sentence message, parameterized by the producer
    /// (the written name, the required version). The rich anatomy —
    /// annotated spans, notes, suggestions — is sub-project 4.
    pub message: String,
}
```

Extend the manual `Ord` so `message` is the final tie-break:

```rust
impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.file,
            self.range.start(),
            self.range.end(),
            self.id,
            self.severity,
            &self.message,
        )
            .cmp(&(
                other.file,
                other.range.start(),
                other.range.end(),
                other.id,
                other.severity,
                &other.message,
            ))
    }
}
```

`crates/celerrate_syntax/src/diagnostic.rs` — message tables and the
projection:

```rust
impl LexerDiagnosticKind {
    fn message(self) -> String {
        match self {
            Self::UnexpectedCharacter => "unexpected character".to_owned(),
            Self::UnterminatedBlockComment => "unterminated block comment".to_owned(),
            Self::UnterminatedString => "unterminated string literal".to_owned(),
            Self::UnterminatedHeredoc => "unterminated heredoc".to_owned(),
            Self::UnterminatedInterpolation => "unterminated string interpolation".to_owned(),
        }
    }
}

impl ParserDiagnosticKind {
    fn message(self) -> String {
        match self {
            Self::ExpectedExpression => "expected an expression".to_owned(),
            Self::ExpectedSemicolon => "expected `;`".to_owned(),
            Self::Expected(kind) => format!("expected {kind:?}"),
            Self::UnexpectedToken => "unexpected token".to_owned(),
            Self::NestingTooDeep => "the input nests too deeply".to_owned(),
            Self::NonAssociativeOperator => {
                "non-associative operators cannot be chained".to_owned()
            }
            Self::NoProgress => "unable to interpret the input here".to_owned(),
            Self::ExpectedMemberName => "expected a member name".to_owned(),
            Self::ExpectedStatement => "expected a statement".to_owned(),
            Self::ExpectedType => "expected a type".to_owned(),
            Self::ExpectedDeclaration => "expected a declaration".to_owned(),
        }
    }
}

impl SyntaxDiagnostic {
    /// The rendered message of this finding.
    pub fn message(&self) -> String {
        match self.kind {
            SyntaxDiagnosticKind::Lexer(kind) => kind.message(),
            SyntaxDiagnosticKind::Parser(kind) => kind.message(),
        }
    }
}
```

and in `to_diagnostic`, add `message: self.message(),`.

`crates/celerrate_db/src/queries.rs` — in `file_diagnostics`, the
`SOURCE_TOO_LARGE` construction gains
`message: "the file exceeds the 4 GiB source size limit".to_owned(),`.

- [ ] **Step 4: Fix the workspace fallout**

Run: `cargo check --workspace --all-targets`
Fix every error: `Diagnostic` literals in tests gain a `message` field
(assert on it where the test's intent covers it, otherwise construct it);
places that moved a `Copy` diagnostic now `.clone()`.

- [ ] **Step 5: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "✨ feat(diagnostics): carry the rendered message in the shared model"
```

---

### Task 2: Reference collection — class-like sites

**Files:**
- Create: `crates/celerrate_semantics/src/references.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (add
  `mod references;` and `pub use references::{Reference, collect_references};`)

**Interfaces:**
- Consumes: `celerrate_syntax::ast` typed nodes, `SymbolSpace` from
  `crate::symbols`, the namespace-walk rules of `crate::item_nodes`
  (mirrored, not imported: this walk descends everywhere, including
  member lists).
- Produces (used by tasks 3-5):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub written: String,      // as typed, qualifiers preserved
    pub space: SymbolSpace,
    pub namespace: String,    // enclosing namespace, "" is global
    pub range: TextRange,     // the Name node's range
}

pub fn collect_references(root: &SyntaxNode) -> Vec<Reference>  // tree order
```

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_semantics/src/references.rs` with the tests
first (module skeleton compiles, collection returns nothing yet):

```rust
#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use celerrate_syntax::parse;

    fn collected(source: &str) -> Vec<(String, SymbolSpace, String)> {
        collect_references(&parse(source).tree())
            .into_iter()
            .map(|reference| (reference.written, reference.space, reference.namespace))
            .collect()
    }

    fn class_like(written: &str, namespace: &str) -> (String, SymbolSpace, String) {
        (written.to_owned(), SymbolSpace::ClassLike, namespace.to_owned())
    }

    #[test]
    fn a_new_expression_references_its_class() {
        assert_eq!(
            collected("<?php namespace App; $x = new Client();"),
            vec![class_like("Client", "App")],
        );
    }

    #[test]
    fn inheritance_clauses_reference_every_name() {
        assert_eq!(
            collected("<?php class A extends B implements C, D {}"),
            vec![class_like("B", ""), class_like("C", ""), class_like("D", "")],
        );
    }

    #[test]
    fn trait_use_catch_and_attributes_reference_classes() {
        assert_eq!(
            collected(
                "<?php #[Route] class A { use Loggable; } \
                 try {} catch (NotFound|\\Lib\\Denied $error) {}",
            ),
            vec![
                class_like("Route", ""),
                class_like("Loggable", ""),
                class_like("NotFound", ""),
                class_like("\\Lib\\Denied", ""),
            ],
        );
    }

    #[test]
    fn relative_class_names_are_not_references() {
        assert_eq!(
            collected(
                "<?php class A extends B {
                    function f() { new self(); new static(); return new parent(); }
                }",
            ),
            vec![class_like("B", "")],
        );
    }

    #[test]
    fn brace_form_namespaces_scope_their_references() {
        assert_eq!(
            collected("<?php namespace A { new X(); } namespace B { new Y(); }"),
            vec![class_like("X", "A"), class_like("Y", "B")],
        );
    }

    #[test]
    fn anonymous_class_inheritance_is_still_referenced() {
        assert_eq!(
            collected("<?php $x = new class extends Base {};"),
            vec![class_like("Base", "")],
        );
    }

    #[test]
    fn wreckage_names_are_skipped() {
        // Error-recovery trees never produce empty written names.
        for reference in collect_references(&parse("<?php new ; class extends {}").tree()) {
            assert!(!reference.written.is_empty());
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics references`
Expected: FAIL to compile (`collect_references` undefined), then after the
skeleton lands, assertion failures on empty vectors.

- [ ] **Step 3: Implement**

The walk mirrors `item_nodes.rs`'s namespace rules (statement-form
switches state, brace form scopes its block) but descends into every
child, member lists and bodies included:

```rust
//! The statically named references of one file, collected from its own
//! syntax tree — the design's deliberate boundary exception: a per-file
//! output may read its own tree, never another file's. Dynamic
//! references (`new $class`, call-by-string) are out of scope by
//! documented engine semantics, as are the relative class names
//! `self`, `parent`, `static`.

use celerrate_source::TextRange;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

use crate::symbols::SymbolSpace;

/// One statically named reference, as typed, with its resolution
/// context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub written: String,
    pub space: SymbolSpace,
    pub namespace: String,
    pub range: TextRange,
}

/// Every statically named reference of the file, in tree order.
pub fn collect_references(root: &SyntaxNode) -> Vec<Reference> {
    let mut references = Vec::new();
    let mut namespace = String::new();
    collect(root, &mut namespace, &mut references);
    references
}

fn collect(node: &SyntaxNode, namespace: &mut String, references: &mut Vec<Reference>) {
    for child in node.children() {
        if child.kind() == SyntaxKind::NamespaceDeclaration {
            let Some(declaration) = ast::NamespaceDeclaration::cast(child.clone()) else {
                continue;
            };
            let declared = declaration
                .name()
                .map(|name| name.text())
                .unwrap_or_default();
            match declaration.block() {
                Some(block) => {
                    let mut inner = declared;
                    collect(block.syntax(), &mut inner, references);
                }
                None => *namespace = declared,
            }
            continue;
        }
        visit(&child, namespace, references);
        collect(&child, namespace, references);
    }
}

/// The reference sites of one node; the descent happens in `collect`.
fn visit(node: &SyntaxNode, namespace: &str, references: &mut Vec<Reference>) {
    match node.kind() {
        SyntaxKind::NewExpression => {
            if let Some(expression) = ast::NewExpression::cast(node.clone()) {
                if let Some(name) = expression.name() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::ExtendsClause => {
            if let Some(clause) = ast::ExtendsClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::ImplementsClause => {
            if let Some(clause) = ast::ImplementsClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::TraitUseClause => {
            if let Some(clause) = ast::TraitUseClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::CatchClause => {
            if let Some(clause) = ast::CatchClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::Attribute => {
            if let Some(attribute) = ast::Attribute::cast(node.clone()) {
                if let Some(name) = attribute.name() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        _ => {}
    }
}

fn push_class_like(name: &ast::Name, namespace: &str, references: &mut Vec<Reference>) {
    let written = name.text();
    if written.is_empty() || is_relative_class_name(&written) {
        return;
    }
    references.push(Reference {
        written,
        space: SymbolSpace::ClassLike,
        namespace: namespace.to_owned(),
        range: name.syntax().text_range(),
    });
}

/// `self`, `parent`, `static`: resolved against the enclosing class,
/// never against the symbol index.
fn is_relative_class_name(written: &str) -> bool {
    ["self", "parent", "static"]
        .iter()
        .any(|relative| written.eq_ignore_ascii_case(relative))
}
```

In `lib.rs`, add `mod references;` and
`pub use references::{Reference, collect_references};` in the existing
alphabetical style.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics references`
Expected: PASS.

- [ ] **Step 5: Run the full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`

```bash
git add -A
git commit -m "✨ feat(semantics): collect class-like references with namespace context"
```

---

### Task 3: Reference collection — type positions

**Files:**
- Modify: `crates/celerrate_semantics/src/references.rs`

**Interfaces:**
- Consumes: `ast::NamedType::name_or_keyword()` returning
  `NamedTypeName::{Name, Keyword}` (keywords — `array`, `callable`,
  `static` — never reach the name path).
- Produces: `NamedType` names as `SymbolSpace::ClassLike` references,
  minus the built-in type list; `collect_references` signature unchanged.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn type_positions_reference_class_names() {
    assert_eq!(
        collected(
            "<?php function f(?Request $request, int|string $count, (Left&Right)|null $union): Response {}
             class C { public UserId $id; }",
        ),
        vec![
            class_like("Request", ""),
            class_like("Left", ""),
            class_like("Right", ""),
            class_like("Response", ""),
            class_like("UserId", ""),
        ],
    );
}

#[test]
fn built_in_type_names_are_not_references() {
    assert_eq!(
        collected(
            "<?php function f(int $a, float $b, string $c, bool $d, mixed $e,
                 iterable $f, object $g, array $h, callable $i, self $j,
                 null|false|true $k): void {}
             enum Suit: string {}",
        ),
        vec![],
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_semantics references`
Expected: FAIL — type names are not collected yet.

- [ ] **Step 3: Implement**

Add to the `visit` match:

```rust
SyntaxKind::NamedType => {
    if let Some(named) = ast::NamedType::cast(node.clone()) {
        if let Some(ast::NamedTypeName::Name(name)) = named.name_or_keyword() {
            let written = name.text();
            if !is_built_in_type_name(&written) {
                push_class_like(&name, namespace, references);
            }
        }
    }
}
```

(`push_class_like` recomputes `text()`; accept the second call or thread
the string — match the crate's plain style, no premature cleverness.)

And the list (`array`, `callable`, `static` are keywords and arrive as
`NamedTypeName::Keyword`, listed here only as documentation):

```rust
/// Type names PHP resolves without the symbol table. `array`,
/// `callable`, and `static` are lexer keywords and never surface as
/// `Name` nodes; the rest lex as plain identifiers.
fn is_built_in_type_name(written: &str) -> bool {
    [
        "bool", "false", "float", "int", "iterable", "mixed", "never",
        "null", "object", "parent", "self", "string", "true", "void",
    ]
    .iter()
    .any(|built_in| written.eq_ignore_ascii_case(built_in))
}
```

- [ ] **Step 4: Run tests, full gate, commit**

Run: `cargo test --package celerrate_semantics references`, then the full
gate.

```bash
git add -A
git commit -m "✨ feat(semantics): collect type-position class references"
```

---

### Task 4: Reference collection — calls, constants, instanceof, scoped subjects

**Files:**
- Modify: `crates/celerrate_semantics/src/references.rs`

**Interfaces:**
- Consumes: `ast::NameExpression` (`name()`, `static_keyword_token()`),
  `ast::CallExpression::callee()`, `ast::ScopedAccessExpression::subject()`,
  `ast::BinaryExpression` (`operator_token()`, `rhs()` — child index 1),
  `SyntaxKind::InstanceOf`.
- Produces: `NameExpression` classification — callee of a call is a
  `Function` reference, subject of `::` and right-hand side of
  `instanceof` are `ClassLike`, everything else is a `Constant`
  reference; `collect_references` signature unchanged.

- [ ] **Step 1: Write the failing tests**

```rust
fn function_reference(written: &str, namespace: &str) -> (String, SymbolSpace, String) {
    (written.to_owned(), SymbolSpace::Function, namespace.to_owned())
}

fn constant_reference(written: &str, namespace: &str) -> (String, SymbolSpace, String) {
    (written.to_owned(), SymbolSpace::Constant, namespace.to_owned())
}

#[test]
fn a_call_references_a_function() {
    assert_eq!(
        collected("<?php namespace App; strlen($x); \\count($y); inner(outer());"),
        vec![
            function_reference("strlen", "App"),
            function_reference("\\count", "App"),
            function_reference("inner", "App"),
            function_reference("outer", "App"),
        ],
    );
}

#[test]
fn a_scoped_subject_references_its_class() {
    assert_eq!(
        collected("<?php Status::Open; Config::class; Client::create(); $x::CONST;"),
        vec![
            class_like("Status", ""),
            class_like("Config", ""),
            class_like("Client", ""),
        ],
    );
}

#[test]
fn an_instanceof_right_hand_side_is_a_class() {
    assert_eq!(
        collected("<?php $ok = $x instanceof Comparable;"),
        vec![class_like("Comparable", "")],
    );
}

#[test]
fn a_bare_name_is_a_constant_reference() {
    assert_eq!(
        collected("<?php $a = PHP_EOL; $b = Config\\LIMIT;"),
        vec![
            constant_reference("PHP_EOL", ""),
            constant_reference("Config\\LIMIT", ""),
        ],
    );
}

#[test]
fn language_and_magic_constants_are_not_references() {
    assert_eq!(
        collected(
            "<?php $a = true; $b = FALSE; $c = null; $d = \\true;
             $e = __DIR__; $f = __class__; $g = __NAMESPACE__;",
        ),
        vec![],
    );
}

#[test]
fn relative_and_dynamic_subjects_are_not_references() {
    assert_eq!(
        collected("<?php self::f(); parent::g(); static::h(); $x instanceof self;"),
        vec![],
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_semantics references`
Expected: FAIL — `NameExpression` is not classified yet.

- [ ] **Step 3: Implement**

Add to the `visit` match:

```rust
SyntaxKind::NameExpression => {
    if let Some(expression) = ast::NameExpression::cast(node.clone()) {
        visit_name_expression(&expression, namespace, references);
    }
}
```

and the classification:

```rust
/// The role of a `NameExpression`, decided by its parent: the callee
/// of a call is a function reference, the subject of `::` and the
/// right-hand side of `instanceof` are class references, everything
/// else is a constant fetch.
enum NameExpressionRole {
    Callee,
    ClassSubject,
    ConstantFetch,
}

fn visit_name_expression(
    expression: &ast::NameExpression,
    namespace: &str,
    references: &mut Vec<Reference>,
) {
    if expression.static_keyword_token().is_some() {
        return;
    }
    let Some(name) = expression.name() else {
        return;
    };
    match role_of(expression.syntax()) {
        NameExpressionRole::Callee => {
            let written = name.text();
            if written.is_empty() {
                return;
            }
            references.push(Reference {
                written,
                space: SymbolSpace::Function,
                namespace: namespace.to_owned(),
                range: name.syntax().text_range(),
            });
        }
        NameExpressionRole::ClassSubject => push_class_like(&name, namespace, references),
        NameExpressionRole::ConstantFetch => {
            let written = name.text();
            if written.is_empty() || is_language_constant(&written) {
                return;
            }
            references.push(Reference {
                written,
                space: SymbolSpace::Constant,
                namespace: namespace.to_owned(),
                range: name.syntax().text_range(),
            });
        }
    }
}

fn role_of(node: &SyntaxNode) -> NameExpressionRole {
    let Some(parent) = node.parent() else {
        return NameExpressionRole::ConstantFetch;
    };
    match parent.kind() {
        SyntaxKind::CallExpression => {
            let is_callee = ast::CallExpression::cast(parent)
                .and_then(|call| call.callee())
                .is_some_and(|callee| callee.syntax() == node);
            if is_callee {
                NameExpressionRole::Callee
            } else {
                NameExpressionRole::ConstantFetch
            }
        }
        SyntaxKind::ScopedAccessExpression => {
            let is_subject = ast::ScopedAccessExpression::cast(parent)
                .and_then(|access| access.subject())
                .is_some_and(|subject| subject.syntax() == node);
            if is_subject {
                NameExpressionRole::ClassSubject
            } else {
                NameExpressionRole::ConstantFetch
            }
        }
        SyntaxKind::BinaryExpression => {
            let is_instanceof_right_hand_side = ast::BinaryExpression::cast(parent)
                .filter(|binary| {
                    binary
                        .operator_token()
                        .is_some_and(|operator| operator.kind() == SyntaxKind::InstanceOf)
                })
                .and_then(|binary| binary.rhs())
                .is_some_and(|right| right.syntax() == node);
            if is_instanceof_right_hand_side {
                NameExpressionRole::ClassSubject
            } else {
                NameExpressionRole::ConstantFetch
            }
        }
        _ => NameExpressionRole::ConstantFetch,
    }
}

/// `true`, `false`, `null`, and the magic constants: language-defined,
/// never symbol-table lookups. Compared after trimming one leading `\`
/// (`\true` is the same literal), only for single-segment names.
fn is_language_constant(written: &str) -> bool {
    let unqualified = written.strip_prefix('\\').unwrap_or(written);
    if unqualified.contains('\\') {
        return false;
    }
    [
        "true", "false", "null", "__LINE__", "__FILE__", "__DIR__",
        "__FUNCTION__", "__CLASS__", "__TRAIT__", "__METHOD__",
        "__NAMESPACE__", "__PROPERTY__",
    ]
    .iter()
    .any(|constant| unqualified.eq_ignore_ascii_case(constant))
}
```

Note: `push_class_like`'s relative-name skip covers `self::f()` and
`instanceof self`; `static::h()` never surfaces (keyword). `exit`/`die`
are keywords with their own expression node and never reach here.

- [ ] **Step 4: Run tests, full gate, commit**

```bash
git add -A
git commit -m "✨ feat(semantics): classify call, constant, and scoped references"
```

---

### Task 5: Unknown-symbol diagnostics (CEL0018–CEL0020)

**Files:**
- Create: `crates/celerrate_semantics/src/reference_checks.rs`
- Modify: `crates/celerrate_semantics/Cargo.toml` (add
  `celerrate_diagnostics = { path = "../celerrate_diagnostics" }`)
- Modify: `crates/celerrate_semantics/src/lib.rs` (add
  `mod reference_checks;` and `pub use reference_checks::{UNKNOWN_CLASS,
  UNKNOWN_CONSTANT, UNKNOWN_FUNCTION, reference_diagnostics};`)

**Interfaces:**
- Consumes: `collect_references` (task 2-4), `item_tree`,
  `UseTables::for_namespace(tree, namespace)`, `SymbolSources`,
  `resolve_name(db, sources, namespace, tables, written, space)`,
  `SymbolResolution`, `Diagnostic` with `message` (task 1),
  `file.file_id(db)`.
- Produces (task 6 extends the same query; tasks 9-11 call it):

```rust
pub const UNKNOWN_CLASS: DiagnosticId = DiagnosticId::new("CEL0018");
pub const UNKNOWN_FUNCTION: DiagnosticId = DiagnosticId::new("CEL0019");
pub const UNKNOWN_CONSTANT: DiagnosticId = DiagnosticId::new("CEL0020");

#[salsa::tracked(returns(ref))]
pub fn reference_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic>
```

Messages: `` unknown class `X` `` / `` unknown function `X` `` /
`` unknown constant `X` `` with the written name; severity `Error`.

- [ ] **Step 1: Write the failing tests**

In `reference_checks.rs`, a test module mirroring `resolve.rs`'s
`sources_of` idiom:

```rust
#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind};

    fn stub(name: &str, kind: StubSymbolKind) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability: StubAvailability::ALWAYS,
        }
    }

    /// The diagnostics of the FIRST source, with the given stubs and
    /// the full supported range.
    fn checked(sources: &[&str], stub_symbols: Vec<StubSymbol>) -> Vec<Diagnostic> {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let file = *handles.first().unwrap();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(stub_symbols))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        reference_diagnostics(&db, file, files, stubs, configuration).clone()
    }

    #[test]
    fn an_unresolved_class_is_reported_at_its_written_name() {
        let source = "<?php namespace App; $x = new Client();";
        let diagnostics = checked(&[source], vec![]);
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostic.id, UNKNOWN_CLASS);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.message, "unknown class `Client`");
        let start: usize = diagnostic.range.start().into();
        let end: usize = diagnostic.range.end().into();
        assert_eq!(&source[start..end], "Client");
    }

    #[test]
    fn a_declaration_anywhere_in_the_file_set_counts() {
        assert_eq!(
            checked(
                &[
                    "<?php namespace App; use Lib\\Helper; $x = new Helper();",
                    "<?php namespace Lib; class Helper {}",
                ],
                vec![],
            ),
            vec![],
        );
    }

    #[test]
    fn a_stub_declaration_counts() {
        assert_eq!(
            checked(
                &["<?php $x = strlen('a'); $t = new \\ArrayObject();"],
                vec![
                    stub("strlen", StubSymbolKind::Function),
                    stub("ArrayObject", StubSymbolKind::Class),
                ],
            ),
            vec![],
        );
    }

    #[test]
    fn an_unresolved_alias_reports_the_written_name() {
        let diagnostics = checked(&["<?php use Lib\\Missing as M; $x = new M();"], vec![]);
        assert_eq!(diagnostics.first().unwrap().message, "unknown class `M`");
    }

    #[test]
    fn functions_fall_back_to_the_global_namespace() {
        let diagnostics = checked(
            &["<?php namespace App; strlen('a'); missing('b');"],
            vec![stub("strlen", StubSymbolKind::Function)],
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics.first().unwrap().id, UNKNOWN_FUNCTION);
        assert_eq!(diagnostics.first().unwrap().message, "unknown function `missing`");
    }

    #[test]
    fn constant_terminal_segments_stay_case_sensitive() {
        let diagnostics = checked(
            &["<?php $a = PHP_EOL; $b = php_eol;"],
            vec![stub("PHP_EOL", StubSymbolKind::Constant)],
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics.first().unwrap().id, UNKNOWN_CONSTANT);
        assert_eq!(diagnostics.first().unwrap().message, "unknown constant `php_eol`");
    }

    #[test]
    fn a_conditionally_declared_symbol_counts_as_declared() {
        assert_eq!(
            checked(
                &["<?php if (!function_exists('helper')) { function helper() {} } helper();"],
                vec![stub("function_exists", StubSymbolKind::Function)],
            ),
            vec![],
        );
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_semantics reference_checks`
Expected: FAIL to compile (`reference_diagnostics` undefined).

- [ ] **Step 3: Implement**

```rust
//! The unknown-symbol family: every statically named reference of one
//! file, resolved; an unresolved reference is a diagnostic. Two
//! conservative stances are documented engine semantics: dynamic
//! references are out of scope, and a symbol declared anywhere in
//! project, vendor, or stubs counts as declared — no reachability
//! analysis of conditional declarations.

use std::collections::HashMap;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::lookup::SymbolResolution;
use crate::queries::item_tree;
use crate::references::{Reference, collect_references};
use crate::resolve::{SymbolSources, UseTables, resolve_name};
use crate::symbols::SymbolSpace;

/// A class-like reference that resolves to no declaration.
pub const UNKNOWN_CLASS: DiagnosticId = DiagnosticId::new("CEL0018");
/// A function call that resolves to no declaration.
pub const UNKNOWN_FUNCTION: DiagnosticId = DiagnosticId::new("CEL0019");
/// A constant reference that resolves to no declaration.
pub const UNKNOWN_CONSTANT: DiagnosticId = DiagnosticId::new("CEL0020");

/// The per-file reference diagnostics: unknown symbols now, the symbol
/// version-gating family joins in the same pass (task 6).
#[salsa::tracked(returns(ref))]
pub fn reference_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let sources = SymbolSources { files, stubs, configuration };
    let tree = item_tree(db, file);
    let root = celerrate_db::parse(db, file).tree();
    let file_id = file.file_id(db);
    let mut tables_by_namespace: HashMap<String, UseTables> = HashMap::new();
    let mut diagnostics = Vec::new();
    for reference in collect_references(&root) {
        let tables = tables_by_namespace
            .entry(reference.namespace.clone())
            .or_insert_with(|| UseTables::for_namespace(tree, &reference.namespace));
        match resolve_name(
            db,
            sources,
            &reference.namespace,
            tables,
            &reference.written,
            reference.space,
        ) {
            None => diagnostics.push(unknown_symbol(&reference, file_id)),
            Some(SymbolResolution::Stub { .. }) => {}
            Some(SymbolResolution::Source { .. }) => {}
        }
    }
    diagnostics.sort();
    diagnostics
}

fn unknown_symbol(reference: &Reference, file: celerrate_source::FileId) -> Diagnostic {
    let (id, kind) = match reference.space {
        SymbolSpace::ClassLike => (UNKNOWN_CLASS, "class"),
        SymbolSpace::Function => (UNKNOWN_FUNCTION, "function"),
        SymbolSpace::Constant => (UNKNOWN_CONSTANT, "constant"),
    };
    Diagnostic {
        id,
        severity: Severity::Error,
        file,
        range: reference.range,
        message: format!("unknown {kind} `{}`", reference.written),
    }
}
```

Add the `celerrate_diagnostics` path dependency to
`crates/celerrate_semantics/Cargo.toml` (alphabetical order) and the
module/exports to `lib.rs`.

- [ ] **Step 4: Run tests, full gate, commit**

```bash
git add -A
git commit -m "✨ feat(semantics): report unknown top-level symbols"
```

---

### Task 6: Symbol version gating (CEL0021–CEL0023)

**Files:**
- Modify: `crates/celerrate_semantics/src/reference_checks.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (export
  `SYMBOL_DEPRECATED, SYMBOL_NOT_AVAILABLE, SYMBOL_REMOVED`)

**Interfaces:**
- Consumes: `SymbolResolution::Stub { availability, .. }` (a
  `StubAvailability { introduced, removed, deprecated }`),
  `configuration.php_version_range(db)` (a
  `PhpVersionRange { minimum, maximum }`), `PhpVersion: Display`.
- Produces: inside the existing `reference_diagnostics` query:

```rust
pub const SYMBOL_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0021");
pub const SYMBOL_REMOVED: DiagnosticId = DiagnosticId::new("CEL0022");
pub const SYMBOL_DEPRECATED: DiagnosticId = DiagnosticId::new("CEL0023");
```

The parent spec's range rule without signatures: availability at
**minimum** (`introduced > minimum` fires), removal and deprecation at
**maximum** (`removed <= maximum` fires; `deprecated` fires when `since`
is absent or `since <= maximum`). Deprecation is `Severity::Warning`;
the other two are `Severity::Error`.

- [ ] **Step 1: Write the failing tests**

Add to the task 5 test module (the `checked` helper gains a range
parameter — refactor the existing tests to pass
`PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5))`):

```rust
fn stub_with(name: &str, kind: StubSymbolKind, availability: StubAvailability) -> StubSymbol {
    StubSymbol { name: name.to_owned(), kind, availability }
}

#[test]
fn a_symbol_introduced_after_the_minimum_is_gated() {
    let diagnostics = checked_in_range(
        &["<?php array_find([], fn($x) => $x);"],
        vec![stub_with(
            "array_find",
            StubSymbolKind::Function,
            StubAvailability {
                introduced: Some(PhpVersion::new(8, 4)),
                removed: None,
                deprecated: None,
            },
        )],
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
    );
    let diagnostic = diagnostics.first().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostic.id, SYMBOL_NOT_AVAILABLE);
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.message,
        "`array_find` requires PHP 8.4, but the project's minimum PHP version is 8.1",
    );
}

#[test]
fn a_symbol_removed_within_the_range_is_gated() {
    let diagnostics = checked_in_range(
        &["<?php utf8_encode('a');"],
        vec![stub_with(
            "utf8_encode",
            StubSymbolKind::Function,
            StubAvailability {
                introduced: None,
                removed: Some(PhpVersion::new(8, 3)),
                deprecated: Some(celerrate_stubs::StubDeprecation {
                    since: Some(PhpVersion::new(8, 2)),
                }),
            },
        )],
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
    );
    assert_eq!(diagnostics.len(), 2);
    let removed = diagnostics.iter().find(|d| d.id == SYMBOL_REMOVED).unwrap();
    assert_eq!(
        removed.message,
        "`utf8_encode` was removed in PHP 8.3, but the project's maximum PHP version is 8.5",
    );
    let deprecated = diagnostics.iter().find(|d| d.id == SYMBOL_DEPRECATED).unwrap();
    assert_eq!(deprecated.severity, Severity::Warning);
    assert_eq!(deprecated.message, "`utf8_encode` is deprecated since PHP 8.2");
}

#[test]
fn a_versionless_deprecation_still_warns() {
    let diagnostics = checked_in_range(
        &["<?php old_helper();"],
        vec![stub_with(
            "old_helper",
            StubSymbolKind::Function,
            StubAvailability {
                introduced: None,
                removed: None,
                deprecated: Some(celerrate_stubs::StubDeprecation { since: None }),
            },
        )],
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
    );
    assert_eq!(diagnostics.first().unwrap().message, "`old_helper` is deprecated");
}

#[test]
fn a_symbol_absent_from_the_whole_range_is_unknown_not_gated() {
    // Removed at or before the minimum: filtered out of the stub table
    // by stubs_in_range, so the reference reports unknown symbol.
    let diagnostics = checked_in_range(
        &["<?php ancient();"],
        vec![stub_with(
            "ancient",
            StubSymbolKind::Function,
            StubAvailability {
                introduced: None,
                removed: Some(PhpVersion::new(8, 1)),
                deprecated: None,
            },
        )],
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
    );
    assert_eq!(diagnostics.first().unwrap().id, UNKNOWN_FUNCTION);
}

#[test]
fn a_project_declaration_is_never_gated() {
    let diagnostics = checked_in_range(
        &["<?php function utf8_encode($s) { return $s; } utf8_encode('a');"],
        vec![stub_with(
            "utf8_encode",
            StubSymbolKind::Function,
            StubAvailability {
                introduced: None,
                removed: Some(PhpVersion::new(8, 3)),
                deprecated: None,
            },
        )],
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
    );
    assert_eq!(diagnostics, vec![]);
}
```

(`checked_in_range` is `checked` with the range as a parameter; keep one
helper.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_semantics reference_checks`
Expected: FAIL — the `Stub` arm emits nothing yet.

- [ ] **Step 3: Implement**

Constants (documented like the others):

```rust
/// A stub symbol introduced after the range minimum.
pub const SYMBOL_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0021");
/// A stub symbol removed at or before the range maximum.
pub const SYMBOL_REMOVED: DiagnosticId = DiagnosticId::new("CEL0022");
/// A stub symbol deprecated at the range maximum.
pub const SYMBOL_DEPRECATED: DiagnosticId = DiagnosticId::new("CEL0023");
```

In `reference_diagnostics`, bind the range once
(`let range = configuration.php_version_range(db);`) and replace the
`Stub` arm:

```rust
Some(SymbolResolution::Stub { availability, .. }) => {
    availability_diagnostics(&reference, availability, range, file_id, &mut diagnostics);
}
```

with:

```rust
fn availability_diagnostics(
    reference: &Reference,
    availability: celerrate_stubs::StubAvailability,
    range: celerrate_project::PhpVersionRange,
    file: celerrate_source::FileId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(introduced) = availability.introduced {
        if introduced > range.minimum {
            diagnostics.push(Diagnostic {
                id: SYMBOL_NOT_AVAILABLE,
                severity: Severity::Error,
                file,
                range: reference.range,
                message: format!(
                    "`{}` requires PHP {introduced}, but the project's minimum PHP version is {}",
                    reference.written, range.minimum,
                ),
            });
        }
    }
    if let Some(removed) = availability.removed {
        if removed <= range.maximum {
            diagnostics.push(Diagnostic {
                id: SYMBOL_REMOVED,
                severity: Severity::Error,
                file,
                range: reference.range,
                message: format!(
                    "`{}` was removed in PHP {removed}, but the project's maximum PHP version is {}",
                    reference.written, range.maximum,
                ),
            });
        }
    }
    if let Some(deprecation) = availability.deprecated {
        let applies = deprecation
            .since
            .is_none_or(|since| since <= range.maximum);
        if applies {
            let message = match deprecation.since {
                Some(since) => format!("`{}` is deprecated since PHP {since}", reference.written),
                None => format!("`{}` is deprecated", reference.written),
            };
            diagnostics.push(Diagnostic {
                id: SYMBOL_DEPRECATED,
                severity: Severity::Warning,
                file,
                range: reference.range,
                message,
            });
        }
    }
}
```

- [ ] **Step 4: Run tests, full gate, commit**

```bash
git add -A
git commit -m "✨ feat(semantics): gate stub symbols by the project version range"
```

---

### Task 7: Syntax version gating — query and first construct (CEL0024)

**Files:**
- Create: `crates/celerrate_semantics/src/syntax_gating.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (add
  `mod syntax_gating;` and `pub use syntax_gating::{SYNTAX_NOT_AVAILABLE,
  syntax_version_diagnostics};`)

**Interfaces:**
- Consumes: `celerrate_db::parse`, `ast::ClassDeclaration::modifiers()`
  (iterator of `SyntaxToken`), `SyntaxKind::Readonly`,
  `configuration.php_version_range(db).minimum`.
- Produces (task 8 extends `gated_uses`; tasks 9-11 call the query):

```rust
pub const SYNTAX_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0024");

#[salsa::tracked(returns(ref))]
pub fn syntax_version_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic>
```

Internal shape: `struct GatedUse { label: &'static str, required:
PhpVersion, range: TextRange }` and
`fn gated_uses(root: &SyntaxNode) -> Vec<GatedUse>`.
Message: `` `<label>` requires PHP <required>, but the project's minimum
PHP version is <minimum> ``; severity `Error`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;

    fn gated(source: &str, minimum: PhpVersion) -> Vec<Diagnostic> {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            minimum,
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        syntax_version_diagnostics(&db, file, configuration).clone()
    }

    #[test]
    fn a_readonly_class_is_gated_below_its_version() {
        let diagnostics = gated("<?php readonly class Point {}", PhpVersion::new(8, 1));
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostic.id, SYNTAX_NOT_AVAILABLE);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.message,
            "`readonly class` requires PHP 8.2, but the project's minimum PHP version is 8.1",
        );
    }

    #[test]
    fn a_construct_within_the_range_minimum_is_silent() {
        assert_eq!(gated("<?php readonly class Point {}", PhpVersion::new(8, 2)), vec![]);
    }

    #[test]
    fn a_readonly_property_is_not_a_readonly_class() {
        assert_eq!(
            gated(
                "<?php class Point { public readonly int $x; }",
                PhpVersion::new(8, 1),
            ),
            vec![],
        );
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_semantics syntax_gating`
Expected: FAIL to compile (`syntax_version_diagnostics` undefined).

- [ ] **Step 3: Implement**

```rust
//! The syntax version-gating family: a construct-to-minimum-version
//! table over the file's own typed AST, checked against the range
//! minimum. This is the design's deliberate boundary exception: an
//! output strictly local to the file may read its own tree. The parser
//! always parses the newest grammar; using a construct the range
//! minimum predates is a semantic diagnostic, never a parse failure.

use celerrate_db::SourceFile;
use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_project::{PhpVersion, ProjectConfiguration};
use celerrate_source::TextRange;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

/// A syntax construct newer than the range minimum.
pub const SYNTAX_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0024");

/// One use of a version-gated construct.
struct GatedUse {
    label: &'static str,
    required: PhpVersion,
    range: TextRange,
}

/// The per-file syntax gating diagnostics.
#[salsa::tracked(returns(ref))]
pub fn syntax_version_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let minimum = configuration.php_version_range(db).minimum;
    let file_id = file.file_id(db);
    let root = celerrate_db::parse(db, file).tree();
    let mut diagnostics: Vec<Diagnostic> = gated_uses(&root)
        .into_iter()
        .filter(|gated| gated.required > minimum)
        .map(|gated| Diagnostic {
            id: SYNTAX_NOT_AVAILABLE,
            severity: Severity::Error,
            file: file_id,
            range: gated.range,
            message: format!(
                "`{}` requires PHP {}, but the project's minimum PHP version is {minimum}",
                gated.label, gated.required,
            ),
        })
        .collect();
    diagnostics.sort();
    diagnostics
}

/// Every gated-construct use in the file, in tree order. One match arm
/// per construct: growing the table is adding an arm.
fn gated_uses(root: &SyntaxNode) -> Vec<GatedUse> {
    let mut uses = Vec::new();
    for node in root.descendants() {
        if node.kind() == SyntaxKind::ClassDeclaration {
            if let Some(declaration) = ast::ClassDeclaration::cast(node.clone()) {
                if let Some(readonly) = declaration
                    .modifiers()
                    .find(|token| token.kind() == SyntaxKind::Readonly)
                {
                    uses.push(GatedUse {
                        label: "readonly class",
                        required: PhpVersion::new(8, 2),
                        range: readonly.text_range(),
                    });
                }
            }
        }
    }
    uses
}
```

- [ ] **Step 4: Run tests, full gate, commit**

```bash
git add -A
git commit -m "✨ feat(semantics): gate syntax constructs by the minimum PHP version"
```

---

### Task 8: The full construct table

**Files:**
- Modify: `crates/celerrate_semantics/src/syntax_gating.rs`

**Interfaces:**
- Consumes: `ast::{ConstantDeclaration, ScopedAccessExpression,
  PropertyDeclaration, Parameter, BinaryExpression, CloneExpression,
  Argument}` accessors; `SyntaxKind::{ParenthesizedType,
  PropertyHookList, OpenBrace, OpenParenthesis, PipeGreater, Public,
  Protected, Private}`.
- Produces: the complete `gated_uses` table. Labels and versions:
  `parenthesized (DNF) type` 8.2, `typed constant` 8.3,
  `dynamic class constant fetch` 8.3, `property hooks` 8.4,
  `asymmetric visibility` 8.4, `pipe operator` 8.5,
  `clone with arguments` 8.5.

- [ ] **Step 1: Write the failing tests**

One table-driven test plus the two precision guards:

```rust
#[test]
fn each_gated_construct_reports_its_version() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "<?php function f((Left&Right)|null $x) {}",
            "parenthesized (DNF) type",
            "8.2",
        ),
        (
            "<?php class C { const int LIMIT = 1; }",
            "typed constant",
            "8.3",
        ),
        (
            "<?php $x = Config::{$name};",
            "dynamic class constant fetch",
            "8.3",
        ),
        (
            "<?php class C { public string $p { get => 'v'; } }",
            "property hooks",
            "8.4",
        ),
        (
            "<?php class C { public private(set) string $p; }",
            "asymmetric visibility",
            "8.4",
        ),
        (
            "<?php $y = $x |> strlen(...);",
            "pipe operator",
            "8.5",
        ),
        (
            "<?php $c = clone($point, ['x' => 1]);",
            "clone with arguments",
            "8.5",
        ),
    ];
    for (source, label, version) in cases {
        let diagnostics = gated(source, PhpVersion::new(8, 1));
        let expected = format!(
            "`{label}` requires PHP {version}, but the project's minimum PHP version is 8.1",
        );
        assert!(
            diagnostics.iter().any(|d| d.message == expected),
            "{source}: {diagnostics:?}",
        );
        assert_eq!(gated(source, PhpVersion::new(8, 5)), vec![], "{source}");
    }
}

#[test]
fn a_static_property_access_is_not_a_dynamic_constant_fetch() {
    assert_eq!(gated("<?php $x = Config::$value;", PhpVersion::new(8, 1)), vec![]);
}

#[test]
fn a_single_positional_clone_argument_is_not_gated() {
    // Pre-8.5 PHP reads `clone($x)` as `clone` of a parenthesized
    // expression; gating it would be a false positive.
    assert_eq!(gated("<?php $c = clone($point);", PhpVersion::new(8, 1)), vec![]);
    let named = gated("<?php $c = clone(object: $point);", PhpVersion::new(8, 1));
    assert_eq!(named.len(), 1);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_semantics syntax_gating`
Expected: FAIL — only `readonly class` is detected.

- [ ] **Step 3: Implement**

Extend the loop in `gated_uses` (the `ClassDeclaration` arm becomes one
arm of a `match node.kind()`):

```rust
SyntaxKind::ParenthesizedType => uses.push(GatedUse {
    label: "parenthesized (DNF) type",
    required: PhpVersion::new(8, 2),
    range: node.text_range(),
}),
SyntaxKind::ConstantDeclaration => {
    if let Some(declaration) = ast::ConstantDeclaration::cast(node.clone()) {
        if let Some(constant_type) = declaration.ty() {
            uses.push(GatedUse {
                label: "typed constant",
                required: PhpVersion::new(8, 3),
                range: constant_type.syntax().text_range(),
            });
        }
    }
}
SyntaxKind::ScopedAccessExpression => {
    if let Some(access) = ast::ScopedAccessExpression::cast(node.clone()) {
        if let Some(member) = access.member_name() {
            let opens_with_brace = member
                .syntax()
                .children_with_tokens()
                .find(|element| {
                    element.as_token().is_none_or(|token| !token.kind().is_trivia())
                })
                .and_then(|element| element.into_token())
                .is_some_and(|token| token.kind() == SyntaxKind::OpenBrace);
            if opens_with_brace {
                uses.push(GatedUse {
                    label: "dynamic class constant fetch",
                    required: PhpVersion::new(8, 3),
                    range: member.syntax().text_range(),
                });
            }
        }
    }
}
SyntaxKind::PropertyHookList => uses.push(GatedUse {
    label: "property hooks",
    required: PhpVersion::new(8, 4),
    range: node.text_range(),
}),
SyntaxKind::PropertyDeclaration | SyntaxKind::Parameter => {
    if let Some(range) = asymmetric_visibility(&node) {
        uses.push(GatedUse {
            label: "asymmetric visibility",
            required: PhpVersion::new(8, 4),
            range,
        });
    }
}
SyntaxKind::BinaryExpression => {
    let operator = ast::BinaryExpression::cast(node.clone())
        .and_then(|binary| binary.operator_token())
        .filter(|token| token.kind() == SyntaxKind::PipeGreater);
    if let Some(operator) = operator {
        uses.push(GatedUse {
            label: "pipe operator",
            required: PhpVersion::new(8, 5),
            range: operator.text_range(),
        });
    }
}
SyntaxKind::CloneExpression => {
    if let Some(clone) = ast::CloneExpression::cast(node.clone()) {
        if let Some(arguments) = clone.argument_list() {
            let is_clone_with = arguments.arguments().count() >= 2
                || arguments
                    .arguments()
                    .any(|argument| argument.label_token().is_some());
            if is_clone_with {
                uses.push(GatedUse {
                    label: "clone with arguments",
                    required: PhpVersion::new(8, 5),
                    range: arguments.syntax().text_range(),
                });
            }
        }
    }
}
```

with the helper (lint-safe pairwise scan, no indexing):

```rust
/// A visibility token directly followed by `(`: the 8.4
/// `private(set)` form, parsed as flat tokens.
fn asymmetric_visibility(node: &SyntaxNode) -> Option<TextRange> {
    let tokens: Vec<_> = node
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia())
        .collect();
    tokens
        .iter()
        .zip(tokens.iter().skip(1))
        .find(|(first, second)| {
            matches!(
                first.kind(),
                SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private
            ) && second.kind() == SyntaxKind::OpenParenthesis
        })
        .map(|(first, _)| first.text_range())
}
```

If an accessor named here does not exist under that exact name
(`ArgumentList::arguments()`, `ConstantDeclaration::ty()`,
`BinaryExpression::operator_token()`), check
`crates/celerrate_syntax/src/ast/generated.rs` for the generated name
and use it — do not add new accessors to the syntax crate.

- [ ] **Step 4: Run tests, full gate, commit**

```bash
git add -A
git commit -m "✨ feat(semantics): complete the construct version-gating table"
```

---

### Task 9: The per-file aggregator

**Files:**
- Modify: `crates/celerrate_semantics/src/queries.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (export
  `semantic_diagnostics`)

**Interfaces:**
- Consumes: `reference_diagnostics` (tasks 5-6),
  `syntax_version_diagnostics` (tasks 7-8).
- Produces (the CLI's per-file entry point in part 7, alongside
  `celerrate_db::file_diagnostics`):

```rust
#[salsa::tracked(returns(ref))]
pub fn semantic_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic>
```

- [ ] **Step 1: Write the failing test**

In the `queries.rs` test module (create one if absent, following the
crate's style — build the four inputs exactly as the task 5 helper does):

```rust
#[test]
fn semantic_diagnostics_merge_both_families_in_order() {
    // One unknown class after one gated construct: the merged output
    // is sorted by range, families interleaved.
    let db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php readonly class Point {} $x = new Missing();".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![file]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let diagnostics = semantic_diagnostics(&db, file, files, stubs, configuration);
    let identifiers: Vec<&str> = diagnostics.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(identifiers, vec!["CEL0024", "CEL0018"]);
    let mut sorted = diagnostics.clone();
    sorted.sort();
    assert_eq!(&sorted, diagnostics);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --package celerrate_semantics semantic_diagnostics`
Expected: FAIL to compile (`semantic_diagnostics` undefined).

- [ ] **Step 3: Implement**

In `queries.rs`:

```rust
/// Every semantic diagnostic of one file: the reference families
/// (unknown symbols, symbol version gating) and syntax version gating,
/// merged and deterministically ordered. Syntax and decode findings
/// live in `celerrate_db::file_diagnostics`; the CLI composes both.
#[salsa::tracked(returns(ref))]
pub fn semantic_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let mut diagnostics =
        crate::reference_checks::reference_diagnostics(db, file, files, stubs, configuration)
            .clone();
    diagnostics.extend(
        crate::syntax_gating::syntax_version_diagnostics(db, file, configuration)
            .iter()
            .cloned(),
    );
    diagnostics.sort();
    diagnostics
}
```

(Add the imports `queries.rs` needs; export from `lib.rs`.)

- [ ] **Step 4: Run tests, full gate, commit**

```bash
git add -A
git commit -m "✨ feat(semantics): aggregate per-file semantic diagnostics"
```

---

### Task 10: Invalidation-scope tests

**Files:**
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: `TestDatabase::take_executed()`, the local `executions_of`
  helper already in the file, `salsa::Setter` (`set_bytes`,
  `set_php_version_range`), `reference_diagnostics`,
  `syntax_version_diagnostics`, `semantic_diagnostics`.
- Produces: the part 6 rows of the canonical edit-class matrix.

- [ ] **Step 1: Read the existing file**

Read `crates/celerrate_semantics/tests/invalidation_scope.rs` and reuse
its input-building idiom (files, stubs, configuration) exactly.

- [ ] **Step 2: Write the failing-or-green tests (behavior pins)**

These tests pin behavior that should already hold; if one fails, the
implementation has a real invalidation bug — investigate, do not weaken
the test.

```rust
#[test]
fn an_unrelated_declaration_spares_other_files_reference_checks() {
    let mut db = TestDatabase::default();
    let library = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php namespace Lib; class Helper {}".to_vec(),
    );
    let consumer = SourceFile::new(
        &db,
        FileId::new(1),
        b"<?php namespace App; use Lib\\Helper; $x = new Helper();".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![library, consumer]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);
    db.take_executed();

    library
        .set_bytes(&mut db)
        .to(b"<?php namespace Lib; class Helper {} class Extra {}".to_vec());
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "reference_diagnostics"),
        1,
        "only the edited file re-checks; the consumer's lookups backdate: {log:?}",
    );
}

#[test]
fn a_version_range_change_re_runs_the_gating_queries() {
    let mut db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php readonly class Point {}".to_vec(),
    );
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    assert_eq!(syntax_version_diagnostics(&db, file, configuration).len(), 1);
    db.take_executed();

    configuration
        .set_php_version_range(&mut db)
        .to(PhpVersionRange::new(PhpVersion::new(8, 2), PhpVersion::new(8, 5)));
    let diagnostics = syntax_version_diagnostics(&db, file, configuration);

    assert_eq!(diagnostics, &vec![]);
    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "syntax_version_diagnostics"),
        1,
        "the configuration is an input of the gating query: {log:?}",
    );
}

#[test]
fn a_comment_only_edit_elsewhere_spares_the_consumer() {
    let mut db = TestDatabase::default();
    let library = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php namespace Lib; class Helper {}".to_vec(),
    );
    let consumer = SourceFile::new(
        &db,
        FileId::new(1),
        b"<?php namespace App; use Lib\\Helper; $x = new Helper();".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![library, consumer]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);
    db.take_executed();

    library
        .set_bytes(&mut db)
        .to(b"<?php namespace Lib; class Helper { /* note */ }".to_vec());
    let _ = reference_diagnostics(&db, library, files, stubs, configuration);
    let _ = reference_diagnostics(&db, consumer, files, stubs, configuration);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "reference_diagnostics"),
        1,
        "the edited file re-checks over its new tree; the consumer's \
         lookups backdate behind the unchanged symbol table: {log:?}",
    );
}
```

- [ ] **Step 3: Run**

Run: `cargo test --package celerrate_semantics --test invalidation_scope`
Expected: PASS. If a count is off, diagnose with the printed log before
touching anything.

- [ ] **Step 4: Full gate, commit**

```bash
git add -A
git commit -m "✅ test(semantics): pin the checks invalidation scope"
```

---

### Task 11: Harness replay and the anti-false-positive smoke suite

**Files:**
- Modify: `crates/celerrate_semantics/tests/incremental_consistency.rs`
- Create: `crates/celerrate_semantics/tests/false_positives.rs`

**Interfaces:**
- Consumes:
  `celerrate_db::testing::assert_incremental_consistency_with_context`
  (signature: `(initial: &[&[u8]], edits: &[(usize, &[u8])],
  make_context: &dyn Fn(&TestDatabase, &[SourceFile]) -> Context,
  assert_state_matches: &dyn Fn(&TestDatabase, &Context, &TestDatabase,
  &Context))`), `celerrate_stubs::embedded_stub_index()`,
  `semantic_diagnostics`.
- Produces: part 6's rows in the incremental correctness harness and the
  smoke corpus.

- [ ] **Step 1: Read the existing consistency test**

Read `crates/celerrate_semantics/tests/incremental_consistency.rs` and
extend it in its own idiom.

- [ ] **Step 2: Add the harness replay**

```rust
#[test]
fn semantic_diagnostics_match_from_scratch_analysis() {
    let initial: &[&[u8]] = &[
        b"<?php namespace App; use Lib\\Helper; $x = new Helper(); missing();",
        b"<?php namespace Lib; class Helper {}",
    ];
    let edits: &[(usize, &[u8])] = &[
        // The reference becomes unknown: the declaration disappears.
        (1, b"<?php namespace Lib;"),
        // It comes back, and a gated construct appears in the consumer.
        (1, b"<?php namespace Lib; class Helper {}"),
        (0, b"<?php namespace App; use Lib\\Helper; $x = new Helper(); readonly class C {}"),
        // Degenerate bytes stay consistent.
        (0, b"<?php class"),
    ];
    assert_incremental_consistency_with_context(
        initial,
        edits,
        &|db, files| {
            (
                AnalyzedFileSet::new(db, files.to_vec()),
                StubIndexInput::builder(StubIndex::default())
                    .durability(salsa::Durability::HIGH)
                    .new(db),
                ProjectConfiguration::builder(PhpVersionRange::new(
                    PhpVersion::new(8, 1),
                    PhpVersion::new(8, 5),
                ))
                .durability(salsa::Durability::MEDIUM)
                .new(db),
                files.to_vec(),
            )
        },
        &|incremental_db, incremental, scratch_db, scratch| {
            let (incremental_set, incremental_stubs, incremental_configuration, incremental_files) =
                incremental;
            let (scratch_set, scratch_stubs, scratch_configuration, scratch_files) = scratch;
            for (incremental_file, scratch_file) in
                incremental_files.iter().zip(scratch_files.iter())
            {
                assert_eq!(
                    semantic_diagnostics(
                        incremental_db,
                        *incremental_file,
                        *incremental_set,
                        *incremental_stubs,
                        *incremental_configuration,
                    ),
                    semantic_diagnostics(
                        scratch_db,
                        *scratch_file,
                        *scratch_set,
                        *scratch_stubs,
                        *scratch_configuration,
                    ),
                );
            }
        },
    );
}
```

- [ ] **Step 3: Write the smoke suite**

`crates/celerrate_semantics/tests/false_positives.rs` — realistic PHP
against the **real embedded stubs**; the assertion is zero diagnostics:

```rust
//! Anti-false-positive smoke: realistic PHP over the real embedded
//! stub index must produce zero semantic diagnostics. A false positive
//! here is a priority bug (parent spec, section 6); the full pinned
//! Symfony corpus enters CI in part 8.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::semantic_diagnostics;
use celerrate_source::FileId;
use celerrate_stubs::{StubIndexInput, embedded_stub_index};

const REALISTIC_SOURCES: &[&str] = &[
    // Conditional declaration, calls, constants, magic constants.
    "<?php
     if (!function_exists('app_helper')) { function app_helper(): string { return __DIR__; } }
     $path = app_helper() . PHP_EOL;
     $length = strlen($path);
     $items = array_map(strtoupper(...), ['a', 'b']);",
    // Class-likes: inheritance, traits, attributes, enums, match.
    "<?php
     namespace App;
     use ArrayAccess;
     #[\\Attribute]
     class Marker {}
     interface Repository extends ArrayAccess {}
     trait Timestamps { public ?\\DateTimeImmutable $updatedAt = null; }
     enum Suit: string {
         case Hearts = 'H';
         public function color(): string {
             return match ($this) { self::Hearts => 'red', default => 'black' };
         }
     }
     final class User implements \\Stringable {
         use Timestamps;
         public function __construct(private readonly string $name) {}
         public function __toString(): string { return $this->name; }
     }",
    // Types, catch, instanceof, scoped access, closures.
    "<?php
     namespace App;
     function load(int|string $id, ?\\Throwable $previous = null): iterable {
         try {
             $when = new \\DateTimeImmutable('now');
         } catch (\\Exception $error) {
             throw new \\RuntimeException($error->getMessage(), 0, $previous);
         }
         $mapper = fn (mixed $value): bool => $value instanceof \\Countable;
         yield from array_filter([$when, $id], $mapper);
     }",
];

#[test]
fn realistic_sources_produce_no_diagnostics() {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = REALISTIC_SOURCES
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(embedded_stub_index().unwrap())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    for file in handles {
        let diagnostics = semantic_diagnostics(&db, file, files, stubs, configuration);
        assert_eq!(diagnostics, &vec![], "file {:?}", file.file_id(&db));
    }
}
```

If a stub-availability warning fires legitimately here (a symbol the
snippet uses really is gated in `[8.1, 8.5]`), replace the symbol in the
snippet with an ungated one — the suite pins "no FALSE positives", so
every snippet must be clean PHP for the whole range.

- [ ] **Step 4: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: PASS. Investigate any smoke failure as a real false positive
(collection too eager, a missing skip rule) before touching the test.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "✅ test(semantics): replay checks and guard against false positives"
```

---

## Self-review checklist (run after writing, fixed inline)

- Spec section 7 coverage: unknown symbols (tasks 2-5), the two
  conservative stances (task 5 tests, task 2/4 skip rules), syntax
  gating at min (tasks 7-8), symbol gating min/max (task 6),
  `celerrate_diagnostics` detail joining the model (task 1), `CEL####`
  born here (tasks 5-8). The preview product itself is part 7.
- Section 9 coverage: TDD (every task), harness (task 11),
  invalidation-scope (task 10), zero-panic lints (global constraints).
  The Symfony corpus and benchmark protocol are part 8.
- Type consistency: `Reference { written, space, namespace, range }`
  used identically in tasks 2-6; the five-field `Diagnostic` everywhere;
  query signatures identical in tasks 5, 6, 9, 10, 11.
