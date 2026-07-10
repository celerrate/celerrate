# Lexer Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clear the lexer backlog deferred from pull request #3: three Zend-fidelity behavior changes, pinning tests, a spec note, comment polish, and fuzzing infrastructure improvements.

**Architecture:** All changes live inside the existing `celerrate_syntax` lexer (mode-stack state machine, kind-and-length tokens, diagnostics beside the stream) plus the fuzz workspace and the CI workflow. No new modules; a small shared helper is extracted inside `src/lexer/scripting.rs` and reused by the variable-offset mode.

**Tech Stack:** Rust (stable 1.94 pinned by `rust-toolchain.toml`), `text-size`, `insta` (snapshots), `cargo-fuzz` (nightly), GitHub Actions.

**Spec:** `.claude/superpowers/specs/2026-07-10-foundations-3-lexer-follow-ups-design.md` (approved). Parent spec: `.claude/superpowers/specs/2026-07-10-foundations-3-lexer-design.md`.

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. No direct indexing; char-cursor iteration only. Test modules may locally `#[allow]` these lints with a reason comment.
- Lossless token stream: concatenating every token's text reproduces the input byte for byte; no empty tokens. Every new test goes through the `tests/support` helpers (`lex_verified`, `texts`, `kinds`), which assert this.
- Guaranteed progress: every dispatch consumes at least one character or strictly shrinks the mode stack.
- Determinism: analysis results are pure functions of their inputs. No wall-clock time, no randomness, no environment reads.
- Diagnostics travel beside the stream, never instead of it; unterminated constructions keep their normal token kind.
- Everything is written in English, full words, no em-dashes. Standard acronyms are fine.
- Commits: gitmoji + Conventional Commits (`<emoji> <type>(<scope>): <summary>`). Never add AI attribution of any kind. Never override the repository git identity.
- Verification gate for every task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` (run `cargo fmt --all` first if it fails).
- Where a step verifies against real PHP, use the installed `php` binary and `token_get_all`; the expected output is stated in the step.

## File Structure

- `crates/celerrate_syntax/src/lexer/scripting.rs`: extract `try_lex_radix_integer` (Task 1), add `is_php_whitespace` and use it (Task 2), report the cut interpolation in `lex_close_tag` (Task 3), `cast_at` and line-comment polish (Task 5).
- `crates/celerrate_syntax/src/lexer/strings.rs`: radix-aware `lex_variable_offset` (Task 1).
- `crates/celerrate_syntax/src/syntax_kind.rs`: keyword-length comment (Task 5).
- `crates/celerrate_syntax/tests/strings.rs`: offset and close-tag tests (Tasks 1, 3).
- `crates/celerrate_syntax/tests/errors.rs`: form feed tests (Task 2).
- `crates/celerrate_syntax/tests/numbers.rs`: text-pinned `0bz`/`0oz` (Task 4).
- `crates/celerrate_syntax/tests/heredoc.rs`: three edge tests (Task 4).
- `.claude/superpowers/specs/2026-07-10-foundations-3-lexer-design.md`: "Recorded divergences" subsection (Task 5).
- `fuzz/.gitignore`, `fuzz/corpus/lex/seed_variable_offsets.php`, `.github/workflows/ci.yml`: fuzzing infrastructure (Task 6).

---

### Task 1: Radix-prefixed variable offsets

Zend's `ST_VAR_OFFSET` state matches `{LNUM}|{HNUM}|{BNUM}|{ONUM}` as `T_NUM_STRING`: decimal, hexadecimal (`0x`), binary (`0b`), and explicit octal (`0o`) integers, with `_` separators. Our `lex_variable_offset` accepts only bare decimal digit runs. Extract the radix scanning already present in `lex_number` into a reusable helper and call it from the offset mode. Binary and octal keep our deliberate maximal-decimal-run behavior (digit validity is judged semantically), consistent with scripting mode.

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs` (`lex_number`, new `try_lex_radix_integer`)
- Modify: `crates/celerrate_syntax/src/lexer/strings.rs` (`lex_variable_offset`, the digit arm)
- Test: `crates/celerrate_syntax/tests/strings.rs`

