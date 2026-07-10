# Foundations Part 3: The Lexer, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the `celerrate_syntax` crate with a complete, error-resilient, hand-written PHP 8.1+ lexer: `lex(&str) -> (Vec<Token>, Vec<LexerDiagnostic>)`, lossless token stream, structured diagnostics, insta snapshot corpus, and a cargo-fuzz target with a CI job.

**Architecture:** A single `SyntaxKind` vocabulary (`#[repr(u16)]`, token kinds only for now) and rust-analyzer style tokens (`kind` + `length`, no stored offsets). The lexer is a state machine over a char cursor with an explicit mode stack: `InlineHtml`, `Scripting`, and the interpolation modes (`DoubleQuotedString`, `Backtick`, `Heredoc`, `Nowdoc`, `VariableOffset`). Trivia are ordinary tokens; nothing is discarded. Diagnostics travel beside the stream, never instead of it. The lexer always terminates and every input produces a complete stream whose concatenated token texts reproduce the input byte for byte.

**Tech Stack:** Rust (edition 2024), `text-size`, `celerrate_source`; dev-dependency `insta` (with the `glob` feature); `cargo-fuzz`/`libfuzzer-sys` in a standalone `fuzz/` package.

**Spec:** `.claude/superpowers/specs/2026-07-10-foundations-3-lexer-design.md` (read it before starting).

## Global Constraints

- Zero-panic policy: workspace lints deny `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` is forbidden. Never index or slice with `[]`; use `get`, `strip_prefix`, `starts_with`, and iterators. Test files may carry a file-level `#![allow(clippy::expect_used)]` (and, where they slice, `#![allow(clippy::indexing_slicing)]`) with a reason comment.
- All files in English, full words for names (standard acronyms fine).
- No em-dashes anywhere in generated files; use commas, colons, or parentheses.
- Commits: gitmoji + Conventional Commits, scope `syntax`. No AI attribution lines. Verify `git config user.email` prints `5817251+jh3ady@users.noreply.github.com` before the first commit.
- TDD strictly: failing test, run to see it fail, minimal implementation, run to see it pass, commit.
- Work happens on branch `foundations-3-lexer` cut from `main` (the executor's worktree skill handles creation; if working inline, `git switch -c foundations-3-lexer` first).
- The crate depends only on `celerrate_source` and `text-size` at runtime. `insta` is a dev-dependency. The fuzz package is its own workspace and may use `libfuzzer-sys`.
- Full PHP 8.1+ lexical grammar, no version gating: short `<?` is always an open tag, `${name}` interpolation is lexed normally, availability and deprecation judgments belong to upper layers.
- Verification commands (from the repository root): `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

## File Structure

- `crates/celerrate_syntax/Cargo.toml` - new: crate manifest.
- `crates/celerrate_syntax/src/lib.rs` - new: crate docs, module declarations, re-exports.
- `crates/celerrate_syntax/src/syntax_kind.rs` - new: the `SyntaxKind` enumeration, keyword lookup, trivia classification.
- `crates/celerrate_syntax/src/token.rs` - new: the `Token` struct.
- `crates/celerrate_syntax/src/diagnostic.rs` - new: `LexerDiagnostic` and `LexerDiagnosticKind`.
- `crates/celerrate_syntax/src/cursor.rs` - new: char cursor with peeking and token-length accounting.
- `crates/celerrate_syntax/src/lexer.rs` - new: `lex()`, the `Lexer` driver, the `Mode` stack, end-of-input flushing.
- `crates/celerrate_syntax/src/lexer/inline_html.rs` - new: `InlineHtml` mode and open tags.
- `crates/celerrate_syntax/src/lexer/scripting.rs` - new: `Scripting` mode (identifiers, keywords, variables, numbers, operators, casts, comments).
- `crates/celerrate_syntax/src/lexer/strings.rs` - new: single-quoted strings, the interpolation modes, heredoc and nowdoc.
- `crates/celerrate_syntax/tests/support/mod.rs` - new: shared test helpers (lossless assertion, kind and text listings).
- `crates/celerrate_syntax/tests/*.rs` - new: one integration test file per feature area.
- `crates/celerrate_syntax/tests/corpus/*.php` + `tests/corpus.rs` - new: insta snapshot corpus.
- `fuzz/` - new: standalone cargo-fuzz package with the `lex` target and committed seed corpus.
- `.github/workflows/ci.yml` - modified: add the fuzz job.
- `CHANGELOG.md` - modified: Unreleased entry.

---

### Task 1: Crate scaffolding, `SyntaxKind`, `Token`, `LexerDiagnostic`

**Files:**
- Create: `crates/celerrate_syntax/Cargo.toml`
- Create: `crates/celerrate_syntax/src/lib.rs`
- Create: `crates/celerrate_syntax/src/syntax_kind.rs`
- Create: `crates/celerrate_syntax/src/token.rs`
- Create: `crates/celerrate_syntax/src/diagnostic.rs`
- Create: `crates/celerrate_syntax/tests/syntax_kind.rs`

**Interfaces:**
- Consumes: `TextRange`, `TextSize` re-exported by `celerrate_source`.
- Produces (all re-exported from the crate root):
  - `pub enum SyntaxKind` (`#[repr(u16)]`, derives `Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord`) with `pub fn from_keyword(text: &str) -> Option<SyntaxKind>` and `pub fn is_trivia(self) -> bool`.
  - `pub struct Token { pub kind: SyntaxKind, pub length: TextSize }` (derives `Debug, Clone, Copy, PartialEq, Eq`) with `pub fn new(kind: SyntaxKind, length: TextSize) -> Token`.
  - `pub struct LexerDiagnostic { pub kind: LexerDiagnosticKind, pub range: TextRange }` and `pub enum LexerDiagnosticKind { UnexpectedCharacter, UnterminatedBlockComment, UnterminatedString, UnterminatedHeredoc, UnterminatedInterpolation }` (both derive `Debug, Clone, Copy, PartialEq, Eq`).

- [ ] **Step 1: Create the crate manifest and empty library**

Create `crates/celerrate_syntax/Cargo.toml`:

```toml
[package]
name = "celerrate_syntax"
description = "PHP lexer and parser for the Celerrate toolchain"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
celerrate_source = { path = "../celerrate_source" }
text-size = { workspace = true }

[lints]
workspace = true
```

Create `crates/celerrate_syntax/src/lib.rs`:

```rust
//! PHP lexical analysis for the Celerrate toolchain. This part ships the
//! lexer: [`lex`] turns decoded source text into a lossless token stream
//! (trivia included, nothing discarded) plus structured diagnostics. The
//! parser and syntax tree arrive in the next Foundations part.

mod diagnostic;
mod syntax_kind;
mod token;

pub use diagnostic::{LexerDiagnostic, LexerDiagnosticKind};
pub use syntax_kind::SyntaxKind;
pub use token::Token;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_syntax/tests/syntax_kind.rs`:

```rust
use celerrate_syntax::SyntaxKind;

#[test]
fn keywords_resolve_case_insensitively() {
    assert_eq!(SyntaxKind::from_keyword("echo"), Some(SyntaxKind::Echo));
    assert_eq!(SyntaxKind::from_keyword("Echo"), Some(SyntaxKind::Echo));
    assert_eq!(SyntaxKind::from_keyword("ECHO"), Some(SyntaxKind::Echo));
    assert_eq!(
        SyntaxKind::from_keyword("include_once"),
        Some(SyntaxKind::IncludeOnce)
    );
    assert_eq!(SyntaxKind::from_keyword("readonly"), Some(SyntaxKind::Readonly));
}

#[test]
fn die_is_an_alias_of_exit() {
    assert_eq!(SyntaxKind::from_keyword("exit"), Some(SyntaxKind::Exit));
    assert_eq!(SyntaxKind::from_keyword("die"), Some(SyntaxKind::Exit));
    assert_eq!(SyntaxKind::from_keyword("DIE"), Some(SyntaxKind::Exit));
}

#[test]
fn non_keywords_do_not_resolve() {
    assert_eq!(SyntaxKind::from_keyword("echoes"), None);
    assert_eq!(SyntaxKind::from_keyword("true"), None);
    assert_eq!(SyntaxKind::from_keyword("self"), None);
    assert_eq!(SyntaxKind::from_keyword(""), None);
    assert_eq!(SyntaxKind::from_keyword("très_long_identifiant"), None);
}

#[test]
fn trivia_kinds_are_classified() {
    assert!(SyntaxKind::Whitespace.is_trivia());
    assert!(SyntaxKind::LineComment.is_trivia());
    assert!(SyntaxKind::BlockComment.is_trivia());
    assert!(SyntaxKind::DocComment.is_trivia());
    assert!(SyntaxKind::Shebang.is_trivia());
    assert!(!SyntaxKind::Identifier.is_trivia());
    assert!(!SyntaxKind::InlineHtml.is_trivia());
    assert!(!SyntaxKind::Error.is_trivia());
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test syntax_kind`
Expected: compilation error, `SyntaxKind` not found.

- [ ] **Step 4: Write the implementation**

Create `crates/celerrate_syntax/src/syntax_kind.rs`:

```rust
/// Every kind of token in PHP source text.
///
/// One vocabulary shared by the whole syntax layer, `#[repr(u16)]` so a
/// future rowan-style tree can store it directly. Token kinds only for
/// now; the parser part appends node kinds after them.
///
/// Keywords each get their own kind, resolved case-insensitively by the
/// lexer. Semi-reserved uses (`$object->list()`, `const FOR = 1;`,
/// `enum` as a plain name) are the parser's business: it re-treats
/// keyword kinds as identifiers where the grammar allows. `true`,
/// `false`, `null`, `self`, `parent`, and the magic constants are plain
/// identifiers, resolved semantically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SyntaxKind {
    // Trivia.
    Whitespace,
    /// `//` and `#` comments, up to the end of the line or a `?>`.
    LineComment,
    /// `/* ... */` comments.
    BlockComment,
    /// `/** ... */` docblocks, a distinct kind: the type engine reads them.
    DocComment,
    /// A `#!` first line.
    Shebang,

    // Tags and inline HTML.
    /// `<?php`.
    OpenTag,
    /// `<?=`.
    OpenTagEcho,
    /// `<?`, lexed unconditionally; availability is a semantic judgment.
    ShortOpenTag,
    /// `?>`, plus the single newline PHP swallows after it, if present.
    CloseTag,
    /// Everything outside PHP tags.
    InlineHtml,

    // Names.
    Identifier,
    /// `$name`.
    Variable,

    // Literals and string structure.
    IntegerLiteral,
    FloatLiteral,
    /// A whole `'...'` (or `b'...'`) string, quotes included.
    SingleQuotedString,
    /// A literal run inside an interpolated string, heredoc, or backtick.
    StringFragment,
    /// A `"` delimiter (or the opening `b"`).
    DoubleQuote,
    /// A `` ` `` delimiter.
    Backtick,
    /// `<<<LABEL` (or quoted label), trailing newline included.
    HeredocStart,
    /// The closing label of a heredoc or nowdoc, indentation included.
    HeredocEnd,
    /// `${` opening the deprecated interpolation form.
    DollarOpenBrace,

    // Keywords.
    Abstract,
    And,
    Array,
    As,
    Break,
    Callable,
    Case,
    Catch,
    Class,
    Clone,
    Const,
    Continue,
    Declare,
    Default,
    Do,
    Echo,
    Else,
    ElseIf,
    Empty,
    EndDeclare,
    EndFor,
    EndForeach,
    EndIf,
    EndSwitch,
    EndWhile,
    Enum,
    Eval,
    /// `exit` and its alias `die`.
    Exit,
    Extends,
    Final,
    Finally,
    Fn,
    For,
    Foreach,
    Function,
    Global,
    Goto,
    If,
    Implements,
    Include,
    IncludeOnce,
    InstanceOf,
    InsteadOf,
    Interface,
    Isset,
    List,
    Match,
    Namespace,
    New,
    Or,
    Print,
    Private,
    Protected,
    Public,
    Readonly,
    Require,
    RequireOnce,
    Return,
    Static,
    Switch,
    Throw,
    Trait,
    Try,
    Unset,
    Use,
    Var,
    While,
    Xor,
    Yield,

    // Casts (single tokens, inner whitespace included).
    IntCast,
    BoolCast,
    FloatCast,
    StringCast,
    BinaryCast,
    ArrayCast,
    ObjectCast,

    // Operators and punctuation.
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,
    Equals,
    PlusEquals,
    MinusEquals,
    StarEquals,
    SlashEquals,
    DotEquals,
    PercentEquals,
    StarStarEquals,
    AmpersandEquals,
    PipeEquals,
    CaretEquals,
    LessLessEquals,
    GreaterGreaterEquals,
    QuestionQuestionEquals,
    EqualsEquals,
    EqualsEqualsEquals,
    /// `!=` and its alias `<>`.
    BangEquals,
    BangEqualsEquals,
    Less,
    Greater,
    LessEquals,
    GreaterEquals,
    /// `<=>`.
    Spaceship,
    PlusPlus,
    MinusMinus,
    LessLess,
    GreaterGreater,
    Dot,
    Bang,
    AmpersandAmpersand,
    PipePipe,
    QuestionQuestion,
    Question,
    Colon,
    ColonColon,
    Semicolon,
    Comma,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    At,
    Dollar,
    Backslash,
    /// `->`.
    Arrow,
    /// `?->`.
    NullsafeArrow,
    /// `=>`.
    FatArrow,
    /// `...`.
    Ellipsis,
    OpenParenthesis,
    CloseParenthesis,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    /// `#[`, distinct from the `#` line comment.
    AttributeOpen,

    /// A character no rule accepts.
    Error,
}

