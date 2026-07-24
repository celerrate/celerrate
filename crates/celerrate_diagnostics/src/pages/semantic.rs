//! Pages for the semantic families: unknown symbols (CEL0018 to
//! CEL0020) and version gating (CEL0021 to CEL0024).

use crate::explain::ExplainPage;

pub(crate) const CEL0018: ExplainPage = ExplainPage {
    why: "\
The referenced class does not exist under any name the project can
resolve: it is neither declared in the project, nor autoloadable
through Composer, nor part of the PHP distribution for the supported
version range. At runtime the reference throws an `Error` (class not
found), so the code path cannot execute at all.",
    failing_example: "\
<?php
namespace App;

function f(): void { $x = new MissingService(); }
",
    fixed_example: "\
<?php
namespace App;

class MissingService {}

function f(): void { $x = new MissingService(); }
",
    configuration: "\
Reported by the `unknown-symbols` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0018 (reason)` on or above the line.",
};

pub(crate) const CEL0019: ExplainPage = ExplainPage {
    why: "\
The referenced function does not exist under any name the project can
resolve: it is neither declared in the project, nor autoloadable
through Composer, nor part of the PHP distribution for the supported
version range. At runtime the call throws an `Error` (call to
undefined function), so the code path cannot execute at all.",
    failing_example: "\
<?php
namespace App;

function f(): void { missing_helper(); }
",
    fixed_example: "\
<?php
namespace App;

function missing_helper(): void {}

function f(): void { missing_helper(); }
",
    configuration: "\
Reported by the `unknown-symbols` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0019 (reason)` on or above the line.",
};

pub(crate) const CEL0020: ExplainPage = ExplainPage {
    why: "\
The referenced constant does not exist under any name the project can
resolve: it is neither declared in the project, nor autoloadable
through Composer, nor part of the PHP distribution for the supported
version range. At runtime the reference throws an `Error` (undefined
constant), so the code path cannot execute at all.",
    failing_example: "\
<?php
namespace App;

function f(): int { return MISSING_LIMIT; }
",
    fixed_example: "\
<?php
namespace App;

const MISSING_LIMIT = 10;

function f(): int { return MISSING_LIMIT; }
",
    configuration: "\
Reported by the `unknown-symbols` rule (correctness group, default
tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0020 (reason)` on or above the line.",
};

pub(crate) const CEL0021: ExplainPage = ExplainPage {
    why: "\
The referenced symbol exists in the PHP distribution, but only from a
version later than the project's declared minimum. `json_validate`
exists only from PHP 8.3; a project whose manifest floor is `^8.1`
admits PHP versions where the call fails at runtime with an
undefined-function error, because the symbol is not yet part of the
running engine on those versions.",
    failing_example: r#"<?php
namespace App;

function f(): bool { return \json_validate('{}'); }
"#,
    fixed_example: r#"//// composer.json
{"require": {"php": "^8.3"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// src/Example.php
<?php
namespace App;

function f(): bool { return \json_validate('{}'); }
"#,
    configuration: "\
Reported by the `symbol-version-gating` rule (correctness group,
default tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0021 (reason)` on or above the line.",
};

pub(crate) const CEL0022: ExplainPage = ExplainPage {
    why: "\
The referenced symbol exists in the PHP distribution, but was removed
at or before the project's declared maximum. A project whose supported
range spans the removal (for example `each()`, removed in PHP 8.0)
still admits versions where the call has already been dropped, and it
fails at runtime with an undefined-function error on those versions.
This page's example is authored, not executed by the explain-page
harness: the shipped stub blob carries no removal inside the currently
supported window, so no product-pipeline fixture can fire it (its
recall coverage lives in the framework-path test in
`celerrate_rules::rules::symbol_version_gating`).",
    failing_example: r#"<?php
namespace App;

// Assumes a manifest whose supported range spans PHP 8.0, e.g.
// "php": ">=7.4 <=8.0" — `each()` was removed in PHP 8.0.
function f(array $items): void {
    while ($pair = each($items)) {
        [$key, $value] = $pair;
    }
}
"#,
    fixed_example: r#"<?php
namespace App;

function f(array $items): void {
    foreach ($items as $key => $value) {
    }
}
"#,
    configuration: "\
Reported by the `symbol-version-gating` rule (correctness group,
default tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0022 (reason)` on or above the line.",
};

pub(crate) const CEL0023: ExplainPage = ExplainPage {
    why: "\
The referenced symbol still exists in the PHP distribution, but
carries a deprecation covering the project's declared version range.
Calling it does not stop execution, but the engine emits a runtime
deprecation notice (`E_DEPRECATED`) on every call, and a later PHP
release that removes the symbol turns this warning into a hard
failure. `utf8_encode` is deprecated since PHP 8.2.",
    failing_example: r#"<?php
namespace App;

function f(): void { \utf8_encode('x'); }
"#,
    fixed_example: r#"<?php
namespace App;

function f(): void { \mb_convert_encoding('x', 'UTF-8', 'ISO-8859-1'); }
"#,
    configuration: "\
Reported by the `symbol-version-gating` rule (correctness group,
default tier) as a warning. Suppress one occurrence with
`// @celerrate-ignore CEL0023 (reason)` on or above the line.",
};

pub(crate) const CEL0024: ExplainPage = ExplainPage {
    why: "\
The syntax construct is only available from a version later than the
project's declared minimum. Readonly classes exist only from PHP 8.2;
a project whose manifest floor is `^8.1` admits PHP versions where the
parser rejects the declaration outright, so the file fails to load at
all on those versions.",
    failing_example: "\
<?php
namespace App;

readonly class Point {}
",
    fixed_example: r#"//// composer.json
{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// src/Example.php
<?php
namespace App;

readonly class Point {}
"#,
    configuration: "\
Reported by the `syntax-version-gating` rule (correctness group,
default tier) as an error. Suppress one occurrence with
`// @celerrate-ignore CEL0024 (reason)` on or above the line.",
};
