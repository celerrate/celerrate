//! Pages for the typed families: unknown members (CEL0030 to
//! CEL0033), null dereference (CEL0034), and argument checks
//! (CEL0035 to CEL0038).

use crate::explain::ExplainPage;

pub(crate) const CEL0030: ExplainPage = ExplainPage {
    why: "\
The receiver's inferred type declares no such method, in the project,
its ancestors, or the stubs for the supported PHP range. At runtime
the call throws an `Error` (call to undefined method) unless a magic
`__call` intercepts it; classes with magic methods are already
exempted conservatively, so what remains is a genuine typo or a
renamed member.",
    failing_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(User $u): void { $u->svae(); }
",
    fixed_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(User $u): void { $u->save(); }
",
    configuration: "\
Reported by the `unknown-members` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0030 (reason)` on or above the line.",
};

pub(crate) const CEL0031: ExplainPage = ExplainPage {
    why: "\
The receiver's inferred type declares no such property, in the
project, its ancestors, or the stubs for the supported PHP range.
PHP does not fail loudly here: the read raises an `E_WARNING`
(undefined property) and evaluates to `null`, unless a magic `__get`
intercepts it; classes with magic accessors are already exempted
conservatively, so what remains is a genuine typo or a renamed
member that silently loses its value to `null`.",
    failing_example: "\
<?php
namespace App;

class User { public string $name = ''; }

function f(User $u): void { $x = $u->nmae; }
",
    fixed_example: "\
<?php
namespace App;

class User { public string $name = ''; }

function f(User $u): void { $x = $u->name; }
",
    configuration: "\
Reported by the `unknown-members` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0031 (reason)` on or above the line.",
};

pub(crate) const CEL0032: ExplainPage = ExplainPage {
    why: "\
The receiver's inferred type declares no such class constant, in the
project, its ancestors, or the stubs for the supported PHP range.
Unlike property or method access, PHP has no magic interception for
constants: at runtime the reference throws an `Error` (undefined
constant), so the code path cannot execute at all.",
    failing_example: "\
<?php
namespace App;

class Config { public const LIMIT = 10; }

function f(): int { return Config::LIMTI; }
",
    fixed_example: "\
<?php
namespace App;

class Config { public const LIMIT = 10; }

function f(): int { return Config::LIMIT; }
",
    configuration: "\
Reported by the `unknown-members` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0032 (reason)` on or above the line.",
};

pub(crate) const CEL0033: ExplainPage = ExplainPage {
    why: "\
The receiver's inferred enum type declares no such case, in the
project or the stubs for the supported PHP range. Enum cases are
class constants under the engine's hood and carry no magic
interception: at runtime the reference throws an `Error` (undefined
constant), so the code path cannot execute at all.",
    failing_example: "\
<?php
namespace App;

enum Status { case Active; }

function f(): Status { return Status::Draft; }
",
    fixed_example: "\
<?php
namespace App;

enum Status { case Active; }

function f(): Status { return Status::Active; }
",
    configuration: "\
Reported by the `unknown-members` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0033 (reason)` on or above the line.",
};

pub(crate) const CEL0034: ExplainPage = ExplainPage {
    why: "\
The receiver's inferred type explicitly contains `null`, and the
member access is not guarded by a narrowing check (an `!== null`
test, an `instanceof`, an early return, or the null-safe `?->`
operator) before it happens. At runtime the access throws an `Error`
(call to a member function on null) whenever the value actually is
`null`, so the code path fails for every caller that passes the
missing case.",
    failing_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(?User $u): void { $u->save(); }
",
    fixed_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(?User $u): void {
    if ($u !== null) {
        $u->save();
    }
}
",
    configuration: "\
Reported by the `null-dereference` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0034 (reason)` on or above the line.",
};

pub(crate) const CEL0035: ExplainPage = ExplainPage {
    why: "\
An argument's inferred type is not assignable to its parameter's
declared type. Under `declare(strict_types=1)` (or whenever no
coercion exists between the two types at all) the call throws a
`TypeError` at runtime, so the code path cannot execute with that
argument.",
    failing_example: "\
<?php
declare(strict_types=1);
namespace App;

class Plain {}
function takes(int $n): void {}

function f(Plain $p): void { takes($p); }
",
    fixed_example: "\
<?php
declare(strict_types=1);
namespace App;

class Plain {}
function takes(int $n): void {}

function f(Plain $p): void { takes(1); }
",
    configuration: "\
Reported by the `argument-checks` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0035 (reason)` on or above the line.",
};

pub(crate) const CEL0036: ExplainPage = ExplainPage {
    why: "\
The call binds fewer arguments than the signature has required
parameters. At runtime PHP throws an `ArgumentCountError` (too few
arguments) before the function body runs at all, so the call fails
unconditionally for every caller that omits the missing argument.",
    failing_example: "\
<?php
namespace App;

function pair(int $a, int $b): void {}

function f(): void { pair(1); }
",
    fixed_example: "\
<?php
namespace App;

function pair(int $a, int $b): void {}

function f(): void { pair(1, 2); }
",
    configuration: "\
Reported by the `argument-checks` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0036 (reason)` on or above the line.",
};

pub(crate) const CEL0037: ExplainPage = ExplainPage {
    why: "\
The call binds more positional arguments than the signature accepts.
PHP does not fail this at runtime: without a variadic parameter the
excess arguments are silently discarded (recoverable only through
`func_get_args()`), so the call keeps running with the extra values
quietly dropped instead of reaching the parameter the caller
presumably intended.",
    failing_example: "\
<?php
namespace App;

function single(int $a): void {}

function f(): void { single(1, 2); }
",
    fixed_example: "\
<?php
namespace App;

function single(int $a): void {}

function f(): void { single(1); }
",
    configuration: "\
Reported by the `argument-checks` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0037 (reason)` on or above the line.",
};

pub(crate) const CEL0038: ExplainPage = ExplainPage {
    why: "\
The call passes a named argument whose name matches no declared
parameter. At runtime PHP throws an `Error` (unknown named
parameter) before the function body runs at all, so the call fails
unconditionally; the required parameter the name was meant to bind
is also left unfilled, which is why this defect often surfaces
alongside a too-few-arguments finding on the same call.",
    failing_example: "\
<?php
namespace App;

function single(int $a): void {}

function f(): void { single(b: 1); }
",
    fixed_example: "\
<?php
namespace App;

function single(int $a): void {}

function f(): void { single(a: 1); }
",
    configuration: "\
Reported by the `argument-checks` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0038 (reason)` on or above the line.",
};