/// The longest PHP keyword is `include_once`: twelve bytes.
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

Note: `require_once` is also twelve bytes; `yield from` is deliberately two tokens (`Yield`, then `from` as an `Identifier`), composed by the parser, so no multi-word token exists.

Create `crates/celerrate_syntax/src/token.rs`:

```rust
use celerrate_source::TextSize;

use crate::syntax_kind::SyntaxKind;

/// One lexed token: a kind and a byte length, rust-analyzer style.
///
/// No offset is stored; positions are reconstructed by accumulating
/// lengths, which makes overlaps and gaps structurally impossible. The
/// lossless invariant: concatenating every token's text reproduces the
/// input byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub length: TextSize,
}

impl Token {
    pub fn new(kind: SyntaxKind, length: TextSize) -> Self {
        Self { kind, length }
    }
}
```

Create `crates/celerrate_syntax/src/diagnostic.rs`:

```rust
use celerrate_source::TextRange;

/// What went wrong, structurally. Rendering into messages is an upper
/// layer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerDiagnosticKind {
    /// A character no lexing rule accepts; the matching token is `Error`.
    UnexpectedCharacter,
    /// `/*` without `*/`; the comment token runs to the end of input.
    UnterminatedBlockComment,
    /// A quoted or backtick string still open at the end of input.
    UnterminatedString,
    /// A heredoc or nowdoc whose closing label never appears.
    UnterminatedHeredoc,
    /// `{$` or `${` without its closing brace.
    UnterminatedInterpolation,
}

/// A lexer diagnostic: a structured kind and the range it points at.
///
/// Diagnostics travel beside the token stream, never instead of it: the
/// stream stays complete and lossless even on degenerate input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexerDiagnostic {
    pub kind: LexerDiagnosticKind,
    pub range: TextRange,
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test syntax_kind`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_syntax Cargo.lock
git commit -m "✨ feat(syntax): add SyntaxKind, Token, and lexer diagnostics"
```

---

### Task 2: The char cursor

**Files:**
- Create: `crates/celerrate_syntax/src/cursor.rs`
- Modify: `crates/celerrate_syntax/src/lib.rs`

**Interfaces:**
- Consumes: `TextSize` from `celerrate_source`.
- Produces (crate-private, used by every lexer module):
  - `pub(crate) struct Cursor<'source>` with:
    - `pub(crate) fn new(source: &'source str) -> Cursor<'source>`
    - `pub(crate) fn peek(&self) -> Option<char>`
    - `pub(crate) fn peek_second(&self) -> Option<char>`
    - `pub(crate) fn rest(&self) -> &'source str` (the unconsumed input)
    - `pub(crate) fn bump(&mut self) -> Option<char>`
    - `pub(crate) fn bump_bytes(&mut self, count: usize)` (advance `count` bytes; callers pass char-boundary counts computed from `rest()`)
    - `pub(crate) fn eat(&mut self, expected: char) -> bool`
    - `pub(crate) fn eat_while(&mut self, predicate: impl Fn(char) -> bool)`
    - `pub(crate) fn is_at_end(&self) -> bool`
    - `pub(crate) fn pending_text(&self) -> &'source str` (text consumed since the current token started)
    - `pub(crate) fn take_length(&mut self) -> TextSize` (finish the current token: its byte length, then reset the token start)

- [ ] **Step 1: Write the failing unit tests**

Create `crates/celerrate_syntax/src/cursor.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    //! `expect` is fine here: failing loudly is what a test should do.
    #![allow(clippy::expect_used)]

    use super::Cursor;

    #[test]
    fn peeks_without_consuming() {
        let cursor = Cursor::new("ab");
        assert_eq!(cursor.peek(), Some('a'));
        assert_eq!(cursor.peek_second(), Some('b'));
        assert_eq!(cursor.rest(), "ab");
    }

    #[test]
    fn bumps_consume_one_character() {
        let mut cursor = Cursor::new("héllo");
        assert_eq!(cursor.bump(), Some('h'));
        assert_eq!(cursor.bump(), Some('é'));
        assert_eq!(cursor.rest(), "llo");
        assert!(!cursor.is_at_end());
    }

    #[test]
    fn take_length_counts_bytes_and_resets() {
        let mut cursor = Cursor::new("é1é2");
        cursor.bump();
        cursor.bump();
        assert_eq!(cursor.pending_text(), "é1");
        assert_eq!(u32::from(cursor.take_length()), 3);
        cursor.bump();
        assert_eq!(cursor.pending_text(), "é");
        assert_eq!(u32::from(cursor.take_length()), 2);
    }

    #[test]
    fn eat_consumes_only_the_expected_character() {
        let mut cursor = Cursor::new("ab");
        assert!(!cursor.eat('b'));
        assert!(cursor.eat('a'));
        assert_eq!(cursor.rest(), "b");
    }

    #[test]
    fn eat_while_stops_at_the_first_rejection() {
        let mut cursor = Cursor::new("aaab");
        cursor.eat_while(|character| character == 'a');
        assert_eq!(cursor.rest(), "b");
        cursor.eat_while(|character| character == 'a');
        assert_eq!(cursor.rest(), "b");
    }

    #[test]
    fn bump_bytes_advances_by_byte_count() {
        let mut cursor = Cursor::new("<?php echo");
        cursor.bump_bytes(5);
        assert_eq!(cursor.pending_text(), "<?php");
        assert_eq!(cursor.rest(), " echo");
    }

    #[test]
    fn end_of_input_is_stable() {
        let mut cursor = Cursor::new("");
        assert!(cursor.is_at_end());
        assert_eq!(cursor.bump(), None);
        assert_eq!(cursor.peek(), None);
        assert_eq!(u32::from(cursor.take_length()), 0);
    }
}
```

Add `mod cursor;` to `crates/celerrate_syntax/src/lib.rs` (below the existing `mod diagnostic;`, keeping modules alphabetical; no `pub use`, the cursor is internal).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --lib`
Expected: compilation error, `Cursor` not found.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/celerrate_syntax/src/cursor.rs`, above the test module:

```rust
use core::str::Chars;

use celerrate_source::TextSize;

/// A char cursor over the source with bounded lookahead and token-length
/// accounting. All arithmetic is in bytes; no indexing anywhere, only
/// iterator consumption and `str` prefix operations on [`rest`](Self::rest).
pub(crate) struct Cursor<'source> {
    characters: Chars<'source>,
    /// The unconsumed input as it was when the current token started.
    rest_at_token_start: &'source str,
}

impl<'source> Cursor<'source> {
    pub(crate) fn new(source: &'source str) -> Self {
        Self {
            characters: source.chars(),
            rest_at_token_start: source,
        }
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.characters.clone().next()
    }

    pub(crate) fn peek_second(&self) -> Option<char> {
        let mut lookahead = self.characters.clone();
        lookahead.next();
        lookahead.next()
    }

    /// The unconsumed input. String-based lookahead (`starts_with`,
    /// case-insensitive tag and cast matching) goes through this.
    pub(crate) fn rest(&self) -> &'source str {
        self.characters.as_str()
    }

    pub(crate) fn bump(&mut self) -> Option<char> {
        self.characters.next()
    }

    /// Advances by `count` bytes. Callers compute `count` from
    /// [`rest`](Self::rest), so it always lands on a char boundary; a
    /// defensive fallback consumes everything on an out-of-range count.
    pub(crate) fn bump_bytes(&mut self, count: usize) {
        self.characters = self.rest().get(count..).unwrap_or_default().chars();
    }

    pub(crate) fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.characters.next();
            true
        } else {
            false
        }
    }

    pub(crate) fn eat_while(&mut self, predicate: impl Fn(char) -> bool) {
        while let Some(character) = self.peek() {
            if !predicate(character) {
                break;
            }
            self.characters.next();
        }
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.rest().is_empty()
    }

    fn pending_byte_length(&self) -> usize {
        self.rest_at_token_start.len() - self.rest().len()
    }

    /// The text consumed since the current token started.
    pub(crate) fn pending_text(&self) -> &'source str {
        self.rest_at_token_start
            .get(..self.pending_byte_length())
            .unwrap_or_default()
    }

    /// Finishes the current token: returns its byte length and starts the
    /// next one. Inputs are within the 4 GiB `TextSize` cap
    /// (`SourceText` guarantees it); the conversion saturates defensively
    /// rather than failing.
    pub(crate) fn take_length(&mut self) -> TextSize {
        let length = self.pending_byte_length();
        self.rest_at_token_start = self.rest();
        u32::try_from(length)
            .map(TextSize::from)
            .unwrap_or_else(|_| TextSize::from(u32::MAX))
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --lib`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax/src/cursor.rs crates/celerrate_syntax/src/lib.rs
git commit -m "✨ feat(syntax): add the lexer char cursor"
```

---

### Task 3: `lex()`, inline HTML, open and close tags

**Files:**
- Create: `crates/celerrate_syntax/src/lexer.rs`
- Create: `crates/celerrate_syntax/src/lexer/inline_html.rs`
- Create: `crates/celerrate_syntax/src/lexer/scripting.rs`
- Create: `crates/celerrate_syntax/tests/support/mod.rs`
- Create: `crates/celerrate_syntax/tests/inline_html.rs`
- Modify: `crates/celerrate_syntax/src/lib.rs`

**Interfaces:**
- Consumes: `Cursor` (Task 2 signatures), `SyntaxKind`, `Token`, `LexerDiagnostic`, `LexerDiagnosticKind` (Task 1).
- Produces:
  - `pub fn lex(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>)`, re-exported from the crate root.
  - Crate-private, for the mode modules: `struct Lexer<'source>` with fields `source: &'source str`, `cursor: Cursor<'source>`, `modes: Vec<Mode>`, `offset: TextSize`, `tokens: Vec<Token>`, `diagnostics: Vec<LexerDiagnostic>`; methods `emit(&mut self, kind: SyntaxKind)`, `diagnose(&mut self, kind: LexerDiagnosticKind, range: TextRange)`, `token_start(&self) -> TextSize`, `current_mode(&self) -> Mode`, `set_mode(&mut self, mode: Mode)`, `push_mode(&mut self, mode: Mode)`, `pop_mode(&mut self)`, `at_line_start(&self) -> bool`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] enum Mode` with variants `InlineHtml`, `Scripting { opened_by_interpolation_at: Option<TextSize> }`, `DoubleQuotedString { opening: TextSize }`, `Backtick { opening: TextSize }`, `Heredoc { start: TextRange, label: TextRange }`, `Nowdoc { start: TextRange, label: TextRange }`, `VariableOffset` (string modes arrive in Tasks 9 to 11; the variants exist from the start so the dispatch loop never changes shape).
  - Test support: `pub fn lex_verified(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>)` (lexes and asserts losslessness), `pub fn kinds(source: &str) -> Vec<SyntaxKind>`, `pub fn texts(source: &str) -> Vec<(SyntaxKind, String)>`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/support/mod.rs`:

```rust
//! Shared helpers for the lexer integration tests. Every lexing goes
//! through the lossless assertion: concatenated token lengths must cover
//! the source exactly.
#![allow(clippy::expect_used, clippy::indexing_slicing)]

use celerrate_syntax::{LexerDiagnostic, SyntaxKind, Token, lex};

pub fn assert_lossless(source: &str, tokens: &[Token]) {
    let total: usize = tokens
        .iter()
        .map(|token| usize::from(token.length))
        .sum();
    assert_eq!(
        total,
        source.len(),
        "token lengths must cover the source exactly: {tokens:?}"
    );
    assert!(
        tokens.iter().all(|token| u32::from(token.length) > 0),
        "no token may be empty: {tokens:?}"
    );
}

pub fn lex_verified(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>) {
    let (tokens, diagnostics) = lex(source);
    assert_lossless(source, &tokens);
    (tokens, diagnostics)
}

pub fn kinds(source: &str) -> Vec<SyntaxKind> {
    lex_verified(source).0.iter().map(|token| token.kind).collect()
}

pub fn texts(source: &str) -> Vec<(SyntaxKind, String)> {
    let (tokens, _diagnostics) = lex_verified(source);
    let mut offset = 0usize;
    tokens
        .iter()
        .map(|token| {
            let end = offset + usize::from(token.length);
            let text = source[offset..end].to_owned();
            offset = end;
            (token.kind, text)
        })
        .collect()
}
```

Create `crates/celerrate_syntax/tests/inline_html.rs`:

```rust
mod support;

use celerrate_syntax::SyntaxKind::*;
use support::{kinds, lex_verified, texts};

#[test]
fn empty_input_yields_no_tokens() {
    let (tokens, diagnostics) = lex_verified("");
    assert!(tokens.is_empty());
    assert!(diagnostics.is_empty());
}

#[test]
fn pure_html_is_one_inline_html_token() {
    assert_eq!(kinds("<h1>Hello</h1>"), [InlineHtml]);
}

#[test]
fn open_tag_after_html() {
    assert_eq!(
        texts("<div><?php"),
        [
            (InlineHtml, "<div>".to_owned()),
            (OpenTag, "<?php".to_owned()),
        ]
    );
}

#[test]
fn open_tag_is_case_insensitive() {
    assert_eq!(kinds("<?PHP"), [OpenTag]);
    assert_eq!(kinds("<?Php\n"), [OpenTag, Whitespace]);
}

#[test]
fn open_tag_requires_a_boundary() {
    // "<?phpx" is a short open tag followed by scripting content.
    let listing = texts("<?phpx");
    assert_eq!(listing.first(), Some(&(ShortOpenTag, "<?".to_owned())));
}

#[test]
fn echo_and_short_open_tags() {
    assert_eq!(kinds("<?="), [OpenTagEcho]);
    assert_eq!(kinds("<?"), [ShortOpenTag]);
}

#[test]
fn close_tag_returns_to_html_and_swallows_one_newline() {
    assert_eq!(
        texts("<?php ?>\nhtml\n<?php"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (CloseTag, "?>\n".to_owned()),
            (InlineHtml, "html\n".to_owned()),
            (OpenTag, "<?php".to_owned()),
        ]
    );
}

#[test]
fn close_tag_swallows_a_crlf_newline() {
    assert_eq!(
        texts("<?php ?>\r\nx"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (CloseTag, "?>\r\n".to_owned()),
            (InlineHtml, "x".to_owned()),
        ]
    );
}

#[test]
fn shebang_on_the_first_line_is_trivia() {
    assert_eq!(
        texts("#!/usr/bin/env php\n<?php"),
        [
            (Shebang, "#!/usr/bin/env php".to_owned()),
            (InlineHtml, "\n".to_owned()),
            (OpenTag, "<?php".to_owned()),
        ]
    );
}

#[test]
fn lone_angle_brackets_stay_inline_html() {
    assert_eq!(kinds("a < b <today>"), [InlineHtml]);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test inline_html`
Expected: compilation error, `lex` not found.

- [ ] **Step 3: Write the driver and the two first modes**

Create `crates/celerrate_syntax/src/lexer.rs`:

```rust
use celerrate_source::{TextRange, TextSize};

use crate::cursor::Cursor;
use crate::diagnostic::{LexerDiagnostic, LexerDiagnosticKind};
use crate::syntax_kind::SyntaxKind;
use crate::token::Token;

mod inline_html;
mod scripting;

/// Lexes decoded PHP source text into a lossless token stream plus
/// structured diagnostics. Always terminates, never fails: degenerate
/// input yields `Error` tokens and diagnostics, never a crash or a hole
/// in the stream.
pub fn lex(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>) {
    let mut lexer = Lexer::new(source);
    lexer.run();
    (lexer.tokens, lexer.diagnostics)
}

/// The lexer's current context. `Copy`: label and opening positions are
/// ranges into the source, not owned strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Outside PHP tags; the initial mode.
    InlineHtml,
    /// Inside PHP code. `opened_by_interpolation_at` is `Some` when this
    /// entry was pushed by `{$` or `${` inside a string, so an
    /// unterminated interpolation can be reported at end of input.
    Scripting {
        opened_by_interpolation_at: Option<TextSize>,
    },
    /// Inside `"..."`; `opening` locates the opening quote.
    DoubleQuotedString { opening: TextSize },
    /// Inside `` `...` ``.
    Backtick { opening: TextSize },
    /// Inside a heredoc body; `start` is the `<<<LABEL` token's range and
    /// `label` the range of the bare label text within it.
    Heredoc { start: TextRange, label: TextRange },
    /// Inside a nowdoc body (no interpolation).
    Nowdoc { start: TextRange, label: TextRange },
    /// Inside the `[...]` offset of a simple string interpolation.
    VariableOffset,
}

const BASE_SCRIPTING: Mode = Mode::Scripting {
    opened_by_interpolation_at: None,
};

struct Lexer<'source> {
    source: &'source str,
    cursor: Cursor<'source>,
    modes: Vec<Mode>,
    /// Absolute offset of the current token's start.
    offset: TextSize,
    tokens: Vec<Token>,
    diagnostics: Vec<LexerDiagnostic>,
}

impl<'source> Lexer<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            cursor: Cursor::new(source),
            modes: vec![Mode::InlineHtml],
            offset: TextSize::from(0),
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn run(&mut self) {
        while !self.cursor.is_at_end() {
            match self.current_mode() {
                Mode::InlineHtml => self.lex_inline_html(),
                Mode::Scripting { .. } => self.lex_scripting(),
                // The string modes arrive in later tasks; until then the
                // dispatch loop only ever sees the two modes above.
                Mode::DoubleQuotedString { .. }
                | Mode::Backtick { .. }
                | Mode::Heredoc { .. }
                | Mode::Nowdoc { .. }
                | Mode::VariableOffset => self.lex_unexpected_character(),
            }
        }
        self.flush_open_modes();
    }

    /// Reports every construction still open at end of input. Base
    /// scripting and brace-pushed scripting are normal (a PHP file needs
    /// no `?>` and unbalanced braces are the parser's business).
    fn flush_open_modes(&mut self) {
        for mode in self.modes.clone() {
            match mode {
                Mode::InlineHtml | Mode::VariableOffset => {}
                Mode::Scripting {
                    opened_by_interpolation_at,
                } => {
                    if let Some(opening) = opened_by_interpolation_at {
                        self.diagnose_at(
                            LexerDiagnosticKind::UnterminatedInterpolation,
                            opening,
                            1,
                        );
                    }
                }
                Mode::DoubleQuotedString { opening } | Mode::Backtick { opening } => {
                    self.diagnose_at(LexerDiagnosticKind::UnterminatedString, opening, 1);
                }
                Mode::Heredoc { start, .. } | Mode::Nowdoc { start, .. } => {
                    self.diagnostics.push(LexerDiagnostic {
                        kind: LexerDiagnosticKind::UnterminatedHeredoc,
                        range: start,
                    });
                }
            }
        }
    }

    // Shared machinery for the mode modules.

    /// Absolute offset where the token being built starts.
    fn token_start(&self) -> TextSize {
        self.offset
    }

    /// Finishes the pending text as one token of the given kind. Never
    /// called with nothing consumed; a defensive guard skips empty
    /// tokens so the stream can never contain one.
    fn emit(&mut self, kind: SyntaxKind) {
        let length = self.cursor.take_length();
        if length == TextSize::from(0) {
            return;
        }
        self.tokens.push(Token::new(kind, length));
        self.offset += length;
    }

    fn diagnose(&mut self, kind: LexerDiagnosticKind, range: TextRange) {
        self.diagnostics.push(LexerDiagnostic { kind, range });
    }

    fn diagnose_at(&mut self, kind: LexerDiagnosticKind, start: TextSize, length: u32) {
        self.diagnose(kind, TextRange::at(start, TextSize::from(length)));
    }

    /// Fallback for any character no rule accepts: a one-character
    /// `Error` token, an `UnexpectedCharacter` diagnostic, and lexing
    /// continues at the next character. This is also the guaranteed
    /// progress argument: the fallback always consumes.
    fn lex_unexpected_character(&mut self) {
        let start = self.token_start();
        if let Some(character) = self.cursor.bump() {
            let length = u32::try_from(character.len_utf8()).unwrap_or(4);
            self.diagnose_at(LexerDiagnosticKind::UnexpectedCharacter, start, length);
        }
        self.emit(SyntaxKind::Error);
    }

    // Mode-stack discipline: tags replace the top (`set_mode`), braces
    // and strings push and pop. `pop_mode` keeps the stack non-empty.

    fn current_mode(&self) -> Mode {
        self.modes.last().copied().unwrap_or(Mode::InlineHtml)
    }

    fn set_mode(&mut self, mode: Mode) {
        if let Some(top) = self.modes.last_mut() {
            *top = mode;
        }
    }

    fn push_mode(&mut self, mode: Mode) {
        self.modes.push(mode);
    }

    fn pop_mode(&mut self) {
        if self.modes.len() > 1 {
            self.modes.pop();
        }
    }

    /// Whether the mode stack has room to pop (used by `}` handling).
    fn can_pop_mode(&self) -> bool {
        self.modes.len() > 1
    }

    /// True at offset zero or right after a line feed; heredoc closing
    /// labels are only recognized at a line start.
    fn at_line_start(&self) -> bool {
        let consumed = self
            .source
            .get(..usize::from(self.offset) + self.cursor.pending_text().len())
            .unwrap_or_default();
        consumed.is_empty() || consumed.ends_with('\n')
    }
}
```

Create `crates/celerrate_syntax/src/lexer/inline_html.rs`:

```rust
use crate::lexer::{BASE_SCRIPTING, Lexer};
use crate::syntax_kind::SyntaxKind;

