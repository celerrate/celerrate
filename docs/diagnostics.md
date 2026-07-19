# Diagnostics

Every diagnostic Celerrate emits carries a `CEL####` identifier.
Identifiers are permanent: once published in a release an identifier
keeps its meaning forever, a retired one is never reused, and a new
diagnostic takes the next free number. This page is the identifier
reference until the rule framework ships `celerrate explain` pages.

A report line looks like:

```text
src/Controller/PostController.php:42:19 CEL0018 unknown class `App\Service\Mailer`
```

Severity is reporting weight, not exit behavior: `celerrate check`
exits 1 as soon as it reports any diagnostic, **error** and
**warning** alike. The project discovery notices
(`CEL0025` to `CEL0029`, `CEL0039`, `CEL0040`) are counted separately
in the summary line and do not affect the exit code.

To silence a single occurrence, use an inline suppression
(`@phpstan-ignore-line`, `@phpstan-ignore-next-line`,
`@phpstan-ignore`, or `@psalm-suppress`): see
[the PHPDoc bridge](phpdoc-bridge.md#suppressions). There is no
configuration file or baseline yet; inline suppression is the only
per-site switch in this preview.

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
v0.0.3. Deliberately conservative: a `mixed`, `object`, or otherwise
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
