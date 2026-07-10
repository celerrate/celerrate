# Foundations Part 3 Follow-ups: Lexer Backlog (Design)

Date: 2026-07-10
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-10-foundations-3-lexer-design.md`
Origin: the "Known follow-ups" list recorded in pull request #3

## 1. Goal and scope

Clear the backlog deferred from the lexer pull request before starting the
parser part: three Zend-fidelity behavior changes, a set of pinning tests,
one spec note, comment polish, and fuzzing infrastructure improvements. One
branch, one pull request. Every behavior change follows TDD and is verified
against real PHP (`token_get_all`).

Global constraints are unchanged from the parent spec: zero panic
(mechanically enforced), lossless token stream, guaranteed progress,
determinism, diagnostics beside the stream and never instead of it.

## 2. Behavior changes (Zend fidelity)

### 2.1 Radix-prefixed variable offsets

`lex_variable_offset` currently accepts only decimal digit runs. Zend's
`ST_VAR_OFFSET` state matches `{LNUM}|{HNUM}|{BNUM}|{ONUM}` as
`T_NUM_STRING`: decimal, hexadecimal (`0x`), binary (`0b`), explicit octal
(`0o`), all with `_` digit separators.

The change: inside `$a[...]` in an interpolation, recognize the same
integer literal forms as scripting mode, reusing the existing
radix-prefix helpers (`starts_with_radix_prefix` and the per-radix digit
predicates). The result stays a single `IntegerLiteral` token. Whether the
offset is a valid array index remains a semantic judgment, exactly as in
scripting mode. Float forms do not apply here; Zend has no float rule in
`ST_VAR_OFFSET` and neither do we.

### 2.2 Form feed is not whitespace

The scripting-mode whitespace predicate currently uses
`is_ascii_whitespace`, which includes form feed (U+000C). Zend's
whitespace is exactly space, horizontal tab, `\n`, and `\r`; a form feed
in PHP code is an unexpected-character error.

The change: replace the predicate with the explicit four-character set. A
form feed in scripting mode becomes a one-character `Error` token with an
`UnexpectedCharacter` diagnostic, and lexing continues at the next
character. Inline HTML, string contents, heredoc and nowdoc bodies, and
comments are untouched: there a form feed is ordinary content, as in Zend.

### 2.3 `UnterminatedInterpolation` cut by `?>`

When `?>` appears inside an open `{$...}` interpolation, the lexer today
emits the Zend-faithful token stream (verified against `token_get_all`)
but silently drops the fact that the interpolation never closed. End of
input in the same situation reports `UnterminatedInterpolation` via
`flush_open_modes`.

The change: when a close tag is lexed while interpolation-opened modes are
on the stack, report `UnterminatedInterpolation` for each of them,
pointing at the opening position already stored in the mode (the
`opened_by_interpolation_at` tag), using the same reporting logic as
`flush_open_modes`. The token stream does not change.

## 3. Pinning tests (no behavior change)

- The `0bz` and `0oz` maximal-run tests assert the token text, not only
  the kind and diagnostic, so the deliberate divergence from Zend is
  visible in the test itself.
- Heredoc edge cases, each verified against `token_get_all` when the
  expected stream is written: a CRLF line ending directly after the
  heredoc header, a non-ASCII heredoc label, and an escaped newline
  immediately before a closing marker.

## 4. Documentation

Add a "Recorded divergences" subsection to the parent spec's state-machine
section, documenting the one deliberate divergence: `0b2` and `0o99` lex
as a single maximal `IntegerLiteral` whose digit validity is judged
semantically, where Zend stops the token at the first invalid digit. The
rationale (better recovery, one token to attach a semantic diagnostic to)
is stated there. The form feed change in section 2.2 removes a divergence
rather than adding one, so it needs no entry.

## 5. Code polish (no behavior change)

- `require_once` keyword handling: state in a comment why the
  keyword-length tie is resolved the way it is.
- `cast_at`: restructure the `strip_prefix` chain for readability, same
  behavior, covered by the existing cast tests.
- Line comments: a comment explaining the `?>` gate (a line comment ends
  at `?>` as well as at a newline).

## 6. Fuzzing infrastructure

- `fuzz/.gitignore`: a comment documenting the corpus policy. Seed inputs
  under `corpus/lex/` are tracked; entries discovered by local fuzzing
  runs are reviewed and committed deliberately or discarded, never
  committed blindly.
- CI fuzz job: cache the `cargo-fuzz` binary with `actions/cache` on
  `~/.cargo/bin/cargo-fuzz`, keyed by the installed version, so the
  three-minute fuzz budget is not dwarfed by a cold install. On job
  failure, upload `fuzz/artifacts/` with `actions/upload-artifact` so the
  crashing input is recoverable from the run page.

## 7. Testing

Every behavior change in section 2 starts from a failing test. The
lossless invariant helper applies to all new tests. The snapshot corpus is
extended only if a change alters an existing snapshot (form feed may:
regenerate and review). The fuzz seed corpus gains one seed exercising
radix-prefixed variable offsets.
