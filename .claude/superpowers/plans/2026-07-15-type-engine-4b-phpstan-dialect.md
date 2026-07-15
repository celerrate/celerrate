# Type Engine 4b — The PHPStan Dialect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The `phpdoc-bridge` plugin learns the full PHPStan docblock
dialect — the type-expression grammar measured against the pinned
`phpstan/phpdoc-parser` reference, a total lowering table into the
lattice, the Psalm synonym and ignored-divergent tag tables,
tool-prefixed tag precedence, `@template` scope resolution, and carried
assertion tags — per section 5 of the design
(`.claude/superpowers/specs/2026-07-14-type-engine-design.md`) and
plan 6 of its section 11.

**Architecture:** The type-expression grammar is rebuilt as a
tokenizer plus recursive-descent parser inside
`celerrate_phpdoc_bridge` (the grammar is shared; PHPStan is the
reference notation and Psalm's non-divergent type syntax is
coincident). The dialects split at the *tag* level into internal
modules `dialect/phpstan` and `dialect/psalm` over the same docblock
lexer, exactly as plan 4a's crate doc promised. A new total lowering
table maps every parsed construct to a lattice builder or a documented
sound widening. `celerrate_types` extends the `AnnotationSite`
extension point (never bypassed) with the declaring-scope and
enclosing-class context that `@template` resolution requires, and the
`ParsedAnnotations` payload with carried assertions. An xtask module
pins `phpstan/phpdoc-parser` and extracts its `TypeParserTest` inputs
into a committed case file; a bridge integration test pins every
case's parse verdict in a committed snapshot whose header is the
published coverage statement.

**Tech Stack:** Rust (edition 2024, toolchain 1.94), salsa 0.27,
cargo-fuzz (nightly, existing `docblock` target), the existing xtask
pin mechanism (`xtask/src/pin.rs`).

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
  No indexing: use `.get()`, `.first()`, `.split_once()`, iterators.
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **The one-dependency rule**: `celerrate_phpdoc_bridge` depends on
  `celerrate_plugin` and nothing else in the workspace (dev-dependencies
  exempt). Enforced by `cargo xtask dependency-shape` — it must stay
  green after every task.
- **An extension point that proves insufficient is extended, never
  bypassed** (design section 4). The `@template` scope gap is closed by
  extending `AnnotationSite` in `celerrate_types`, not by a side channel.
- **No docblock diagnostics**: a malformed annotation is silently
  ignored, per construct, never per annotation (design section 5).
- **Determinism**: no wall clock, no randomness, no environment reads
  inside queries. The bridge's trait implementations are pure functions
  of their arguments.
- **Widening direction**: a documented widening is always a
  *supertype* (silence-safe). `class-string<T>` never widens to
  `string` (it would sever template solving — design section 5).
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.

## Fixed decisions (the header the tasks implement)

1. **One grammar, two tag dialects.** The type-expression grammar
   (tokenizer + parser) is one shared module: PHPStan's notation is
   the reference, and Psalm's non-divergent type syntax is coincident
   with it. The dialect split happens at the tag level:
   `dialect/phpstan.rs` owns the bare and `@phpstan-`-prefixed tag
   vocabulary, `dialect/psalm.rs` owns the `@psalm-` synonyms and the
   enumerated ignored-divergent table.
2. **Prefix parsing replaces the whitespace splitter** (plan 4a's
   decision 6 said so: "4b replaces the splitter together with the
   grammar"). `parse_type_expression_prefix(text)` consumes a maximal
   well-formed type expression and reports its byte length;
   `parse_type_expression_text` wraps it with a rest-must-be-empty
   check. Tag extraction takes the type from the prefix and the
   variable or prose from the remainder — `array{id: int} $x` extracts.
3. **The lowering table is total and lives in one module**
   (`src/lowering.rs`). Every parsed construct has a defined outcome: a
   lattice builder, or a documented sound widening. Loss stays
   per construct: an out-of-grammar *expression* answers `None` at the
   parser (that element is absent); a *parsed* expression always
   lowers.
4. **The nullable-suffix binding follows the reference**: `?int[]`
   parses as `(?int)[]` — array of nullable int — matching
   phpstan/phpdoc-parser, where plan 4a's grammar bound `?` outside
   the suffix. The 4a grammar had no test pinning the old binding;
   the new binding gets one.
5. **Template names resolve from a docblock-scoped set, case
   sensitively, before keywords.** The scope-key convention is the one
   `TypeId::template` already documents (`construction.rs:590-593`):
   the declaring symbol's folded key — `<class key>::<member key>` for
   members, the class key for class-level declarations, the folded
   function key for free functions. The keys are produced by
   `celerrate_types` (the seam), never invented by the bridge: the
   bridge reads them from the extended `AnnotationSite`.
6. **Variance-marked template tags declare the variable; the variance
   itself is the ignored divergence.** `@template-covariant T` declares
   `T` (dropping the whole tag would mistype every `T` below it as a
   class named `T`); the variance marker joins the ignored table.
7. **Assertions are carried, not consumed.** `ParsedAnnotations` gains
   `assertions` (subject text verbatim, asserted `TypeId`, polarity
   always/if-true/if-false, negation flag). Plan 5's narrowing is the
   consumer — the `@throws` precedent. The `=`-prefixed Psalm forms
   are ignored-divergent.
8. **Precedence tiers: PHPStan-prefixed > Psalm-prefixed > bare.**
   Within a tier, the first *parseable* tag wins (the 4a rule,
   preserved); `@param` resolves per parameter name; `@throws` and
   assertions accumulate across tiers. The conflict table is the
   rustdoc of `dialect/mod.rs`; plan 9c owns its publication.
9. **The coverage yardstick** is `tests/PHPStan/Parser/TypeParserTest.php`
   of `phpstan/phpdoc-parser` at tag 2.3.3, commit
   `fb19eedd2bb67ff8cf7a5502ad329e701d6398a3` (resolved 2026-07-15).
   xtask extracts the `provideParseData` inputs into a committed,
   attributed case file (the stubs-blob pattern: pin → fetch into
   `target/` → committed derived artifact → CI `--check`); a bridge
   integration test pins per-case parse verdicts in a committed
   snapshot whose header is the coverage statement. The docblock-level
   corpus (`PhpDocParserTest.php`) is traced debt, per the design's
   "everything beyond it traced as debt".
10. **Not extracted in 4b, traced in the ledger**: `@extends` /
    `@implements` / `@use` generic inheritance positions (the
    linearization threading channel is plan 6's —
    `crates/celerrate_semantics/src/linearize.rs:75` says so),
    `@mixin`, `@param-out`, `@var` on non-members, `@template`
    defaults (`= X` parsed and dropped). Suppression directives
    (`@phpstan-ignore*`, `@psalm-suppress`) are plan 4c's
    comment-directive extension point, not this plan.
11. **Conditional types**: a template-subject conditional whose subject
    is in scope lowers to `TypeId::conditional`; a template subject
    not in scope, and every parameter-subject conditional
    (`$param is X ? A : B`), lower to the branch union — the design's
    documented undecided fallback (section 3). A lattice form for
    parameter-subject conditionals is plan 6's call, traced as debt.
12. **Callable-scoped templates** (`\Closure<T>(T): T`): the template
    list parses; occurrences of those names inside the signature lower
    to `mixed` (sound, deterministic); a bound-respecting lowering is
    traced debt.

## File structure

```
crates/celerrate_phpdoc_bridge/src/
  lib.rs                    module declarations + re-exports (extended)
  lexer.rs                  the docblock lexer (unchanged)
  expression/
    mod.rs                  TypeExpression + entry points (moved from expression.rs, extended)
    tokens.rs               NEW: the tag-content tokenizer (byte-offset tokens)
    parser.rs               NEW: the recursive-descent grammar over tokens
  lowering.rs               NEW: the total lowering table (replaces syntax.rs's lower)
  dialect/
    mod.rs                  NEW: tag classification, tiers, the conflict table rustdoc
    phpstan.rs              NEW: bare + @phpstan- vocabulary
    psalm.rs                NEW: @psalm- synonyms + the ignored-divergent table
  tags.rs                   REWRITTEN over prefix parsing + tiers (+ templates, assertions)
  syntax.rs                 TypeSyntax impl: wires extraction → scope → lowering
  virtual_members.rs        unchanged
crates/celerrate_phpdoc_bridge/tests/
  end_to_end.rs             extended: dialect constructs through the real seam
  phpstan_corpus.rs         NEW: the pinned-reference coverage test
  phpstan_corpus/cases.txt  NEW: committed extracted inputs (attributed)
  phpstan_corpus/verdicts.txt NEW: committed verdicts (the coverage statement)
crates/celerrate_types/src/
  type_syntax.rs            AnnotationContext, site accessors, ParsedAssertion
  declared.rs               context threading at the three call sites
crates/celerrate_types/tests/
  invalidation_scope.rs     extended: class-docblock prose-edit pin
crates/celerrate_plugin/src/lib.rs  re-exports ParsedAssertion, AssertionPolarity
xtask/src/phpdoc_corpus.rs  NEW: fetch + extract for the pinned reference
xtask/phpdoc-parser.pin     NEW: repository + commit
xtask/src/{lib,main}.rs     dispatch for fetch-phpdoc-parser / phpdoc-cases
.github/workflows/corpus.yml  NEW job: phpdoc-cases --check
fuzz/corpus/docblock/seed_dialect  NEW fuzz seed
```

---

### Task 1: The expression tokenizer

The tag-content tokenizer: byte-offset tokens over one type
expression's text, greedy and total (it stops at the first character
it cannot tokenize and returns what it has — prose after a type is
allowed to be untokenizable). This is the substrate every dialect
construct parses over.

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/expression/tokens.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/expression.rs` → move to
  `crates/celerrate_phpdoc_bridge/src/expression/mod.rs` (verbatim move
  in this task; content changes come in Task 2)
- Test: inline `#[cfg(test)] mod tests` in `tokens.rs`

**Interfaces:**
- Consumes: nothing (leaf module).
- Produces (crate-internal, used by Task 2's parser and Task 7's tag
  layer):

```rust
pub(crate) enum TokenKind {
    Name(String), Variable(String), Integer(i64), Float(String),
    StringLiteral(String),
    Pipe, Ampersand, Question, Comma, Colon, DoubleColon, Equals,
    OpenParenthesis, CloseParenthesis, OpenAngle, CloseAngle,
    OpenBrace, CloseBrace, OpenBracket, CloseBracket,
    Ellipsis, Asterisk,
}
pub(crate) struct Token { pub(crate) kind: TokenKind, pub(crate) start: usize, pub(crate) end: usize }
pub(crate) fn tokenize(text: &str) -> Vec<Token>
```

- [ ] **Step 1: Move the module into a directory**

```bash
mkdir -p crates/celerrate_phpdoc_bridge/src/expression
git mv crates/celerrate_phpdoc_bridge/src/expression.rs crates/celerrate_phpdoc_bridge/src/expression/mod.rs
```

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS (a pure move; `mod expression;` in `lib.rs` resolves the directory form).

- [ ] **Step 2: Write the failing tokenizer tests**

Create `crates/celerrate_phpdoc_bridge/src/expression/tokens.rs` with
the tests first (the module skeleton so it compiles):

```rust
//! The dialect token stream over one tag's content. Tokens carry byte
//! offsets so the tag layer can slice a consumed prefix verbatim.
//! Tokenization is greedy and total: it stops at the first character
//! it cannot tokenize and returns the tokens it has — the prose after
//! a type expression is allowed to be untokenizable. `//` comments
//! (the pinned reference accepts them inside array shapes) are
//! skipped to the end of their line.

/// One token kind. Names capture PHP identifiers including `\`,
/// non-ASCII leading bytes, and interior hyphens (`class-string`,
/// `non-empty-array`); numbers capture an optional leading minus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Name(String),
    /// `$name`, including `$this`. The `$` is not stored.
    Variable(String),
    Integer(i64),
    /// Float literals keep their written text (the expression layer
    /// stays `Eq`; lowering parses). An integer literal that
    /// overflows `i64` also lands here: its lowering degrades, the
    /// tokenizer never fails on it.
    Float(String),
    StringLiteral(String),
    Pipe,
    Ampersand,
    Question,
    Comma,
    Colon,
    DoubleColon,
    Equals,
    OpenParenthesis,
    CloseParenthesis,
    OpenAngle,
    CloseAngle,
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Ellipsis,
    Asterisk,
}

/// One token with its byte span in the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let _ = text;
    Vec::new()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<TokenKind> {
        tokenize(text).into_iter().map(|token| token.kind).collect()
    }

    #[test]
    fn names_capture_backslashes_hyphens_and_unicode() {
        use TokenKind::*;
        assert_eq!(
            kinds("\\App\\User class-string non-empty-array Café"),
            vec![
                Name("\\App\\User".to_owned()),
                Name("class-string".to_owned()),
                Name("non-empty-array".to_owned()),
                Name("Café".to_owned()),
            ],
        );
    }

    #[test]
    fn punctuation_tokenizes_including_multi_character_forms() {
        use TokenKind::*;
        assert_eq!(
            kinds("|&?,:()<>{}[]=...*::"),
            vec![
                Pipe, Ampersand, Question, Comma, Colon,
                OpenParenthesis, CloseParenthesis, OpenAngle, CloseAngle,
                OpenBrace, CloseBrace, OpenBracket, CloseBracket,
                Equals, Ellipsis, Asterisk, DoubleColon,
            ],
        );
    }

    #[test]
    fn numbers_tokenize_with_sign_separators_radix_and_floats() {
        use TokenKind::*;
        assert_eq!(
            kinds("42 -1 1_000 0x7F 0b0110 0o777 1.5 -2.5e3"),
            vec![
                Integer(42),
                Integer(-1),
                Integer(1_000),
                Integer(0x7F),
                Integer(0b0110),
                Integer(0o777),
                Float("1.5".to_owned()),
                Float("-2.5e3".to_owned()),
            ],
        );
        // An integer beyond i64 degrades to a Float token, never an error.
        assert_eq!(
            kinds("99999999999999999999"),
            vec![Float("99999999999999999999".to_owned())],
        );
    }

    #[test]
    fn strings_tokenize_with_escapes_per_quote_kind() {
        use TokenKind::*;
        assert_eq!(
            kinds(r"'it\'s' 'a\\b'"),
            vec![
                StringLiteral("it's".to_owned()),
                StringLiteral("a\\b".to_owned()),
            ],
        );
        assert_eq!(
            kinds("\"a\\\"b\\n\""),
            vec![StringLiteral("a\"b\n".to_owned())],
        );
    }

    #[test]
    fn variables_and_comments_tokenize() {
        use TokenKind::*;
        assert_eq!(
            kinds("$this $items // trailing noise\n$next"),
            vec![
                Variable("this".to_owned()),
                Variable("items".to_owned()),
                Variable("next".to_owned()),
            ],
        );
    }

    #[test]
    fn tokenization_stops_at_the_first_untokenizable_character() {
        // `'` opens an unterminated string: everything before it
        // survives, nothing after it is invented.
        use TokenKind::*;
        assert_eq!(
            kinds("int $id the identifier isn't typed"),
            vec![
                Name("int".to_owned()),
                Variable("id".to_owned()),
                Name("the".to_owned()),
                Name("identifier".to_owned()),
                Name("isn".to_owned()),
            ],
        );
    }

    #[test]
    fn offsets_reconstruct_the_consumed_prefix() {
        let text = "array{id: int} $x";
        let tokens = tokenize(text);
        let close_brace = tokens
            .iter()
            .find(|token| token.kind == TokenKind::CloseBrace)
            .unwrap();
        assert_eq!(text.get(..close_brace.end), Some("array{id: int}"));
    }

    #[test]
    fn adversarial_inputs_never_panic() {
        let repeated = "?".repeat(10_000);
        for text in ["", "'", "\"", "\\", "$", "-", ".", "..", "0x", "1_",
                     "\u{0}::\u{0}", repeated.as_str()] {
            let _ = tokenize(text);
        }
    }
}
```

Register the module: in `crates/celerrate_phpdoc_bridge/src/expression/mod.rs`
add at the top (after the module doc comment):

```rust
mod tokens;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge tokens`
Expected: FAIL — every assertion against the empty stub.

- [ ] **Step 4: Implement the tokenizer**

Replace the `tokenize` stub in `tokens.rs`:

```rust
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut cursor = text.char_indices().peekable();
    while let Some(&(start, character)) = cursor.peek() {
        if character.is_whitespace() {
            cursor.next();
            continue;
        }
        // `//` comments run to the end of their line (the pinned
        // reference accepts them inside array shapes).
        if character == '/' && starts_with_at(text, start, "//") {
            while let Some(&(_, character)) = cursor.peek() {
                if character == '\n' {
                    break;
                }
                cursor.next();
            }
            continue;
        }
        let Some(token) = lex_token(text, &mut cursor, start, character) else {
            break;
        };
        tokens.push(token);
    }
    tokens
}

fn starts_with_at(text: &str, start: usize, prefix: &str) -> bool {
    text.get(start..)
        .is_some_and(|remainder| remainder.starts_with(prefix))
}

fn lex_token(
    text: &str,
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
    character: char,
) -> Option<Token> {
    let simple = |kind: TokenKind, width: usize| Token {
        kind,
        start,
        end: start + width,
    };
    match character {
        '|' => advance(cursor, 1).then(|| simple(TokenKind::Pipe, 1)),
        '&' => advance(cursor, 1).then(|| simple(TokenKind::Ampersand, 1)),
        '?' => advance(cursor, 1).then(|| simple(TokenKind::Question, 1)),
        ',' => advance(cursor, 1).then(|| simple(TokenKind::Comma, 1)),
        '=' => advance(cursor, 1).then(|| simple(TokenKind::Equals, 1)),
        '(' => advance(cursor, 1).then(|| simple(TokenKind::OpenParenthesis, 1)),
        ')' => advance(cursor, 1).then(|| simple(TokenKind::CloseParenthesis, 1)),
        '<' => advance(cursor, 1).then(|| simple(TokenKind::OpenAngle, 1)),
        '>' => advance(cursor, 1).then(|| simple(TokenKind::CloseAngle, 1)),
        '{' => advance(cursor, 1).then(|| simple(TokenKind::OpenBrace, 1)),
        '}' => advance(cursor, 1).then(|| simple(TokenKind::CloseBrace, 1)),
        '[' => advance(cursor, 1).then(|| simple(TokenKind::OpenBracket, 1)),
        ']' => advance(cursor, 1).then(|| simple(TokenKind::CloseBracket, 1)),
        '*' => advance(cursor, 1).then(|| simple(TokenKind::Asterisk, 1)),
        ':' => {
            if starts_with_at(text, start, "::") {
                advance(cursor, 2).then(|| simple(TokenKind::DoubleColon, 2))
            } else {
                advance(cursor, 1).then(|| simple(TokenKind::Colon, 1))
            }
        }
        '.' => {
            if starts_with_at(text, start, "...") {
                advance(cursor, 3).then(|| simple(TokenKind::Ellipsis, 3))
            } else {
                None
            }
        }
        '$' => lex_variable(cursor, start),
        '\'' | '"' => lex_string(cursor, start, character),
        '-' => {
            let mut lookahead = cursor.clone();
            lookahead.next();
            match lookahead.peek() {
                Some(&(_, digit)) if digit.is_ascii_digit() => lex_number(text, cursor, start),
                _ => None,
            }
        }
        character if character.is_ascii_digit() => lex_number(text, cursor, start),
        character if is_name_start(character) => lex_name(cursor, start),
        _ => None,
    }
}

/// Advances the cursor by `count` characters; answers `true` so the
/// caller can chain with `.then()`.
fn advance(cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>, count: usize) -> bool {
    for _ in 0..count {
        cursor.next();
    }
    true
}

fn is_name_start(character: char) -> bool {
    character.is_alphabetic() || character == '_' || character == '\\' || character >= '\u{80}'
}

fn is_name_continue(character: char) -> bool {
    character.is_alphanumeric()
        || character == '_'
        || character == '\\'
        || character == '-'
        || character >= '\u{80}'
}