impl Lexer<'_> {
    pub(super) fn lex_inline_html(&mut self) {
        // A first-line shebang is trivia, before anything else.
        if u32::from(self.token_start()) == 0 && self.cursor.rest().starts_with("#!") {
            self.cursor
                .eat_while(|character| character != '\n' && character != '\r');
            self.emit(SyntaxKind::Shebang);
            return;
        }
        match self.cursor.rest().find("<?") {
            Some(0) => self.lex_open_tag(),
            Some(tag_position) => {
                self.cursor.bump_bytes(tag_position);
                self.emit(SyntaxKind::InlineHtml);
            }
            None => {
                self.cursor.bump_bytes(self.cursor.rest().len());
                self.emit(SyntaxKind::InlineHtml);
            }
        }
    }

    fn lex_open_tag(&mut self) {
        let rest = self.cursor.rest();
        if starts_with_full_open_tag(rest) {
            self.cursor.bump_bytes(5);
            self.emit(SyntaxKind::OpenTag);
        } else if rest.starts_with("<?=") {
            self.cursor.bump_bytes(3);
            self.emit(SyntaxKind::OpenTagEcho);
        } else {
            // The short tag is lexed unconditionally: its availability
            // depends on an ini setting, judged semantically upstairs.
            self.cursor.bump_bytes(2);
            self.emit(SyntaxKind::ShortOpenTag);
        }
        self.set_mode(BASE_SCRIPTING);
    }
}

/// `<?php` case-insensitively, followed by whitespace or end of input;
/// otherwise `<?phpx` must lex as a short tag plus scripting content.
fn starts_with_full_open_tag(rest: &str) -> bool {
    let Some(tag) = rest.get(..5) else {
        return false;
    };
    if !tag.eq_ignore_ascii_case("<?php") {
        return false;
    }
    matches!(
        rest.as_bytes().get(5),
        None | Some(b' ' | b'\t' | b'\n' | b'\r')
    )
}
```

Create `crates/celerrate_syntax/src/lexer/scripting.rs` (minimal for this task: whitespace, the close tag, and the error fallback; later tasks grow the match):

```rust
use crate::lexer::{Lexer, Mode};
use crate::syntax_kind::SyntaxKind;

impl Lexer<'_> {
    pub(super) fn lex_scripting(&mut self) {
        let Some(character) = self.cursor.peek() else {
            return;
        };
        match character {
            character if character.is_ascii_whitespace() => {
                self.cursor
                    .eat_while(|character| character.is_ascii_whitespace());
                self.emit(SyntaxKind::Whitespace);
            }
            '?' if self.cursor.rest().starts_with("?>") => self.lex_close_tag(),
            _ => self.lex_unexpected_character(),
        }
    }

    fn lex_close_tag(&mut self) {
        self.cursor.bump_bytes(2);
        // PHP swallows one newline right after `?>`; it belongs to the
        // close tag token so the stream stays lossless.
        if self.cursor.rest().starts_with("\r\n") {
            self.cursor.bump_bytes(2);
        } else {
            self.cursor.eat('\n');
        }
        self.emit(SyntaxKind::CloseTag);
        self.set_mode(Mode::InlineHtml);
    }
}
```

Modify `crates/celerrate_syntax/src/lib.rs`:

```rust
//! PHP lexical analysis for the Celerrate toolchain. This part ships the
//! lexer: [`lex`] turns decoded source text into a lossless token stream
//! (trivia included, nothing discarded) plus structured diagnostics. The
//! parser and syntax tree arrive in the next Foundations part.

mod cursor;
mod diagnostic;
mod lexer;
mod syntax_kind;
mod token;

pub use diagnostic::{LexerDiagnostic, LexerDiagnosticKind};
pub use lexer::lex;
pub use syntax_kind::SyntaxKind;
pub use token::Token;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test inline_html`
Expected: 10 passed. (`open_tag_requires_a_boundary` passes because everything after `<?` becomes `Error` tokens for now; only its first token is asserted.)

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex inline HTML, open and close tags"
```

---

### Task 4: Identifiers, keywords, variables

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs`
- Create: `crates/celerrate_syntax/tests/names.rs`

**Interfaces:**
- Consumes: `Lexer` machinery (Task 3), `SyntaxKind::from_keyword` (Task 1).
- Produces: `pub(crate) fn is_name_start(character: char) -> bool` and `pub(crate) fn is_name_continue(character: char) -> bool` in `scripting.rs` (the string modes reuse them in Tasks 10 and 11); scripting arms for names and variables.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/names.rs`:

