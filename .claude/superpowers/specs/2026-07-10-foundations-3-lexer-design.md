# Foundations Part 3: The Lexer (Design)

Date: 2026-07-10
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (sections 3, 8, 9)
Predecessor plan: `.claude/superpowers/plans/2026-07-10-foundations-2-source-files.md`

## 1. Goal and scope

Create the `celerrate_syntax` crate with a complete, error-resilient,
hand-written PHP 8.1+ lexer. After this part, the crate turns any decoded
source text into a lossless token stream plus structured diagnostics,
which is everything the parser (the next Foundations part) needs.

In scope:

- The `crates/celerrate_syntax` crate, depending only on
  `celerrate_source`.
- `SyntaxKind`: the single kind vocabulary, token kinds only for now.
- The lexer: a stateful, hand-written state machine covering the full
  PHP 8.1+ lexical grammar, including string interpolation.
- Lexer diagnostics as structured data beside the token stream.
- Snapshot testing infrastructure (`insta`) and a `cargo-fuzz` target
  with a dedicated CI job.

Out of scope (deliberately):

- The concrete syntax tree and the parser: the next Foundations part.
  `SyntaxKind` is designed to receive node kinds later but contains
  none.
- The rowan-versus-homegrown tree decision: made in the next part, with
  the parser as first consumer. Nothing here prejudges it; the
  `#[repr(u16)]` representation suits both.
- Any version or configuration judgment. Short open tags, the
  deprecated `${}` interpolation, and post-8.1 syntax are all lexed
  uniformly; availability and deprecation are semantic diagnostics
  produced by upper layers, never lexer failures.
- Measured performance: no criterion benchmarks in this part.
  Benchmarks arrive once there is a full pipeline to measure. The
  design is already performance-oriented (compact tokens, no
  per-token allocation outside the stream).
- A high-level public API. The entry point is a single function:
  `lex(&str) -> (Vec<Token>, Vec<LexerDiagnostic>)`.