fn lex_name(
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Option<Token> {
    let mut name = String::new();
    let mut end = start;
    while let Some(&(offset, character)) = cursor.peek() {
        let accepted = if name.is_empty() {
            is_name_start(character)
        } else {
            is_name_continue(character)
        };
        if !accepted {
            break;
        }
        name.push(character);
        end = offset + character.len_utf8();
        cursor.next();
    }
    // A trailing hyphen belongs to prose (`foo- bar`), not the name:
    // give it back so the parser never sees `foo-` as one identifier.
    while name.ends_with('-') {
        name.pop();
        end -= 1;
    }
    if name.is_empty() {
        None
    } else {
        Some(Token {
            kind: TokenKind::Name(name),
            start,
            end,
        })
    }
}

fn lex_variable(
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Option<Token> {
    cursor.next(); // the `$`
    let mut name = String::new();
    let mut end = start + 1;
    while let Some(&(offset, character)) = cursor.peek() {
        if character.is_ascii_alphanumeric() || character == '_' {
            name.push(character);
            end = offset + character.len_utf8();
            cursor.next();
        } else {
            break;
        }
    }
    if name.is_empty() {
        None
    } else {
        Some(Token {
            kind: TokenKind::Variable(name),
            start,
            end,
        })
    }
}

fn lex_string(
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
    quote: char,
) -> Option<Token> {
    cursor.next(); // the opening quote
    let mut value = String::new();
    while let Some((offset, character)) = cursor.next() {
        if character == quote {
            return Some(Token {
                kind: TokenKind::StringLiteral(value),
                start,
                end: offset + character.len_utf8(),
            });
        }
        if character == '\\' {
            let Some((_, escaped)) = cursor.next() else {
                return None;
            };
            match escaped {
                '\\' => value.push('\\'),
                escaped if escaped == quote => value.push(quote),
                'n' if quote == '"' => value.push('\n'),
                't' if quote == '"' => value.push('\t'),
                'r' if quote == '"' => value.push('\r'),
                // PHP single-quote semantics: any other escape keeps
                // both characters.
                other => {
                    value.push('\\');
                    value.push(other);
                }
            }
        } else {
            value.push(character);
        }
    }
    None // unterminated: the construct stops here
}

fn lex_number(
    text: &str,
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Option<Token> {
    let mut written = String::new();
    let mut end = start;
    let mut is_float = false;
    if let Some(&(_, '-')) = cursor.peek() {
        written.push('-');
        end += 1;
        cursor.next();
    }
    let radix_prefix = ["0x", "0X", "0b", "0B", "0o", "0O"]
        .iter()
        .find(|prefix| starts_with_at(text, end, prefix))
        .copied();
    if let Some(prefix) = radix_prefix {
        written.push_str(prefix);
        end += 2;
        advance(cursor, 2);
    }
    let mut digits = String::new();
    while let Some(&(offset, character)) = cursor.peek() {
        let accepted = match radix_prefix {
            Some("0x" | "0X") => character.is_ascii_hexdigit() || character == '_',
            Some(_) => character.is_ascii_digit() || character == '_',
            None => {
                character.is_ascii_digit()
                    || character == '_'
                    || character == '.'
                    || character == 'e'
                    || character == 'E'
                    || ((character == '+' || character == '-')
                        && (digits.ends_with('e') || digits.ends_with('E')))
            }
        };
        if !accepted {
            break;
        }
        if character == '.' {
            // `..` would be an ellipsis after an integer, not a float
            // dot: only consume a dot followed by a digit.
            let mut lookahead = cursor.clone();
            lookahead.next();
            if !matches!(lookahead.peek(), Some(&(_, next)) if next.is_ascii_digit()) {
                break;
            }
            is_float = true;
        }
        if character == 'e' || character == 'E' {
            is_float = true;
        }
        digits.push(character);
        written.push(character);
        end = offset + character.len_utf8();
        cursor.next();
    }
    if digits.trim_matches('_').is_empty() {
        return None;
    }
    let kind = if is_float {
        TokenKind::Float(written)
    } else {
        let cleaned: String = written.chars().filter(|&character| character != '_').collect();
        let parsed = match radix_prefix {
            Some("0x" | "0X") => i64::from_str_radix(cleaned.trim_start_matches("0x").trim_start_matches("0X"), 16),
            Some("0b" | "0B") => i64::from_str_radix(cleaned.trim_start_matches("0b").trim_start_matches("0B"), 2),
            Some(_) => i64::from_str_radix(cleaned.trim_start_matches("0o").trim_start_matches("0O"), 8),
            None => cleaned.parse::<i64>(),
        };
        match parsed {
            Ok(value) => TokenKind::Integer(value),
            // Beyond i64: degrade to Float text; lowering widens.
            Err(_) => TokenKind::Float(written),
        }
    };
    Some(Token { kind, start, end })
}
```

Note for the implementer: negative radix literals (`-0x7F`) parse the
sign into `written` but the radix strip above only strips the prefix —
`cleaned` still carries the leading `-`, so hex/binary/octal parsing
of a negative literal fails into the `Float` degradation. That is
acceptable (no such form exists in the reference corpus); do not add
complexity for it.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_phpdoc_bridge tokens`
Expected: PASS (all 8 tests).

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS, no diff from fmt.

```bash
git add crates/celerrate_phpdoc_bridge/src/expression
git commit -m "✨ feat(phpdoc-bridge): the dialect tokenizer over tag content"
```

---

### Task 2: The core grammar over tokens, with prefix parsing

Port the 4a grammar (names, nullable, union, intersection, `[]`
suffix, parentheses) onto the token stream, introduce
`parse_type_expression_prefix`, and fix the nullable-suffix binding to
the reference's (`?int[]` is `(?int)[]`). Every 4a entry-level test
keeps passing unchanged except none — the dialect-constructs test
still answers `None` for everything it lists (generics arrive in
Task 3).

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/expression/parser.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/mod.rs`
  (replace the char-cursor parser with the token entries; keep
  `TypeExpression` and the tests)
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (re-export
  `parse_type_expression_prefix`)
- Test: inline tests in `expression/mod.rs`

**Interfaces:**
- Consumes: `tokens::{Token, TokenKind, tokenize}` (Task 1).
- Produces:

```rust
// expression/mod.rs (public, re-exported from lib.rs)
pub fn parse_type_expression_text(text: &str) -> Option<TypeExpression>
pub fn parse_type_expression_prefix(text: &str) -> Option<(TypeExpression, usize)>
// expression/parser.rs (crate-internal)
pub(crate) struct Parser<'a> { /* tokens + position */ }
pub(crate) fn parse_type(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression>
pub(crate) const MAXIMUM_DEPTH: u32 = 64;
```

- [ ] **Step 1: Write the failing tests**

In `expression/mod.rs`'s test module, add:

```rust
    #[test]
    fn nullable_binds_inside_the_array_suffix() {
        // The reference parses `?int[]` as an array of nullable int
        // ((?int)[]), not a nullable array — decision 4.
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("?int[]"),
            Some(ArrayOf(Box::new(Nullable(Box::new(Name("int".to_owned())))))),
        );
    }

    #[test]
    fn prefix_parsing_reports_the_consumed_length() {
        let (expression, consumed) =
            parse_type_expression_prefix("int|string $x the identifier").unwrap();
        assert_eq!(
            expression,
            TypeExpression::Union(vec![
                TypeExpression::Name("int".to_owned()),
                TypeExpression::Name("string".to_owned()),
            ]),
        );
        assert_eq!(consumed, "int|string".len());
        assert!(parse_type_expression_prefix("$x only prose").is_none());
    }
```

The test module needs `#[allow(clippy::unwrap_used)]` on it if not
already present (4a's module has no such allow — add
`#![allow(clippy::unwrap_used)]` as the first line of `mod tests`).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge expression`
Expected: FAIL — `parse_type_expression_prefix` does not exist, and
`?int[]` currently parses as `Nullable(ArrayOf(int))`.

- [ ] **Step 3: Write the token parser**

Create `crates/celerrate_phpdoc_bridge/src/expression/parser.rs`:

```rust
//! The recursive-descent grammar over the token stream. Every parse
//! function threads the shared depth guard: adversarial nesting
//! answers `None`, never a stack overflow. Grammar failures answer
//! `None` for the whole expression — loss is per construct (one tag
//! element), never per annotation.

use super::TypeExpression;
use super::tokens::{Token, TokenKind};

/// Nesting is refused past this depth: adversarial input (`(((((...`)
/// must not overflow the stack.
pub(crate) const MAXIMUM_DEPTH: u32 = 64;

pub(crate) struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    pub(crate) fn peek(&self) -> Option<&'a TokenKind> {
        self.tokens.get(self.position).map(|token| &token.kind)
    }

    pub(crate) fn peek_at(&self, offset: usize) -> Option<&'a TokenKind> {
        self.tokens
            .get(self.position + offset)
            .map(|token| &token.kind)
    }

    pub(crate) fn advance(&mut self) -> Option<&'a TokenKind> {
        let token = self.tokens.get(self.position)?;
        self.position += 1;
        Some(&token.kind)
    }

    /// Consumes the next token when it equals `expected` (payload-free
    /// punctuation kinds).
    pub(crate) fn eat(&mut self, expected: &TokenKind) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    /// The byte offset just past the last consumed token — the
    /// consumed-prefix length the tag layer slices with.
    pub(crate) fn consumed_end(&self) -> Option<usize> {
        self.position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map(|token| token.end)
    }
}

pub(crate) fn parse_type(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    // Task 5 adds the conditional tail (`is`) here.
    parse_union(parser, depth)
}

fn parse_union(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut members = vec![parse_intersection(parser, depth + 1)?];
    while parser.eat(&TokenKind::Pipe) {
        members.push(parse_intersection(parser, depth + 1)?);
    }
    if members.len() == 1 {
        members.into_iter().next()
    } else {
        Some(TypeExpression::Union(members))
    }
}

fn parse_intersection(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut members = vec![parse_suffixed(parser, depth + 1)?];
    while parser.eat(&TokenKind::Ampersand) {
        members.push(parse_suffixed(parser, depth + 1)?);
    }
    if members.len() == 1 {
        members.into_iter().next()
    } else {
        Some(TypeExpression::Intersection(members))
    }
}

fn parse_suffixed(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut expression = parse_atom(parser, depth + 1)?;
    loop {
        if parser.peek() == Some(&TokenKind::OpenBracket)
            && parser.peek_at(1) == Some(&TokenKind::CloseBracket)
        {
            parser.advance();
            parser.advance();
            expression = TypeExpression::ArrayOf(Box::new(expression));
            continue;
        }
        // Task 5 adds offset access (`[` type `]`) here.
        break;
    }
    Some(expression)
}

fn parse_atom(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    match parser.peek()? {
        TokenKind::Question => {
            parser.advance();
            // The nullable marker binds to the atom; array suffixes
            // wrap outside (`?int[]` is `(?int)[]`) — decision 4.
            let inner = parse_atom(parser, depth + 1)?;
            Some(TypeExpression::Nullable(Box::new(inner)))
        }
        TokenKind::OpenParenthesis => {
            parser.advance();
            let inner = parse_type(parser, depth + 1)?;
            if parser.eat(&TokenKind::CloseParenthesis) {
                Some(inner)
            } else {
                None
            }
        }
        TokenKind::Name(name) => {
            let name = name.clone();
            parser.advance();
            // Tasks 3-5 extend name-headed constructs here (generics,
            // shapes, callables, const fetches).
            Some(TypeExpression::Name(name))
        }
        _ => None,
    }
}
```

- [ ] **Step 4: Rewrite the entries in `expression/mod.rs`**

Replace everything between the module doc comment and `mod tests` (the
old `TypeExpression`, `MAXIMUM_DEPTH`, `parse_type_expression_text`,
and every char-cursor `parse_*`/helper function) with:

```rust
mod parser;
mod tokens;

/// A parsed type expression of the inherited PHPDoc dialect family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpression {
    Name(String),
    Nullable(Box<TypeExpression>),
    Union(Vec<TypeExpression>),
    Intersection(Vec<TypeExpression>),
    ArrayOf(Box<TypeExpression>),
}

/// Parses `text` as one type expression consuming the whole input
/// (trailing whitespace allowed); anything left over, anything outside
/// the grammar, or anything nested past the depth guard answers
/// `None`.
pub fn parse_type_expression_text(text: &str) -> Option<TypeExpression> {
    let (expression, consumed) = parse_type_expression_prefix(text)?;
    let remainder = text.get(consumed..)?;
    if remainder.trim().is_empty() {
        Some(expression)
    } else {
        None
    }
}

/// Parses a maximal well-formed type expression from the start of
/// `text` and reports the consumed byte length — the tag layer takes
/// the type from the prefix and the variable or prose from the
/// remainder. Grammar failure anywhere answers `None` for the whole
/// expression: loss is per construct, never partially recovered.
pub fn parse_type_expression_prefix(text: &str) -> Option<(TypeExpression, usize)> {
    let tokens = tokens::tokenize(text);
    let mut cursor = parser::Parser::new(&tokens);
    let expression = parser::parse_type(&mut cursor, 0)?;
    let consumed = cursor.consumed_end()?;
    Some((expression, consumed))
}
```

Update the module doc comment to describe the dialect grammar and the
two entries (it currently advertises the 4a grammar and the "answers
`None`" list — keep that list accurate until Tasks 3-5 remove items
from it).

In `crates/celerrate_phpdoc_bridge/src/lib.rs` extend the re-export:

```rust
pub use expression::{TypeExpression, parse_type_expression_prefix, parse_type_expression_text};
```

- [ ] **Step 5: Run the full bridge test suite**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS — the two new tests, all 4a expression tests
(`the_standard_grammar_parses`, `dialect_constructs_and_garbage_answer_none`,
`adversarial_expressions_never_panic`), the tag tests, and
`tests/end_to_end.rs` all green over the new parser.

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src
git commit -m "♻️ refactor(phpdoc-bridge): the grammar re-ported over tokens with prefix parsing"
```

---

### Task 3: Literals, generics, and integer ranges

`'active'`, `42`, `-1`, `1.5`, `array<int, string>`,
`class-string<T>`, `int<1, max>`, `Collection<covariant User>` parse.
Call-site variance keywords are consumed and dropped (decision 6's
ignored-variance posture applies to type arguments too, documented).

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/mod.rs`
  (new `TypeExpression` variants, tests)
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/parser.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/tags.rs` (one test
  fixture: `array<int>` no longer serves as "unparseable")

**Interfaces:**
- Produces (variants consumed by Task 6's lowering table):

```rust
pub enum TypeExpression {
    // ... existing variants ...
    IntLiteral(i64),
    /// The written text; lowering parses it (`Eq` stays derivable).
    FloatLiteral(String),
    StringLiteral(String),
    Generic { base: String, arguments: Vec<TypeExpression> },
}
```

- [ ] **Step 1: Write the failing tests**

In `expression/mod.rs` tests:

```rust
    #[test]
    fn literals_parse() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("'active'"),
            Some(StringLiteral("active".to_owned())),
        );
        assert_eq!(parse_type_expression_text("42"), Some(IntLiteral(42)));
        assert_eq!(parse_type_expression_text("-1"), Some(IntLiteral(-1)));
        assert_eq!(
            parse_type_expression_text("1.5"),
            Some(FloatLiteral("1.5".to_owned())),
        );
        assert_eq!(
            parse_type_expression_text("'yes'|'no'"),
            Some(Union(vec![
                StringLiteral("yes".to_owned()),
                StringLiteral("no".to_owned()),
            ])),
        );
    }

    #[test]
    fn generics_parse_with_nesting_ranges_and_variance() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("array<int, string>"),
            Some(Generic {
                base: "array".to_owned(),
                arguments: vec![Name("int".to_owned()), Name("string".to_owned())],
            }),
        );
        assert_eq!(
            parse_type_expression_text("array<int, array<string, User>>"),
            Some(Generic {
                base: "array".to_owned(),
                arguments: vec![
                    Name("int".to_owned()),
                    Generic {
                        base: "array".to_owned(),
                        arguments: vec![Name("string".to_owned()), Name("User".to_owned())],
                    },
                ],
            }),
        );
        assert_eq!(
            parse_type_expression_text("class-string<T>"),
            Some(Generic {
                base: "class-string".to_owned(),
                arguments: vec![Name("T".to_owned())],
            }),
        );
        assert_eq!(
            parse_type_expression_text("int<1, max>"),
            Some(Generic {
                base: "int".to_owned(),
                arguments: vec![IntLiteral(1), Name("max".to_owned())],
            }),
        );
        // Call-site variance keywords are consumed and dropped.
        assert_eq!(
            parse_type_expression_text("Collection<covariant User>"),
            Some(Generic {
                base: "Collection".to_owned(),
                arguments: vec![Name("User".to_owned())],
            }),
        );
    }
```

And **edit `dialect_constructs_and_garbage_answer_none`**: remove
`"array<int, string>"`, `"class-string<T>"`, `"'literal'"`, and
`"int<1, max>"` from its list (they parse now); add `"array<int"`
(unterminated generic) and `"Foo<>"` (empty argument list) to it.
`"array{id: int}"` stays until Task 4.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge expression`
Expected: FAIL — the new variants do not exist.

- [ ] **Step 3: Implement**

Add the four variants to `TypeExpression` in `expression/mod.rs`
exactly as in the Interfaces block. In `parser.rs`, extend
`parse_atom`'s match:

```rust
        TokenKind::Integer(value) => {
            let value = *value;
            parser.advance();
            Some(TypeExpression::IntLiteral(value))
        }
        TokenKind::Float(text) => {
            let text = text.clone();
            parser.advance();
            Some(TypeExpression::FloatLiteral(text))
        }
        TokenKind::StringLiteral(value) => {
            let value = value.clone();
            parser.advance();
            Some(TypeExpression::StringLiteral(value))
        }
```

and replace the `Name` arm with:

```rust
        TokenKind::Name(name) => {
            let name = name.clone();
            parser.advance();
            if parser.eat(&TokenKind::OpenAngle) {
                let arguments = parse_generic_arguments(parser, depth + 1)?;
                return Some(TypeExpression::Generic {
                    base: name,
                    arguments,
                });
            }
            // Tasks 4-5 extend name-headed constructs here (shapes,
            // callables, const fetches).
            Some(TypeExpression::Name(name))
        }
```

Add below `parse_atom`:

```rust
/// The `<...>` argument list of a name-headed generic. Call-site
/// variance keywords (`covariant`, `contravariant`) are consumed and
/// dropped — the ignored-variance posture, documented in the ledger.
/// A trailing comma is tolerated; an empty list is refused.
fn parse_generic_arguments(
    parser: &mut Parser<'_>,
    depth: u32,
) -> Option<Vec<TypeExpression>> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut arguments = Vec::new();
    loop {
        if let Some(TokenKind::Name(keyword)) = parser.peek()
            && (keyword == "covariant" || keyword == "contravariant")
            && !matches!(
                parser.peek_at(1),
                Some(TokenKind::CloseAngle | TokenKind::Comma) | None
            )
        {
            parser.advance();
        }
        arguments.push(parse_type(parser, depth + 1)?);
        if parser.eat(&TokenKind::Comma) {
            if parser.eat(&TokenKind::CloseAngle) {
                return Some(arguments);
            }
            continue;
        }
        if parser.eat(&TokenKind::CloseAngle) {
            return Some(arguments);
        }
        return None;
    }
}
```

- [ ] **Step 4: Fix the tag-layer fixture that relied on generics not parsing**

In `crates/celerrate_phpdoc_bridge/src/tags.rs`, the test
`the_value_slot_takes_the_first_parseable_tag` uses
`@return array<int>` as its unparseable first tag — that now parses.
Replace the fixture docblock with an unterminated shape, which stays
out of the grammar permanently:

```rust
        let tags = lex_docblock("/**\n * @return array{\n * @return string\n */");
```

(The expected assertion — the slot takes `string` — is unchanged.)

- [ ] **Step 5: Run to verify green**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS.

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src
git commit -m "✨ feat(phpdoc-bridge): literals, generics, and integer ranges parse"
```

---

### Task 4: Array, list, and object shapes