**Interfaces:**
- Consumes: `starts_with_radix_prefix(rest, prefix, is_digit)` (private helper already in `scripting.rs`), `Cursor::{rest, bump_bytes, eat_while}`, `Lexer::emit`.
- Produces: `pub(super) fn try_lex_radix_integer(&mut self) -> bool` on `Lexer`, defined in `scripting.rs`, callable from `strings.rs` (both are children of the `lexer` module). Consumes a `0x`/`0X`/`0b`/`0B`/`0o`/`0O` integer and emits one `IntegerLiteral` when one starts at the cursor; consumes nothing and returns `false` otherwise.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/strings.rs`:

```rust
#[test]
fn radix_prefixed_offsets_interpolate_as_one_literal() {
    // Zend's ST_VAR_OFFSET lexes LNUM, HNUM, BNUM, and ONUM alike as
    // T_NUM_STRING; offset validity is judged semantically.
    assert_eq!(
        texts(r#"<?php "$a[0x1A] $a[0b11] $a[0o17] $a[1_000]""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$a".to_owned()),
            (OpenBracket, "[".to_owned()),
            (IntegerLiteral, "0x1A".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$a".to_owned()),
            (OpenBracket, "[".to_owned()),
            (IntegerLiteral, "0b11".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$a".to_owned()),
            (OpenBracket, "[".to_owned()),
            (IntegerLiteral, "0o17".to_owned()),
            (CloseBracket, "]".to_owned()),
            (StringFragment, " ".to_owned()),
            (Variable, "$a".to_owned()),
            (OpenBracket, "[".to_owned()),
            (IntegerLiteral, "1_000".to_owned()),
            (CloseBracket, "]".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}

#[test]
fn a_radix_prefix_without_a_digit_falls_back_in_offsets_too() {
    // "0x" with no hex digit after it: the integer zero, then the name,
    // exactly as in scripting mode.
    assert_eq!(
        texts(r#"<?php "$a[0xg]""#),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (DoubleQuote, "\"".to_owned()),
            (Variable, "$a".to_owned()),
            (OpenBracket, "[".to_owned()),
            (IntegerLiteral, "0".to_owned()),
            (Identifier, "xg".to_owned()),
            (CloseBracket, "]".to_owned()),
            (DoubleQuote, "\"".to_owned()),
        ]
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test strings radix -- --nocapture`
Expected: FAIL. `radix_prefixed_offsets_interpolate_as_one_literal` currently lexes `0x1A` as `IntegerLiteral "0"` then `Identifier "x1A"`, and `1_000` as `IntegerLiteral "1"` then `Identifier "_000"`.

- [ ] **Step 3: Extract the helper in `scripting.rs`**

In `crates/celerrate_syntax/src/lexer/scripting.rs`, replace the whole `lex_number` radix section. The current function begins:

```rust
    fn lex_number(&mut self) {
        // Binary and octal deliberately take the maximal decimal-digit
        // run: digit validity ("0b2", "0o99") is judged upstairs, so each
        // stays a single literal.
        let rest = self.cursor.rest();
        let is_hex_digit = |c: char| c.is_ascii_hexdigit();
        let is_decimal_digit = |c: char| c.is_ascii_digit();
        if starts_with_radix_prefix(rest, "0x", is_hex_digit)
            || starts_with_radix_prefix(rest, "0X", is_hex_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_hexdigit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        if starts_with_radix_prefix(rest, "0b", is_decimal_digit)
            || starts_with_radix_prefix(rest, "0B", is_decimal_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        if starts_with_radix_prefix(rest, "0o", is_decimal_digit)
            || starts_with_radix_prefix(rest, "0O", is_decimal_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        // Decimal digits. Separator placement and octal digit validity
```

Replace everything from `fn lex_number(&mut self) {` down to (not including) the `// Decimal digits.` comment with:

```rust
    fn lex_number(&mut self) {
        if self.try_lex_radix_integer() {
            return;
        }
        // Decimal digits. Separator placement and octal digit validity
```

Then add the new method right after `lex_number`'s closing brace (before `eat_exponent`):

```rust
    /// Consumes a `0x`/`0b`/`0o` integer (either case) when one starts
    /// at the cursor and returns true; consumes nothing otherwise.
    /// Binary and octal deliberately take the maximal decimal-digit
    /// run: digit validity ("0b2", "0o99") is judged upstairs, so each
    /// stays a single literal. Also used by the variable-offset mode:
    /// Zend's ST_VAR_OFFSET accepts the same integer forms as
    /// scripting.
    pub(super) fn try_lex_radix_integer(&mut self) -> bool {
        let rest = self.cursor.rest();
        let is_hex_digit = |c: char| c.is_ascii_hexdigit();
        let is_decimal_digit = |c: char| c.is_ascii_digit();
        if starts_with_radix_prefix(rest, "0x", is_hex_digit)
            || starts_with_radix_prefix(rest, "0X", is_hex_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_hexdigit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return true;
        }
        if starts_with_radix_prefix(rest, "0b", is_decimal_digit)
            || starts_with_radix_prefix(rest, "0B", is_decimal_digit)
            || starts_with_radix_prefix(rest, "0o", is_decimal_digit)
            || starts_with_radix_prefix(rest, "0O", is_decimal_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return true;
        }
        false
    }
```

Note the binary and octal branches merge into one `if`: their bodies were identical.

- [ ] **Step 4: Use the helper in `lex_variable_offset`**

In `crates/celerrate_syntax/src/lexer/strings.rs`, replace the digit arm of `lex_variable_offset`:

```rust
            Some(character) if character.is_ascii_digit() => {
                self.cursor.eat_while(|c| c.is_ascii_digit());
                self.emit(SyntaxKind::IntegerLiteral);
            }
```

with:

```rust
            Some(character) if character.is_ascii_digit() => {
                // Zend's ST_VAR_OFFSET accepts the same integer forms as
                // scripting mode (LNUM, HNUM, BNUM, ONUM, all lexed as
                // T_NUM_STRING); offset validity is a semantic judgment,
                // as everywhere else.
                if !self.try_lex_radix_integer() {
                    self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
                    self.emit(SyntaxKind::IntegerLiteral);
                }
            }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test strings`
Expected: PASS, including the pre-existing `simple_offset_interpolation` test.

- [ ] **Step 6: Verify against real PHP**

Run:

```bash
php -r '
$code = "<?php \"\$a[0x1A] \$a[0b11] \$a[0o17] \$a[1_000]\"";
foreach (token_get_all($code) as $t) {
    if (is_array($t)) { echo token_name($t[0]), " ", var_export($t[1], true), "\n"; }
    else { echo $t, "\n"; }
}'
```

Expected: the output contains `T_NUM_STRING '0x1A'`, `T_NUM_STRING '0b11'`, `T_NUM_STRING '0o17'`, and `T_NUM_STRING '1_000'`, each as one token. If `php` is not installed, note it in the report and rely on the Zend grammar citation above.

- [ ] **Step 7: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green (127 workspace tests: 125 existing plus the 2 new ones).

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_syntax/src/lexer/scripting.rs crates/celerrate_syntax/src/lexer/strings.rs crates/celerrate_syntax/tests/strings.rs
git commit -m "✨ feat(syntax): lex radix-prefixed variable offsets in interpolation"
```

---

### Task 2: Form feed is not whitespace in scripting mode

Zend's whitespace is exactly space, horizontal tab, `\n`, and `\r`. Our scripting mode uses `is_ascii_whitespace`, which also accepts form feed (U+000C); in real PHP a form feed in code is an unexpected character. Introduce the exact predicate and use it for both the whitespace token and the docblock-opener check (Zend's `"/**" {WHITESPACE}` rule uses the same set). No other mode changes: in inline HTML, strings, heredoc bodies, and comments a form feed is ordinary content, and the open-tag boundary check in `inline_html.rs` already uses the exact four-character set.

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs` (whitespace arm of `lex_scripting`, docblock check in `lex_block_comment`, new `is_php_whitespace`)
- Test: `crates/celerrate_syntax/tests/errors.rs`

**Interfaces:**
- Consumes: `Lexer::lex_unexpected_character` (the existing fallback arm catches the form feed once no other arm matches; no new dispatch arm is needed).
- Produces: `fn is_php_whitespace(character: char) -> bool`, private to `scripting.rs`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/celerrate_syntax/tests/errors.rs`:

```rust
#[test]
fn form_feed_is_not_whitespace_in_scripting_mode() {
    // Zend's whitespace is space, tab, \n, and \r only; a form feed in
    // PHP code is an unexpected character.
    let (tokens, diagnostics) = lex_verified("<?php \u{C};");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [OpenTag, Whitespace, Error, Semicolon]
    );
    let diagnostic = diagnostics.first().copied().expect("one diagnostic");
    assert_eq!(diagnostic.kind, LexerDiagnosticKind::UnexpectedCharacter);
    assert_eq!(u32::from(diagnostic.range.start()), 6);
}

#[test]
fn form_feed_stays_ordinary_content_outside_scripting() {
    let (tokens, diagnostics) = lex_verified("a\u{C}b<?php '\u{C}' ?>\u{C}");
    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [
            InlineHtml,
            OpenTag,
            Whitespace,
            SingleQuotedString,
            Whitespace,
            CloseTag,
            InlineHtml,
        ]
    );
    assert!(diagnostics.is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_syntax --test errors form_feed`
Expected: `form_feed_is_not_whitespace_in_scripting_mode` FAILS (the form feed currently joins a `Whitespace` token, so the kinds are `[OpenTag, Whitespace, Semicolon]`). `form_feed_stays_ordinary_content_outside_scripting` may already pass; that is fine, it pins the boundary of the change.

- [ ] **Step 3: Implement the exact predicate**

In `crates/celerrate_syntax/src/lexer/scripting.rs`, add after `is_name_continue`:

```rust
/// Zend's whitespace is exactly these four characters. Notably, form
/// feed (U+000C) is not one of them: in PHP code it lexes as an
/// unexpected character.
fn is_php_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r')
}
```

Replace the whitespace arm in `lex_scripting`:

```rust
            character if character.is_ascii_whitespace() => {
                self.cursor
                    .eat_while(|character| character.is_ascii_whitespace());
                self.emit(SyntaxKind::Whitespace);
            }
```

with:

```rust
            character if is_php_whitespace(character) => {
                self.cursor.eat_while(is_php_whitespace);
                self.emit(SyntaxKind::Whitespace);
            }
```

And in `lex_block_comment`, replace the docblock check:

```rust
        let is_docblock = rest
            .strip_prefix("/**")
            .is_some_and(|after| after.starts_with(|c: char| c.is_ascii_whitespace()));
```

with:

```rust
        let is_docblock = rest
            .strip_prefix("/**")
            .is_some_and(|after| after.starts_with(is_php_whitespace));
```

No dispatch arm is added: a form feed now matches no arm and falls through to `lex_unexpected_character`, which emits the `Error` token and the diagnostic.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test errors`
Expected: PASS.

- [ ] **Step 5: Check the snapshot corpus is unaffected**

Run: `cargo test --package celerrate_syntax --test corpus`
Expected: PASS with no snapshot changes (no corpus file contains a form feed). If a snapshot does change, inspect it: only `Whitespace`/`Error` splits around U+000C are acceptable; anything else means a regression, stop and report.

- [ ] **Step 6: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_syntax/src/lexer/scripting.rs crates/celerrate_syntax/tests/errors.rs
git commit -m "✨ feat(syntax): reject form feed as whitespace, matching Zend"
```

---

### Task 3: Report the interpolation a close tag cuts

`?>` inside an open `{$...}` (or `${...}`) interpolation emits the Zend-faithful token stream, but `set_mode(Mode::InlineHtml)` replaces the interpolation-tagged scripting mode, so the opening offset is lost and no `UnterminatedInterpolation` is reported. End of input in the same situation reports it via `flush_open_modes`. Report the diagnostic in `lex_close_tag` before the mode is replaced. Only the top mode is destroyed by `set_mode`; deeper open constructions stay on the stack and are still reported by `flush_open_modes` at end of input.

**Files:**
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs` (`lex_close_tag`)
- Test: `crates/celerrate_syntax/tests/strings.rs`

**Interfaces:**
- Consumes: `Lexer::current_mode() -> Mode`, `Mode::Scripting { opened_by_interpolation_at }`, `Lexer::diagnose_at`, `LexerDiagnosticKind::UnterminatedInterpolation` (all already imported or importable in `scripting.rs`; `LexerDiagnosticKind` and `Mode` are already in its `use` list).
- Produces: nothing new; behavior only.

- [ ] **Step 1: Write the failing test**

Append to `crates/celerrate_syntax/tests/strings.rs`:

```rust
#[test]
fn a_close_tag_cutting_brace_interpolation_diagnoses_the_opening() {
    // `?>` returns to inline HTML even inside `{$ }` (the stream is
    // Zend-faithful); the interpolation opened at the brace never
    // closes and must be reported exactly once.
    let (_tokens, diagnostics) = lex_verified(r#"<?php "a {$x ?>html"#);
    let openings: Vec<u32> = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.kind == LexerDiagnosticKind::UnterminatedInterpolation
        })
        .map(|diagnostic| u32::from(diagnostic.range.start()))
        .collect();
    assert_eq!(openings, [9]);
    // The double-quoted string is still open too.
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == LexerDiagnosticKind::UnterminatedString)
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_syntax --test strings close_tag_cutting`
Expected: FAIL with `assert_eq!(openings, [9])` seeing an empty vector (the diagnostic is currently lost).

- [ ] **Step 3: Report before replacing the mode**

In `crates/celerrate_syntax/src/lexer/scripting.rs`, replace `lex_close_tag`:

```rust
    fn lex_close_tag(&mut self) {
        self.cursor.bump_bytes(2);
```

becomes:

```rust
    fn lex_close_tag(&mut self) {
        // `?>` also cuts an open `{$` or `${` interpolation. `set_mode`
        // below replaces the tagged mode and would lose its opening
        // offset, so report it now; deeper open constructions stay on
        // the stack for `flush_open_modes`.
        if let Mode::Scripting {
            opened_by_interpolation_at: Some(opening),
        } = self.current_mode()
        {
            self.diagnose_at(LexerDiagnosticKind::UnterminatedInterpolation, opening, 1);
        }
        self.cursor.bump_bytes(2);
```

The rest of the function (newline swallowing, `emit`, `set_mode`) is unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_syntax --test strings`
Expected: PASS, including `unterminated_brace_interpolation_diagnoses_the_opening` (end-of-input path, which must not double-report: the close-tag path and the flush path are mutually exclusive for the same mode entry).

- [ ] **Step 5: Check the snapshot corpus**

Run: `cargo test --package celerrate_syntax --test corpus`
Expected: PASS. If `errors.php` gains a diagnostic line, that is this fix working on corpus content; regenerate with `INSTA_UPDATE=always cargo test --package celerrate_syntax --test corpus`, review the diff (only an added `UnterminatedInterpolation @ ...` line is acceptable), and include the snapshot in the commit.

- [ ] **Step 6: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_syntax/src/lexer/scripting.rs crates/celerrate_syntax/tests/strings.rs crates/celerrate_syntax/tests/snapshots
git commit -m "🐛 fix(syntax): report the interpolation a close tag cuts"
```

(Drop the snapshots path from `git add` if Step 5 changed nothing.)

---

### Task 4: Pinning tests

Pure test additions that pin behavior already implemented. These tests are expected to pass on first run: if any fails, STOP and report BLOCKED with the failure output; that is a real bug, not a test to adjust.

**Files:**
- Modify: `crates/celerrate_syntax/tests/numbers.rs` (text-pin `0bz`/`0oz`)
- Modify: `crates/celerrate_syntax/tests/heredoc.rs` (three edge tests)

**Interfaces:**
- Consumes: `texts` from `tests/support`.
- Produces: nothing; tests only.

- [ ] **Step 1: Text-pin the `0bz` and `0oz` cases**

In `crates/celerrate_syntax/tests/numbers.rs`, inside `a_radix_prefix_without_a_valid_digit_is_a_plain_zero`, replace the two kind-only assertions:

```rust
    assert_eq!(number_kinds("0bz"), [IntegerLiteral, Identifier]);
    assert_eq!(number_kinds("0oz"), [IntegerLiteral, Identifier]);
```

with text assertions that make the token boundaries visible:

```rust
    assert_eq!(
        texts("<?php 0bz 0oz"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "0".to_owned()),
            (Identifier, "bz".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "0".to_owned()),
            (Identifier, "oz".to_owned()),
        ]
    );
```

- [ ] **Step 2: Add the heredoc edge tests**

Append to `crates/celerrate_syntax/tests/heredoc.rs`:

```rust
#[test]
fn crlf_line_endings_work_throughout_a_heredoc() {
    // The header newline (CRLF included) belongs to the start token;
    // a body line's CRLF stays in the fragment before the closer.
    assert_eq!(
        texts("<?php <<<EOT\r\nx\r\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\r\n".to_owned()),
            (StringFragment, "x\r\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}

#[test]
fn a_non_ascii_label_closes_correctly() {
    // Any non-ASCII character is a name character under PHP's
    // byte-oriented rule, so a label like this is valid.
    assert_eq!(
        texts("<?php <<<Été\nx\nÉté"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<Été\n".to_owned()),
            (StringFragment, "x\n".to_owned()),
            (HeredocEnd, "Été".to_owned()),
        ]
    );
}

#[test]
fn an_escaped_newline_still_starts_the_closer_line() {
    // The trailing backslash is literal heredoc content; the newline it
    // precedes still begins the closing-label line, as in Zend.
    assert_eq!(
        texts("<?php <<<EOT\nx\\\nEOT"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (HeredocStart, "<<<EOT\n".to_owned()),
            (StringFragment, "x\\\n".to_owned()),
            (HeredocEnd, "EOT".to_owned()),
        ]
    );
}
```

Watch the label test: the closing line uses the exact same characters as the opening label (`Été` twice, capital É both times).

- [ ] **Step 3: Run the tests, expecting immediate PASS**

Run: `cargo test --package celerrate_syntax --test numbers && cargo test --package celerrate_syntax --test heredoc`
Expected: PASS on first run. Any failure means a real lexer bug: STOP, do not modify production code or weaken the test, report BLOCKED with the output.

- [ ] **Step 4: Verify the heredoc expectations against real PHP**

Run:

```bash
php -r '
$cases = [
    "<?php <<<EOT\r\nx\r\nEOT",
    "<?php <<<\u{c9}t\u{e9}\nx\n\u{c9}t\u{e9}",
    "<?php <<<EOT\nx\\\nEOT",
];
foreach ($cases as $code) {
    foreach (token_get_all($code) as $t) {
        if (is_array($t)) { echo token_name($t[0]), " ", var_export($t[1], true), "\n"; }
        else { echo $t, "\n"; }
    }
    echo "---\n";
}'
```

Expected, for each case in order: `T_START_HEREDOC` covering the header (newline included), `T_ENCAPSED_AND_WHITESPACE` matching the test's `StringFragment` text, and `T_END_HEREDOC` matching the test's `HeredocEnd` text. If `php` is not installed, note it in the report; the fragment boundaries then rest on the part 3 review, which verified the CRLF and escaped-newline cases empirically.

- [ ] **Step 5: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_syntax/tests/numbers.rs crates/celerrate_syntax/tests/heredoc.rs
git commit -m "✅ test(syntax): pin radix fallback texts and heredoc edge cases"
```

---

### Task 5: Documentation and comment polish

No behavior changes: a spec note recording the deliberate divergence, and three comment or readability items from the pull request #3 review.

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-10-foundations-3-lexer-design.md` (new subsection at the end of section 3)
- Modify: `crates/celerrate_syntax/src/syntax_kind.rs` (the `LONGEST_KEYWORD_LENGTH` comment)
- Modify: `crates/celerrate_syntax/src/lexer/scripting.rs` (`cast_at`, `lex_line_comment`)

**Interfaces:**
- Consumes: nothing new.
- Produces: nothing new; documentation and an equivalent-behavior restructure covered by existing cast tests.

- [ ] **Step 1: Add the "Recorded divergences" subsection to the parent spec**

In `.claude/superpowers/specs/2026-07-10-foundations-3-lexer-design.md`, at the end of section 3 (after the "Full PHP 8.1+ grammar, no gating" bullet, before the `## 4. Error handling` heading), insert:

```markdown
### Recorded divergences

One deliberate divergence from Zend's lexer, chosen for error recovery:
binary and octal literals take the maximal digit run. `0b2` and `0o99`
each lex as a single `IntegerLiteral` whose digit validity is judged
semantically; Zend stops at the first invalid digit (`0b2` is the
integer `0` followed by the name `b2`). One token gives the semantic
layer a single literal to attach an invalid-digit diagnostic to,
instead of a confusing name-after-number token pair. The same rule
applies to radix-prefixed offsets in string interpolation
(`"$a[0b2]"`).
```

- [ ] **Step 2: Acknowledge the keyword-length tie**

In `crates/celerrate_syntax/src/syntax_kind.rs`, replace:

```rust
/// The longest PHP keyword is `include_once`: twelve bytes.
const LONGEST_KEYWORD_LENGTH: usize = 12;
```

with:

```rust
/// The longest PHP keywords are `include_once` and `require_once`,
/// tied at twelve bytes.
const LONGEST_KEYWORD_LENGTH: usize = 12;
```

- [ ] **Step 3: Restructure `cast_at` around `strip_prefix`**

In `crates/celerrate_syntax/src/lexer/scripting.rs`, replace the end of `cast_at`:

```rust
    after_trailing.strip_prefix(')')?;
    let kind = cast_kind(word)?;
    let total_length = rest.len() - after_trailing.len() + ')'.len_utf8();
    Some((kind, total_length))
```

with:

```rust
    let after_parenthesis = after_trailing.strip_prefix(')')?;
    let kind = cast_kind(word)?;
    let total_length = rest.len() - after_parenthesis.len();
    Some((kind, total_length))
```

Same arithmetic, but the length now falls out of the remainder that was actually stripped instead of a manual `+ 1` correction.

- [ ] **Step 4: Note why the line-comment gate leaves `?>` unconsumed**

In `crates/celerrate_syntax/src/lexer/scripting.rs`, inside `lex_line_comment`, replace:

```rust
            if self.cursor.rest().starts_with("?>") {
                break;
            }
```

with:

```rust
            if self.cursor.rest().starts_with("?>") {
                // Left unconsumed: the scripting dispatch lexes the
                // close tag itself on the next step.
                break;
            }
```

- [ ] **Step 5: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green; the cast tests in `tests/operators.rs` cover the `cast_at` restructure.

- [ ] **Step 6: Commit in two pieces**

```bash
git add .claude/superpowers/specs/2026-07-10-foundations-3-lexer-design.md crates/celerrate_syntax/src/syntax_kind.rs
git commit -m "📝 docs(syntax): record the maximal-run divergence and the keyword-length tie"
git add crates/celerrate_syntax/src/lexer/scripting.rs
git commit -m "🎨 style(syntax): clarify cast_at arithmetic and the line-comment gate"
```

---

### Task 6: Fuzzing infrastructure

Corpus policy documentation, a seed for the new variable-offset forms, binary caching for `cargo-fuzz` in CI, and crash-artifact upload.

**Files:**
- Modify: `fuzz/.gitignore`
- Create: `fuzz/corpus/lex/seed_variable_offsets.php`
- Modify: `.github/workflows/ci.yml` (the `fuzz` job)

**Interfaces:**
- Consumes: the existing `fuzz` workspace and `lex` fuzz target; the CI `fuzz` job added in pull request #3.
- Produces: nothing code-visible; infrastructure only.

- [ ] **Step 1: Document the corpus policy**

Replace the content of `fuzz/.gitignore` with:

```gitignore
target/
artifacts/
coverage/
Cargo.lock

# corpus/lex holds the committed seed inputs. Local fuzzing writes the
# entries it discovers into the same directory: review them and commit
# the interesting ones deliberately, never wholesale.
```

- [ ] **Step 2: Add the variable-offset seed**

Create `fuzz/corpus/lex/seed_variable_offsets.php` with exactly this content:

```php
<?php echo "$a[0x1F] $a[0b1_0] $a[0o17] $a[-1] $a[key] $a[$k]"; ?>
```

- [ ] **Step 3: Cache the cargo-fuzz binary and upload crash artifacts in CI**

In `.github/workflows/ci.yml`, replace the `fuzz` job's install and run steps:

```yaml
      - run: cargo install cargo-fuzz --locked
      # The +nightly proxy overrides the repository's rust-toolchain.toml
      # pin, which would otherwise route the sanitizer build to stable.
      - run: cargo +nightly fuzz run lex -- -max_total_time=180
```

with:

```yaml
      - name: Cache the cargo-fuzz binary
        id: cargo-fuzz-cache
        uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/cargo-fuzz
          # Bump the key suffix to pick up a newer cargo-fuzz release.
          key: cargo-fuzz-${{ runner.os }}-1
      - if: steps.cargo-fuzz-cache.outputs.cache-hit != 'true'
        run: cargo install cargo-fuzz --locked
      # The +nightly proxy overrides the repository's rust-toolchain.toml
      # pin, which would otherwise route the sanitizer build to stable.
      - run: cargo +nightly fuzz run lex -- -max_total_time=180
      - name: Upload crash artifacts
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: fuzz-artifacts
          path: fuzz/artifacts/
          if-no-files-found: ignore
```

The rest of the job (checkout, nightly toolchain, `Swatinem/rust-cache` with `workspaces: fuzz`) is unchanged.

- [ ] **Step 4: Smoke-run the fuzzer locally with the new seed**

Run: `cargo +nightly fuzz run lex -- -max_total_time=30` (from the repository root)
Expected: exits cleanly after 30 seconds, no crash, no lossless-invariant failure. If the nightly toolchain is missing locally, install it first with `rustup toolchain install nightly`.

Afterwards, discard any locally discovered corpus entries so the commit contains only the seed:

```bash
git status --porcelain fuzz/corpus/
git clean -n fuzz/corpus/   # review what would be removed
git clean -f fuzz/corpus/
```

- [ ] **Step 5: Run the full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green (the fuzz workspace is standalone; this gate covers the main workspace).

- [ ] **Step 6: Commit in two pieces**

```bash
git add fuzz/.gitignore fuzz/corpus/lex/seed_variable_offsets.php
git commit -m "✅ test(fuzz): seed variable offsets and record the corpus policy"
git add .github/workflows/ci.yml
git commit -m "👷 ci(fuzz): cache the cargo-fuzz binary and upload crash artifacts"
```

---

## Final verification

After all tasks: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check && cargo deny check`, all green, on the branch tip.