Architecture decision, recorded: the lexer is hand-written rather than
generated (for example with `logos`). PHP is strongly modal (inline
HTML, heredoc with flexible indentation, three interpolation forms,
cast tokens), so a generator would keep the mode-driver complexity
while adding a dependency and an indirection layer. Production lexers
for interpolated languages (Zend's own, rust-analyzer, ty) are
hand-written for the same reason.

## 2. Token model

### `SyntaxKind`

A single `#[repr(u16)]` enumeration shared by the whole syntax layer.
In this part it contains only token kinds; the parser part appends node
kinds. One vocabulary avoids a permanent `TokenKind -> SyntaxKind`
conversion layer and matches the contract of rowan-style trees, should
that path be chosen.

Token kind families:

- Trivia: whitespace, line comments (`//` and `#`), block comments
  (`/* */`), docblocks (`/** */`, a distinct kind: the future type
  engine reads them), and the first-line shebang (`#!`).
- Tags: `<?php`, `<?=`, the short `<?`, and `?>`.
- `InlineHtml` for everything outside PHP tags.
- Identifiers, variables (`$name`), and one kind per keyword.
- Literals: integers (decimal, hexadecimal, octal including the legacy
  leading-zero form and `0o`, binary, `_` separators), floats, and the
  string-content kinds described below.
- Operators and punctuation, including `?->`, `??=`, `<=>`, `...`, and
  the `#[` attribute opener (a distinct kind from the `#` comment).
- Cast tokens: `(int)`, `( string )`, and the other forms Zend
  recognizes, each as one token (see section 3).
- `Error` for bytes no rule accepts.

### Tokens

```rust
pub struct Token {
    pub kind: SyntaxKind,
    pub length: TextSize,
}
```

The lexer emits kind-and-length pairs, rust-analyzer style. No offset
is stored; positions are reconstructed by accumulation, which makes
overlaps and gaps structurally impossible. The lossless invariant at
this layer: concatenating every token's text reproduces the input
byte for byte.

Trivia are ordinary tokens in the stream. Nothing is discarded; this
is the precondition of the lossless tree built on top later.

### Input and diagnostics

The lexer consumes a `&str` (the decoded text of a `SourceText`); it
knows nothing about files or decoding. It returns the token stream and
a separate `Vec<LexerDiagnostic>`, where each diagnostic carries a
structured kind (for example `UnexpectedCharacter`, an
`Unterminated*` family) and a `TextRange`. The stream itself stays
complete and lossless even on degenerate input: diagnostics travel
beside the tokens, never instead of them.

## 3. The state machine

A struct advancing over the characters with bounded lookahead
(`Chars` plus peeking) and an explicit mode stack.

Modes:

- **`InlineHtml`**: the initial state. Everything before an opening
  tag is a single `InlineHtml` token. Recognizes `<?php`, `<?=`, and
  the short `<?`. The short tag is lexed as an opening tag
  unconditionally: its validity depends on an ini setting, so it is a
  semantic diagnostic for upper layers, keeping the lexer a pure
  function of its input.
- **`Scripting`**: the main mode. Identifiers, variables, numeric
  literals, operators, attributes, comments. `?>` returns to
  `InlineHtml` and swallows one following newline, as PHP does.
- **`DoubleQuotedString`**, **`Heredoc`**, **`Nowdoc`**,
  **`Backtick`**: interpolation modes. Inside them the lexer emits
  fine-grained tokens: literal fragments, `$variable` with the simple
  `->property` and `[index]` forms, `{$expr}` which pushes a nested
  `Scripting` mode until the matching closing brace, and the
  deprecated `${name}` (lexed normally; deprecation is gated
  semantically). Heredoc handles the flexible closing-marker
  indentation of PHP 7.3+. Nowdoc and single-quoted strings do not
  interpolate.

Cross-cutting rules:

- **Keywords are case-insensitive** and resolved by the lexer:
  `echo`, `Echo`, and `ECHO` yield the same kind while the exact
  spelling stays in the source text. Semi-reserved contexts
  (`$object->list()`, `const FOR = 1;`) are the parser's business: it
  will re-treat keyword kinds as identifiers where the grammar allows,
  because only it has the context.
- **Casts are single tokens**, as in Zend's lexer: `(int)` and
  `( string )` (inner whitespace included) become `IntCast`,
  `StringCast`, and so on. This stays lossless (the full text is in
  the token) and spares the parser a painful disambiguation against
  parenthesized expressions. Only the exact forms PHP 8.1 recognizes
  match: `(int)`, `(integer)`, `(bool)`, `(boolean)`, `(float)`,
  `(double)`, `(string)`, `(binary)`, `(array)`, `(object)`. The
  `(real)` and `(unset)` casts were removed in PHP 8.0 and are not
  cast tokens.
- **Full PHP 8.1+ grammar, no gating.** The lexer knows the newest
  lexical grammar it supports (`readonly`, enums, `#[`, `0o` octals,
  and so on); per-version availability is a semantic diagnostic,
  never a lexing failure, per the parent spec.

### Recorded divergences

One deliberate divergence from Zend's lexer, chosen for error recovery:
binary and octal literals take the maximal digit run. `0b2` and `0o99`
each lex as a single `IntegerLiteral` whose digit validity is judged
semantically; Zend stops at the first invalid digit (`0b2` is the
integer `0` followed by the name `b2`). One token gives the semantic
layer a single literal to attach an invalid-digit diagnostic to,
instead of a confusing name-after-number token pair. The same rule
applies to radix-prefixed offsets in string interpolation
(`"$a[0b2]"`). The maximal run also swallows misplaced `_` separators:
`1_` is one `IntegerLiteral` where Zend splits it into the integer and
a name, in scripting mode and in offsets alike.

## 4. Error handling

The lexer always terminates and always produces a complete stream.

- **Unexpected character** (a stray control byte in scripting mode):
  a one-character `Error` token, an `UnexpectedCharacter` diagnostic,
  and lexing continues at the next character.
- **Unterminated constructions** (string, heredoc, `/*` without `*/`,
  `{$` without `}`): the token is emitted up to the end of input with
  its normal kind, plus an `Unterminated*` diagnostic pointing at the
  opening. The normal kind rather than `Error`, because mid-edit code
  in an editor is the nominal case: the parser must still get a
  useful tree out of it.
- **Guaranteed progress**: every iteration of the main loop consumes
  at least one character. This is the termination argument, and the
  fuzzer verifies it (no infinite loop is possible).
- **Mechanical zero-panic**: iteration through `Chars` and peeking
  only, no direct indexing, length arithmetic in `TextSize` (input is
  already capped at 4 GiB by `celerrate_source`). The workspace deny
  lints apply to the crate with no exceptions outside test modules.

## 5. Testing

1. **Unit tests, TDD**: every behavior (a mode, an operator, an
   interpolation edge case) starts from a failing test.
2. **`insta` snapshots**: a corpus of PHP files under `tests/corpus/`,
   each snapshotted as a `kind @ range "text"` listing plus
   diagnostics. This is the day-to-day tool; the `insta` dev
   dependency enters the workspace here.
3. **The lossless invariant**: for every input (corpus and fuzzer),
   `concat(token texts) == source`, checked by a shared helper used
   everywhere.
4. **`cargo-fuzz` target**: arbitrary bytes through
   `SourceText::from_bytes` then the lexer; invariants: no panic,
   lossless, termination (hangs are detected by the fuzzer). A
   dedicated CI job runs it with a short time budget (a few minutes
   per run); the fuzzing seed corpus is committed.
