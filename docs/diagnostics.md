# Diagnostics

Every diagnostic Celerrate emits carries a `CEL####` identifier.
Identifiers are permanent: once published in a release an identifier
keeps its meaning forever, a retired one is never reused, and a new
diagnostic takes the next free number. This page is the identifier
reference. Every identifier below also ships an embedded page:
`celerrate explain CEL0030` prints why it fires, a failing and a fixed
example, and its configuration notes.

A reported diagnostic looks like:

```text
error[CEL0018]: unknown class `App\Service\Mailer`
 --> src/PostController.php:7:41
  |
7 |     public function __construct(private App\Service\Mailer $mailer)
  |                                         ^^^^^^^^^^^^^^^^^^

0 notices, 1 diagnostic

for more information, run `celerrate explain CEL0018`
```

Severity is reporting weight, not exit behavior: `celerrate check`
exits 1 as soon as it reports any diagnostic, **error** and
**warning** alike. The project discovery notices
(`CEL0025` to `CEL0029`, `CEL0039`, `CEL0040`) and the baseline
notices (`CEL0050`, `CEL0051`) are counted separately in the summary
line and do not affect the exit code.

## Suppressing diagnostics

To silence a single occurrence, use an inline suppression comment.
`celerrate.toml` is read, validated, and applied (see
[Configuration](#configuration-cel0043-to-cel0049) below). A present
`celerrate-baseline.toml` at the project root hides its recorded
findings from the report and the exit code; `celerrate check
--baseline` records or refreshes it from the current findings, and
`celerrate check --ignore-baseline` runs strict, ignoring the file
entirely. See [Baseline notices](#baseline-notices-cel0050-cel0051)
below.

Celerrate's own directive is `@celerrate-ignore`, written in a line
comment, a block comment, or a docblock:

```text
// @celerrate-ignore CEL0030, CEL0031 (reason)
```

Its identifiers are mandatory: there is no blanket form, so a bare
`@celerrate-ignore` parses but suppresses nothing (a mistake worth
flagging on its own, rather than silently widening). The optional
parenthesized trailer after the identifiers is a reason for the
suppression; it is not otherwise interpreted.

All identifiers must sit on the same physical line as the tag: the
parser reads the identifier list up to the end of the tag's own
line, so wrapping the list onto a continuation line of a block
comment or docblock silently drops the identifiers left on that
continuation line, and the directive still applies, it just protects
fewer codes than written. For example, in

```text
/**
 * @celerrate-ignore CEL0030,
 * CEL0031 (reason)
 */
```

only `CEL0030` is suppressed; `CEL0031` is not. Either keep the
whole list on the tag's line, or repeat the tag on its own line:

```text
/**
 * @celerrate-ignore CEL0030 (reason)
 * @celerrate-ignore CEL0031 (reason)
 */
```

The scope depends on where the directive sits:

- Trailing a line of code (in any of the three comment kinds), it
  covers that line.
- Alone on its own line, it covers the line that follows, or, when
  there is no next line (the directive sits on the file's last line),
  the end-of-file position; such a directive is then reported unused.
- In a docblock, it covers the declaration the docblock annotates.

For the foreign dialects `@phpstan-ignore-line`,
`@phpstan-ignore-next-line`, `@phpstan-ignore`, and `@psalm-suppress`,
see [the PHPDoc bridge](phpdoc-bridge.md#suppressions).

## Syntax (CEL0001 to CEL0017)

Produced while reading and parsing source files. Parsing is error
resilient: a syntax diagnostic never stops the analysis of the rest
of the file or the project.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0001 | error | source too large: the file exceeds the analyzable size limit |
| CEL0002 | error | unexpected character |
| CEL0003 | error | unterminated block comment |
| CEL0004 | error | unterminated string |
| CEL0005 | error | unterminated heredoc |
| CEL0006 | error | unterminated interpolation |
| CEL0007 | error | expected an expression |
| CEL0008 | error | expected a semicolon |
| CEL0009 | error | expected a specific token |
| CEL0010 | error | unexpected token |
| CEL0011 | error | nesting too deep |
| CEL0012 | error | non-associative operator chained |
| CEL0013 | error | the parser made no progress (an internal guard, never expected on real input) |
| CEL0014 | error | expected a member name |
| CEL0015 | error | expected a statement |
| CEL0016 | error | expected a type |
| CEL0017 | error | expected a declaration |

## Unknown symbols (CEL0018 to CEL0020)

References that resolve nowhere, with the project, its Composer
dependencies, and the bundled PHP stubs all considered.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0018 | error | unknown class |
| CEL0019 | error | unknown function |
| CEL0020 | error | unknown constant |

## PHP version gating (CEL0021 to CEL0024)

Symbols or syntax used outside the PHP version range the project's
`composer.json` declares.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0021 | error | the symbol is not available in the project's minimum PHP version |
| CEL0022 | error | the symbol was removed before the project's maximum PHP version |
| CEL0023 | warning | the symbol is deprecated within the project's version range |
| CEL0024 | error | the syntax construct is not available in the project's minimum PHP version |

## Project discovery notices (CEL0025 to CEL0029, CEL0039, CEL0040)

About the project's own configuration, reported once per run.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0025 | warning | no `composer.json` found; the whole project root is analyzed |
| CEL0026 | warning | `composer.json` exists but is not a JSON object; defaults are used |
| CEL0027 | warning | no PHP version configured; the latest supported stable version (currently PHP 8.5) is assumed |
| CEL0028 | warning | the PHP version constraint is unusable (unparseable, or admitting no supported version); the latest supported stable version is assumed |
| CEL0029 | warning | `vendor/composer/installed.json` is not a JSON object; installed packages are not indexed |
| CEL0039 | warning | `composer.json` exists but could not be read (an IO error other than not-found, named in the message); the whole project root is analyzed |
| CEL0040 | warning | `vendor/composer/installed.json` exists but could not be read (an IO error other than not-found, named in the message); installed packages are not indexed |

## Unknown members (CEL0030 to CEL0033)

Members that do not exist on the receiver's resolved type, new in
v0.1.0. Deliberately conservative: a `mixed`, `object`, or otherwise
dynamic receiver is silent; magic methods suppress their own kind
(`__get`/`__set` for properties, `__call` for methods, `__callStatic`
for static methods), directly or by inheritance; `stdClass` and
`#[AllowDynamicProperties]` classes never report unknown properties;
members declared by `@property` or `@method` docblocks count as
existing; on a union type the member must be missing on every
non-null constituent before anything is reported.

| Identifier | Severity | Meaning | Message shape |
| --- | --- | --- | --- |
| CEL0030 | error | unknown method | ``unknown method `save` on `App\User` `` |
| CEL0031 | error | unknown property | ``unknown property `$name` on `App\User` `` |
| CEL0032 | error | unknown class constant | as above, for constants |
| CEL0033 | error | unknown enum case | as above, for enum cases |

## Nullability (CEL0034)

A method call or property access on a value that may be `null` at
that point. Flow narrowing decides what is still nullable:
`instanceof`, `null` comparisons, `isset()`/`empty()`, the `is_*`
family, truthiness, negation and boolean composition, `??`/`??=`,
`?->` chains (one null receiver short-circuits the whole chain),
`match`, `switch`, early returns, `assert()`, and assertion
annotations (`@phpstan-assert`, non-divergent `@psalm-assert`) are
all honored.

| Identifier | Severity | Meaning | Message shape |
| --- | --- | --- | --- |
| CEL0034 | error | possibly null dereference | ``accessing `save` on a possibly null `App\User|null` `` |

## Argument types (CEL0035 to CEL0038)

Each argument checked against its parameter, plus arity, named
arguments included. `mixed` passes everywhere. Coercion follows the
calling file's declared mode: under `declare(strict_types=1)` the
check is strict; in a weak-mode file, coercions PHP performs at
runtime are not reported. Argument unpacking of a value whose shape
is unknown silences arity for that call.

| Identifier | Severity | Meaning | Message shape |
| --- | --- | --- | --- |
| CEL0035 | error | argument type mismatch | ``argument 2 of `substr` expects `int`, `string` given`` |
| CEL0036 | error | too few arguments | a required parameter is bound neither positionally nor by name |
| CEL0037 | error | too many arguments | more positional arguments than parameters, no variadic |
| CEL0038 | error | unknown named argument | a named argument matches no declared parameter name |

## Suppression directives (CEL0041, CEL0042)

About Celerrate's own `@celerrate-ignore` directive (never about
foreign directives, which legitimately target diagnostics Celerrate
does not emit).

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0041 | warning | a `@celerrate-ignore` directive names an identifier Celerrate does not know, so a typo cannot silently suppress nothing |
| CEL0042 | warning | a `@celerrate-ignore` directive suppressed nothing (exempt when it names an identifier of a rule not active in this run, or an unknown identifier - that mistake is already CEL0041's) |

## Configuration (CEL0043 to CEL0049)

See [Configuration](configuration.md) for the full `celerrate.toml`
reference: discovery, the `[project]`, `[rules]`, and `[severity]`
tables, and what is deliberately not configurable.

About `celerrate.toml` itself, read from the project root next to
`composer.json`; a missing file is not an event, because zero
configuration is the contract. Each is span-anchored, is an error,
counts toward the exit code, and is neither disableable nor remappable.
A typoed configuration fails CI while that same run analyzes with the
default configuration. CEL0043 drops the whole file; the others skip
only the malformed part.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0043 | error | `celerrate.toml` exists but cannot be read as TOML (a syntax error, an encoding problem, or an IO error); the default configuration is used |
| CEL0044 | error | a key outside the schema, anywhere in the file |
| CEL0045 | error | a known key whose value has the wrong type or shape |
| CEL0046 | error | a `[rules.<name>]` table naming a rule that does not exist |
| CEL0047 | error | a `[rules.<name>]` key other than `enabled`: no shipped rule takes options yet |
| CEL0048 | error | a `[severity]` key naming an identifier the registry does not know |
| CEL0049 | error | a `[severity]` key naming a resilience diagnostic, whose severity is not the user's to move |

### Rule names

These are the names `[rules.<name>]` accepts in `celerrate.toml`. A
`[rules.<name>]` table naming anything else is CEL0046.

| Rule name |
| --- |
| argument-checks |
| null-dereference |
| symbol-version-gating |
| syntax-version-gating |
| unknown-members |
| unknown-suppression-identifier |
| unknown-symbols |
| unused-suppression |

## Baseline notices (CEL0050, CEL0051)

About `celerrate-baseline.toml` itself, the file `celerrate check
--baseline` records at the project root next to `composer.json`.
Recording writes one structural entry per known finding: a
project-relative path, the diagnostic identifier, the enclosing
symbol path, the rendered message, and an occurrence count. There is
no line number in an entry, so it survives ordinary code motion, and
it applies only to its own finding, so it dies (and is reported
obsolete) the moment that finding stops recurring. The count caps how
many occurrences an entry absorbs: occurrence `count + 1` is never
masked and is still reported as new. Both notices below are
exit-neutral, project-anchored (there is no span to suppress), and
counted separately in the summary line, like the project discovery
notices above.

| Identifier | When it fires | What to do |
| --- | --- | --- |
| CEL0050 | a recorded entry counts more occurrences than the current findings still produce, whether all of them or only some (the code was fixed, the enclosing method was renamed, or an engine upgrade reworded the message) | re-record with `celerrate check --baseline` to refresh the file |
| CEL0051 | `celerrate-baseline.toml` exists but could not be fully read (invalid TOML, a missing or unsupported version, or a malformed entry); unreadable entries are ignored and their findings reported, while valid entries in the same file still apply | fix the file by hand, or re-record it with `celerrate check --baseline` |

See [Baseline](baseline.md) for recording, applying, the file format,
and the invariants and failure modes behind these two notices.