`array{id: int, name?: string}`, quoted and integer keys, tuples
(`array{int, string}`), the unsealed tails (`...`, `...<V>`,
`...<K, V>`), `list{...}`, `non-empty-array{...}`, `non-empty-list{...}`,
and `object{...}` parse. A brace after a non-shape base
(`Foo{...}`) is not a shape — the name stands alone and the brace ends
the prefix.

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/mod.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/parser.rs`

**Interfaces:**
- Produces (consumed by Task 6's lowering table):

```rust
pub enum TypeExpression {
    // ... existing variants ...
    Shape {
        base: String,
        fields: Vec<ShapeFieldExpression>,
        unsealed: Option<UnsealedTail>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeFieldExpression {
    pub key: Option<ShapeKeyExpression>,
    pub optional: bool,
    pub value: TypeExpression,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeKeyExpression {
    Integer(i64),
    String(String),
    Identifier(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsealedTail {
    pub key: Option<Box<TypeExpression>>,
    pub value: Option<Box<TypeExpression>>,
}
```

- [ ] **Step 1: Write the failing tests**

In `expression/mod.rs` tests:

```rust
    #[test]
    fn shapes_parse_with_every_key_form() {
        let Some(TypeExpression::Shape { base, fields, unsealed }) =
            parse_type_expression_text("array{id: int, name?: string, 'q': bool, 0: float}")
        else {
            panic!("expected a shape");
        };
        assert_eq!(base, "array");
        assert!(unsealed.is_none());
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].key, Some(ShapeKeyExpression::Identifier("id".to_owned())));
        assert!(!fields[0].optional);
        assert_eq!(fields[1].key, Some(ShapeKeyExpression::Identifier("name".to_owned())));
        assert!(fields[1].optional);
        assert_eq!(fields[2].key, Some(ShapeKeyExpression::String("q".to_owned())));
        assert_eq!(fields[3].key, Some(ShapeKeyExpression::Integer(0)));
    }

    #[test]
    fn tuples_empty_shapes_and_other_bases_parse() {
        let Some(TypeExpression::Shape { fields, .. }) =
            parse_type_expression_text("array{int, string}")
        else {
            panic!("expected a tuple shape");
        };
        assert!(fields.iter().all(|field| field.key.is_none()));
        assert!(matches!(
            parse_type_expression_text("array{}"),
            Some(TypeExpression::Shape { fields, unsealed: None, .. }) if fields.is_empty()
        ));
        for text in ["list{int, string}", "object{a: int}", "non-empty-array{a: int}",
                     "non-empty-list{int}"] {
            assert!(
                matches!(parse_type_expression_text(text), Some(TypeExpression::Shape { .. })),
                "{text}",
            );
        }
        // A brace after a non-shape base is not a shape.
        assert_eq!(parse_type_expression_text("Foo{a: int}"), None);
        assert!(matches!(
            parse_type_expression_prefix("Foo{a: int}"),
            Some((TypeExpression::Name(name), 3)) if name == "Foo"
        ));
    }

    #[test]
    fn unsealed_tails_parse() {
        let Some(TypeExpression::Shape { unsealed: Some(tail), .. }) =
            parse_type_expression_text("array{a: int, ...}")
        else {
            panic!("expected an unsealed shape");
        };
        assert_eq!(tail, UnsealedTail { key: None, value: None });
        let Some(TypeExpression::Shape { unsealed: Some(tail), .. }) =
            parse_type_expression_text("array{a: int, ...<string, bool>}")
        else {
            panic!("expected a typed unsealed tail");
        };
        assert_eq!(
            tail,
            UnsealedTail {
                key: Some(Box::new(TypeExpression::Name("string".to_owned()))),
                value: Some(Box::new(TypeExpression::Name("bool".to_owned()))),
            },
        );
    }
```

Also **edit `dialect_constructs_and_garbage_answer_none`**: remove
`"array{id: int}"`; add `"array{a: int"` (unterminated — stays `None`
forever).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge expression`
Expected: FAIL — the `Shape` variant does not exist.

- [ ] **Step 3: Implement**

Add the variant and the three support types to `expression/mod.rs` as
in the Interfaces block, and export them from
`crates/celerrate_phpdoc_bridge/src/lib.rs`:

```rust
pub use expression::{
    ShapeFieldExpression, ShapeKeyExpression, TypeExpression, UnsealedTail,
    parse_type_expression_prefix, parse_type_expression_text,
};
```

In `parser.rs`, in `parse_atom`'s `Name` arm, after the generics
check, add:

```rust
            if is_shape_base(&name) && parser.peek() == Some(&TokenKind::OpenBrace) {
                parser.advance();
                let (fields, unsealed) = parse_shape_body(parser, depth + 1)?;
                return Some(TypeExpression::Shape {
                    base: name,
                    fields,
                    unsealed,
                });
            }
```

and add below:

```rust
/// The bases the reference accepts a `{...}` body on. Everything else
/// keeps its brace unconsumed: the name ends the prefix.
fn is_shape_base(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "array" | "non-empty-array" | "list" | "non-empty-list" | "object"
    )
}

/// The `{...}` body: fields, an optional unsealed tail (`...`,
/// `...<V>`, `...<K, V>`, always last), trailing commas tolerated.
fn parse_shape_body(
    parser: &mut Parser<'_>,
    depth: u32,
) -> Option<(Vec<ShapeFieldExpression>, Option<UnsealedTail>)> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut fields = Vec::new();
    if parser.eat(&TokenKind::CloseBrace) {
        return Some((fields, None));
    }
    loop {
        if parser.eat(&TokenKind::Ellipsis) {
            let tail = parse_unsealed_tail(parser, depth + 1)?;
            let _ = parser.eat(&TokenKind::Comma);
            if parser.eat(&TokenKind::CloseBrace) {
                return Some((fields, Some(tail)));
            }
            return None;
        }
        fields.push(parse_shape_field(parser, depth + 1)?);
        if parser.eat(&TokenKind::Comma) {
            if parser.eat(&TokenKind::CloseBrace) {
                return Some((fields, None));
            }
            continue;
        }
        if parser.eat(&TokenKind::CloseBrace) {
            return Some((fields, None));
        }
        return None;
    }
}

fn parse_unsealed_tail(parser: &mut Parser<'_>, depth: u32) -> Option<UnsealedTail> {
    if !parser.eat(&TokenKind::OpenAngle) {
        return Some(UnsealedTail {
            key: None,
            value: None,
        });
    }
    let first = parse_type(parser, depth + 1)?;
    if parser.eat(&TokenKind::Comma) {
        let second = parse_type(parser, depth + 1)?;
        if parser.eat(&TokenKind::CloseAngle) {
            return Some(UnsealedTail {
                key: Some(Box::new(first)),
                value: Some(Box::new(second)),
            });
        }
        return None;
    }
    if parser.eat(&TokenKind::CloseAngle) {
        return Some(UnsealedTail {
            key: None,
            value: Some(Box::new(first)),
        });
    }
    None
}

/// One field: `key ?': type`, `key: type`, or a keyless tuple entry.
/// Keys are identifiers, string literals, or integer literals — the
/// two-token lookahead (`key ':'` / `key '?' ':'`) is what separates
/// a key from a keyless field whose type happens to be a name.
fn parse_shape_field(parser: &mut Parser<'_>, depth: u32) -> Option<ShapeFieldExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let key = match parser.peek() {
        Some(TokenKind::Name(name)) => Some(ShapeKeyExpression::Identifier(name.clone())),
        Some(TokenKind::StringLiteral(value)) => Some(ShapeKeyExpression::String(value.clone())),
        Some(TokenKind::Integer(value)) => Some(ShapeKeyExpression::Integer(*value)),
        _ => None,
    };
    let keyed = key.is_some()
        && matches!(
            (parser.peek_at(1), parser.peek_at(2)),
            (Some(TokenKind::Colon), _) | (Some(TokenKind::Question), Some(TokenKind::Colon))
        );
    if keyed {
        parser.advance();
        let optional = parser.eat(&TokenKind::Question);
        if !parser.eat(&TokenKind::Colon) {
            return None;
        }
        let value = parse_type(parser, depth + 1)?;
        return Some(ShapeFieldExpression {
            key,
            optional,
            value,
        });
    }
    let value = parse_type(parser, depth + 1)?;
    Some(ShapeFieldExpression {
        key: None,
        optional: false,
        value,
    })
}
```

Import the new types at the top of `parser.rs`:

```rust
use super::{ShapeFieldExpression, ShapeKeyExpression, TypeExpression, UnsealedTail};
```

- [ ] **Step 4: Run to verify green**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS.

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src
git commit -m "✨ feat(phpdoc-bridge): array, list, and object shapes parse"
```

---

### Task 5: Callables, const fetches, `$this`, offsets, and conditionals

`callable(int, string=): bool`, `\Closure(User): static`,
`Closure<T of Foo>(T): T` (template list parsed, names recorded),
`Foo::BAR`, `Foo::*`, `Foo::BAR_*`, `$this`, offset access `T[K]`,
and conditional types (`T is string ? int : bool`,
`($flags is 1 ? A : B)`, `is not`) parse. This completes the grammar;
the module doc's "answers `None`" list empties down to genuine
garbage.

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/mod.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/parser.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (export the new
  support types)

**Interfaces:**
- Produces (consumed by Task 6's lowering table):

```rust
pub enum TypeExpression {
    // ... existing variants ...
    Callable {
        base: String,
        /// Callable-scoped template names — decision 12: their
        /// occurrences inside the signature lower to `mixed`.
        templates: Vec<String>,
        parameters: Vec<CallableParameterExpression>,
        return_type: Box<TypeExpression>,
    },
    ConstFetch { class: String, constant: String },
    This,
    Offset { base: Box<TypeExpression>, offset: Box<TypeExpression> },
    Conditional {
        subject: ConditionalSubject,
        negated: bool,
        target: Box<TypeExpression>,
        then_branch: Box<TypeExpression>,
        otherwise_branch: Box<TypeExpression>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableParameterExpression {
    pub parameter_type: TypeExpression,
    pub by_reference: bool,
    pub variadic: bool,
    pub optional: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalSubject {
    /// A bare name — a template variable if one is in scope at
    /// lowering time, otherwise undecided.
    Template(String),
    /// `$param` — undecided until plan 6 evaluates call sites.
    Parameter(String),
}
```

- Parser additions: `Parser::checkpoint(&self) -> usize`,
  `Parser::rewind(&mut self, checkpoint: usize)`,
  `Parser::peek_token(&self) -> Option<&Token>` (spans, for const-fetch
  adjacency).

- [ ] **Step 1: Write the failing tests**

In `expression/mod.rs` tests:

```rust
    #[test]
    fn callables_parse_with_parameter_flags_and_templates() {
        let Some(TypeExpression::Callable { base, templates, parameters, return_type }) =
            parse_type_expression_text("callable(int, string&$out, User...$rest, bool=): ?string")
        else {
            panic!("expected a callable");
        };
        assert_eq!(base, "callable");
        assert!(templates.is_empty());
        assert_eq!(parameters.len(), 4);
        assert!(parameters[1].by_reference);
        assert!(parameters[2].variadic);
        assert!(parameters[3].optional);
        assert_eq!(
            *return_type,
            TypeExpression::Nullable(Box::new(TypeExpression::Name("string".to_owned()))),
        );

        let Some(TypeExpression::Callable { base, templates, .. }) =
            parse_type_expression_text("\\Closure<T of Foo>(T): T")
        else {
            panic!("expected a closure");
        };
        assert_eq!(base, "\\Closure");
        assert_eq!(templates, vec!["T".to_owned()]);
        // A bare `callable` without a signature stays a plain name.
        assert_eq!(
            parse_type_expression_text("callable"),
            Some(TypeExpression::Name("callable".to_owned())),
        );
        // A generic Closure without a signature stays a generic.
        assert!(matches!(
            parse_type_expression_text("Closure<int>"),
            Some(TypeExpression::Generic { .. })
        ));
    }

    #[test]
    fn const_fetches_this_and_offsets_parse() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("Foo::BAR"),
            Some(ConstFetch { class: "Foo".to_owned(), constant: "BAR".to_owned() }),
        );
        assert_eq!(
            parse_type_expression_text("Foo::*"),
            Some(ConstFetch { class: "Foo".to_owned(), constant: "*".to_owned() }),
        );
        assert_eq!(
            parse_type_expression_text("Foo::BAR_*"),
            Some(ConstFetch { class: "Foo".to_owned(), constant: "BAR_*".to_owned() }),
        );
        assert_eq!(parse_type_expression_text("$this"), Some(This));
        assert_eq!(
            parse_type_expression_text("T[K]"),
            Some(Offset {
                base: Box::new(Name("T".to_owned())),
                offset: Box::new(Name("K".to_owned())),
            }),
        );
        // A lone `$param` is not a type.
        assert_eq!(parse_type_expression_text("$param"), None);
    }

    #[test]
    fn conditional_types_parse_for_both_subjects() {
        let Some(TypeExpression::Conditional { subject, negated, .. }) =
            parse_type_expression_text("T is string ? int : bool")
        else {
            panic!("expected a conditional");
        };
        assert_eq!(subject, ConditionalSubject::Template("T".to_owned()));
        assert!(!negated);

        let Some(TypeExpression::Conditional { subject, negated, then_branch, .. }) =
            parse_type_expression_text("($flags is not 1 ? array<string> : string)")
        else {
            panic!("expected a parameter conditional");
        };
        assert_eq!(subject, ConditionalSubject::Parameter("flags".to_owned()));
        assert!(negated);
        assert!(matches!(*then_branch, TypeExpression::Generic { .. }));
    }
```

Add `"callable(int"` to the answers-`None` test list.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge expression`
Expected: FAIL — the variants do not exist.

- [ ] **Step 3: Implement**

Add the variants and support types to `expression/mod.rs` as in the
Interfaces block; extend the `lib.rs` re-export with
`CallableParameterExpression, ConditionalSubject`.

In `parser.rs`:

1. Add the three `Parser` methods:

```rust
    pub(crate) fn checkpoint(&self) -> usize {
        self.position
    }

    pub(crate) fn rewind(&mut self, checkpoint: usize) {
        self.position = checkpoint;
    }

    pub(crate) fn peek_token(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }
```

2. Replace `parse_type` with the conditional-aware entry:

```rust
pub(crate) fn parse_type(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    // Conditional lookahead: a bare name or `$variable` followed by
    // `is` opens a conditional type; everything else is a union.
    let subject = match (parser.peek(), parser.peek_at(1)) {
        (Some(TokenKind::Variable(name)), Some(TokenKind::Name(keyword))) if keyword == "is" => {
            Some(ConditionalSubject::Parameter(name.clone()))
        }
        (Some(TokenKind::Name(name)), Some(TokenKind::Name(keyword))) if keyword == "is" => {
            Some(ConditionalSubject::Template(name.clone()))
        }
        _ => None,
    };
    if let Some(subject) = subject {
        // Prose can begin with `is` too (`@return Foo is the widget`):
        // a failed conditional rewinds and the plain union stands, so
        // the annotation survives with the prose as remainder.
        let checkpoint = parser.checkpoint();
        if let Some(conditional) = parse_conditional(parser, depth, subject) {
            return Some(conditional);
        }
        parser.rewind(checkpoint);
    }
    parse_union(parser, depth)
}

fn parse_conditional(
    parser: &mut Parser<'_>,
    depth: u32,
    subject: ConditionalSubject,
) -> Option<TypeExpression> {
    parser.advance(); // the subject
    parser.advance(); // `is`
    let negated = matches!(parser.peek(), Some(TokenKind::Name(keyword)) if keyword == "not");
    if negated {
        parser.advance();
    }
    let target = parse_union(parser, depth + 1)?;
    if !parser.eat(&TokenKind::Question) {
        return None;
    }
    let then_branch = parse_type(parser, depth + 1)?;
    if !parser.eat(&TokenKind::Colon) {
        return None;
    }
    let otherwise_branch = parse_type(parser, depth + 1)?;
    Some(TypeExpression::Conditional {
        subject,
        negated,
        target: Box::new(target),
        then_branch: Box::new(then_branch),
        otherwise_branch: Box::new(otherwise_branch),
    })
}
```

3. In `parse_suffixed`, replace the `// Task 5 adds offset access`
comment with the offset arm:

```rust
        if parser.peek() == Some(&TokenKind::OpenBracket) {
            parser.advance();
            let offset = parse_type(parser, depth + 1)?;
            if !parser.eat(&TokenKind::CloseBracket) {
                return None;
            }
            expression = TypeExpression::Offset {
                base: Box::new(expression),
                offset: Box::new(offset),
            };
            continue;
        }
```

(placed after the `[]` arm, so the empty suffix still wins first).

4. In `parse_atom`, add a `Variable` arm and extend the `Name` arm:

```rust
        TokenKind::Variable(name) if name == "this" => {
            parser.advance();
            Some(TypeExpression::This)
        }
```

The `Name` arm becomes (replacing the Task 4 version in full):

```rust
        TokenKind::Name(name) => {
            let name = name.clone();
            parser.advance();
            if parser.eat(&TokenKind::DoubleColon) {
                let constant = parse_constant_name(parser)?;
                return Some(TypeExpression::ConstFetch {
                    class: name,
                    constant,
                });
            }
            if parser.peek() == Some(&TokenKind::OpenAngle) {
                if is_callable_base(&name) {
                    // `Closure<T of Foo>(T): T` — try the callable
                    // template list; rewind to a generic on failure.
                    let checkpoint = parser.checkpoint();
                    parser.advance();
                    if let Some(templates) = parse_callable_templates(parser, depth + 1)
                        && parser.peek() == Some(&TokenKind::OpenParenthesis)
                    {
                        return parse_callable_signature(parser, depth, name, templates);
                    }
                    parser.rewind(checkpoint);
                }
                parser.advance();
                let arguments = parse_generic_arguments(parser, depth + 1)?;
                return Some(TypeExpression::Generic {
                    base: name,
                    arguments,
                });
            }
            if is_callable_base(&name) && parser.peek() == Some(&TokenKind::OpenParenthesis) {
                return parse_callable_signature(parser, depth, name, Vec::new());
            }
            if is_shape_base(&name) && parser.peek() == Some(&TokenKind::OpenBrace) {
                parser.advance();
                let (fields, unsealed) = parse_shape_body(parser, depth + 1)?;
                return Some(TypeExpression::Shape {
                    base: name,
                    fields,
                    unsealed,
                });
            }
            Some(TypeExpression::Name(name))
        }
```

5. Add the helpers:

```rust
/// The bases the reference accepts a `(signature)` on. The purity
/// prefixes lower with their purity dropped (documented).
fn is_callable_base(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase()
            .trim_start_matches('\\'),
        "callable" | "closure" | "pure-callable" | "pure-closure"
    )
}

/// `Foo::BAR`, `Foo::*`, `Foo::BAR_*`: the constant is the adjacent
/// run of name and `*` tokens after the `::` (adjacency by byte
/// offset — whitespace breaks the run).
fn parse_constant_name(parser: &mut Parser<'_>) -> Option<String> {
    let mut constant = String::new();
    let mut previous_end: Option<usize> = None;
    while let Some(token) = parser.peek_token() {
        if previous_end.is_some_and(|end| token.start != end) {
            break;
        }
        match &token.kind {
            TokenKind::Name(part) => constant.push_str(part),
            TokenKind::Asterisk => constant.push('*'),
            _ => break,
        }
        previous_end = Some(token.end);
        parser.advance();
    }
    if constant.is_empty() {
        None
    } else {
        Some(constant)
    }
}

/// The `<T, U of Bound>` list of a callable. Bounds are parsed and
/// dropped (decision 12); the caller has already consumed the `<`.
fn parse_callable_templates(parser: &mut Parser<'_>, depth: u32) -> Option<Vec<String>> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut templates = Vec::new();
    loop {
        let Some(TokenKind::Name(name)) = parser.peek() else {
            return None;
        };
        templates.push(name.clone());
        parser.advance();
        if let Some(TokenKind::Name(keyword)) = parser.peek()
            && (keyword == "of" || keyword == "as")
        {
            parser.advance();
            let _bound = parse_type(parser, depth + 1)?;
        }
        if parser.eat(&TokenKind::Comma) {
            if parser.eat(&TokenKind::CloseAngle) {
                return Some(templates);
            }
            continue;
        }
        if parser.eat(&TokenKind::CloseAngle) {
            return Some(templates);
        }
        return None;
    }
}

/// `(type [&] [...] [$name] [=], ...) : return`. Parameter names are
/// parsed and dropped (the lattice's `CallableParameter` carries
/// none); the return type is required, per the reference.
fn parse_callable_signature(
    parser: &mut Parser<'_>,
    depth: u32,
    base: String,
    templates: Vec<String>,
) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    if !parser.eat(&TokenKind::OpenParenthesis) {
        return None;
    }
    let mut parameters = Vec::new();
    if !parser.eat(&TokenKind::CloseParenthesis) {
        loop {
            let parameter_type = parse_type(parser, depth + 1)?;
            let by_reference = parser.eat(&TokenKind::Ampersand);
            let variadic = parser.eat(&TokenKind::Ellipsis);
            if matches!(parser.peek(), Some(TokenKind::Variable(_))) {
                parser.advance();
            }
            let optional = parser.eat(&TokenKind::Equals);
            parameters.push(CallableParameterExpression {
                parameter_type,
                by_reference,
                variadic,
                optional,
            });
            if parser.eat(&TokenKind::Comma) {
                continue;
            }
            if parser.eat(&TokenKind::CloseParenthesis) {
                break;
            }
            return None;
        }
    }
    if !parser.eat(&TokenKind::Colon) {
        return None;
    }
    let return_type = parse_suffixed(parser, depth + 1)?;
    Some(TypeExpression::Callable {
        base,
        templates,
        parameters,
        return_type: Box::new(return_type),
    })
}
```

Extend the `use super::{...}` import with
`CallableParameterExpression, ConditionalSubject`.

- [ ] **Step 4: Run to verify green**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS.

- [ ] **Step 5: Update the module doc**

The grammar is complete: rewrite `expression/mod.rs`'s module doc to
describe the dialect grammar (union, intersection, nullable-inside-
suffix, `[]` and offset suffixes, generics with dropped call-site
variance, shapes with unsealed tails, callables with required returns
and dropped parameter names, const fetches, `$this`, conditionals) and
state what answers `None`: out-of-grammar text, unterminated
constructs, nesting past the depth guard — per construct, never per
annotation.

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src
git commit -m "✨ feat(phpdoc-bridge): callables, const fetches, offsets, and conditional types parse"
```

---

### Task 6: The tag layer moves onto prefix parsing

The whitespace splitter dies (plan 4a's decision 6 mandated exactly
this): tag contents parse their type as a maximal prefix, so
space-bearing dialect types (`array{id: int}`, `int<1, max>`,
`callable(int): bool`) flow through `@param`/`@return`/`@var`/`@throws`
and through `@property`/`@method` verbatim `type_text` slices.
`@method` gains matching-parenthesis and depth-aware comma handling,
lifting 4a's no-nested-parentheses restriction.

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/tags.rs`
- Test: inline tests in `tags.rs`

**Interfaces:**
- Consumes: `parse_type_expression_prefix` (Task 2).
- Produces: `MemberDocblock` and `extract_*` signatures unchanged —
  only the content grammar changes. Two internal helpers later tasks
  reuse: `split_at_matching_parenthesis(&str) -> Option<(&str, &str)>`
  and `split_top_level_commas(&str) -> Vec<&str>`.

- [ ] **Step 1: Write the failing tests**

In `tags.rs` tests add:

```rust
    #[test]
    fn dialect_types_with_spaces_extract() {
        let tags = lex_docblock(
            "/**\n * @param array{id: int, name?: string} $subject\n * @return array<int, string> the rows\n * @var int<1, max>\n * @throws \\RuntimeException\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.parameters.len(), 1);
        assert_eq!(extracted.parameters[0].0, "subject");
        assert!(matches!(
            extracted.parameters[0].1,
            TypeExpression::Shape { .. }
        ));
        assert!(matches!(
            extracted.return_type,
            Some(TypeExpression::Generic { .. })
        ));
        assert!(matches!(
            extracted.value_type,
            Some(TypeExpression::Generic { .. })
        ));
        assert_eq!(extracted.throws.len(), 1);
    }

    #[test]
    fn method_tags_carry_dialect_types_and_nested_parentheses() {
        let tags = lex_docblock(
            "/** @method static Collection<User> map(callable(User): string $mapper, array{limit?: int} $options = []) */",
        );
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        let map = &members[0];
        assert!(map.is_static);
        assert_eq!(map.type_text.as_deref(), Some("Collection<User>"));
        assert_eq!(map.parameters.len(), 2);
        assert_eq!(map.parameters[0].name, "mapper");
        assert_eq!(
            map.parameters[0].type_text.as_deref(),
            Some("callable(User): string"),
        );
        assert_eq!(map.parameters[1].name, "options");
        assert!(map.parameters[1].optional);
        assert_eq!(
            map.parameters[1].type_text.as_deref(),
            Some("array{limit?: int}"),
        );
    }

    #[test]
    fn property_tags_carry_dialect_types_verbatim() {
        let tags = lex_docblock("/** @property array{id: int} $row */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "row");
        assert_eq!(members[0].type_text.as_deref(), Some("array{id: int}"));
    }
```

And **update `malformed_tags_are_ignored_per_construct`**: its
"unparseable" fixture `@param array<int> $broken` now parses — replace
that line of the fixture docblock with `@param array{ $broken`
(an unterminated shape stays out of the grammar permanently). The
expected name list `["good", "reference", "rest"]` is unchanged.

- [ ] **Step 2: Run to verify the new tests fail**

Run: `cargo test --package celerrate_phpdoc_bridge tags`
Expected: FAIL — the whitespace splitter cannot see past the first
space of `array{id: int}`.

- [ ] **Step 3: Rewrite the content grammar in `tags.rs`**

Replace `first_token_type`, `parse_param_tag`, `parse_property_tag`,
`parse_method_tag`, and `parse_method_parameter(s)` with the
prefix-parsing forms. `strip_variable_sigils` and
`is_valid_identifier` stay as they are. Change the import line to:

```rust
use crate::{Tag, TypeExpression, parse_type_expression_prefix};
```

(`parse_type_expression_text` is no longer used here.)

```rust
/// The tag's value slot: a maximal type-expression prefix; trailing
/// prose is free text.
fn value_type(content: &str) -> Option<TypeExpression> {
    let (expression, _) = parse_type_expression_prefix(content)?;
    Some(expression)
}

/// `@param type $name ...prose` (or `&$name` / `...$name` when the
/// type is omitted, in which case there is nothing to contribute).
/// The first tag for a given parameter name wins; later duplicates
/// are dropped.
fn parse_param_tag(content: &str, seen: &mut HashSet<String>) -> Option<(String, TypeExpression)> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('$') || trimmed.starts_with("...$") || trimmed.starts_with("&$") {
        return None;
    }
    let (type_expression, consumed) = parse_type_expression_prefix(content)?;
    let remainder = content.get(consumed..)?;
    let variable_token = remainder.split_whitespace().next()?;
    let name = strip_variable_sigils(variable_token)?;
    if seen.contains(&name) {
        return None;
    }
    seen.insert(name.clone());
    Some((name, type_expression))
}

/// `@property[-read|-write] [type] $name`: a leading `$name` means
/// untyped (the member still exists). `type_text` stores the consumed
/// prefix verbatim: unresolved text is the virtual-symbol contract.
fn parse_property_tag(content: &str) -> Option<VirtualMember> {
    let first_word = content.split_whitespace().next()?;
    if let Some(name) = first_word.strip_prefix('$') {
        if name.is_empty() {
            return None;
        }
        return Some(VirtualMember {
            kind: VirtualMemberKind::Property,
            name: name.to_owned(),
            is_static: false,
            type_text: None,
            parameters: Vec::new(),
        });
    }
    let (_, consumed) = parse_type_expression_prefix(content)?;
    let type_text = content.get(..consumed)?.trim().to_owned();
    let remainder = content.get(consumed..)?;
    let name = remainder.split_whitespace().next()?.strip_prefix('$')?;
    if name.is_empty() {
        return None;
    }
    Some(VirtualMember {
        kind: VirtualMemberKind::Property,
        name: name.to_owned(),
        is_static: false,
        type_text: Some(type_text),
        parameters: Vec::new(),
    })
}

/// `@method [static] [type] name(parameters)`. The return type is a
/// dialect prefix taken verbatim; when the prefix turns out to be the
/// method name itself (the next character is `(`), the method is
/// untyped. The parameter segment ends at the matching parenthesis,
/// so callable parameters nest.
fn parse_method_tag(content: &str) -> Option<VirtualMember> {
    let trimmed = content.trim_start();
    let (is_static, after_static) = match trimmed.strip_prefix("static") {
        Some(rest) if rest.starts_with(char::is_whitespace) => (true, rest.trim_start()),
        _ => (false, trimmed),
    };
    let (type_text, rest) = match parse_type_expression_prefix(after_static) {
        Some((_, consumed)) => {
            let text = after_static.get(..consumed)?.trim();
            let after_type = after_static.get(consumed..)?.trim_start();
            if after_type.starts_with('(') {
                (None, after_static)
            } else {
                (Some(text.to_owned()), after_type)
            }
        }
        None => (None, after_static),
    };
    let open = rest.find('(')?;
    let name = rest.get(..open)?.trim();
    if !is_valid_identifier(name) {
        return None;
    }
    let after_open = rest.get(open + 1..)?;
    let (parameter_segment, _) = split_at_matching_parenthesis(after_open)?;
    Some(VirtualMember {
        kind: VirtualMemberKind::Method,
        name: name.to_owned(),
        is_static,
        type_text,
        parameters: parse_method_parameters(parameter_segment),
    })
}

/// Splits `text` at the parenthesis matching an already-consumed `(`:
/// the segment before it, and the remainder after it. `None` when
/// unbalanced.
fn split_at_matching_parenthesis(text: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (offset, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some((text.get(..offset)?, text.get(offset + 1..)?));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn parse_method_parameters(segment: &str) -> Vec<VirtualParameter> {
    split_top_level_commas(segment)
        .into_iter()
        .filter_map(parse_method_parameter)
        .collect()
}

/// Top-level comma split, depth-aware across `()<>{}[]`, so callable
/// signatures, generics, shapes, and array defaults ride inside one
/// parameter chunk.
fn split_top_level_commas(segment: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut depth = 0i64;
    let mut start = 0usize;
    for (offset, character) in segment.char_indices() {
        match character {
            '(' | '<' | '{' | '[' => depth += 1,
            ')' | '>' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                if let Some(chunk) = segment.get(start..offset) {
                    chunks.push(chunk);
                }
                start = offset + 1;
            }
            _ => {}
        }
    }
    if let Some(chunk) = segment.get(start..) {
        chunks.push(chunk);
    }
    chunks.retain(|chunk| !chunk.trim().is_empty());
    chunks
}

/// `[type] $name [= default]`: the type is a dialect prefix taken
/// verbatim; a `=` after the name marks `optional`, a `...$` prefix
/// marks `variadic`. A by-reference `&$name` drops, as in 4a.
fn parse_method_parameter(chunk: &str) -> Option<VirtualParameter> {
    let trimmed = chunk.trim();
    let (type_text, rest) = if trimmed.starts_with('$') || trimmed.starts_with("...$") {
        (None, trimmed)
    } else {
        let (_, consumed) = parse_type_expression_prefix(trimmed)?;
        let text = trimmed.get(..consumed)?.trim();
        let rest = trimmed.get(consumed..)?.trim_start();
        (Some(text.to_owned()), rest)
    };
    let optional = rest.contains('=');
    let name_token = rest.split_whitespace().next()?;
    let variadic = name_token.starts_with("...$");
    let name = if variadic {
        name_token.strip_prefix("...$")?
    } else {
        name_token.strip_prefix('$')?
    };
    // A space-less default (`$x=5`) rides on the name token: the name
    // stops at the first `=`.
    let name = name.split_once('=').map_or(name, |(head, _)| head).trim();
    if name.is_empty() {
        return None;
    }
    Some(VirtualParameter {
        name: name.to_owned(),
        type_text,
        optional,
        variadic,
    })
}
```

In `extract_member_docblock`, replace the three `first_token_type`
call sites with `value_type` (same slot-filling logic, unchanged
otherwise). Update the module doc comment: the whitespace splitter is
gone; contents parse a maximal type prefix.

- [ ] **Step 4: Run the bridge test suite**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS — the three new tests and every pre-existing tag test
(`method_parameter_names_stop_at_the_default`,
`a_method_tag_can_return_the_static_type`, the property and method
declaration tests, the malformed-tags test with its updated fixture).

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src/tags.rs
git commit -m "♻️ refactor(phpdoc-bridge): tag contents parse a maximal type prefix, the splitter dies"
```

---

### Task 7: The total lowering table

Every parsed construct lowers: a lattice builder or a documented sound
widening, concentrated in one module whose rustdoc **is** the lowering
table. The e2e suite pins each table family through the real
declared-signature seam.

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/lowering.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/syntax.rs` (delete its
  private `lower`, call the new module)
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (`mod lowering;`)
- Test: `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs`

**Interfaces:**
- Consumes: the complete `TypeExpression` (Tasks 2-5),
  `AnnotationSite::{database, keyword_type, qualify_class_name}`,
  the `TypeId` builders re-exported by `celerrate_plugin`
  (`class, union, intersection, array, non_empty_array, list,
  non_empty_list, shape, callable, class_string, int_range,
  int_literal, float_literal, string_literal, key_of, value_of,
  iterable, static_placeholder, mixed, never, object, resource,
  non_empty_string, numeric_string, literal_string_type, int, float,
  string, bool, null`), and `CallableParameter`, `ShapeField`,
  `ShapeKey`.
  Verify these are all present in `celerrate_plugin`'s re-export list
  (`crates/celerrate_plugin/src/lib.rs:34-37` re-exports `TypeId`,
  whose associated functions carry all builders — only the three
  support structs need to be named in the `use`).
- Produces (crate-internal; Task 9 extends `LoweringScope` with the
  docblock template set):

```rust
pub(crate) struct LoweringScope { /* callable-template names, Task 9 adds docblock templates */ }
pub(crate) fn lower<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope,
    expression: &TypeExpression,
) -> TypeId<'db>
```

- [ ] **Step 1: Write the failing e2e tests**

Append to `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs` (the
existing `fixture`/`member_query`/`register_bridge` helpers carry
everything; every annotated method below has no native type, so
refinement runs against `mixed` and any parsed annotation lands as
`Trust::Refined`):

```rust
#[test]
fn dialect_atoms_and_generics_lower_through_the_table() {
    let fixture = fixture(&[
        "<?php class C {\n\
         /** @return array<int, string> */ public function a() {}\n\
         /** @return positive-int */ public function b() {}\n\
         /** @return 'yes'|'no' */ public function c() {}\n\
         /** @return int<1, max> */ public function d() {}\n\
         /** @return class-string<\\App\\User> */ public function e() {}\n\
         /** @return array-key */ public function f() {}\n\
         /** @return non-empty-list<string> */ public function g() {}\n\
         /** @return iterable<User> */ public function h() {}\n\
         }",
        "<?php class User {}",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let value = |name: &str| {
        let query = member_query(&fixture, "C", MemberKind::Method, name);
        celerrate_types::declared_member_signature(
            db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap()
        .value_type
    };
    use celerrate_types::TypeId;
    assert_eq!(
        value("a"),
        TypeId::array(db, TypeId::int(db), TypeId::string(db)),
    );
    assert_eq!(value("b"), TypeId::int_range(db, Some(1), None));
    assert_eq!(
        value("c"),
        TypeId::union(db, [
            TypeId::string_literal(db, "yes"),
            TypeId::string_literal(db, "no"),
        ]),
    );
    assert_eq!(value("d"), TypeId::int_range(db, Some(1), None));
    assert_eq!(
        value("e"),
        TypeId::class_string(db, Some(TypeId::class(db, "App\\User", Vec::new()))),
    );
    assert_eq!(
        value("f"),
        TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
    );
    assert_eq!(
        value("g"),
        TypeId::non_empty_list(db, TypeId::string(db)),
    );
    assert_eq!(
        value("h"),
        TypeId::iterable(db, TypeId::mixed(db), TypeId::class(db, "User", Vec::new())),
    );
}

#[test]
fn shapes_callables_and_the_documented_widenings_lower() {
    let fixture = fixture(&[
        "<?php class C {\n\
         /** @return array{id: int, name?: string} */ public function a() {}\n\
         /** @return array{id: int, ...} */ public function b() {}\n\
         /** @return array{id: int, ...<string, bool>} */ public function c() {}\n\
         /** @return object{a: int} */ public function d() {}\n\
         /** @return callable(int, string=): bool */ public function e() {}\n\
         /** @return $this */ public function f() {}\n\
         /** @return Foo::BAR */ public function g() {}\n\
         /** @return ($flags is 1 ? string : bool) */ public function h() {}\n\
         /** @return \\Closure<T of Mode>(T): T */ public function i() {}\n\
         }",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let value = |name: &str| {
        let query = member_query(&fixture, "C", MemberKind::Method, name);
        celerrate_types::declared_member_signature(
            db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap()
        .value_type
    };
    use celerrate_types::{CallableParameter, ShapeField, ShapeKey, TypeId};
    let array_key = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
    assert_eq!(
        value("a"),
        TypeId::shape(db, vec![
            ShapeField {
                key: ShapeKey::String("id".to_owned()),
                optional: false,
                value: TypeId::int(db),
            },
            ShapeField {
                key: ShapeKey::String("name".to_owned()),
                optional: true,
                value: TypeId::string(db),
            },
        ]),
    );
    // Unsealed shapes give up their field knowledge: the documented
    // widening is the general (non-empty when a field is required)
    // array — a supertype, never a truncation into wrongness.
    assert_eq!(
        value("b"),
        TypeId::non_empty_array(db, array_key, TypeId::mixed(db)),
    );
    assert_eq!(
        value("c"),
        TypeId::non_empty_array(
            db,
            array_key,
            TypeId::union(db, [TypeId::int(db), TypeId::bool(db)]),
        ),
    );
    // No object-shape lattice form: `object` is the widening.
    assert_eq!(value("d"), TypeId::object(db));
    assert_eq!(
        value("e"),
        TypeId::callable(
            db,
            vec![
                CallableParameter {
                    parameter_type: TypeId::int(db),
                    optional: false,
                    variadic: false,
                    by_reference: false,
                },
                CallableParameter {
                    parameter_type: TypeId::string(db),
                    optional: true,
                    variadic: false,
                    by_reference: false,
                },
            ],
            TypeId::bool(db),
        ),
    );
    // `@return $this` collapses into `static` (design section 3).
    assert_eq!(value("f"), TypeId::static_placeholder(db));
    // Constant fetches await member facts: `mixed`, documented.
    assert_eq!(value("g"), TypeId::mixed(db));
    // Parameter-subject conditionals: the undecided branch union.
    assert_eq!(
        value("h"),
        TypeId::union(db, [TypeId::string(db), TypeId::bool(db)]),
    );
    // Callable-scoped templates lower their occurrences to `mixed`.
    assert_eq!(
        value("i"),
        TypeId::callable(
            db,
            vec![CallableParameter {
                parameter_type: TypeId::mixed(db),
                optional: false,
                variadic: false,
                by_reference: false,
            }],
            TypeId::mixed(db),
        ),
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge --test end_to_end`
Expected: FAIL — the new variants reach `syntax.rs`'s `lower`, which
does not know them (non-exhaustive match compile error is the expected
form of failure once the variants exist; the test drives writing the
table).

- [ ] **Step 3: Write the lowering module**

Create `crates/celerrate_phpdoc_bridge/src/lowering.rs`:

```rust
//! The total lowering table (decision 3): every parsed construct maps
//! to a lattice value or a documented sound widening — a supertype,
//! never a subtype, so a widening can silence but never mis-report.
//!
//! | construct | lowering |
//! |---|---|
//! | names: native keywords | the shared keyword table (`AnnotationSite::keyword_type`) |
//! | `list`, `non-empty-list`, `non-empty-array`, `associative-array` | their builders over `mixed` |
//! | `non-empty-string`, `numeric-string`, `literal-string` | their builders |
//! | `class-string[<T>]` | `class_string` (the template argument is never severed) |
//! | `interface-string[<T>]`, `enum-string[<T>]`, `trait-string[<T>]` | `class_string` (kind refinement: recorded debt) |
//! | `callable-string` | `non-empty-string` (widening) |
//! | `lowercase-string`, `uppercase-string` | `string` (widening) |
//! | `non-falsy-string`, `truthy-string` | `non-empty-string` (widening) |
//! | `literal-int` | `int` (no literal-int marker: widening) |
//! | `positive-int`, `negative-int`, `non-negative-int`, `non-positive-int` | `int_range` |
//! | `int<a, b>` (`min`/`max` open ends) | `int_range`; a non-literal bound widens to `int` |
//! | `int-mask<...>`, `int-mask-of<...>` | `int` (widening) |
//! | `array-key` | `int\|string` |
//! | `scalar` | `bool\|int\|float\|string`; `numeric` | `int\|float\|numeric-string` |
//! | `double`/`integer`/`boolean` | the PHP aliases |
//! | `noreturn`, `no-return`, `never-return`, `never-returns` | `never` |
//! | `non-empty-mixed` | `mixed`; `open-resource`, `closed-resource` | `resource` |
//! | `pure-callable` | `mixed` (the bare-callable widening); `pure-Closure` | `Closure` |
//! | `callable-object` | `object` (widening) |
//! | literals | `int_literal`/`float_literal`/`string_literal` (an unparseable float text widens to `float`) |
//! | `array<K, V>`, `list<V>`, `iterable<K, V>` and the non-empty forms | their builders; wrong arity widens the slots to their defaults |
//! | `key-of<T>`, `value-of<T>` | their builders |
//! | sealed shapes | `shape` (keyless tuple fields number sequentially; identifier keys are string keys) |
//! | unsealed shapes | the general array (`non_empty_array` when a field is required): key `int\|string`, value = the field-and-tail union (`mixed` for a bare `...`) — widening |
//! | `object{...}` | `object` (widening) |
//! | callables (`callable`, `Closure`, purity prefixes) | `callable` (purity and Closure classness drop: widening); callable-scoped template names lower to `mixed` (decision 12) |
//! | `Foo::BAR`, `Foo::*` | `mixed` (constant and enum-case facts arrive with plans 6-7: recorded debt) |
//! | `$this` | `static` (design section 3) |
//! | offset access `T[K]` | `mixed` (widening) |
//! | conditionals | `conditional` for an in-scope template subject (Task 9); otherwise the undecided branch union (design section 3) |
//! | a keyword or dialect atom with a spurious `<...>` list | the atom, arguments dropped |
//! | any other name | a class type, qualified at the declaring site |

use celerrate_plugin::{AnnotationSite, CallableParameter, ShapeField, ShapeKey, TypeId};

use crate::expression::{
    ConditionalSubject, ShapeKeyExpression, TypeExpression, UnsealedTail,
};

/// The name-resolution scope one docblock lowers under. Task 9 adds
/// the docblock template set; today it carries only the
/// callable-scoped names active while lowering one signature.
#[derive(Debug, Default)]
pub(crate) struct LoweringScope {
    callable_templates: Vec<String>,
}

pub(crate) fn lower<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope,
    expression: &TypeExpression,
) -> TypeId<'db> {
    let db = site.database();
    match expression {
        TypeExpression::Name(name) => lower_name(site, scope, name),
        TypeExpression::Nullable(inner) => {
            TypeId::union(db, [lower(site, scope, inner), TypeId::null(db)])
        }
        TypeExpression::Union(parts) => {
            let mut lowered = Vec::with_capacity(parts.len());
            for part in parts {
                lowered.push(lower(site, scope, part));
            }
            TypeId::union(db, lowered)
        }
        TypeExpression::Intersection(parts) => {
            let mut lowered = Vec::with_capacity(parts.len());
            for part in parts {
                lowered.push(lower(site, scope, part));
            }
            TypeId::intersection(db, lowered)
        }
        TypeExpression::ArrayOf(element) => {
            TypeId::array(db, array_key(db), lower(site, scope, element))
        }
        TypeExpression::IntLiteral(value) => TypeId::int_literal(db, *value),
        TypeExpression::FloatLiteral(text) => text
            .parse::<f64>()
            .map(|value| TypeId::float_literal(db, value))
            .unwrap_or_else(|_| TypeId::float(db)),
        TypeExpression::StringLiteral(value) => TypeId::string_literal(db, value),
        TypeExpression::Generic { base, arguments } => {
            lower_generic(site, scope, base, arguments)
        }
        TypeExpression::Shape {
            base,
            fields,
            unsealed,
        } => lower_shape(site, scope, base, fields, unsealed.as_ref()),
        TypeExpression::Callable {
            templates,
            parameters,
            return_type,
            ..
        } => lower_callable(site, scope, templates, parameters, return_type),
        TypeExpression::ConstFetch { .. } => TypeId::mixed(db),
        TypeExpression::This => TypeId::static_placeholder(db),
        TypeExpression::Offset { .. } => TypeId::mixed(db),
        TypeExpression::Conditional {
            subject,
            negated,
            target,
            then_branch,
            otherwise_branch,
        } => lower_conditional(
            site,
            scope,
            subject,
            *negated,
            target,
            then_branch,
            otherwise_branch,
        ),
    }
}

fn array_key<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    TypeId::union(db, [TypeId::int(db), TypeId::string(db)])
}

fn lower_name<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope,
    name: &str,
) -> TypeId<'db> {
    let db = site.database();
    if scope
        .callable_templates
        .iter()
        .any(|template| template == name)
    {
        return TypeId::mixed(db);
    }
    // Task 9 resolves the docblock template set here, before keywords.
    if let Some(keyword) = site.keyword_type(name) {
        return keyword;
    }
    if let Some(dialect) = lower_dialect_name(db, name) {
        return dialect;
    }
    TypeId::class(db, &site.qualify_class_name(name), Vec::new())
}

/// The dialect atom table, folded ASCII-case-insensitively like the
/// native keyword table. `None` means "an ordinary class name".
fn lower_dialect_name<'db>(db: &'db dyn salsa::Database, name: &str) -> Option<TypeId<'db>> {
    let folded = name.to_ascii_lowercase();
    Some(match folded.as_str() {
        "list" => TypeId::list(db, TypeId::mixed(db)),
        "non-empty-list" => TypeId::non_empty_list(db, TypeId::mixed(db)),
        "non-empty-array" => TypeId::non_empty_array(db, array_key(db), TypeId::mixed(db)),
        "associative-array" => TypeId::array(db, array_key(db), TypeId::mixed(db)),
        "non-empty-string" => TypeId::non_empty_string(db),
        "numeric-string" => TypeId::numeric_string(db),
        "literal-string" => TypeId::literal_string_type(db),
        "class-string" | "interface-string" | "enum-string" | "trait-string" => {
            TypeId::class_string(db, None)
        }
        "callable-string" => TypeId::non_empty_string(db),
        "lowercase-string" | "uppercase-string" => TypeId::string(db),
        "non-falsy-string" | "truthy-string" => TypeId::non_empty_string(db),
        "literal-int" => TypeId::int(db),
        "positive-int" => TypeId::int_range(db, Some(1), None),
        "negative-int" => TypeId::int_range(db, None, Some(-1)),
        "non-negative-int" => TypeId::int_range(db, Some(0), None),
        "non-positive-int" => TypeId::int_range(db, None, Some(0)),
        "array-key" => array_key(db),
        "scalar" => TypeId::union(
            db,
            [
                TypeId::bool(db),
                TypeId::int(db),
                TypeId::float(db),
                TypeId::string(db),
            ],
        ),
        "numeric" => TypeId::union(
            db,
            [TypeId::int(db), TypeId::float(db), TypeId::numeric_string(db)],
        ),
        "double" => TypeId::float(db),
        "integer" => TypeId::int(db),
        "boolean" => TypeId::bool(db),
        "noreturn" | "no-return" | "never-return" | "never-returns" => TypeId::never(db),
        "non-empty-mixed" => TypeId::mixed(db),
        "open-resource" | "closed-resource" => TypeId::resource(db),
        "pure-callable" => TypeId::mixed(db),
        "pure-closure" => TypeId::class(db, "Closure", Vec::new()),
        "callable-object" => TypeId::object(db),
        _ => return None,
    })
}

fn lower_generic<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope,
    base: &str,
    arguments: &[TypeExpression],
) -> TypeId<'db> {
    let db = site.database();
    let folded = base.to_ascii_lowercase();
    // `int<a, b>` reads its bounds at the expression level: a lowered
    // bound would already have lost `min`/`max`.
    if folded == "int" {
        if let (Some(minimum), Some(maximum)) = (
            range_bound(arguments.first()),
            range_bound(arguments.get(1)),
        ) && arguments.len() == 2
        {
            return TypeId::int_range(db, minimum, maximum);
        }
        return TypeId::int(db);
    }
    if folded == "int-mask" || folded == "int-mask-of" {
        return TypeId::int(db);
    }
    let mut lowered = Vec::with_capacity(arguments.len());
    for argument in arguments {
        lowered.push(lower(site, scope, argument));
    }
    match (folded.as_str(), lowered.as_slice()) {
        ("array", [value]) => TypeId::array(db, array_key(db), *value),
        ("array", [key, value]) => TypeId::array(db, *key, *value),
        ("array", _) => TypeId::array(db, array_key(db), TypeId::mixed(db)),
        ("non-empty-array", [value]) => TypeId::non_empty_array(db, array_key(db), *value),
        ("non-empty-array", [key, value]) => TypeId::non_empty_array(db, *key, *value),
        ("non-empty-array", _) => {
            TypeId::non_empty_array(db, array_key(db), TypeId::mixed(db))
        }
        ("list", [value]) => TypeId::list(db, *value),
        ("list", _) => TypeId::list(db, TypeId::mixed(db)),
        ("non-empty-list", [value]) => TypeId::non_empty_list(db, *value),
        ("non-empty-list", _) => TypeId::non_empty_list(db, TypeId::mixed(db)),
        ("iterable", [value]) => TypeId::iterable(db, TypeId::mixed(db), *value),
        ("iterable", [key, value]) => TypeId::iterable(db, *key, *value),
        ("iterable", _) => TypeId::iterable(db, TypeId::mixed(db), TypeId::mixed(db)),
        (
            "class-string" | "interface-string" | "enum-string" | "trait-string",
            [argument],
        ) => TypeId::class_string(db, Some(*argument)),
        ("class-string" | "interface-string" | "enum-string" | "trait-string", _) => {
            TypeId::class_string(db, None)
        }
        ("key-of", [subject]) => TypeId::key_of(db, *subject),
        ("value-of", [subject]) => TypeId::value_of(db, *subject),
        ("key-of" | "value-of", _) => TypeId::mixed(db),
        _ => {
            if site.keyword_type(base).is_some() || lower_dialect_name(db, base).is_some() {
                // A keyword or dialect atom with a spurious argument
                // list: the atom stands, the arguments drop.
                lower_name(site, scope, base)
            } else {
                TypeId::class(db, &site.qualify_class_name(base), lowered)
            }
        }
    }
}

/// `int<a, b>` bounds: an integer literal, or `min`/`max` for an open
/// end. Anything else invalidates the range and the construct widens
/// to plain `int`.
fn range_bound(argument: Option<&TypeExpression>) -> Option<Option<i64>> {
    match argument? {
        TypeExpression::IntLiteral(value) => Some(Some(*value)),
        TypeExpression::Name(name)
            if name.eq_ignore_ascii_case("min") || name.eq_ignore_ascii_case("max") =>
        {
            Some(None)
        }
        _ => None,
    }
}

fn lower_shape<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope,
    base: &str,
    fields: &[crate::expression::ShapeFieldExpression],
    unsealed: Option<&UnsealedTail>,
) -> TypeId<'db> {
    let db = site.database();
    if base.eq_ignore_ascii_case("object") {
        return TypeId::object(db);
    }
    if let Some(tail) = unsealed {
        let mut values = Vec::with_capacity(fields.len() + 1);
        for field in fields {
            values.push(lower(site, scope, &field.value));
        }
        let value = match tail.value.as_deref() {
            Some(tail_value) => {
                values.push(lower(site, scope, tail_value));
                TypeId::union(db, values)
            }
            None => TypeId::mixed(db),
        };
        let key = array_key(db);
        return if fields.iter().any(|field| !field.optional) {
            TypeId::non_empty_array(db, key, value)
        } else {
            TypeId::array(db, key, value)
        };
    }
    let mut next_index: i64 = 0;
    let mut lowered_fields = Vec::with_capacity(fields.len());
    for field in fields {
        let key = match &field.key {
            Some(ShapeKeyExpression::Integer(value)) => {
                if *value >= next_index {
                    next_index = value.saturating_add(1);
                }
                ShapeKey::Integer(*value)
            }
            Some(ShapeKeyExpression::String(value)) => ShapeKey::String(value.clone()),
            Some(ShapeKeyExpression::Identifier(name)) => ShapeKey::String(name.clone()),
            None => {
                let key = ShapeKey::Integer(next_index);
                next_index = next_index.saturating_add(1);
                key
            }
        };
        lowered_fields.push(ShapeField {
            key,
            optional: field.optional,
            value: lower(site, scope, &field.value),
        });
    }
    TypeId::shape(db, lowered_fields)
}

fn lower_callable<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope,
    templates: &[String],
    parameters: &[crate::expression::CallableParameterExpression],
    return_type: &TypeExpression,
) -> TypeId<'db> {
    let db = site.database();
    let before = scope.callable_templates.len();
    scope.callable_templates.extend(templates.iter().cloned());
    let mut lowered_parameters = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        lowered_parameters.push(CallableParameter {
            parameter_type: lower(site, scope, &parameter.parameter_type),
            optional: parameter.optional,
            variadic: parameter.variadic,
            by_reference: parameter.by_reference,
        });
    }
    let lowered_return = lower(site, scope, return_type);
    scope.callable_templates.truncate(before);
    TypeId::callable(db, lowered_parameters, lowered_return)
}

#[allow(clippy::too_many_arguments)]
fn lower_conditional<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope,
    subject: &ConditionalSubject,
    negated: bool,
    target: &TypeExpression,
    then_branch: &TypeExpression,
    otherwise_branch: &TypeExpression,
) -> TypeId<'db> {
    let db = site.database();
    let then_lowered = lower(site, scope, then_branch);
    let otherwise_lowered = lower(site, scope, otherwise_branch);
    // Task 9 resolves `ConditionalSubject::Template` through the
    // docblock template scope into `TypeId::conditional`. Until then
    // (and permanently for parameter subjects — plan 6's debt), the
    // undecided fallback is the branch union (design section 3).
    let _ = (subject, negated, target);
    TypeId::union(db, [then_lowered, otherwise_lowered])
}
```

In `syntax.rs`: delete the private `lower` function and its imports of
`TypeExpression`; add `use crate::lowering::{LoweringScope, lower};`;
rewrite the two trait methods to thread a per-docblock scope:

```rust
    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db> {
        let tags = lex_docblock(docblock);
        let extracted = extract_member_docblock(&tags);
        let mut scope = LoweringScope::default();
        let return_type = extracted
            .return_type
            .as_ref()
            .map(|expression| lower(site, &mut scope, expression));
        let value_type = extracted
            .value_type
            .as_ref()
            .map(|expression| lower(site, &mut scope, expression));
        let parameters = extracted
            .parameters
            .iter()
            .map(|(name, expression)| (name.clone(), lower(site, &mut scope, expression)))
            .collect();
        let throws = extracted
            .throws
            .iter()
            .map(|expression| lower(site, &mut scope, expression))
            .collect();
        ParsedAnnotations {
            return_type,
            value_type,
            parameters,
            throws,
        }
    }

    fn parse_type_expression<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<TypeId<'db>> {
        let parsed = crate::expression::parse_type_expression_text(expression)?;
        let mut scope = LoweringScope::default();
        Some(lower(site, &mut scope, &parsed))
    }
```

Add `mod lowering;` to `lib.rs`.

- [ ] **Step 4: Run to verify green**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS — both new e2e tests and every pre-existing test.

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src crates/celerrate_phpdoc_bridge/tests
git commit -m "✨ feat(phpdoc-bridge): the total lowering table over the dialect grammar"
```

---

### Task 8: The dialect tag tables and tool-prefix precedence

The two semantic dialects become explicit internal modules:
`dialect/phpstan` (bare and `@phpstan-`-prefixed vocabulary),
`dialect/psalm` (`@psalm-` synonyms and the enumerated
ignored-divergent bucket). Slot resolution becomes tier-aware:
PHPStan-prefixed > Psalm-prefixed > bare, first parseable within a
tier, per parameter name for `@param`, accumulation for `@throws`.
The conflict table is the rustdoc of `dialect/mod.rs`.

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/src/dialect/mod.rs`
- Create: `crates/celerrate_phpdoc_bridge/src/dialect/phpstan.rs`
- Create: `crates/celerrate_phpdoc_bridge/src/dialect/psalm.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/tags.rs` (tier-aware
  slot resolution)
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (`mod dialect;`)
- Test: inline tests in `dialect/mod.rs` and `tags.rs`, plus one e2e

**Interfaces:**
- Produces (crate-internal; Tasks 9-10 extend `TagRole` with
  `Template` and `Assert`):

```rust
pub(crate) enum TagTier { PhpstanPrefixed, PsalmPrefixed, Bare } // Ord: lower wins
pub(crate) enum TagRole { Param, Return, Var, Throws, Property, Method, Ignored }
pub(crate) struct ClassifiedTag { pub(crate) role: TagRole, pub(crate) tier: TagTier }
pub(crate) fn classify(name: &str) -> Option<ClassifiedTag>
```

- [ ] **Step 1: Write the failing tests**

In `tags.rs` tests:

```rust
    #[test]
    fn tool_prefixed_tags_win_over_bare_regardless_of_order() {
        let tags = lex_docblock(
            "/**\n * @return string\n * @psalm-return bool\n * @phpstan-return int\n */",
        );
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("int".to_owned())),
        );
        // Without a PHPStan-prefixed tag, the Psalm synonym beats bare.
        let tags = lex_docblock("/**\n * @psalm-return bool\n * @return string\n */");
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("bool".to_owned())),
        );
    }

    #[test]
    fn an_unparseable_prefixed_tag_never_clears_a_parseable_bare_one() {
        let tags = lex_docblock("/**\n * @phpstan-return array{\n * @return string\n */");
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("string".to_owned())),
        );
    }

    #[test]
    fn param_precedence_resolves_per_parameter_name() {
        let tags = lex_docblock(
            "/**\n * @param string $a\n * @param string $b\n * @phpstan-param int $a\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.parameters.len(), 2);
        assert_eq!(
            extracted.parameters[0],
            ("a".to_owned(), TypeExpression::Name("int".to_owned())),
        );
        assert_eq!(
            extracted.parameters[1],
            ("b".to_owned(), TypeExpression::Name("string".to_owned())),
        );
    }

    #[test]
    fn psalm_synonyms_and_virtual_member_prefixes_extract() {
        let tags = lex_docblock("/** @psalm-var non-empty-string */");
        assert!(extract_member_docblock(&tags).value_type.is_some());
        let tags = lex_docblock("/** @psalm-property string $title */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "title");
    }

    #[test]
    fn the_ignored_divergent_bucket_contributes_nothing_and_disturbs_nothing() {
        // The enumerated bucket (design section 5): parsed, ignored
        // without error, siblings survive.
        let tags = lex_docblock(
            "/**\n * @psalm-pure\n * @psalm-mutation-free\n * @psalm-taint-sink html $output\n * @psalm-taint-source input\n * @psalm-if-this-is Foo\n * @phpstan-pure\n * @return int\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(
            extracted.return_type,
            Some(TypeExpression::Name("int".to_owned())),
        );
        assert!(extracted.parameters.is_empty());
        assert!(extracted.throws.is_empty());
    }
```

And in `tests/end_to_end.rs`:

```rust
#[test]
fn tool_prefixed_precedence_holds_through_the_seam() {
    let fixture = fixture(&[
        "<?php class C { /**\n * @return string\n * @phpstan-return int\n */ public function pick() {} }",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "C", MemberKind::Method, "pick");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap();
    assert_eq!(signature.value_type, celerrate_types::TypeId::int(&fixture.db));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: FAIL — prefixed tags are unrecognized today.

- [ ] **Step 3: Write the dialect modules**

`crates/celerrate_phpdoc_bridge/src/dialect/mod.rs`:

```rust
//! Tag classification across the two semantic dialects — the
//! inter-dialect precedence lives here because the dialects coexist
//! on one docblock in real code (design section 5).
//!
//! # The conflict table (decision 8; published by plan 9c)
//!
//! For one slot, the tiers resolve as:
//!
//! | slot | wins | over | over |
//! |---|---|---|---|
//! | return | `@phpstan-return` | `@psalm-return` | `@return` |
//! | param (per name) | `@phpstan-param` | `@psalm-param` | `@param` |
//! | var | `@phpstan-var` | `@psalm-var` | `@var` |
//! | property / method | `@phpstan-` form | `@psalm-` form | bare form |
//!
//! Within one tier the first *parseable* tag wins; an unparseable tag
//! never consumes a slot (the 4a rule, preserved). `@throws`
//! accumulates across tiers instead of resolving. The enumerated
//! ignored-divergent bucket (purity, taint, Psalm-specific `this`
//! refinements) classifies as `Ignored`: recognized, contributing
//! nothing, disturbing nothing — traced as debt toward a later
//! complement.

pub(crate) mod phpstan;
pub(crate) mod psalm;

/// Precedence tiers, strongest first: `Ord` derives so a lower
/// variant wins a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TagTier {
    PhpstanPrefixed,
    PsalmPrefixed,
    Bare,
}

/// What a recognized tag feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagRole {
    Param,
    Return,
    Var,
    Throws,
    Property,
    Method,
    /// The enumerated divergent bucket: parsed, ignored without error.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassifiedTag {
    pub(crate) role: TagRole,
    pub(crate) tier: TagTier,
}

/// PHPStan's vocabulary is consulted first, Psalm's second — an
/// arbitrary-looking order that is in fact inert: the two `classify`
/// functions match disjoint tag names.
pub(crate) fn classify(name: &str) -> Option<ClassifiedTag> {
    phpstan::classify(name).or_else(|| psalm::classify(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_covers_both_dialects_and_the_tiers_order() {
        assert!(TagTier::PhpstanPrefixed < TagTier::PsalmPrefixed);
        assert!(TagTier::PsalmPrefixed < TagTier::Bare);
        let param = classify("param").unwrap();
        assert_eq!((param.role, param.tier), (TagRole::Param, TagTier::Bare));
        let phpstan = classify("phpstan-return").unwrap();
        assert_eq!(
            (phpstan.role, phpstan.tier),
            (TagRole::Return, TagTier::PhpstanPrefixed),
        );
        let psalm = classify("psalm-var").unwrap();
        assert_eq!((psalm.role, psalm.tier), (TagRole::Var, TagTier::PsalmPrefixed));
        assert_eq!(classify("psalm-pure").unwrap().role, TagRole::Ignored);
        assert_eq!(classify("author"), None);
    }
}
```

(The test module needs `#![allow(clippy::unwrap_used)]`.)

`crates/celerrate_phpdoc_bridge/src/dialect/phpstan.rs`:

```rust
//! The PHPStan dialect's tag vocabulary: the bare inherited tags and
//! their `@phpstan-` prefixed forms.

use super::{ClassifiedTag, TagRole, TagTier};

pub(crate) fn classify(name: &str) -> Option<ClassifiedTag> {
    let (tier, bare) = match name.strip_prefix("phpstan-") {
        Some(rest) => (TagTier::PhpstanPrefixed, rest),
        None => (TagTier::Bare, name),
    };
    let role = match bare {
        "param" => TagRole::Param,
        "return" => TagRole::Return,
        "var" => TagRole::Var,
        "throws" => TagRole::Throws,
        "property" | "property-read" | "property-write" => TagRole::Property,
        "method" => TagRole::Method,
        // Purity is out of this sub-project's scope end to end
        // (design section 1): ignored without error.
        "pure" | "impure" => TagRole::Ignored,
        _ => return None,
    };
    Some(ClassifiedTag { role, tier })
}
```

`crates/celerrate_phpdoc_bridge/src/dialect/psalm.rs`:

```rust
//! The Psalm dialect: tags with PHPStan-coincident semantics are
//! synonyms, fully honored; the genuinely divergent behaviors are the
//! enumerated ignored bucket (design section 5) — parsed, ignored
//! without error, traced as debt toward a later complement.

use super::{ClassifiedTag, TagRole, TagTier};

pub(crate) fn classify(name: &str) -> Option<ClassifiedTag> {
    let bare = name.strip_prefix("psalm-")?;
    let role = match bare {
        "param" => TagRole::Param,
        "return" => TagRole::Return,
        "var" => TagRole::Var,
        "property" | "property-read" | "property-write" => TagRole::Property,
        "method" => TagRole::Method,
        // The enumerated ignored-divergent bucket: purity, taint, and
        // the Psalm-specific `this` refinements.
        "pure" | "mutation-free" | "immutable" | "external-mutation-free"
        | "taint-source" | "taint-sink" | "taint-escape" | "taint-unescape"
        | "taint-specialize" | "flow" | "if-this-is" | "this-out" | "self-out" => {
            TagRole::Ignored
        }
        _ => return None,
    };
    Some(ClassifiedTag {
        role,
        tier: TagTier::PsalmPrefixed,
    })
}
```

- [ ] **Step 4: Rewire slot resolution in `tags.rs`**

Add `use crate::dialect::{self, TagRole, TagTier};`, drop the
`HashSet` import, and replace `extract_member_docblock`,
`extract_virtual_members`, and `parse_param_tag`'s signature:

```rust
/// Extracts the member slots under the tier rule (decision 8):
/// PHPStan-prefixed over Psalm-prefixed over bare; within a tier the
/// first parseable tag wins; `@param` resolves per parameter name;
/// `@throws` accumulates across tiers.
pub fn extract_member_docblock(tags: &[Tag]) -> MemberDocblock {
    let mut return_slot: Option<(TagTier, TypeExpression)> = None;
    let mut value_slot: Option<(TagTier, TypeExpression)> = None;
    let mut parameters: Vec<(String, TagTier, TypeExpression)> = Vec::new();
    let mut throws = Vec::new();
    for tag in tags {
        let Some(classified) = dialect::classify(&tag.name) else {
            continue;
        };
        match classified.role {
            TagRole::Return => offer_value(&mut return_slot, classified.tier, &tag.content),
            TagRole::Var => offer_value(&mut value_slot, classified.tier, &tag.content),
            TagRole::Param => offer_parameter(&mut parameters, classified.tier, &tag.content),
            TagRole::Throws => {
                if let Some(expression) = value_type(&tag.content) {
                    throws.push(expression);
                }
            }
            TagRole::Property | TagRole::Method | TagRole::Ignored => {}
        }
    }
    MemberDocblock {
        return_type: return_slot.map(|(_, expression)| expression),
        value_type: value_slot.map(|(_, expression)| expression),
        parameters: parameters
            .into_iter()
            .map(|(name, _, expression)| (name, expression))
            .collect(),
        throws,
    }
}

/// A stronger tier replaces; the same or a weaker tier keeps the
/// holder (first parseable within a tier). An unparseable candidate
/// never touches the slot.
fn offer_value(slot: &mut Option<(TagTier, TypeExpression)>, tier: TagTier, content: &str) {
    if matches!(slot, Some((existing, _)) if *existing <= tier) {
        return;
    }
    if let Some(expression) = value_type(content) {
        *slot = Some((tier, expression));
    }
}

/// Per-name slots in first-appearance order, so the output stays
/// deterministic without a map.
fn offer_parameter(
    parameters: &mut Vec<(String, TagTier, TypeExpression)>,
    tier: TagTier,
    content: &str,
) {
    let Some((name, expression)) = parse_param_tag(content) else {
        return;
    };
    match parameters
        .iter_mut()
        .find(|(existing, _, _)| *existing == name)
    {
        Some((_, existing_tier, existing_expression)) => {
            if tier < *existing_tier {
                *existing_tier = tier;
                *existing_expression = expression;
            }
        }
        None => parameters.push((name, tier, expression)),
    }
}
```

`parse_param_tag` loses its `seen` parameter (the per-name vector now
owns deduplication): signature becomes
`fn parse_param_tag(content: &str) -> Option<(String, TypeExpression)>`
— delete the `seen` check and insert, keep everything else from
Task 6's version.

`extract_virtual_members` becomes tier-aware per `(kind, name)`:

```rust
/// Virtual members under the same tier rule, resolved per
/// `(kind, name)`; the first declaration wins within a tier.
pub fn extract_virtual_members(tags: &[Tag]) -> Vec<VirtualMember> {
    let mut members: Vec<(TagTier, VirtualMember)> = Vec::new();
    for tag in tags {
        let Some(classified) = dialect::classify(&tag.name) else {
            continue;
        };
        let parsed = match classified.role {
            TagRole::Property => parse_property_tag(&tag.content),
            TagRole::Method => parse_method_tag(&tag.content),
            _ => None,
        };
        let Some(member) = parsed else {
            continue;
        };
        match members.iter_mut().find(|(_, existing)| {
            existing.kind == member.kind && existing.name == member.name
        }) {
            Some((existing_tier, existing)) => {
                if classified.tier < *existing_tier {
                    *existing_tier = classified.tier;
                    *existing = member;
                }
            }
            None => members.push((classified.tier, member)),
        }
    }
    members.into_iter().map(|(_, member)| member).collect()
}
```

Add `mod dialect;` to `lib.rs`. Update `tags.rs`'s module doc: the
extraction is dialect-classified and tier-resolved.

- [ ] **Step 5: Run to verify green**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: PASS — new tests and all pre-existing ones (bare tags
classify to the same slots as before; `var_reads_the_value_type_and_first_tag_wins_on_duplicates`
still holds through the within-tier rule).

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src crates/celerrate_phpdoc_bridge/tests
git commit -m "✨ feat(phpdoc-bridge): the dialect tag tables with tool-prefix precedence"
```

---

### Task 9: `@template` and the annotation scope

The extension point is extended, never bypassed: `AnnotationSite`
gains the declaring-scope context (`declaring_scope`,
`enclosing_class_scope`, `enclosing_class_docblock`), built by
`celerrate_types` at the three existing call sites under the
scope-key convention `TypeId::template` documents. The bridge collects
`@template T [of|as Bound]` declarations (class-level and
member-level, member shadows class; variance-marked variants declare
with their variance dropped), and names resolve through the scope —
case-sensitively, before keywords — into `TypeId::template`.
Template-subject conditionals with an in-scope subject now lower to
`TypeId::conditional`.

**Files:**
- Modify: `crates/celerrate_types/src/type_syntax.rs`
  (`AnnotationContext`, site accessors, dispatch signatures, tests)
- Modify: `crates/celerrate_types/src/declared.rs` (context at the
  three call sites, `owner_class_docblock`)
- Modify: `crates/celerrate_phpdoc_bridge/src/tags.rs`
  (`TemplateDeclaration`, the template tag)
- Modify: `crates/celerrate_phpdoc_bridge/src/dialect/{phpstan,psalm}.rs`
  (`TagRole::Template`)
- Modify: `crates/celerrate_phpdoc_bridge/src/dialect/mod.rs` (variant)
- Modify: `crates/celerrate_phpdoc_bridge/src/lowering.rs`
  (`LoweringScope<'db>` with the template set)
- Modify: `crates/celerrate_phpdoc_bridge/src/syntax.rs` (scope build)
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (export
  `TemplateDeclaration`)
- Test: `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs`,
  `crates/celerrate_types/tests/invalidation_scope.rs`

**Interfaces:**
- Produces in `celerrate_types` (host-side; not re-exported by the
  facade — plugins reach it only through the site accessors):

```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct AnnotationContext<'a> {
    pub declaring_scope: &'a str,
    pub enclosing_class_scope: Option<&'a str>,
    pub enclosing_class_docblock: Option<&'a str>,
}
impl<'db, 'site> AnnotationSite<'db, 'site> {
    pub fn declaring_scope(&self) -> &'site str
    pub fn enclosing_class_scope(&self) -> Option<&'site str>
    pub fn enclosing_class_docblock(&self) -> Option<&'site str>
}
pub(crate) fn annotations_for_docblock<'db>(db, site: &NameSite<'_>, context: &AnnotationContext<'_>, docblock: &str) -> ParsedAnnotations<'db>
pub(crate) fn type_of_expression<'db>(db, site: &NameSite<'_>, context: &AnnotationContext<'_>, expression: &str) -> Option<TypeId<'db>>
```

- Produces in the bridge:

```rust
pub struct TemplateDeclaration { pub name: String, pub bound: Option<TypeExpression> }
// MemberDocblock gains: pub templates: Vec<TemplateDeclaration>
```

- Scope-key convention (decision 5, matching
  `crates/celerrate_types/src/construction.rs:590-593`): members get
  `format!("{owner}::{folded_member_key(kind, name)}")`, class-level
  declarations get the owner class key, free functions get the folded
  function key.

- [ ] **Step 1: Write the failing e2e tests**

In `tests/end_to_end.rs`:

```rust
#[test]
fn template_variables_resolve_through_the_annotation_scope() {
    let fixture = fixture(&[
        "<?php /** @template T of \\Entity */ class Repository {\n\
         /** @return T */ public function find() {}\n\
         /** @template U\n * @return U\n */ public function pluck() {}\n\
         }",
        "<?php class Entity {}",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let value = |name: &str| {
        let query = member_query(&fixture, "Repository", MemberKind::Method, name);
        celerrate_types::declared_member_signature(
            db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap()
        .value_type
    };
    let class_scope = folded_symbol_key(SymbolSpace::ClassLike, "Repository");
    // Class-level templates reach member docblocks, keyed at the
    // class scope with their bound lowered.
    assert_eq!(
        value("find"),
        celerrate_types::TypeId::template(
            db,
            &class_scope,
            "T",
            celerrate_types::TypeId::class(db, "Entity", Vec::new()),
        ),
    );
    // Member-level templates key at `<class key>::<member key>` and
    // default their bound to `mixed`.
    let member_scope = format!(
        "{class_scope}::{}",
        folded_member_key(MemberKind::Method, "pluck"),
    );
    assert_eq!(
        value("pluck"),
        celerrate_types::TypeId::template(
            db,
            &member_scope,
            "U",
            celerrate_types::TypeId::mixed(db),
        ),
    );
}

#[test]
fn member_templates_shadow_class_templates_and_virtual_payloads_see_the_class_scope() {
    let fixture = fixture(&[
        "<?php class A {} class B {}\n\
         /** @template T of A */ class Box {\n\
         /** @template T of B\n * @return T\n */ public function shadowed() {}\n\
         }",
        "<?php /** @template T\n * @property list<T> $items\n */ class Bag {}",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let box_scope = folded_symbol_key(SymbolSpace::ClassLike, "Box");
    let shadowed = member_query(&fixture, "Box", MemberKind::Method, "shadowed");
    let signature = celerrate_types::declared_member_signature(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        shadowed,
    )
    .unwrap();
    let member_scope = format!(
        "{box_scope}::{}",
        folded_member_key(MemberKind::Method, "shadowed"),
    );
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::template(
            db,
            &member_scope,
            "T",
            celerrate_types::TypeId::class(db, "B", Vec::new()),
        ),
    );
    // A virtual member's payload resolves class-level templates.
    let items = member_query(&fixture, "Bag", MemberKind::Property, "items");
    let signature = celerrate_types::declared_member_signature(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        items,
    )
    .unwrap();
    let bag_scope = folded_symbol_key(SymbolSpace::ClassLike, "Bag");
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::list(
            db,
            celerrate_types::TypeId::template(
                db,
                &bag_scope,
                "T",
                celerrate_types::TypeId::mixed(db),
            ),
        ),
    );
}

#[test]
fn a_variance_marked_template_still_declares_and_a_template_conditional_lowers() {
    let fixture = fixture(&[
        "<?php /** @template-covariant T */ class Producer {\n\
         /** @return T */ public function produce() {}\n\
         /** @return (T is string ? int : bool) */ public function branch() {}\n\
         }",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let scope = folded_symbol_key(SymbolSpace::ClassLike, "Producer");
    let template = celerrate_types::TypeId::template(
        db,
        &scope,
        "T",
        celerrate_types::TypeId::mixed(db),
    );
    let value = |name: &str| {
        let query = member_query(&fixture, "Producer", MemberKind::Method, name);
        celerrate_types::declared_member_signature(
            db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap()
        .value_type
    };
    assert_eq!(value("produce"), template);
    assert_eq!(
        value("branch"),
        celerrate_types::TypeId::conditional(
            db,
            template,
            celerrate_types::TypeId::string(db),
            celerrate_types::TypeId::int(db),
            celerrate_types::TypeId::bool(db),
            false,
        ),
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge --test end_to_end`
Expected: FAIL — `T` lowers to a class named `T` today.

- [ ] **Step 3: Extend `celerrate_types`**

In `type_syntax.rs`:

1. Add `AnnotationContext` (as in the Interfaces block, with the
   doc comment: the declaring-scope context `@template` resolution
   needs beyond name qualification; the scope-key convention is
   `TypeId::template`'s).
2. `AnnotationSite` gains a `context: AnnotationContext<'site>` field,
   `new` takes it as a third parameter, and the three public accessors
   return from it (`declaring_scope(&self) -> &'site str`, the two
   `Option<&'site str>` accessors). Document on each: call-scoped,
   never retained.
3. `annotations_for_docblock` and `type_of_expression` take
   `context: &AnnotationContext<'_>` after `site` and pass
   `AnnotationSite::new(db, site, *context)`.
4. Update this module's tests: every `annotations_for_docblock(db,
   &NameSite::Global, ...)` / `type_of_expression(...)` call gains
   `&AnnotationContext::default()`; every `AnnotationSite::new(db,
   &NameSite::Global)` gains `AnnotationContext::default()`. Add one
   accessor test:

```rust
    #[test]
    fn the_annotation_site_exposes_the_declaring_context() {
        let fixture = fixture(&["<?php class C {}"]);
        let context = AnnotationContext {
            declaring_scope: "c::find",
            enclosing_class_scope: Some("c"),
            enclosing_class_docblock: Some("/** @template T */"),
        };
        let site = AnnotationSite::new(&fixture.db, &NameSite::Global, context);
        assert_eq!(site.declaring_scope(), "c::find");
        assert_eq!(site.enclosing_class_scope(), Some("c"));
        assert_eq!(site.enclosing_class_docblock(), Some("/** @template T */"));
    }
```

In `declared.rs`:

1. Add the helper (verify the exact tree accessor against
   `declaring_site`'s own body — the class groups of `member_tree`
   carry `docblock`, the field `linearize.rs` already reads):

```rust
/// The owner class-like's own docblock text: class-level `@template`
/// declarations are visible inside member annotations.
fn owner_class_docblock(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    owner_key: &str,
) -> Option<String> {
    let site = declaring_site(db, files, owner_key)?;
    member_tree(db, site.file)
        .classes
        .iter()
        .find(|group| group.ast_id == site.ast_id)?
        .docblock
        .clone()
}
```

2. `member_annotations`: before the `with_declaring_site` call, build
   the scope key and fetch the enclosing docblock; thread the context:

```rust
    let member_key = folded_member_key(member.kind, &member.name);
    let declaring_scope = format!("{owner}::{member_key}");
    let enclosing_docblock = owner_class_docblock(db, files, &owner);
    let parsed = with_declaring_site(db, files, &owner, |site| {
        let context = AnnotationContext {
            declaring_scope: &declaring_scope,
            enclosing_class_scope: Some(&owner),
            enclosing_class_docblock: enclosing_docblock.as_deref(),
        };
        crate::type_syntax::annotations_for_docblock(db, site, &context, &docblock)
    });
```

3. `function_annotations`: the folded function key is the scope, no
   enclosing class:

```rust
    let function_key = query.key(db).clone();
    let context = AnnotationContext {
        declaring_scope: &function_key,
        enclosing_class_scope: None,
        enclosing_class_docblock: None,
    };
    let parsed = crate::type_syntax::annotations_for_docblock(db, &site, &context, &docblock);
```

4. The `Virtual` arm of `declared_member_signature`: virtual members
   are declared by the class docblock, so the declaring scope IS the
   class scope:

```rust
        MemberResolution::Virtual { member, owner } => {
            let mixed = TypeId::mixed(db);
            let enclosing_docblock = owner_class_docblock(db, files, &owner);
            return Some(with_declaring_site(db, files, &owner, |site| {
                let context = AnnotationContext {
                    declaring_scope: &owner,
                    enclosing_class_scope: Some(&owner),
                    enclosing_class_docblock: enclosing_docblock.as_deref(),
                };
                // ... both existing `type_of_expression(db, site, text)`
                // calls become `type_of_expression(db, site, &context, text)`;
                // everything else unchanged ...
```

Import `AnnotationContext` where used. Run
`cargo test --package celerrate_types` — everything must stay green
(the default context changes no behavior yet).

- [ ] **Step 4: Extend the bridge**

1. `tags.rs`: add the declaration type, the field, and the tag parser;
   classify `template` in both dialect tables.

```rust
/// One `@template` declaration: `T`, `T of Bound`, `T as Bound`
/// (the Psalm keyword is a synonym). A `= Default` tail and the
/// variance of `-covariant`/`-contravariant` variants are dropped
/// (decision 6; recorded debt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDeclaration {
    pub name: String,
    pub bound: Option<TypeExpression>,
}
```

`MemberDocblock` gains `pub templates: Vec<TemplateDeclaration>`.
In `extract_member_docblock`, add a `templates` vector resolved with
the same per-name tier rule as parameters (reuse `offer`-style logic
over `(String, TagTier, TemplateDeclaration)`), fed by:

```rust
/// `@template T [of|as Bound] [= Default]`.
fn parse_template_tag(content: &str) -> Option<TemplateDeclaration> {
    let trimmed = content.trim_start();
    let name_end = trimmed
        .find(char::is_whitespace)
        .unwrap_or(trimmed.len());
    let name = trimmed.get(..name_end)?;
    if !is_valid_identifier(name) {
        return None;
    }
    let rest = trimmed.get(name_end..)?.trim_start();
    let bound = match rest
        .strip_prefix("of")
        .or_else(|| rest.strip_prefix("as"))
    {
        Some(after_keyword) if after_keyword.starts_with(char::is_whitespace) => {
            let (expression, _) = parse_type_expression_prefix(after_keyword)?;
            Some(expression)
        }
        _ => None,
    };
    Some(TemplateDeclaration {
        name: name.to_owned(),
        bound,
    })
}
```

In `dialect/mod.rs` add `Template` to `TagRole`; in `phpstan.rs` and
`psalm.rs` classify
`"template" | "template-covariant" | "template-contravariant" => TagRole::Template`
(bare/`phpstan-` tiers in `phpstan.rs`, the `psalm-` tier in
`psalm.rs`). Export `TemplateDeclaration` from `lib.rs`'s `tags`
re-export list.

2. `lowering.rs`: `LoweringScope` gains the lifetime and the template
   set; resolution slots in exactly where the Task 7 comments said:

```rust
#[derive(Debug, Default)]
pub(crate) struct LoweringScope<'db> {
    /// Declared template variables, resolved at declaration into
    /// their lattice value — later declarations shadow earlier ones.
    templates: Vec<(String, TypeId<'db>)>,
    callable_templates: Vec<String>,
}

impl<'db> LoweringScope<'db> {
    pub(crate) fn declare_template(
        &mut self,
        db: &'db dyn salsa::Database,
        scope_key: &str,
        name: String,
        bound: TypeId<'db>,
    ) {
        let resolved = TypeId::template(db, scope_key, &name, bound);
        self.templates.push((name, resolved));
    }

    fn resolve_template(&self, name: &str) -> Option<TypeId<'db>> {
        self.templates
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, resolved)| *resolved)
    }
}
```

`lower` and every helper take `scope: &mut LoweringScope<'db>`. In
`lower_name`, between the callable check and the keyword check:

```rust
    if let Some(resolved) = scope.resolve_template(name) {
        return resolved;
    }
```

In `lower_generic`'s catch-all arm, before the keyword/dialect check:
a template base drops its arguments —
`if let Some(resolved) = scope.resolve_template(base) { return resolved; }`.
In `lower_conditional`, replace the fallback-only body:

```rust
    if let ConditionalSubject::Template(name) = subject
        && let Some(template) = scope.resolve_template(name)
    {
        let target_lowered = lower(site, scope, target);
        return TypeId::conditional(
            db,
            template,
            target_lowered,
            then_lowered,
            otherwise_lowered,
            negated,
        );
    }
    TypeId::union(db, [then_lowered, otherwise_lowered])
```

3. `syntax.rs`: build the scope per docblock (class level first, then
   own declarations — sequential, so a bound may reference an earlier
   template and shadowing is last-wins):

```rust
fn docblock_scope<'db>(
    site: &AnnotationSite<'db, '_>,
    own_templates: &[TemplateDeclaration],
) -> LoweringScope<'db> {
    let mut scope = LoweringScope::default();
    if let (Some(class_scope), Some(class_docblock)) =
        (site.enclosing_class_scope(), site.enclosing_class_docblock())
    {
        let class_templates =
            extract_member_docblock(&lex_docblock(class_docblock)).templates;
        for declaration in &class_templates {
            declare_into(site, &mut scope, declaration, class_scope);
        }
    }
    let declaring = site.declaring_scope();
    for declaration in own_templates {
        declare_into(site, &mut scope, declaration, declaring);
    }
    scope
}

fn declare_into<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    declaration: &TemplateDeclaration,
    scope_key: &str,
) {
    let db = site.database();
    let bound = declaration
        .bound
        .as_ref()
        .map(|expression| lower(site, scope, expression))
        .unwrap_or_else(|| TypeId::mixed(db));
    scope.declare_template(db, scope_key, declaration.name.clone(), bound);
}
```

`parse_docblock` replaces `LoweringScope::default()` with
`docblock_scope(site, &extracted.templates)`;
`parse_type_expression` with `docblock_scope(site, &[])` (a bare
payload's own docblock IS the enclosing one). Import
`TemplateDeclaration` and `extract_member_docblock` accordingly.

- [ ] **Step 5: Run to verify green**

Run: `cargo test --workspace`
Expected: PASS — the three e2e tests, all bridge tests, all
`celerrate_types` tests.

- [ ] **Step 6: Pin the new invalidation edge**

`member_annotations` now reads the CLASS docblock. Extend
`crates/celerrate_types/tests/invalidation_scope.rs` with a pin
mirroring its neighbour
`a_prose_only_docblock_edit_backdates_at_the_parsed_annotation_stage`
(same fixture recipe and `executions_of` helper):

- Fixture: a class whose docblock is prose only, one method with a
  parseable `@return` annotation; a probe query
  (`subtype_of`-based, as the neighbour does) primed warm.
- Edit: change only the CLASS docblock's prose.
- Assert: `member_annotations` re-runs (`executions_of == 1`) and its
  value is unchanged; the probe is spared at 0 executions.

Name it
`a_class_docblock_prose_edit_backdates_at_the_member_annotations_stage`.
If the observed honest counts differ (salsa may spare or re-run
`declared_member_signature` depending on backdating order), pin the
honest mechanism and record the observation in the closure ledger —
the 4a Task 7 precedent applies.

Run: `cargo test --package celerrate_types --test invalidation_scope`
Expected: PASS.

- [ ] **Step 7: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_types crates/celerrate_phpdoc_bridge
git commit -m "✨ feat(types,phpdoc-bridge): template variables resolve through the annotation scope"
```

---

### Task 10: Assertion tags are carried for plan 5

`@phpstan-assert` (and `-if-true`/`-if-false`) plus the non-divergent
`@psalm-assert` family extract into a carried payload — parsed,
lowered, delivered, with no consumer until plan 5's narrowing (the
`@throws` precedent). The `=`-prefixed Psalm exact-assertion forms
join the ignored-divergent bucket.

**Files:**
- Modify: `crates/celerrate_types/src/type_syntax.rs`
  (`AssertionPolarity`, `ParsedAssertion`, the `assertions` field)
- Modify: `crates/celerrate_types/src/declared.rs`
  (`MemberAnnotations.assertions`, copied at both annotation seams)
- Modify: `crates/celerrate_plugin/src/lib.rs` (re-export both)
- Modify: `crates/celerrate_phpdoc_bridge/src/dialect/{mod,phpstan,psalm}.rs`
  (`TagRole::Assert`)
- Modify: `crates/celerrate_phpdoc_bridge/src/tags.rs`
  (`AssertionDeclaration`, the assert tag)
- Modify: `crates/celerrate_phpdoc_bridge/src/syntax.rs` (lowering the
  payload)
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (export)
- Test: `tags.rs` inline + `tests/end_to_end.rs`

**Interfaces:**
- Produces in `celerrate_types` (re-exported by `celerrate_plugin`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum AssertionPolarity { Always, IfTrue, IfFalse }

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct ParsedAssertion<'db> {
    /// The asserted subject, verbatim (`$value`, `$this->prop`):
    /// interpretation is plan 5's.
    pub subject: String,
    pub asserted: TypeId<'db>,
    pub polarity: AssertionPolarity,
    pub negated: bool,
}
// ParsedAnnotations gains: pub assertions: Vec<ParsedAssertion<'db>>
// MemberAnnotations gains: pub assertions: Vec<ParsedAssertion<'db>>
```

- Produces in the bridge:

```rust
pub struct AssertionDeclaration {
    pub subject: String,
    pub asserted: TypeExpression,
    pub polarity: AssertionPolarity,
    pub negated: bool,
}
// MemberDocblock gains: pub assertions: Vec<AssertionDeclaration>
// TagRole gains: Assert(AssertionPolarity)
```

- [ ] **Step 1: Write the failing tests**

In `tags.rs` tests:

```rust
    #[test]
    fn assertion_tags_extract_with_polarity_and_negation() {
        use celerrate_plugin::AssertionPolarity;
        let tags = lex_docblock(
            "/**\n * @psalm-assert string $value\n * @phpstan-assert-if-true !null $user\n * @psalm-assert =string $exact\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.assertions.len(), 2);
        let first = &extracted.assertions[0];
        assert_eq!(first.subject, "$value");
        assert_eq!(first.polarity, AssertionPolarity::Always);
        assert!(!first.negated);
        let second = &extracted.assertions[1];
        assert_eq!(second.subject, "$user");
        assert_eq!(second.polarity, AssertionPolarity::IfTrue);
        assert!(second.negated);
        // The `=`-prefixed exact form is the divergent bucket: ignored.
        assert!(!extracted.assertions.iter().any(|a| a.subject == "$exact"));
    }
```

In `tests/end_to_end.rs`:

```rust
#[test]
fn assertions_are_carried_through_the_annotation_seam() {
    // The webmozart/assert pattern (design section 5).
    let fixture = fixture(&[
        "<?php class Assert { /** @psalm-assert string $value */ public static function string($value) {} }",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let query = member_query(&fixture, "Assert", MemberKind::Method, "string");
    let annotations = celerrate_types::member_annotations(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    );
    assert_eq!(
        annotations.assertions,
        vec![celerrate_types::ParsedAssertion {
            subject: "$value".to_owned(),
            asserted: celerrate_types::TypeId::string(db),
            polarity: celerrate_types::AssertionPolarity::Always,
            negated: false,
        }],
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --package celerrate_phpdoc_bridge`
Expected: FAIL — the field and types do not exist.

- [ ] **Step 3: Implement**

1. `celerrate_types/src/type_syntax.rs`: add `AssertionPolarity` and
   `ParsedAssertion` as in the Interfaces block (doc comments: carried
   for plan 5's narrowing, the non-divergent assertion family of
   design section 5); add
   `pub assertions: Vec<ParsedAssertion<'db>>` to `ParsedAnnotations`.
2. `celerrate_types/src/declared.rs`: add
   `pub assertions: Vec<ParsedAssertion<'db>>` to `MemberAnnotations`;
   in `member_annotations` set `assertions: parsed.assertions` (all
   member kinds carry them — the consumer filters); same in
   `function_annotations`.
3. `celerrate_plugin/src/lib.rs`: extend the `celerrate_types`
   re-export list with `AssertionPolarity, ParsedAssertion`.
4. Bridge `dialect/mod.rs`: `TagRole` gains
   `Assert(celerrate_plugin::AssertionPolarity)` (import via
   `use celerrate_plugin::AssertionPolarity;`).
   `phpstan.rs` (prefixed tier only — there is no bare `@assert`):

```rust
        "assert" if tier == TagTier::PhpstanPrefixed => {
            TagRole::Assert(AssertionPolarity::Always)
        }
        "assert-if-true" if tier == TagTier::PhpstanPrefixed => {
            TagRole::Assert(AssertionPolarity::IfTrue)
        }
        "assert-if-false" if tier == TagTier::PhpstanPrefixed => {
            TagRole::Assert(AssertionPolarity::IfFalse)
        }
```

   `psalm.rs`:

```rust
        "assert" => TagRole::Assert(AssertionPolarity::Always),
        "assert-if-true" => TagRole::Assert(AssertionPolarity::IfTrue),
        "assert-if-false" => TagRole::Assert(AssertionPolarity::IfFalse),
```

5. Bridge `tags.rs`: add `AssertionDeclaration` (derive
   `Debug, Clone, PartialEq, Eq`), the `assertions` field on
   `MemberDocblock`, the accumulation arm in `extract_member_docblock`
   (`TagRole::Assert(polarity) => { if let Some(assertion) =
   parse_assert_tag(&tag.content, polarity) {
   extracted_assertions.push(assertion); } }` — accumulate across
   tiers in tag order, like `@throws`), and:

```rust
/// `[!]Type $subject`: the negation applies to the asserted type; the
/// subject travels verbatim. A `=`-prefixed content is Psalm's
/// exact-assertion divergence: ignored without error.
fn parse_assert_tag(
    content: &str,
    polarity: AssertionPolarity,
) -> Option<AssertionDeclaration> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('=') {
        return None;
    }
    let (negated, rest) = match trimmed.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let (asserted, consumed) = parse_type_expression_prefix(rest)?;
    let remainder = rest.get(consumed..)?;
    let subject = remainder.split_whitespace().next()?;
    if !subject.starts_with('$') {
        return None;
    }
    Some(AssertionDeclaration {
        subject: subject.to_owned(),
        asserted,
        polarity,
        negated,
    })
}
```

6. Bridge `syntax.rs`, in `parse_docblock`: compute the payload after
   the throws mapping and add the field to the struct literal:

```rust
        let assertions = extracted
            .assertions
            .iter()
            .map(|assertion| ParsedAssertion {
                subject: assertion.subject.clone(),
                asserted: lower(site, &mut scope, &assertion.asserted),
                polarity: assertion.polarity,
                negated: assertion.negated,
            })
            .collect();
        ParsedAnnotations {
            return_type,
            value_type,
            parameters,
            throws,
            assertions,
        }
```

   (import `ParsedAssertion` from `celerrate_plugin`; `phpstan.rs` and
   `psalm.rs` import `AssertionPolarity` the same way).
7. Bridge `lib.rs`: export `AssertionDeclaration` from `tags`.

- [ ] **Step 4: Run to verify green**

Run: `cargo test --workspace`
Expected: PASS (the `celerrate_types` struct-update sites and every
`ParsedAnnotations`/`MemberAnnotations` literal in tests gain the new
field via `..Default::default()` or an explicit `Vec::new()` — fix
any that list fields exhaustively).

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_types crates/celerrate_plugin crates/celerrate_phpdoc_bridge
git commit -m "✨ feat(types,phpdoc-bridge): assertion tags are carried for narrowing"
```

---

### Task 11: The pinned-reference coverage harness

The design's yardstick becomes mechanical: a pin file names
`phpstan/phpdoc-parser` at 2.3.3, xtask fetches it and extracts every
`TypeParserTest::provideParseData` input into a committed, attributed
case file, and a bridge integration test pins each case's parse
verdict in a committed snapshot whose header **is** the published
coverage statement. CI checks the extraction against the pin.

**Files:**
- Create: `xtask/phpdoc-parser.pin`
- Create: `xtask/src/phpdoc_corpus.rs`
- Modify: `xtask/src/lib.rs` (`pub mod phpdoc_corpus;`)
- Modify: `xtask/src/main.rs` (dispatch + usage string)
- Create: `crates/celerrate_phpdoc_bridge/tests/phpstan_corpus/cases.txt`
  (generated, committed)
- Create: `crates/celerrate_phpdoc_bridge/tests/phpstan_corpus.rs`
- Create: `crates/celerrate_phpdoc_bridge/tests/phpstan_corpus/verdicts.txt`
  (blessed, committed)
- Modify: `.github/workflows/corpus.yml` (the `phpdoc-cases` job)

**Interfaces:**
- Consumes: `xtask::pin` (`read`, `fetch_snapshot`),
  `xtask::workspace_root`, `parse_type_expression_text`.
- Produces: `cargo xtask fetch-phpdoc-parser`,
  `cargo xtask phpdoc-cases [--check]`; the two committed artifacts.
- Case-file line format: one case per line, `\` `\n` `\r` `\t`
  escaped as `\\`, `\n`, `\r`, `\t`; header lines start with `#` and
  carry provenance (repository, commit, MIT attribution) and
  `# cases = N`.

- [ ] **Step 1: Write the pin file**

`xtask/phpdoc-parser.pin`:

```
# phpstan/phpdoc-parser, the PHPStan dialect's pinned reference
# (type-engine design, section 5). Tag 2.3.3. Bump deliberately:
# change the commit, run `cargo xtask phpdoc-cases`, re-bless the
# bridge's verdict snapshot, and commit all three together.
repository = https://github.com/phpstan/phpdoc-parser
commit = fb19eedd2bb67ff8cf7a5502ad329e701d6398a3
```

- [ ] **Step 2: Write the failing extractor test**

Create `xtask/src/phpdoc_corpus.rs` with the tests and stubs:

```rust
//! The pinned phpstan/phpdoc-parser reference: fetches the snapshot
//! at its pin and extracts the `TypeParserTest::provideParseData`
//! inputs into the committed case file the bridge's coverage test
//! consumes. The extractor is a string-aware bracket scanner over the
//! provider region — layout-coupled to the pinned commit by design,
//! guarded by `--check` in CI.

use std::path::PathBuf;

use crate::{Result, pin, workspace_root};

const PIN_FILE: &str = "phpdoc-parser.pin";
const SOURCE_FILE: &str = "tests/PHPStan/Parser/TypeParserTest.php";
const CASES_FILE: &str = "crates/celerrate_phpdoc_bridge/tests/phpstan_corpus/cases.txt";

pub fn fetch() -> Result<PathBuf> {
    let root = workspace_root()?;
    let pin = pin::read(&root.join("xtask").join(PIN_FILE))?;
    let directory = root.join("target").join("phpdoc-parser").join(&pin.commit);
    pin::fetch_snapshot(&pin, &directory)?;
    Ok(directory)
}

pub fn extract(check: bool) -> Result<()> {
    let root = workspace_root()?;
    let pin = pin::read(&root.join("xtask").join(PIN_FILE))?;
    let snapshot = fetch()?;
    let source = std::fs::read_to_string(snapshot.join(SOURCE_FILE))?;
    let cases = extract_cases(&source)?;
    let mut rendered = String::new();
    rendered.push_str("# Type-expression inputs extracted from tests/PHPStan/Parser/TypeParserTest.php\n");
    rendered.push_str("# (provideParseData). One case per line; \\ \\n \\r \\t escaped.\n");
    rendered.push_str(&format!("# repository = {}\n", pin.repository));
    rendered.push_str(&format!("# commit = {}\n", pin.commit));
    rendered.push_str("# license = MIT (the upstream LICENSE covers the extracted inputs)\n");
    rendered.push_str(&format!("# cases = {}\n", cases.len()));
    for case in &cases {
        rendered.push_str(&escape(case));
        rendered.push('\n');
    }
    let destination = root.join(CASES_FILE);
    if check {
        let committed = std::fs::read_to_string(&destination)?;
        if committed != rendered {
            return Err(
                "the committed phpdoc corpus cases are stale: run `cargo xtask phpdoc-cases`"
                    .into(),
            );
        }
        println!("phpdoc corpus cases are current ({} cases)", cases.len());
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&destination, rendered)?;
        println!("wrote {} cases to {}", cases.len(), destination.display());
    }
    Ok(())
}

fn extract_cases(source: &str) -> Result<Vec<String>> {
    let _ = source;
    Err("unimplemented".into())
}

fn escape(case: &str) -> String {
    case.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{escape, extract_cases};

    const MINIATURE: &str = r#"
class TypeParserTest extends TestCase
{
    public function provideParseData(): array
    {
        return [
            [
                'string',
                new IdentifierTypeNode('string'),
            ],
            [
                'array{
                    // a is for [apple]
                    a: int,
                }',
                ArrayShapeNode::createSealed([
                    new ArrayShapeItemNode(null, false, new IdentifierTypeNode('int')),
                ]),
            ],
            [
                'it\'s',
                new ConstTypeNode(new ConstExprStringNode("it's", 1)),
            ],
        ];
    }

    public function unrelated(): array
    {
        return [['not a case']];
    }
}
"#;

    #[test]
    fn cases_extract_from_the_provider_region_only() {
        let cases = extract_cases(MINIATURE).unwrap();
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[0], "string");
        // Multiline inputs survive whole, brackets inside strings do
        // not derail the depth scan.
        assert!(cases[1].contains("// a is for [apple]"));
        // PHP single-quote escapes decode.
        assert_eq!(cases[2], "it's");
    }

    #[test]
    fn escaping_is_line_safe() {
        assert_eq!(escape("a\nb\\c"), "a\\nb\\\\c");
    }
}
```

Register in `xtask/src/lib.rs`: add `pub mod phpdoc_corpus;` to the
module list (alphabetical position), and extend the crate doc's task
enumeration with one clause.

Run: `cargo test --package xtask phpdoc`
Expected: FAIL — `extract_cases` is unimplemented.

- [ ] **Step 3: Implement the extractor**

Replace `extract_cases`:

```rust
/// The provider region runs from the `provideParseData` header
/// through its `return [` to the bracket that closes it. Each case
/// opens one bracket level below the return array; its input is the
/// PHP string literal that follows the opener. Depth counting is
/// string-aware, so brackets inside inputs do not derail it.
fn extract_cases(source: &str) -> Result<Vec<String>> {
    let start = source
        .find("public function provideParseData(): array")
        .ok_or("provideParseData not found: the pinned layout changed")?;
    let region = source.get(start..).ok_or("provider region out of range")?;
    let open = region
        .find("return [")
        .ok_or("provider return not found: the pinned layout changed")?
        + "return [".len();
    let mut characters = region
        .get(open..)
        .ok_or("provider body out of range")?
        .chars()
        .peekable();
    let mut depth: u32 = 1;
    let mut cases = Vec::new();
    while let Some(character) = characters.next() {
        match character {
            '\'' | '"' => {
                let _ = read_php_string(&mut characters, character);
            }
            '[' => {
                depth += 1;
                if depth == 2 {
                    while let Some(next) = characters.peek() {
                        if next.is_whitespace() {
                            characters.next();
                        } else {
                            break;
                        }
                    }
                    if let Some(&quote) = characters.peek()
                        && (quote == '\'' || quote == '"')
                    {
                        characters.next();
                        if let Some(case) = read_php_string(&mut characters, quote) {
                            cases.push(case);
                        }
                    }
                }
            }
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    if cases.is_empty() {
        return Err("no cases extracted: the pinned layout changed".into());
    }
    Ok(cases)
}

/// PHP string semantics per quote kind: single quotes decode `\\` and
/// `\'` and keep any other escape verbatim; double quotes additionally
/// decode `\"`, `\n`, `\t`, `\r`. `None` on an unterminated literal.
fn read_php_string(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
    quote: char,
) -> Option<String> {
    let mut value = String::new();
    while let Some(character) = characters.next() {
        if character == quote {
            return Some(value);
        }
        if character == '\\' {
            match characters.next()? {
                '\\' => value.push('\\'),
                escaped if escaped == quote => value.push(quote),
                'n' if quote == '"' => value.push('\n'),
                't' if quote == '"' => value.push('\t'),
                'r' if quote == '"' => value.push('\r'),
                other => {
                    value.push('\\');
                    value.push(other);
                }
            }
        } else {
            value.push(character);
        }
    }
    None
}
```

Wire the dispatch in `xtask/src/main.rs` (two new arms before the
fallthrough, and the usage string gains
`fetch-phpdoc-parser | phpdoc-cases [--check]`):

```rust
        (Some("fetch-phpdoc-parser"), None) => xtask::phpdoc_corpus::fetch().map(|_| ()),
        (Some("phpdoc-cases"), None) => xtask::phpdoc_corpus::extract(false),
        (Some("phpdoc-cases"), Some("--check")) => xtask::phpdoc_corpus::extract(true),
```

Run: `cargo test --package xtask phpdoc`
Expected: PASS.

- [ ] **Step 4: Generate and commit the case file**

Run: `cargo xtask phpdoc-cases`
Expected: `wrote N cases to .../cases.txt` with N in the vicinity of
253 (the count of `provideParseData` case openers at the pinned
commit). Inspect the file: provenance header first, then one escaped
case per line; spot-check a multiline shape case and a
`ParserException` input (invalid inputs are corpus members too — they
pin the rejected side). Then verify the check mode round-trips:

Run: `cargo xtask phpdoc-cases --check`
Expected: `phpdoc corpus cases are current (N cases)`.

- [ ] **Step 5: Write the coverage test and bless the verdicts**

Create `crates/celerrate_phpdoc_bridge/tests/phpstan_corpus.rs`:

```rust
//! The pinned-reference coverage statement (design section 5): every
//! `TypeParserTest` input from the pinned phpstan/phpdoc-parser gets
//! a parse verdict, pinned in a committed snapshot. The snapshot's
//! header is the published coverage number; the gate is regression
//! against the committed file, re-blessed deliberately with
//! `CELERRATE_BLESS=1`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;

use celerrate_phpdoc_bridge::parse_type_expression_text;

const CASES: &str = include_str!("phpstan_corpus/cases.txt");
const VERDICTS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/phpstan_corpus/verdicts.txt"
);

/// Undoes `xtask/src/phpdoc_corpus.rs`'s `escape` (kept in mirror by
/// the round-trip nature of the snapshot itself).
fn unescape(line: &str) -> String {
    let mut value = String::new();
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => value.push('\n'),
            Some('r') => value.push('\r'),
            Some('t') => value.push('\t'),
            Some('\\') => value.push('\\'),
            Some(other) => {
                value.push('\\');
                value.push(other);
            }
            None => value.push('\\'),
        }
    }
    value
}

fn escape(case: &str) -> String {
    case.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[test]
fn the_pinned_reference_coverage_statement_is_current() {
    let declared: usize = CASES
        .lines()
        .find_map(|line| line.strip_prefix("# cases = "))
        .expect("the case file carries its count header")
        .parse()
        .unwrap();
    // Only header lines are filtered: an empty line is a legitimate
    // case (the upstream corpus includes an empty-input case).
    let cases: Vec<String> = CASES
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(unescape)
        .collect();
    assert_eq!(cases.len(), declared, "the case list and its header disagree");
    let mut parsed = 0usize;
    let mut body = String::new();
    for case in &cases {
        let verdict = if parse_type_expression_text(case).is_some() {
            parsed += 1;
            "ok"
        } else {
            "rejected"
        };
        let _ = writeln!(body, "{verdict}: {}", escape(case));
    }
    let percentage = parsed * 100 / cases.len().max(1);
    let rendered = format!(
        "# The pinned-reference coverage statement (type-engine design, section 5).\n\
         # {parsed} of {} TypeParserTest inputs parse ({percentage}%).\n\
         # The corpus deliberately includes invalid inputs (upstream\n\
         # expects a ParserException on them): they count as rejected.\n\
         {body}",
        cases.len(),
    );
    if std::env::var_os("CELERRATE_BLESS").is_some() {
        std::fs::write(VERDICTS_PATH, &rendered).unwrap();
    }
    let committed = std::fs::read_to_string(VERDICTS_PATH).unwrap();
    assert_eq!(
        committed, rendered,
        "coverage drifted: re-bless with CELERRATE_BLESS=1 and review the diff",
    );
    assert!(
        percentage >= 50,
        "under half the pinned reference parses ({percentage}%): investigate before shipping",
    );
}
```

Bless the first snapshot, then verify it holds without the variable:

```bash
CELERRATE_BLESS=1 cargo test --package celerrate_phpdoc_bridge --test phpstan_corpus
cargo test --package celerrate_phpdoc_bridge --test phpstan_corpus
```

Expected: both PASS. **Read the blessed `verdicts.txt`**: the header
percentage is the coverage statement — record the number for
Task 13's ledger, and skim the `rejected:` lines: each should be
either a deliberately-invalid upstream input or a construct this
plan's ledger names as debt. A rejection outside those two buckets is
a grammar bug: fix it now, re-bless, and keep the diff.

- [ ] **Step 6: The CI job**

In `.github/workflows/corpus.yml`, add a third job alongside
`snapshot` and `bench`:

```yaml
  phpdoc-cases:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - uses: Swatinem/rust-cache@v2
      - uses: actions/cache@v4
        with:
          path: target/phpdoc-parser
          key: phpdoc-parser-${{ hashFiles('xtask/phpdoc-parser.pin') }}
      - run: cargo xtask phpdoc-cases --check
```

(The verdict test itself runs in the ordinary `ci.yml` test job — the
case file is committed, so no network is needed there.)

- [ ] **Step 7: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add xtask crates/celerrate_phpdoc_bridge/tests .github/workflows/corpus.yml
git commit -m "✨ feat(xtask,phpdoc-bridge): the pinned phpdoc-parser corpus pins parse coverage"
```

---

### Task 12: Fuzz seeds and adversarial pins

The docblock fuzz target already drives the whole new surface
(`lex_docblock` → extraction → `parse_type_expression_text`); it needs
seeds that reach the dialect grammar, and the unit suites need the
adversarial pins that CI fuzz smoke cannot guarantee to find.

**Files:**
- Create: `fuzz/corpus/docblock/seed_dialect`
- Modify: `crates/celerrate_phpdoc_bridge/src/expression/mod.rs`
  (adversarial tests)

- [ ] **Step 1: Extend the adversarial unit pins**

In `expression/mod.rs`'s `adversarial_expressions_never_panic`, extend
the input list:

```rust
        let deep_generics = format!("{}int{}", "array<".repeat(200), ">".repeat(200));
        let deep_shapes = format!("{}int{}", "array{a:".repeat(200), "}".repeat(200));
        let deep_callables = format!("{}int{}", "callable(".repeat(200), "): int".repeat(200));
        let comment_bomb = "array{".to_owned() + &"// bomb\n".repeat(5_000) + "}";
        for text in [
            "????", "(((((", "]][[", "\u{0}|\u{0}", "&&&",
            "'unterminated", "\"unterminated", "Foo::", "$",
            "T is ? :", "int<", "array{...<", "callable():",
            "-", "...", "::", "a::*b::*c",
            deep_generics.as_str(), deep_shapes.as_str(),
            deep_callables.as_str(), comment_bomb.as_str(),
            repeated.as_str(),
        ] {
            let _ = parse_type_expression_text(text);
            let _ = parse_type_expression_prefix(text);
        }
```

Run: `cargo test --package celerrate_phpdoc_bridge expression`
Expected: PASS (any panic or stack overflow here is a Task 1-5 bug to
fix before proceeding — the depth guard and `.get()`-only access are
the invariants).

- [ ] **Step 2: Seed the fuzz corpus**

Create `fuzz/corpus/docblock/seed_dialect` (plain text, no trailing
spaces needed):

```
/**
 * @template T of \Entity
 * @param array{id: int, name?: string, ...<string, mixed>} $row
 * @param class-string<T> $class
 * @phpstan-param int<1, max> $limit
 * @psalm-param non-empty-list<'a'|'b'> $choices
 * @param callable(User, int=): ?string $mapper
 * @return ($limit is 1 ? T : list<T>)
 * @psalm-assert !null $row
 * @phpstan-assert-if-true string $class
 * @throws \RuntimeException|\LogicException
 * @property Collection<covariant T> $items
 * @method static Closure<U of T>(U): U pipe(callable(): U ...$stages)
 * @psalm-pure
 */
```

- [ ] **Step 3: Smoke the target (if a nightly toolchain is present)**

Run: `cargo +nightly fuzz run docblock -- -max_total_time=60 -timeout=25 -rss_limit_mb=4096`
Expected: no crash in 60 seconds. If nightly is unavailable locally,
rely on the scheduled `fuzz.yml` run and say so in the commit body —
do not skip silently.

- [ ] **Step 4: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

```bash
git add crates/celerrate_phpdoc_bridge/src fuzz/corpus/docblock
git commit -m "✅ test(phpdoc-bridge): dialect fuzz seeds and adversarial pins"
```

---

### Task 13: Closure — documentation, ledger, the full gate

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (crate doc)
- Modify: `.claude/superpowers/plans/2026-07-15-type-engine-4b-phpstan-dialect.md`
  (the ledger section, appended at the end)

- [ ] **Step 1: Rewrite the crate doc**

`lib.rs`'s module doc currently says the dialect "arrives with plan
4b" — it has arrived. Rewrite to state: the bridge translates the
inherited PHPDoc convention family — standard PHPDoc plus the PHPStan
dialect, with Psalm synonyms — as one plugin with one docblock lexer
and two semantic dialect modules (`dialect/phpstan`, `dialect/psalm`);
the tag conflict table is `dialect`'s rustdoc, the total lowering
table is `lowering`'s rustdoc, and the pinned-reference coverage
statement lives in `tests/phpstan_corpus/verdicts.txt` (repository
documentation is the interim publication home until plan 9c). Keep
the one-dependency and no-docblock-diagnostics sentences.

- [ ] **Step 2: Append the closure ledger to this plan**

Append an `## Accepted debt at closure` section to this plan file
recording (verbatim list, with the measured coverage number filled
in):

- The coverage statement: N% of the pinned `TypeParserTest` inputs
  parse (from `verdicts.txt`); the remainder is deliberately-invalid
  upstream inputs plus the constructs below. The docblock-level
  reference corpus (`PhpDocParserTest.php`) is not measured: debt.
- `@extends`/`@implements`/`@use` are not extracted — the
  linearization threading channel is plan 6's; `@mixin`,
  `@param-out`: no consumer slot, not extracted.
- `@template` defaults (`= X`) parse and drop; variance markers
  declare their variable with the variance dropped.
- Callable-scoped template names lower to `mixed`; a bound-respecting
  lowering is debt. Purity prefixes and Closure classness drop at
  callable lowering (sound widenings).
- Parameter-subject conditionals lower to the branch union; a lattice
  form for them is plan 6's call.
- Const fetches (`Foo::BAR`, `Foo::*`) lower to `mixed` until member
  facts arrive (plans 6-7). `interface-string`/`enum-string`/
  `trait-string` lower as `class-string` (kind refinement debt);
  the string-family and `int-mask` widening rows of the lowering
  table stand as documented.
- Object shapes widen to `object`; unsealed shapes widen to their
  general array.
- Template names resolve before keywords, case-sensitively: a
  template literally named `int` would shadow the keyword within its
  docblock (pathological, accepted).
- The class docblock re-lexes once per member annotation parse
  (deterministic; memoized at the `member_annotations` layer).
- The corpus extractor is layout-coupled to the pinned commit,
  guarded by `cargo xtask phpdoc-cases --check` in CI.
- Assertion subjects travel verbatim; interpretation (including
  `$this->prop` forms) is plan 5's.
- Unknown `@psalm-*` tags outside the enumerated bucket are simply
  unrecognized — indistinguishable from typos, by design.
- The plugin identity version still tracks the workspace version:
  4b's behavior change inside 0.0.x relies on plan 9a's schema and
  version bumps for persistent-cache correctness.
- Any invalidation-pin deviation observed in Task 9 Step 6 (record
  the honest counts, the 4a Task 7 precedent).

Plus anything genuinely encountered during execution.

- [ ] **Step 3: The full gate**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo xtask dependency-shape
cargo xtask phpdoc-cases --check
```

Expected: every command PASS. The dependency-shape check proves the
bridge still depends on `celerrate_plugin` alone.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_phpdoc_bridge/src/lib.rs .claude/superpowers/plans/2026-07-15-type-engine-4b-phpstan-dialect.md
git commit -m "📝 docs(phpdoc-bridge): the dialect tables, the coverage statement, and the closure ledger"
```

---

## Accepted debt at closure

- **The coverage statement**: 226 of 241 pinned `TypeParserTest` inputs
  parse (93%), against phpstan/phpdoc-parser 2.3.3
  (`fb19eedd2bb67ff8cf7a5502ad329e701d6398a3`), per
  `tests/phpstan_corpus/verdicts.txt`. The plan's original "~253"
  estimate was an over-count: the pinned commit's `provideParseData`
  carries 241 cases, not ~253. The 15 non-parsing inputs break down as:
  13 deliberately-invalid upstream inputs (the reference itself expects
  a `ParserException` on them), 1 full-consumption probe artifact
  (`MongoCollection <p>...`, prose that a prefix parse consumes into
  and a full-text parse correctly rejects), and 1 structural gap (the
  const-fetch shape key below). The docblock-level reference corpus
  (`PhpDocParserTest.php`) is not measured: debt, as decision 9 already
  traces.
- `@extends`/`@implements`/`@use` are not extracted — the
  linearization threading channel is plan 6's; `@mixin`, `@param-out`:
  no consumer slot, not extracted.
- `@template` defaults (`= X`) parse and drop; variance markers
  declare their variable with the variance dropped.
- Callable-scoped template names lower to `mixed`; a bound-respecting
  lowering is debt. Purity prefixes and Closure classness drop at
  callable lowering (sound widenings).
- Parameter-subject conditionals lower to the branch union; a lattice
  form for them is plan 6's call.
- Const fetches (`Foo::BAR`, `Foo::*`) lower to `mixed` until member
  facts arrive (plans 6-7). `interface-string`/`enum-string`/
  `trait-string` lower as `class-string` (kind refinement debt); the
  string-family and `int-mask` widening rows of the lowering table
  stand as documented.
- Object shapes widen to `object`; unsealed shapes widen to their
  general array.
- Template names resolve before keywords, case-sensitively: a template
  literally named `int` would shadow the keyword within its docblock
  (pathological, accepted).
- The class docblock re-lexes once per member annotation parse
  (deterministic; memoized at the `member_annotations` layer).
- The corpus extractor is layout-coupled to the pinned commit, guarded
  by `cargo xtask phpdoc-cases --check` in CI.
- Assertion subjects travel verbatim; interpretation (including
  `$this->prop` forms) is plan 5's.
- Unknown `@psalm-*` tags outside the enumerated bucket are simply
  unrecognized — indistinguishable from typos, by design.
- The plugin identity version still tracks the workspace version: 4b's
  behavior change inside 0.0.x relies on plan 9a's schema and version
  bumps for persistent-cache correctness.
- **Invalidation pin (Task 9 Step 6)**: no deviation. The honest counts
  matched the plan's expectation on the first run —
  `a_class_docblock_prose_edit_backdates_at_the_member_annotations_stage`
  observed `member_annotations` re-executing exactly once with an
  unchanged value and the `subtype_of` probe spared at zero executions,
  exactly as the neighbouring pin predicted. Unlike 4a's Task 7, there
  was no honest-mechanism surprise to reconcile.

### Additional debt discovered during execution

- `array{Foo::BAR: int}` (a const-fetch expression used as a shape
  key) does not parse. `parse_shape_key` only recognizes an
  identifier, a string literal, or an integer literal
  (`expression/parser.rs`); a const-fetch key would need a new
  `ShapeKeyExpression` variant plus its lowering. Structural gap,
  deferred — this is the one structural rejection inside the 15
  non-parsing corpus cases above.
- Signed radix integer literals (`-0x7F`, `-0b11`) degrade through the
  `Float` token path into nonsense written text (the radix strip only
  removes the `0x`/`0b`/`0o` prefix, not the leading `-`, so
  `i64::from_str_radix` fails and the literal falls back to a `Float`
  token whose text is not a valid float either) which lowers to plain
  `float`. Pre-existing Task 1 behavior, explicitly called out in that
  task's implementer note; recorded here as accepted debt rather than
  fixed, since no such form appears in the reference corpus.
- The `*` wildcard generic argument (the bivariant "unknown, don't
  care" argument) is rewritten to `Name("mixed")` at the parser
  (`parser::parse_generic_arguments`), before the lowering table ever
  sees it — so the lowering table's rustdoc had no row for it. Fixed
  as a doc-only change: `lowering.rs`'s table now carries a
  cross-reference row pointing back at the parser rewrite.
- `parse_constant_name` tolerates whitespace between `::` and the
  constant name (`Foo:: BAR` parses the same as `Foo::BAR`), contradicting
  its doc comment's adjacency claim. The adjacency check only guards
  the gap between successive `Name`/`Asterisk` tokens inside a
  multi-part constant (`Foo::BAR_*`); it never checks the gap between
  the `DoubleColon` token and the first constant token. Recorded for
  final-review triage rather than fixed here, since tightening it has
  no test pressure from the pinned corpus.
- A trailing `&` or `&$var` after an otherwise-valid type ends the
  type prefix instead of failing the whole construct. This is a
  deliberate consequence of the intersection-parsing stop set (an
  intersection member that fails to parse simply stops the
  intersection rather than invalidating what was already parsed) and
  it is what lets `@param string &$ref` (a type followed by a
  by-reference parameter marker) parse correctly instead of being
  swallowed by an attempted-and-failed intersection. Benefit, not a
  bug; recorded so it reads as intentional.
- `inherited_annotations` (`crates/celerrate_types/src/declared.rs`)
  fills a member's `assertions` from an ancestor with the same
  fill-if-missing rule already used for `throws` and `value`: an
  ancestor's whole assertions list is taken only while the member's
  own list is still empty; the first ancestor found with a non-empty
  list wins and no further ancestor contributes on top of it. This
  behavior falls out of reusing the existing merge path and goes
  beyond what the plan's text specified. Cross-ancestor assertion
  inheritance itself has no dedicated test in this plan; traced as
  debt alongside the general assertion-consumer gap (plan 5's).
- Four grammar gaps were found and fixed by the corpus audit (Task 11)
  rather than left as debt: newline-scoped `*` leading trivia inside a
  docblock line, a leading `+` on numeric literals, the `*` wildcard
  generic argument, and a trailing comma in callable parameter lists.
  These are recorded here as validated-by-harness fixes, not as
  outstanding debt — the harness (`cargo xtask phpdoc-cases --check`
  plus the pinned verdict snapshot) is what caught them and now guards
  against regression.