```rust
mod support;

use celerrate_syntax::SyntaxKind::*;
use support::{kinds, texts};

#[test]
fn identifiers_and_keywords() {
    assert_eq!(
        kinds("<?php echo $name;"),
        [OpenTag, Whitespace, Echo, Whitespace, Variable, Semicolon]
    );
}

#[test]
fn keywords_are_case_insensitive_but_keep_their_spelling() {
    assert_eq!(
        texts("<?php ECHO Fn"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (Echo, "ECHO".to_owned()),
            (Whitespace, " ".to_owned()),
            (Fn, "Fn".to_owned()),
        ]
    );
}

#[test]
fn non_keyword_names_are_identifiers() {
    assert_eq!(
        kinds("<?php strlen true parent"),
        [
            OpenTag, Whitespace, Identifier, Whitespace, Identifier, Whitespace, Identifier
        ]
    );
}

#[test]
fn names_accept_underscores_digits_and_non_ascii() {
    assert_eq!(
        kinds("<?php _private2 éléphant"),
        [OpenTag, Whitespace, Identifier, Whitespace, Identifier]
    );
}

#[test]
fn variables_carry_their_dollar_sign() {
    assert_eq!(
        texts("<?php $café"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (Variable, "$café".to_owned()),
        ]
    );
}

#[test]
fn variable_variables_split_into_dollar_then_variable() {
    assert_eq!(kinds("<?php $$name"), [OpenTag, Whitespace, Dollar, Variable]);
}

#[test]
fn a_lone_dollar_is_its_own_token() {
    assert_eq!(kinds("<?php $ "), [OpenTag, Whitespace, Dollar, Whitespace]);
}

#[test]
fn keywords_are_not_matched_inside_longer_names() {
    assert_eq!(kinds("<?php echoing"), [OpenTag, Whitespace, Identifier]);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test names`
Expected: failures; names currently lex as `Error` tokens.

- [ ] **Step 3: Write the implementation**

In `crates/celerrate_syntax/src/lexer/scripting.rs`, add the helpers (file scope) and the match arms.

Helpers:

```rust
/// PHP name start: `[a-zA-Z_\x80-\xff]`. Any non-ASCII char qualifies,
/// matching Zend's byte-oriented rule on UTF-8 input.
pub(crate) fn is_name_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic() || !character.is_ascii()
}

pub(crate) fn is_name_continue(character: char) -> bool {
    is_name_start(character) || character.is_ascii_digit()
}
```

New arms in `lex_scripting`, before the fallback:

```rust
            '$' => self.lex_dollar(),
            character if is_name_start(character) => self.lex_name(),
```

New methods in the `impl Lexer<'_>` block:

```rust
    fn lex_dollar(&mut self) {
        self.cursor.eat('$');
        if self.cursor.peek().is_some_and(is_name_start) {
            self.cursor.eat_while(is_name_continue);
            self.emit(SyntaxKind::Variable);
        } else {
            // `$$name` and a lone `$`: the dollar is its own token.
            self.emit(SyntaxKind::Dollar);
        }
    }

    fn lex_name(&mut self) {
        self.cursor.eat_while(is_name_continue);
        let kind = SyntaxKind::from_keyword(self.cursor.pending_text())
            .unwrap_or(SyntaxKind::Identifier);
        self.emit(kind);
    }
```

The first test also needs `;`. Add one temporary arm before the fallback (Task 6 subsumes it into the operator table):

```rust
            ';' => {
                self.cursor.eat(';');
                self.emit(SyntaxKind::Semicolon);
            }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test names`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex names, keywords, and variables"
```

---

### Task 5: Numeric literals

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs`
- Create: `crates/celerrate_syntax/tests/numbers.rs`

**Interfaces:**
- Consumes: `Lexer` machinery (Task 3).
- Produces: `fn lex_number(&mut self)` on `Lexer`, plus scripting arms for digits and for `.` followed by a digit. Emits `IntegerLiteral` and `FloatLiteral`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/numbers.rs`:

```rust
mod support;

use celerrate_syntax::SyntaxKind::*;
use support::{kinds, texts};

fn number_kinds(expression: &str) -> Vec<celerrate_syntax::SyntaxKind> {
    let source = format!("<?php {expression}");
    let mut listing = kinds(&source);
    // Drop the open tag and the following whitespace.
    listing.drain(..2);
    listing
}

#[test]
fn decimal_integers() {
    assert_eq!(number_kinds("0"), [IntegerLiteral]);
    assert_eq!(number_kinds("1234567890"), [IntegerLiteral]);
    assert_eq!(number_kinds("1_000_000"), [IntegerLiteral]);
}

#[test]
fn radix_prefixed_integers() {
    assert_eq!(number_kinds("0xDEAD_beef"), [IntegerLiteral]);
    assert_eq!(number_kinds("0b1010_1010"), [IntegerLiteral]);
    assert_eq!(number_kinds("0o777"), [IntegerLiteral]);
    assert_eq!(number_kinds("0O17"), [IntegerLiteral]);
    assert_eq!(number_kinds("0777"), [IntegerLiteral]);
}

#[test]
fn floats_in_all_shapes() {
    assert_eq!(number_kinds("1.5"), [FloatLiteral]);
    assert_eq!(number_kinds(".5"), [FloatLiteral]);
    assert_eq!(number_kinds("1."), [FloatLiteral]);
    assert_eq!(number_kinds("1e3"), [FloatLiteral]);
    assert_eq!(number_kinds("1E+3"), [FloatLiteral]);
    assert_eq!(number_kinds("1.5e-3"), [FloatLiteral]);
    assert_eq!(number_kinds("1_0.5_0"), [FloatLiteral]);
}

#[test]
fn an_exponent_without_digits_is_not_consumed() {
    // Zend lexes "1e" as an integer then a name; so do we.
    assert_eq!(number_kinds("1e"), [IntegerLiteral, Identifier]);
    assert_eq!(number_kinds("1e+"), [IntegerLiteral, Identifier, Plus]);
}

#[test]
fn number_texts_are_exact() {
    assert_eq!(
        texts("<?php 1.5e3"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (FloatLiteral, "1.5e3".to_owned()),
        ]
    );
}

#[test]
fn hex_prefix_without_digits_is_a_plain_zero() {
    // "0x" alone: integer zero, then the name "x", as in Zend.
    assert_eq!(number_kinds("0x"), [IntegerLiteral, Identifier]);
}
```

Note: `an_exponent_without_digits_is_not_consumed` uses `Plus`, which Task 6 introduces. To keep this task green on its own, add the temporary arm below alongside the number arms and let Task 6 subsume it:

```rust
            '+' => {
                self.cursor.eat('+');
                self.emit(SyntaxKind::Plus);
            }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test numbers`
Expected: failures; digits currently lex as `Error` tokens.

- [ ] **Step 3: Write the implementation**

New arms in `lex_scripting`, before the name arm:

```rust
            character if character.is_ascii_digit() => self.lex_number(),
            '.' if self.cursor.peek_second().is_some_and(|c| c.is_ascii_digit()) => {
                self.lex_number()
            }
```

New methods and helpers in `crates/celerrate_syntax/src/lexer/scripting.rs`:

```rust
    fn lex_number(&mut self) {
        let rest = self.cursor.rest();
        if starts_with_radix_prefix(rest, "0x") || starts_with_radix_prefix(rest, "0X") {
            self.cursor.bump_bytes(2);
            self.cursor
                .eat_while(|c| c.is_ascii_hexdigit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        if starts_with_radix_prefix(rest, "0b") || starts_with_radix_prefix(rest, "0B") {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        if starts_with_radix_prefix(rest, "0o") || starts_with_radix_prefix(rest, "0O") {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        // Decimal digits. Separator placement and octal digit validity
        // are judged upstairs; the lexer takes the maximal run.
        let mut is_float = false;
        self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
        if self.cursor.peek() == Some('.')
            && (self.cursor.peek_second().is_some_and(|c| c.is_ascii_digit())
                || !self.cursor.pending_text().is_empty())
        {
            // "1.5", "1.", and ".5" are all floats, as in Zend's DNUM.
            is_float = true;
            self.cursor.eat('.');
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
        }
        if self.eat_exponent() {
            is_float = true;
        }
        let kind = if is_float {
            SyntaxKind::FloatLiteral
        } else {
            SyntaxKind::IntegerLiteral
        };
        self.emit(kind);
    }

    /// Consumes `[eE][+-]?digits` only when the digits are there;
    /// otherwise consumes nothing ("1e" is an integer then a name).
    fn eat_exponent(&mut self) -> bool {
        if !matches!(self.cursor.peek(), Some('e' | 'E')) {
            return false;
        }
        let after_marker = self.cursor.rest().get(1..).unwrap_or_default();
        let after_sign = after_marker
            .strip_prefix(['+', '-'])
            .unwrap_or(after_marker);
        if !after_sign.starts_with(|c: char| c.is_ascii_digit()) {
            return false;
        }
        self.cursor.bump();
        if matches!(self.cursor.peek(), Some('+' | '-')) {
            self.cursor.bump();
        }
        self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
        true
    }
```

File-scope helper:

```rust
/// A radix prefix counts only when a digit-ish character follows: "0x"
/// alone lexes as the integer zero then the name "x", as in Zend.
fn starts_with_radix_prefix(rest: &str, prefix: &str) -> bool {
    rest.strip_prefix(prefix)
        .is_some_and(|after| after.starts_with(|c: char| c.is_ascii_alphanumeric()))
}
```

Note on `1..2`: the first `.` follows digits, so `1.` lexes as a float and `.2` as another float, matching Zend's longest-match behavior.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test numbers`
Expected: 6 passed. Also run `cargo test --package celerrate_syntax` to confirm no regression.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex numeric literals"
```

---

### Task 6: Operators, punctuation, braces, casts

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs`
- Create: `crates/celerrate_syntax/tests/operators.rs`

**Interfaces:**
- Consumes: `Lexer` machinery, `push_mode`/`pop_mode`/`can_pop_mode` (Task 3).
- Produces: the full operator table, brace push/pop discipline, and single-token casts. Replaces the temporary `;` and `+` arms from Tasks 4 and 5.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/operators.rs`:

```rust
mod support;

use celerrate_syntax::SyntaxKind::{self, *};
use support::{kinds, texts};

fn operator_kinds(expression: &str) -> Vec<SyntaxKind> {
    let source = format!("<?php {expression}");
    let mut listing = kinds(&source);
    listing.drain(..2);
    listing.retain(|kind| *kind != Whitespace);
    listing
}

#[test]
fn compound_assignment_operators_lex_longest_first() {
    assert_eq!(operator_kinds("**="), [StarStarEquals]);
    assert_eq!(operator_kinds("** ="), [StarStar, Equals]);
    assert_eq!(operator_kinds("??="), [QuestionQuestionEquals]);
    assert_eq!(operator_kinds("<<="), [LessLessEquals]);
    assert_eq!(operator_kinds(">>="), [GreaterGreaterEquals]);
    assert_eq!(operator_kinds(".="), [DotEquals]);
}

#[test]
fn comparison_operators() {
    assert_eq!(operator_kinds("==="), [EqualsEqualsEquals]);
    assert_eq!(operator_kinds("!=="), [BangEqualsEquals]);
    assert_eq!(operator_kinds("<=>"), [Spaceship]);
    assert_eq!(operator_kinds("<>"), [BangEquals]);
    assert_eq!(operator_kinds("<="), [LessEquals]);
    assert_eq!(operator_kinds("< = >"), [Less, Equals, Greater]);
}

#[test]
fn arrows_and_scope_operators() {
    assert_eq!(operator_kinds("->"), [Arrow]);
    assert_eq!(operator_kinds("?->"), [NullsafeArrow]);
    assert_eq!(operator_kinds("=>"), [FatArrow]);
    assert_eq!(operator_kinds("::"), [ColonColon]);
    assert_eq!(operator_kinds("..."), [Ellipsis]);
    assert_eq!(operator_kinds(".."), [Dot, Dot]);
}

#[test]
fn punctuation_and_delimiters() {
    assert_eq!(
        operator_kinds("( ) [ ] { } , ; @ ~ \\"),
        [
            OpenParenthesis, CloseParenthesis, OpenBracket, CloseBracket,
            OpenBrace, CloseBrace, Comma, Semicolon, At, Tilde, Backslash
        ]
    );
}

#[test]
fn logic_and_bit_operators() {
    assert_eq!(operator_kinds("&& & || | ^ !"), [
        AmpersandAmpersand, Ampersand, PipePipe, Pipe, Caret, Bang
    ]);
    assert_eq!(operator_kinds("?? ? :"), [QuestionQuestion, Question, Colon]);
    assert_eq!(operator_kinds("++ -- + -"), [PlusPlus, MinusMinus, Plus, Minus]);
    assert_eq!(operator_kinds("<< >>"), [LessLess, GreaterGreater]);
}

#[test]
fn casts_are_single_tokens_with_inner_whitespace() {
    assert_eq!(
        texts("<?php (int)( String )"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntCast, "(int)".to_owned()),
            (StringCast, "( String )".to_owned()),
        ]
    );
}

#[test]
fn all_php_81_cast_forms_resolve() {
    assert_eq!(operator_kinds("(integer)"), [IntCast]);
    assert_eq!(operator_kinds("(bool)"), [BoolCast]);
    assert_eq!(operator_kinds("(boolean)"), [BoolCast]);
    assert_eq!(operator_kinds("(float)"), [FloatCast]);
    assert_eq!(operator_kinds("(double)"), [FloatCast]);
    assert_eq!(operator_kinds("(binary)"), [BinaryCast]);
    assert_eq!(operator_kinds("(array)"), [ArrayCast]);
    assert_eq!(operator_kinds("(object)"), [ObjectCast]);
}

#[test]
fn removed_and_unknown_casts_are_plain_parentheses() {
    assert_eq!(
        operator_kinds("(real)"),
        [OpenParenthesis, Identifier, CloseParenthesis]
    );
    assert_eq!(
        operator_kinds("(unset)"),
        [OpenParenthesis, Unset, CloseParenthesis]
    );
    assert_eq!(
        operator_kinds("(int $x)"),
        [OpenParenthesis, Identifier, Variable, CloseParenthesis]
    );
}

#[test]
fn close_tag_wins_over_question_mark() {
    assert_eq!(kinds("<?php ?>"), [OpenTag, Whitespace, CloseTag]);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test operators`
Expected: failures; operators currently lex as `Error` tokens.

- [ ] **Step 3: Write the implementation**

In `crates/celerrate_syntax/src/lexer/scripting.rs`, remove the temporary `;` and `+` arms, then add these arms to `lex_scripting` before the fallback (the `?>` arm from Task 3 stays above them, so the close tag wins over `?`):

```rust
            '(' => self.lex_parenthesis_or_cast(),
            '{' => self.lex_open_brace(),
            '}' => self.lex_close_brace(),
            _ if self.try_lex_operator() => {}
```

New methods in the `impl Lexer<'_>` block:

```rust
    /// Every `{` pushes a scripting mode and every `}` pops one, exactly
    /// like Zend's state stack. Balanced braces are a no-op; the payoff
    /// is `{$expr}` interpolation, whose closing brace pops back into
    /// the string mode with no extra bookkeeping.
    fn lex_open_brace(&mut self) {
        self.cursor.eat('{');
        self.emit(SyntaxKind::OpenBrace);
        self.push_mode(BASE_SCRIPTING);
    }

    fn lex_close_brace(&mut self) {
        self.cursor.eat('}');
        self.emit(SyntaxKind::CloseBrace);
        if self.can_pop_mode() {
            self.pop_mode();
        }
    }

    fn lex_parenthesis_or_cast(&mut self) {
        if let Some((kind, byte_length)) = cast_at(self.cursor.rest()) {
            self.cursor.bump_bytes(byte_length);
            self.emit(kind);
        } else {
            self.cursor.eat('(');
            self.emit(SyntaxKind::OpenParenthesis);
        }
    }

    /// Longest-match scan of the operator table. Returns false when no
    /// operator starts here, letting the fallback take over.
    fn try_lex_operator(&mut self) -> bool {
        let rest = self.cursor.rest();
        for (text, kind) in OPERATORS {
            if rest.starts_with(text) {
                self.cursor.bump_bytes(text.len());
                self.emit(*kind);
                return true;
            }
        }
        false
    }
```

File-scope table and cast detection:

```rust
/// Operators and punctuation, longest first so prefixes never shadow a
/// longer operator. A linear scan is fine for now; a first-character
/// match tree can come with the benchmark part if it ever shows up.
const OPERATORS: &[(&str, SyntaxKind)] = &[
    ("<=>", SyntaxKind::Spaceship),
    ("===", SyntaxKind::EqualsEqualsEquals),
    ("!==", SyntaxKind::BangEqualsEquals),
    ("**=", SyntaxKind::StarStarEquals),
    ("<<=", SyntaxKind::LessLessEquals),
    (">>=", SyntaxKind::GreaterGreaterEquals),
    ("??=", SyntaxKind::QuestionQuestionEquals),
    ("...", SyntaxKind::Ellipsis),
    ("?->", SyntaxKind::NullsafeArrow),
    ("**", SyntaxKind::StarStar),
    ("==", SyntaxKind::EqualsEquals),
    ("!=", SyntaxKind::BangEquals),
    ("<>", SyntaxKind::BangEquals),
    ("<=", SyntaxKind::LessEquals),
    (">=", SyntaxKind::GreaterEquals),
    ("&&", SyntaxKind::AmpersandAmpersand),
    ("||", SyntaxKind::PipePipe),
    ("??", SyntaxKind::QuestionQuestion),
    ("++", SyntaxKind::PlusPlus),
    ("--", SyntaxKind::MinusMinus),
    ("<<", SyntaxKind::LessLess),
    (">>", SyntaxKind::GreaterGreater),
    ("+=", SyntaxKind::PlusEquals),
    ("-=", SyntaxKind::MinusEquals),
    ("*=", SyntaxKind::StarEquals),
    ("/=", SyntaxKind::SlashEquals),
    (".=", SyntaxKind::DotEquals),
    ("%=", SyntaxKind::PercentEquals),
    ("&=", SyntaxKind::AmpersandEquals),
    ("|=", SyntaxKind::PipeEquals),
    ("^=", SyntaxKind::CaretEquals),
    ("->", SyntaxKind::Arrow),
    ("=>", SyntaxKind::FatArrow),
    ("::", SyntaxKind::ColonColon),
    ("+", SyntaxKind::Plus),
    ("-", SyntaxKind::Minus),
    ("*", SyntaxKind::Star),
    ("/", SyntaxKind::Slash),
    ("%", SyntaxKind::Percent),
    ("=", SyntaxKind::Equals),
    ("<", SyntaxKind::Less),
    (">", SyntaxKind::Greater),
    ("!", SyntaxKind::Bang),
    ("&", SyntaxKind::Ampersand),
    ("|", SyntaxKind::Pipe),
    ("^", SyntaxKind::Caret),
    ("?", SyntaxKind::Question),
    (":", SyntaxKind::Colon),
    (";", SyntaxKind::Semicolon),
    (",", SyntaxKind::Comma),
    (".", SyntaxKind::Dot),
    ("@", SyntaxKind::At),
    ("~", SyntaxKind::Tilde),
    ("\\", SyntaxKind::Backslash),
    (")", SyntaxKind::CloseParenthesis),
    ("[", SyntaxKind::OpenBracket),
    ("]", SyntaxKind::CloseBracket),
];

/// Detects a cast at the start of `rest`: `(`, optional spaces and tabs,
/// one of the exact PHP 8.1 cast words (case-insensitive), optional
/// spaces and tabs, `)`. Returns the kind and the total byte length.
/// `(real)` and `(unset)` were removed in PHP 8.0 and do not match.
fn cast_at(rest: &str) -> Option<(SyntaxKind, usize)> {
    let inner = rest.strip_prefix('(')?;
    let after_leading = inner.trim_start_matches([' ', '\t']);
    let word_length = after_leading
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .count();
    let word = after_leading.get(..word_length)?;
    let after_word = after_leading.get(word_length..)?;
    let after_trailing = after_word.trim_start_matches([' ', '\t']);
    after_trailing.strip_prefix(')')?;
    let kind = cast_kind(word)?;
    let total_length = rest.len() - after_trailing.len() + ')'.len_utf8();
    Some((kind, total_length))
}

fn cast_kind(word: &str) -> Option<SyntaxKind> {
    const CASTS: &[(&str, SyntaxKind)] = &[
        ("int", SyntaxKind::IntCast),
        ("integer", SyntaxKind::IntCast),
        ("bool", SyntaxKind::BoolCast),
        ("boolean", SyntaxKind::BoolCast),
        ("float", SyntaxKind::FloatCast),
        ("double", SyntaxKind::FloatCast),
        ("string", SyntaxKind::StringCast),
        ("binary", SyntaxKind::BinaryCast),
        ("array", SyntaxKind::ArrayCast),
        ("object", SyntaxKind::ObjectCast),
    ];
    CASTS
        .iter()
        .find(|(name, _)| word.eq_ignore_ascii_case(name))
        .map(|(_, kind)| *kind)
}
```

Also add `BASE_SCRIPTING` to the imports at the top of the file:

```rust
use crate::lexer::{BASE_SCRIPTING, Lexer, Mode};
```

Note: `/` currently reaches the operator table and lexes as `Slash`; Task 7 adds the comment arm above it. `#` still falls through to the error fallback until Task 7.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test operators`
Expected: 9 passed. Then `cargo test --package celerrate_syntax` for no regression.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex operators, punctuation, and casts"
```

---

### Task 7: Comments, docblocks, attribute opener

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs`
- Create: `crates/celerrate_syntax/tests/comments.rs`

**Interfaces:**
- Consumes: `Lexer` machinery, `diagnose_at` (Task 3).
- Produces: scripting arms for `//`, `#`, `#[`, `/* */`, `/** */`. Emits `LineComment`, `BlockComment`, `DocComment`, `AttributeOpen`, and the `UnterminatedBlockComment` diagnostic.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/comments.rs`:

```rust
mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{kinds, lex_verified, texts};

#[test]
fn line_comments_stop_before_the_newline() {
    assert_eq!(
        texts("<?php // hello\n# world"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (LineComment, "// hello".to_owned()),
            (Whitespace, "\n".to_owned()),
            (LineComment, "# world".to_owned()),
        ]
    );
}

#[test]
fn line_comments_stop_before_a_close_tag() {
    assert_eq!(
        texts("<?php // note ?>x"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (LineComment, "// note ".to_owned()),
            (CloseTag, "?>".to_owned()),
            (InlineHtml, "x".to_owned()),
        ]
    );
}

#[test]
fn block_comments_span_lines() {
    assert_eq!(
        kinds("<?php /* a\nb */ ;"),
        [OpenTag, Whitespace, BlockComment, Whitespace, Semicolon]
    );
}

#[test]
fn docblocks_are_distinct_from_block_comments() {
    assert_eq!(
        kinds("<?php /** @param int $x */"),
        [OpenTag, Whitespace, DocComment]
    );
    // "/**/" is an empty block comment, and "/***/" has no whitespace
    // after the doc opener: both stay plain block comments, as in Zend.
    assert_eq!(kinds("<?php /**/"), [OpenTag, Whitespace, BlockComment]);
    assert_eq!(kinds("<?php /***/"), [OpenTag, Whitespace, BlockComment]);
}

#[test]
fn unterminated_block_comment_runs_to_the_end() {
    let (tokens, diagnostics) = lex_verified("<?php /* open");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, BlockComment]
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics.first().map(|d| d.kind),
        Some(LexerDiagnosticKind::UnterminatedBlockComment)
    );
    // The diagnostic points at the opening "/*".
    assert_eq!(
        diagnostics.first().map(|d| (u32::from(d.range.start()), u32::from(d.range.end()))),
        Some((6, 8))
    );
}

#[test]
fn attribute_opener_is_not_a_comment() {
    assert_eq!(
        kinds("<?php #[Attribute] # comment"),
        [
            OpenTag, Whitespace, AttributeOpen, Identifier, CloseBracket,
            Whitespace, LineComment
        ]
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test comments`
Expected: failures (`//` lexes as two `Slash`, `#` as `Error`).

- [ ] **Step 3: Write the implementation**

New arms in `lex_scripting`, above the `_ if self.try_lex_operator()` arm:

```rust
            '/' if self.cursor.rest().starts_with("//") => self.lex_line_comment(),
            '/' if self.cursor.rest().starts_with("/*") => self.lex_block_comment(),
            '#' if self.cursor.rest().starts_with("#[") => {
                self.cursor.bump_bytes(2);
                self.emit(SyntaxKind::AttributeOpen);
            }
            '#' => self.lex_line_comment(),
```

New methods in the `impl Lexer<'_>` block:

```rust
    /// `//` and `#` comments end before the newline, and also before a
    /// `?>` (the close tag still closes inside a line comment, as in
    /// Zend).
    fn lex_line_comment(&mut self) {
        while let Some(character) = self.cursor.peek() {
            if character == '\n' || character == '\r' {
                break;
            }
            if self.cursor.rest().starts_with("?>") {
                break;
            }
            self.cursor.bump();
        }
        self.emit(SyntaxKind::LineComment);
    }

    /// `/* ... */`, and `/** ... */` as a docblock when whitespace
    /// follows the doc opener (Zend's rule, which keeps "/**/" a plain
    /// comment). Unterminated comments run to the end of input with a
    /// diagnostic pointing at the opener.
    fn lex_block_comment(&mut self) {
        let start = self.token_start();
        let rest = self.cursor.rest();
        let is_docblock = rest
            .strip_prefix("/**")
            .is_some_and(|after| after.starts_with(|c: char| c.is_ascii_whitespace()));
        self.cursor.bump_bytes(2);
        match self.cursor.rest().find("*/") {
            Some(terminator_position) => {
                self.cursor.bump_bytes(terminator_position + 2);
            }
            None => {
                self.cursor.bump_bytes(self.cursor.rest().len());
                self.diagnose_at(LexerDiagnosticKind::UnterminatedBlockComment, start, 2);
            }
        }
        let kind = if is_docblock {
            SyntaxKind::DocComment
        } else {
            SyntaxKind::BlockComment
        };
        self.emit(kind);
    }
```

Add `LexerDiagnosticKind` to the imports at the top of the file:

```rust
use crate::diagnostic::LexerDiagnosticKind;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test comments`
Expected: 6 passed. Then `cargo test --package celerrate_syntax` for no regression.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex comments, docblocks, and the attribute opener"
```

---

### Task 8: Error tokens and guaranteed progress

**Files:**
- Create: `crates/celerrate_syntax/tests/errors.rs`

**Interfaces:**
- Consumes: the `lex_unexpected_character` fallback (Task 3); no new production code is expected. This task pins the error contract with tests; if any assertion fails, fix the fallback, not the tests.

- [ ] **Step 1: Write the tests**

Create `crates/celerrate_syntax/tests/errors.rs`:

```rust
mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{kinds, lex_verified};

#[test]
fn a_stray_control_byte_is_a_one_character_error_token() {
    let (tokens, diagnostics) = lex_verified("<?php \u{1};");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, Error, Semicolon]
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnexpectedCharacter);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
    assert_eq!(u32::from(diagnostic.range.end()), 7);
}

#[test]
fn lexing_continues_after_an_error() {
    assert_eq!(
        kinds("<?php \u{1}\u{2} echo"),
        [OpenTag, Whitespace, Error, Error, Whitespace, Echo]
    );
}

#[test]
fn ascii_delete_is_an_unexpected_character() {
    // Non-ASCII characters are all name starts under PHP's
    // byte-oriented rule, so unexpected characters are always ASCII:
    // assert the DEL control byte.
    assert_eq!(kinds("<?php \u{7F}"), [OpenTag, Whitespace, Error]);
}

#[test]
fn degenerate_input_terminates_and_stays_lossless() {
    // A pathological soup of control bytes, quotes-free: every char
    // must come back out, one Error token each, no hang.
    let soup: String = ('\u{0}'..='\u{8}').cycle().take(300).collect();
    let source = format!("<?php {soup}");
    let (tokens, diagnostics) = lex_verified(&source);
    assert_eq!(tokens.len(), 302);
    assert_eq!(diagnostics.len(), 300);
}
```

Add the allow header at the top of the file (it uses `expect`):

```rust
#![allow(clippy::expect_used)]
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --package celerrate_syntax --test errors`
Expected: 4 passed with no production change. If a test fails, the fallback or a mode dispatch is wrong; fix it minimally and re-run the whole package.

- [ ] **Step 3: Commit**

```bash
git add crates/celerrate_syntax/tests/errors.rs
git commit -m "✅ test(syntax): pin the error token and progress contract"
```

---

### Task 9: Single-quoted strings

**Files:**
- Create: `crates/celerrate_syntax/src/lexer/strings.rs`
- Modify: `crates/celerrate_syntax/src/lexer.rs` (declare `mod strings;`)
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs` (dispatch arms)
- Create: `crates/celerrate_syntax/tests/strings.rs`

**Interfaces:**
- Consumes: `Lexer` machinery, `diagnose_at` (Task 3).
- Produces: `pub(super) fn lex_single_quoted_string(&mut self)` in `strings.rs`; whole-token `SingleQuotedString` (with optional `b`/`B` prefix), `UnterminatedString` diagnostic.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/strings.rs`:

```rust
mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{lex_verified, texts};

#[test]
fn single_quoted_strings_are_one_token() {
    assert_eq!(
        texts("<?php 'hello';"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "'hello'".to_owned()),
            (Semicolon, ";".to_owned()),
        ]
    );
}

#[test]
fn single_quoted_escapes_do_not_terminate() {
    assert_eq!(
        texts(r"<?php 'a\'b\\'"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, r"'a\'b\\'".to_owned()),
        ]
    );
}

#[test]
fn single_quoted_strings_do_not_interpolate() {
    assert_eq!(
        texts("<?php '$name {$x}'"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "'$name {$x}'".to_owned()),
        ]
    );
}

#[test]
fn binary_prefix_belongs_to_the_string_token() {
    assert_eq!(
        texts("<?php b'x' B'y'"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "b'x'".to_owned()),
            (Whitespace, " ".to_owned()),
            (SingleQuotedString, "B'y'".to_owned()),
        ]
    );
}

#[test]
fn unterminated_single_quoted_string_keeps_its_kind() {
    let (tokens, diagnostics) = lex_verified("<?php 'open");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, SingleQuotedString]
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnterminatedString);
    // Points at the opening quote.
    assert_eq!(u32::from(diagnostic.range.start()), 6);
    assert_eq!(u32::from(diagnostic.range.end()), 7);
}
```

Add the allow header at the top of the file:

```rust
#![allow(clippy::expect_used)]
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test strings`
Expected: failures; `'` lexes as `Error`.

- [ ] **Step 3: Write the implementation**

Add `mod strings;` to the module list in `crates/celerrate_syntax/src/lexer.rs` (alphabetical, after `mod scripting;`).

New arms in `lex_scripting`, above the name arm (so `b'...'` wins over the identifier rule):

```rust
            '\'' => self.lex_single_quoted_string(),
            'b' | 'B' if self.cursor.peek_second() == Some('\'') => {
                self.cursor.bump();
                self.lex_single_quoted_string();
            }
```

Create `crates/celerrate_syntax/src/lexer/strings.rs`:

```rust
use crate::diagnostic::LexerDiagnosticKind;
use crate::lexer::Lexer;
use crate::syntax_kind::SyntaxKind;

impl Lexer<'_> {
    /// A whole `'...'` string as one token: no interpolation exists in
    /// single quotes, so there is nothing fine-grained to emit. Only
    /// `\\` and `\'` are escapes; any other backslash is literal. An
    /// unterminated string runs to the end of input, keeps its normal
    /// kind (mid-edit code is the nominal case in an editor), and
    /// reports `UnterminatedString` at the opening quote.
    pub(super) fn lex_single_quoted_string(&mut self) {
        let opening = self.token_start() + self.cursor.pending_length();
        self.cursor.eat('\'');
        loop {
            match self.cursor.bump() {
                Some('\'') => break,
                Some('\\') => {
                    self.cursor.bump();
                }
                Some(_) => {}
                None => {
                    self.diagnose_at(LexerDiagnosticKind::UnterminatedString, opening, 1);
                    break;
                }
            }
        }
        self.emit(SyntaxKind::SingleQuotedString);
    }
}
```

The `pending_length` call above needs one addition to `crates/celerrate_syntax/src/cursor.rs` (the `b` prefix may already be consumed, so the opening quote is not always at `token_start`):

```rust
    /// Byte length consumed so far in the current token, without
    /// finishing it. Lets diagnostics point inside a token being built.
    pub(crate) fn pending_length(&self) -> TextSize {
        u32::try_from(self.pending_byte_length())
            .map(TextSize::from)
            .unwrap_or_else(|_| TextSize::from(u32::MAX))
    }
```

And rewrite `take_length` to reuse it:

```rust
    pub(crate) fn take_length(&mut self) -> TextSize {
        let length = self.pending_length();
        self.rest_at_token_start = self.rest();
        length
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test strings`
Expected: 5 passed. Then `cargo test --package celerrate_syntax` for no regression.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex single-quoted strings"
```

---

### Task 10: Double-quoted strings, backticks, interpolation

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/strings.rs`
- Modify: `crates/celerrate_syntax/src/lexer.rs` (dispatch the string modes)
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs` (dispatch arms)
- Modify: `crates/celerrate_syntax/tests/strings.rs`

**Interfaces:**
- Consumes: `Mode` variants and mode-stack methods (Task 3), `is_name_start`/`is_name_continue` (Task 4).
- Produces on `Lexer`, all `pub(super)`: `lex_double_quoted(&mut self)`, `lex_backtick(&mut self)`, `lex_variable_offset(&mut self)`, plus the shared `lex_interpolation(&mut self) -> bool` and `lex_interpolated_fragment(&mut self, terminator: Option<char>)` used again by heredoc in Task 11.
- Token shapes: `DoubleQuote` delimiters (opening may be `b"`), `Backtick` delimiters, `StringFragment` runs, `Variable` with optional `Arrow`/`NullsafeArrow` + `Identifier` or `[...]` offset (`VariableOffset` mode), `OpenBrace` + nested scripting for `{$`, `DollarOpenBrace` + nested scripting for `${`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/strings.rs`:

```rust
#[test]
fn a_plain_double_quoted_string_is_delimiters_around_one_fragment() {
    assert_eq!(
        texts(r#"<?php "hello""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "hello".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn an_empty_double_quoted_string_is_two_delimiters() {
    assert_eq!(
        texts(r#"<?php """#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn simple_variable_interpolation() {
    assert_eq!(
        texts(r#"<?php "a $name b""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "a ".to_owned()),
            (Variable, "$name".to_owned()),
            (StringFragment, " b".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn escaped_dollars_and_quotes_stay_in_the_fragment() {
    assert_eq!(
        texts(r#"<?php "a \" \$x b""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, r#"a \" \$x b"#.to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn simple_property_interpolation() {
    assert_eq!(
        texts(r#"<?php "$user->name and $user?->name""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$user".to_owned()),
            (Arrow, "->".to_owned()),
            (Identifier, "name".to_owned()),
            (StringFragment, " and ".to_owned()),
            (Variable, "$user".to_owned()),
            (NullsafeArrow, "?->".to_owned()),
            (Identifier, "name".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn only_one_property_level_interpolates() {
    // "$a->b->c" interpolates $a->b; "->c" is literal, as in Zend.
    assert_eq!(
        texts(r#"<?php "$a->b->c""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$a".to_owned()),
            (Arrow, "->".to_owned()),
            (Identifier, "b".to_owned()),
            (StringFragment, "->c".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn simple_offset_interpolation() {
    assert_eq!(
        texts(r#"<?php "$items[0] $map[key] $grid[$x] $list[-1]""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$items".to_owned()),
            (OpenBracket, "[".to_owned()),
            (IntegerLiteral, "0".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$map".to_owned()),
            (OpenBracket, "[".to_owned()),
            (Identifier, "key".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$grid".to_owned()),
            (OpenBracket, "[".to_owned()),
            (Variable, "$x".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$list".to_owned()),
            (OpenBracket, "[".to_owned()),
            (Minus, "-".to_owned()),
            (IntegerLiteral, "1".to_owned()),
            (CloseBracket, "]".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn brace_interpolation_opens_nested_scripting() {
    assert_eq!(
        texts(r#"<?php "x {$a->b(1)} y""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "x ".to_owned()),
            (OpenBrace, "{".to_owned()),
            (Variable, "$a".to_owned()),
            (Arrow, "->".to_owned()),
            (Identifier, "b".to_owned()),
            (OpenParenthesis, "(".to_owned()),
            (IntegerLiteral, "1".to_owned()),
            (CloseParenthesis, ")".to_owned()),
            (CloseBrace, "}".to_owned()),
            (StringFragment, " y".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn nested_braces_inside_brace_interpolation_balance() {
    assert_eq!(
        texts(r#"<?php "{$f(['k' => 1])}""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (OpenBrace, "{".to_owned()),
            (Variable, "$f".to_owned()),
            (OpenParenthesis, "(".to_owned()),
            (OpenBracket, "[".to_owned()),
            (SingleQuotedString, "'k'".to_owned()),
            (Whitespace, " ".to_owned()),
            (FatArrow, "=>".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "1".to_owned()),
            (CloseBracket, "]".to_owned()),
            (CloseParenthesis, ")".to_owned()),
            (CloseBrace, "}".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn deprecated_dollar_brace_interpolation_still_lexes() {
    assert_eq!(
        texts(r#"<?php "${name}""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (DollarOpenBrace, "${".to_owned()),
            (Identifier, "name".to_owned()),
            (CloseBrace, "}".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn a_lone_dollar_or_brace_stays_in_the_fragment() {
    assert_eq!(
        texts(r#"<?php "a $ b { c""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (StringFragment, "a $ b { c".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn backtick_strings_interpolate_like_double_quotes() {
    assert_eq!(
        texts("<?php `ls $dir`"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (Backtick, "`".to_owned()),
            (StringFragment, "ls ".to_owned()),
            (Variable, "$dir".to_owned()),
            (Backtick, "`".to_owned()),
        ]
    );
}

#[test]
fn binary_prefix_on_double_quotes() {
    assert_eq!(
        texts(r#"<?php b"x""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "b\"".to_owned()),
            (StringFragment, "x".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn unterminated_double_quoted_string_diagnoses_the_opening() {
    let (tokens, diagnostics) = lex_verified(r#"<?php "open $x"#);
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, DoubleQuote, StringFragment, Variable]
    );
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnterminatedString);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
}

#[test]
fn unterminated_brace_interpolation_diagnoses_the_opening() {
    let (_tokens, diagnostics) = lex_verified(r#"<?php "a {$x"#);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == LexerDiagnosticKind::UnterminatedInterpolation
            && u32::from(diagnostic.range.start()) == 9
    }));
    // The string opening is reported too.
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == LexerDiagnosticKind::UnterminatedString
    }));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test strings`
Expected: the new tests fail; `"` and `` ` `` lex as `Error`.

- [ ] **Step 3: Write the implementation**

New arms in `lex_scripting` (`crates/celerrate_syntax/src/lexer/scripting.rs`), next to the single-quote arms:

```rust
            '"' => self.lex_double_quote_delimiter(),
            'b' | 'B' if self.cursor.peek_second() == Some('"') => {
                self.cursor.bump();
                self.lex_double_quote_delimiter();
            }
            '`' => self.lex_backtick_delimiter(),
```

(The existing `'b' | 'B'` arm for single quotes stays; both prefix arms sit above the name arm.)

New methods in `crates/celerrate_syntax/src/lexer/strings.rs` (scripting-side entry points):

```rust
    pub(super) fn lex_double_quote_delimiter(&mut self) {
        let opening = self.token_start();
        self.cursor.eat('"');
        self.emit(SyntaxKind::DoubleQuote);
        self.push_mode(Mode::DoubleQuotedString { opening });
    }

    pub(super) fn lex_backtick_delimiter(&mut self) {
        let opening = self.token_start();
        self.cursor.eat('`');
        self.emit(SyntaxKind::Backtick);
        self.push_mode(Mode::Backtick { opening });
    }
```

In `crates/celerrate_syntax/src/lexer.rs`, replace the placeholder dispatch arms in `run`:

```rust
                Mode::DoubleQuotedString { .. } => self.lex_double_quoted(),
                Mode::Backtick { .. } => self.lex_backtick(),
                // Heredoc bodies arrive in Task 11.
                Mode::Heredoc { .. } | Mode::Nowdoc { .. } => self.lex_unexpected_character(),
                Mode::VariableOffset => self.lex_variable_offset(),
```

The string-mode machinery, in `crates/celerrate_syntax/src/lexer/strings.rs` (keep the Task 9 imports and add `Mode` plus `use super::scripting::{is_name_continue, is_name_start};`, so the top of the file reads `use crate::diagnostic::LexerDiagnosticKind;`, `use crate::lexer::{Lexer, Mode};`, `use crate::syntax_kind::SyntaxKind;`, and the scripting helpers):

```rust
    pub(super) fn lex_double_quoted(&mut self) {
        if self.cursor.eat('"') {
            self.emit(SyntaxKind::DoubleQuote);
            self.pop_mode();
            return;
        }
        if self.lex_interpolation() {
            return;
        }
        self.lex_interpolated_fragment(Some('"'));
    }

    pub(super) fn lex_backtick(&mut self) {
        if self.cursor.eat('`') {
            self.emit(SyntaxKind::Backtick);
            self.pop_mode();
            return;
        }
        if self.lex_interpolation() {
            return;
        }
        self.lex_interpolated_fragment(Some('`'));
    }

    /// Handles the three interpolation openers when the cursor sits on
    /// one; returns false when the current character is plain content.
    /// `${` and `{$` push a scripting mode tagged with the opener's
    /// offset so end-of-input can report an unterminated interpolation;
    /// the matching `}` pops it through the ordinary brace rule.
    pub(super) fn lex_interpolation(&mut self) -> bool {
        let rest = self.cursor.rest();
        if rest.starts_with("${") {
            let opening = self.token_start();
            self.cursor.bump_bytes(2);
            self.emit(SyntaxKind::DollarOpenBrace);
            self.push_mode(Mode::Scripting {
                opened_by_interpolation_at: Some(opening),
            });
            return true;
        }
        if self.cursor.peek() == Some('$')
            && self.cursor.peek_second().is_some_and(is_name_start)
        {
            self.lex_string_variable();
            return true;
        }
        if rest.starts_with("{$") {
            let opening = self.token_start();
            self.cursor.eat('{');
            self.emit(SyntaxKind::OpenBrace);
            self.push_mode(Mode::Scripting {
                opened_by_interpolation_at: Some(opening),
            });
            return true;
        }
        false
    }

    /// `$name` plus at most one simple suffix, as in Zend's simple
    /// interpolation: `->prop` or `?->prop` (one level only), or a
    /// bracketed offset, which switches to the `VariableOffset` mode.
    fn lex_string_variable(&mut self) {
        self.cursor.eat('$');
        self.cursor.eat_while(is_name_continue);
        self.emit(SyntaxKind::Variable);
        let rest = self.cursor.rest();
        if let Some(after_arrow) = rest.strip_prefix("->") {
            if after_arrow.starts_with(is_name_start) {
                self.cursor.bump_bytes(2);
                self.emit(SyntaxKind::Arrow);
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Identifier);
            }
        } else if let Some(after_arrow) = rest.strip_prefix("?->") {
            if after_arrow.starts_with(is_name_start) {
                self.cursor.bump_bytes(3);
                self.emit(SyntaxKind::NullsafeArrow);
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Identifier);
            }
        } else if rest.starts_with('[') {
            self.cursor.eat('[');
            self.emit(SyntaxKind::OpenBracket);
            self.push_mode(Mode::VariableOffset);
        }
    }

    /// One step inside `$var[...]`: an offset atom, the closing
    /// bracket, or (on anything unrecognized) a bare pop so the
    /// enclosing string mode takes over at this character. The pop
    /// consumes nothing but strictly shrinks the mode stack, so
    /// progress is preserved.
    pub(super) fn lex_variable_offset(&mut self) {
        match self.cursor.peek() {
            Some(']') => {
                self.cursor.eat(']');
                self.emit(SyntaxKind::CloseBracket);
                self.pop_mode();
            }
            Some('-') => {
                self.cursor.eat('-');
                self.emit(SyntaxKind::Minus);
            }
            Some(character) if character.is_ascii_digit() => {
                self.cursor.eat_while(|c| c.is_ascii_digit());
                self.emit(SyntaxKind::IntegerLiteral);
            }
            Some('$')
                if self.cursor.peek_second().is_some_and(is_name_start) =>
            {
                self.cursor.eat('$');
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Variable);
            }
            Some(character) if is_name_start(character) => {
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Identifier);
            }
            _ => self.pop_mode(),
        }
    }

    /// A literal run: consumes up to (not including) the terminator, an
    /// interpolation opener, or the end of input. `\` escapes the next
    /// character, so `\"`, `\$`, and `\\` stay in the fragment. Always
    /// consumes at least one character: the callers only reach here
    /// after excluding the terminator and the openers at the current
    /// position.
    pub(super) fn lex_interpolated_fragment(&mut self, terminator: Option<char>) {
        while let Some(character) = self.cursor.peek() {
            if Some(character) == terminator {
                break;
            }
            if character == '\\' {
                self.cursor.bump();
                self.cursor.bump();
                continue;
            }
            if character == '$'
                && self
                    .cursor
                    .peek_second()
                    .is_some_and(|next| is_name_start(next) || next == '{')
            {
                break;
            }
            if character == '{' && self.cursor.peek_second() == Some('$') {
                break;
            }
            self.cursor.bump();
        }
        self.emit(SyntaxKind::StringFragment);
    }
```

Note the mode discipline this relies on, all already in place from Task 6: `{` inside the nested scripting pushes another scripting entry, `}` pops one, so `{$f(['k' => 1])}`'s closing brace pops exactly back into the string mode.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test strings`
Expected: 20 passed. Then `cargo test --package celerrate_syntax` for no regression.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex interpolated double-quoted and backtick strings"
```

---

### Task 11: Heredoc and nowdoc

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/strings.rs`
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs` (dispatch arm)
- Modify: `crates/celerrate_syntax/src/lexer.rs` (dispatch the heredoc modes)
- Create: `crates/celerrate_syntax/tests/heredoc.rs`

**Interfaces:**
- Consumes: everything from Task 10, `at_line_start` (Task 3).
- Produces on `Lexer`: `pub(super) fn try_lex_heredoc_start(&mut self) -> bool` (called from scripting on `<<<`), `pub(super) fn lex_heredoc(&mut self, label: TextRange)`, `pub(super) fn lex_nowdoc(&mut self, label: TextRange)`. Tokens: `HeredocStart` (the whole `<<<LABEL` header, trailing newline included), `StringFragment` and interpolation tokens in heredoc bodies, `HeredocEnd` (indentation plus label).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_syntax/tests/heredoc.rs`:

```rust
#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::LexerDiagnosticKind;
use celerrate_syntax::SyntaxKind::*;
use support::{lex_verified, texts};

#[test]
fn a_basic_heredoc() {
    assert_eq!(
        texts("<?php <<<EOT\nhello\nEOT;"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "hello\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
            (Semicolon, ";".to_owned()),
        ]
    );
}

#[test]
fn a_double_quoted_label_is_a_heredoc() {
    assert_eq!(
        texts("<?php <<<\"EOT\"\nx\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<\"EOT\"\n".to_owned()),
            (StringFragment, "x\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn heredocs_interpolate() {
    assert_eq!(
        texts("<?php <<<EOT\na $name b {$x}\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "a ".to_owned()),
            (Variable, "$name".to_owned()),
            (StringFragment, " b ".to_owned()),
            (OpenBrace, "{".to_owned()),
            (Variable, "$x".to_owned()),
            (CloseBrace, "}".to_owned()),
            (StringFragment, "\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn flexible_indentation_belongs_to_the_end_token() {
    assert_eq!(
        texts("<?php <<<EOT\n    body\n    EOT;"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "    body\n".to_owned()),
            (HeredocEnd, "    EOT".to_owned()),
            (Semicolon, ";".to_owned()),
        ]
    );
}

#[test]
fn a_label_prefix_inside_the_body_does_not_close() {
    // "EOTX" starts with the label but continues with a name character,
    // so the heredoc stays open until the bare "EOT" line.
    assert_eq!(
        texts("<?php <<<EOT\nEOTX\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "EOTX\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn nowdocs_do_not_interpolate() {
    assert_eq!(
        texts("<?php <<<'EOT'\na $name {$x}\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<'EOT'\n".to_owned()),
            (StringFragment, "a $name {$x}\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn spaces_are_allowed_between_the_arrows_and_the_label() {
    assert_eq!(
        texts("<?php <<<  EOT\nx\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<  EOT\n".to_owned()),
            (StringFragment, "x\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn triple_less_without_a_label_is_shifts_not_heredoc() {
    assert_eq!(
        texts("<?php 1 <<< 2").last(),
        Some(&(IntegerLiteral, "2".to_owned()))
    );
    let (_tokens, diagnostics) = lex_verified("<?php 1 <<< 2");
    assert!(diagnostics.is_empty());
}

#[test]
fn an_unterminated_heredoc_diagnoses_the_start() {
    let (tokens, diagnostics) = lex_verified("<?php <<<EOT\nbody");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, HeredocStart, StringFragment]
    );
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnterminatedHeredoc);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
    assert_eq!(u32::from(diagnostic.range.end()), 13);
}

#[test]
fn an_empty_heredoc_closes_immediately() {
    assert_eq!(
        texts("<?php <<<EOT\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test heredoc`
Expected: failures; `<<<` currently lexes as `LessLess` then `Less`.

- [ ] **Step 3: Write the implementation**

New arm in `lex_scripting` (`crates/celerrate_syntax/src/lexer/scripting.rs`), above the `_ if self.try_lex_operator()` arm; when the header does not parse (no valid label or no newline), the guard fails and the operator table lexes `<<` then `<`:

```rust
            '<' if heredoc_header_at(self.cursor.rest()).is_some() => {
                self.lex_heredoc_start();
            }
```

with `use super::strings::heredoc_header_at;` added to the imports of `scripting.rs`.

In `crates/celerrate_syntax/src/lexer.rs`, replace the heredoc placeholder arms in `run`:

```rust
                Mode::Heredoc { label, .. } => self.lex_heredoc(label),
                Mode::Nowdoc { label, .. } => self.lex_nowdoc(label),
```

Additions to `crates/celerrate_syntax/src/lexer/strings.rs` (add `use celerrate_source::{TextRange, TextSize};` to the imports):

```rust
/// A parsed `<<<LABEL` header: `<<<`, optional spaces and tabs, the
/// label (bare, double-quoted for a heredoc, single-quoted for a
/// nowdoc), and the line's newline.
pub(super) struct HeredocHeader {
    /// Total header length in bytes, trailing newline included.
    pub(super) length: usize,
    /// The bare label's position, relative to the header start.
    pub(super) label_start: usize,
    pub(super) label_length: usize,
    pub(super) is_nowdoc: bool,
}

/// Parses a heredoc or nowdoc header at the start of `rest`, or returns
/// `None` so `<<<` falls back to shift operators.
pub(super) fn heredoc_header_at(rest: &str) -> Option<HeredocHeader> {
    let after_arrows = rest.strip_prefix("<<<")?;
    let after_spaces = after_arrows.trim_start_matches([' ', '\t']);
    let spaces = after_arrows.len() - after_spaces.len();
    let quote = after_spaces
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\''));
    let after_quote = match quote {
        Some(_) => after_spaces.get(1..)?,
        None => after_spaces,
    };
    if !after_quote.starts_with(is_name_start) {
        return None;
    }
    let label_length: usize = after_quote
        .chars()
        .take_while(|character| is_name_continue(*character))
        .map(char::len_utf8)
        .sum();
    let after_label = after_quote.get(label_length..)?;
    let after_closing_quote = match quote {
        Some(quote) => after_label.strip_prefix(quote)?,
        None => after_label,
    };
    let newline_length = if after_closing_quote.starts_with("\r\n") {
        2
    } else if after_closing_quote.starts_with('\n') {
        1
    } else {
        return None;
    };
    let quote_length = usize::from(quote.is_some());
    let label_start = 3 + spaces + quote_length;
    Some(HeredocHeader {
        length: label_start + label_length + quote_length + newline_length,
        label_start,
        label_length,
        is_nowdoc: quote == Some('\''),
    })
}

/// Saturating usize-to-TextSize conversion; inputs are within the 4 GiB
/// cap (`SourceText` guarantees it).
fn text_size(length: usize) -> TextSize {
    u32::try_from(length)
        .map(TextSize::from)
        .unwrap_or_else(|_| TextSize::from(u32::MAX))
}
```

New methods in the `impl Lexer<'_>` block of `strings.rs`:

```rust
    /// Only called when `heredoc_header_at` matched; the redundant parse
    /// keeps the call sites free of unwraps.
    pub(super) fn lex_heredoc_start(&mut self) {
        let Some(header) = heredoc_header_at(self.cursor.rest()) else {
            self.lex_unexpected_character();
            return;
        };
        let start_offset = self.token_start();
        let start = TextRange::at(start_offset, text_size(header.length));
        let label = TextRange::at(
            start_offset + text_size(header.label_start),
            text_size(header.label_length),
        );
        self.cursor.bump_bytes(header.length);
        self.emit(SyntaxKind::HeredocStart);
        if header.is_nowdoc {
            self.push_mode(Mode::Nowdoc { start, label });
        } else {
            self.push_mode(Mode::Heredoc { start, label });
        }
    }

    pub(super) fn lex_heredoc(&mut self, label: TextRange) {
        if self.at_line_start() && self.lex_heredoc_end(label) {
            return;
        }
        if self.lex_interpolation() {
            return;
        }
        self.lex_heredoc_fragment(label, true);
    }

    pub(super) fn lex_nowdoc(&mut self, label: TextRange) {
        if self.at_line_start() && self.lex_heredoc_end(label) {
            return;
        }
        self.lex_heredoc_fragment(label, false);
    }

    /// Emits `HeredocEnd` (indentation plus label, per PHP 7.3 flexible
    /// closing markers) when the closing line starts here.
    fn lex_heredoc_end(&mut self, label: TextRange) -> bool {
        let Some(closer_length) = self.heredoc_closer_length(label) else {
            return false;
        };
        self.cursor.bump_bytes(closer_length);
        self.emit(SyntaxKind::HeredocEnd);
        self.pop_mode();
        true
    }

    /// When the unconsumed input begins a closing-label line (optional
    /// spaces and tabs, the label, then no name character), returns the
    /// byte length of indentation plus label.
    fn heredoc_closer_length(&self, label: TextRange) -> Option<usize> {
        let label_text = self
            .source
            .get(usize::from(label.start())..usize::from(label.end()))?;
        let rest = self.cursor.rest();
        let after_indentation = rest.trim_start_matches([' ', '\t']);
        let indentation = rest.len() - after_indentation.len();
        let after_label = after_indentation.strip_prefix(label_text)?;
        if after_label.starts_with(is_name_continue) {
            return None;
        }
        Some(indentation + label_text.len())
    }

    /// A heredoc or nowdoc literal run: stops before an interpolation
    /// opener (heredoc only) and right after a newline that begins the
    /// closing-label line.
    fn lex_heredoc_fragment(&mut self, label: TextRange, interpolated: bool) {
        while let Some(character) = self.cursor.peek() {
            if interpolated {
                if character == '\\' {
                    self.cursor.bump();
                    let escaped = self.cursor.bump();
                    // A backslash at the end of a line is literal; the
                    // newline it precedes may still start the closer.
                    if escaped == Some('\n') && self.heredoc_closer_length(label).is_some() {
                        break;
                    }
                    continue;
                }
                if character == '$'
                    && self
                        .cursor
                        .peek_second()
                        .is_some_and(|next| is_name_start(next) || next == '{')
                {
                    break;
                }
                if character == '{' && self.cursor.peek_second() == Some('$') {
                    break;
                }
            }
            self.cursor.bump();
            if character == '\n' && self.heredoc_closer_length(label).is_some() {
                break;
            }
        }
        self.emit(SyntaxKind::StringFragment);
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test heredoc`
Expected: 10 passed. Then `cargo test --package celerrate_syntax` for no regression.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_syntax
git commit -m "✨ feat(syntax): lex heredoc and nowdoc"
```

---

### Task 12: Snapshot corpus with insta

**Files:**
- Modify: `Cargo.toml` (workspace dependency `insta`)
- Modify: `crates/celerrate_syntax/Cargo.toml` (dev-dependency)
- Create: `crates/celerrate_syntax/tests/corpus.rs`
- Create: `crates/celerrate_syntax/tests/corpus/*.php` (five files below)
- Generated: `crates/celerrate_syntax/tests/snapshots/*.snap` (committed)

**Interfaces:**
- Consumes: `lex`, `Token`, `LexerDiagnostic` (public API).
- Produces: a `render(source) -> String` listing in `kind @ start..end "text"` form plus a diagnostics section; one snapshot per corpus file via `insta::glob!`.

- [ ] **Step 1: Add the dependency**

In the workspace `Cargo.toml`, under `[workspace.dependencies]`:

```toml
insta = { version = "1", features = ["glob"] }
```

In `crates/celerrate_syntax/Cargo.toml`:

```toml
[dev-dependencies]
insta = { workspace = true }
```

- [ ] **Step 2: Write the harness and the corpus**

Create `crates/celerrate_syntax/tests/corpus.rs`:

```rust
//! Snapshot corpus: every `tests/corpus/*.php` file is lexed and
//! snapshotted as a `kind @ start..end "text"` listing plus diagnostics.
//! The lossless invariant is asserted on every file.
#![allow(clippy::expect_used, clippy::indexing_slicing)]

use std::fmt::Write as _;

use celerrate_syntax::lex;

fn render(source: &str) -> String {
    let (tokens, diagnostics) = lex(source);
    let mut output = String::new();
    let mut offset = 0usize;
    for token in &tokens {
        let end = offset + usize::from(token.length);
        let text = &source[offset..end];
        let _ = writeln!(output, "{:?} @ {offset}..{end} {text:?}", token.kind);
        offset = end;
    }
    assert_eq!(offset, source.len(), "the token stream must be lossless");
    if !diagnostics.is_empty() {
        let _ = writeln!(output, "---");
        for diagnostic in &diagnostics {
            let _ = writeln!(
                output,
                "{:?} @ {}..{}",
                diagnostic.kind,
                u32::from(diagnostic.range.start()),
                u32::from(diagnostic.range.end()),
            );
        }
    }
    output
}

#[test]
fn corpus() {
    insta::glob!("corpus/*.php", |path| {
        let source = std::fs::read_to_string(path).expect("corpus file is readable");
        insta::assert_snapshot!(render(&source));
    });
}
```

Create the five corpus files:

`crates/celerrate_syntax/tests/corpus/hello.php`:

```php
<!DOCTYPE html>
<body>
<?php

declare(strict_types=1);

$greeting = 'Hello';
echo "$greeting, {$_SERVER['REMOTE_ADDR']} !";

?>
</body>
```

`crates/celerrate_syntax/tests/corpus/class.php`:

```php
<?php

namespace App\Domain;

#[Attribute]
final readonly class Money
{
    public function __construct(
        private int $amount,
        private Currency $currency = Currency::Euro,
    ) {
    }

    public function add(self $other): static
    {
        return new static($this->amount + $other->amount, $this->currency);
    }
}

enum Currency: string
{
    case Euro = 'EUR';
    case Dollar = 'USD';
}
```

`crates/celerrate_syntax/tests/corpus/strings.php`:

```php
<?php

$simple = 'single \' quoted';
$double = "double $interpolated \"escaped\" ${deprecated} {$complex->call()}";
$offsets = "$array[0] $array[key] $array[$variable] $object->property";
$heredoc = <<<TEXT
    Indented $body text
    TEXT;
$nowdoc = <<<'RAW'
No $interpolation here
RAW;
$shell = `ls -la $directory`;
$binary = b"bytes";
```

`crates/celerrate_syntax/tests/corpus/numbers_operators.php`:

```php
<?php

$mix = 0xFF_EC + 0b1010 - 0o777 + 0777 + 1_000_000;
$floats = .5 + 1. + 1.5e-3 + 2E8;
$compare = $a <=> $b ?: $a ?? $b;
$assign ??= $a ** $b % $c;
$casts = (int) '1' . (string)2 . ( FLOAT )$x;
$arrow = fn(int $n): int => $n <<= 2;
$attribute = new #[Pure] class {};
list($x, [$y]) = [1, [2]];
```

`crates/celerrate_syntax/tests/corpus/errors.php`:

```php
<?php

$unterminated = "still open {$brace
```

- [ ] **Step 3: Generate, review, and accept the snapshots**

Run: `INSTA_UPDATE=always cargo test --package celerrate_syntax --test corpus`
Expected: the run passes and writes `crates/celerrate_syntax/tests/snapshots/corpus__*.snap`.

Review each generated `.snap` by hand against the spec (kinds, boundaries, diagnostics; `errors.php` must show `UnterminatedString` and `UnterminatedInterpolation`). Then re-run without the variable to confirm stability:

Run: `cargo test --package celerrate_syntax --test corpus`
Expected: passes with no pending snapshots.

- [ ] **Step 4: Commit (snapshots included)**

```bash
git add Cargo.toml Cargo.lock crates/celerrate_syntax
git commit -m "✅ test(syntax): add the insta snapshot corpus"
```

---

### Task 13: Fuzz target, CI job, changelog

**Files:**
- Create: `fuzz/Cargo.toml`
- Create: `fuzz/fuzz_targets/lex.rs`
- Create: `fuzz/corpus/lex/*.php` (seed corpus)
- Create: `fuzz/.gitignore`
- Modify: `.github/workflows/ci.yml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `SourceText::from_bytes` (celerrate_source), `lex` (public API).
- Produces: the `lex` fuzz target asserting no panic plus the lossless invariant on arbitrary bytes; a CI job running it a few minutes per push.

- [ ] **Step 1: Create the fuzz package**

The fuzz package lives at the repository root and is its own workspace (the root workspace's `members = ["crates/*"]` never sees it, and `cargo test --workspace` stays unaffected). It is a fuzzing harness, not a shipped crate: `publish = false`, and asserts are the point, so the workspace zero-panic lints deliberately do not apply to it.

Create `fuzz/Cargo.toml`:

```toml
[package]
name = "celerrate-fuzz"
version = "0.0.0"
publish = false
edition = "2024"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
celerrate_source = { path = "../crates/celerrate_source" }
celerrate_syntax = { path = "../crates/celerrate_syntax" }

[[bin]]
name = "lex"
path = "fuzz_targets/lex.rs"
test = false
doc = false
bench = false

# The fuzz harness is its own workspace, separate from the root one.
[workspace]
```

Create `fuzz/fuzz_targets/lex.rs`:

```rust
//! Arbitrary bytes through `SourceText::from_bytes` then the lexer.
//! Invariants: no panic anywhere, the token stream is lossless, and
//! lexing terminates (libFuzzer's timeout catches hangs).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = celerrate_source::SourceText::from_bytes(data) else {
        return;
    };
    let (tokens, _diagnostics) = celerrate_syntax::lex(source.text());
    let total: usize = tokens
        .iter()
        .map(|token| usize::from(token.length))
        .sum();
    assert_eq!(total, source.text().len(), "the token stream must be lossless");
});
```

Create `fuzz/.gitignore` (cargo-fuzz writes artifacts and coverage data next to the package):

```gitignore
target/
artifacts/
coverage/
Cargo.lock
```

Create the seed corpus, one construct-dense file per area:

`fuzz/corpus/lex/seed_basic.php`:

```php
<?php declare(strict_types=1); echo "Hello {$user->name}!"; ?>
<html><?= $title ?></html>
```

`fuzz/corpus/lex/seed_strings.php`:

```php
<?php $a = 'x\''; $b = "$v[0] ${d} \" `$c` "; $h = <<<EOT
  $body
  EOT; $n = <<<'RAW'
raw
RAW;
```

`fuzz/corpus/lex/seed_numbers.php`:

```php
<?php $n = 0xFF + 0b10 + 0o7 + 077 + 1_0.5e-3 + .5 + 1.; $c = (int)( string )$x <=> $y ??= 2;
```

`fuzz/corpus/lex/seed_errors.php`:

```php
#!/usr/bin/env php
<?php /* open
"unterminated {$x
```

- [ ] **Step 2: Run the fuzzer locally**

cargo-fuzz needs a nightly toolchain and the cargo-fuzz binary:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --locked
cargo +nightly fuzz run lex -- -max_total_time=60
```

Expected: one minute of fuzzing with no crash and no timeout (`Done ... runs` and exit code 0). If it finds a crash, cargo-fuzz writes the failing input under `fuzz/artifacts/lex/`; minimize with `cargo +nightly fuzz tmin lex <artifact>`, fix the lexer with a regular TDD cycle (turn the input into a unit test first), then re-run.

- [ ] **Step 3: Add the CI job**

Append to the `jobs:` section of `.github/workflows/ci.yml`, matching the existing job style:

```yaml
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: nightly
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: fuzz
      - run: cargo install cargo-fuzz --locked
      - run: cargo fuzz run lex -- -max_total_time=180
```

- [ ] **Step 4: Update the changelog**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, append:

```markdown
- `celerrate_syntax`: complete PHP 8.1+ lexer (lossless token stream,
  string interpolation, structured diagnostics), snapshot corpus, and a
  continuous fuzz target.
```

- [ ] **Step 5: Run the full verification suite**

From the repository root:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

Expected: all four succeed. If `cargo fmt` reports differences, apply `cargo fmt --all` and re-run the tests.

- [ ] **Step 6: Commit**

```bash
git add fuzz .github/workflows/ci.yml CHANGELOG.md
git commit -m "✅ test(syntax): add the lex fuzz target and its CI job"
```

---

## Completion

When all tasks are done and the verification suite is green, use superpowers:finishing-a-development-branch to integrate `foundations-3-lexer` (Parts 1 and 2 went through pull requests to `main`; expect the same here).









