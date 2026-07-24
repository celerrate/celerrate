//! The explain pages for the configuration diagnostics (CEL0043 to
//! CEL0049), owned by `celerrate_config`. Configuration errors are
//! span-anchored in `celerrate.toml` and affect the exit code: a
//! typoed configuration silently half-applying would be a #58-class
//! hole, so CI fails loudly instead (CLI product design, section 2).

use crate::explain::ExplainPage;

pub(crate) const CEL0043: ExplainPage = ExplainPage {
    why: "\
`celerrate.toml` exists but cannot be read as TOML (a syntax error,
an encoding problem, or an unreadable file). Analysis continues with
the default configuration, but the file's intent is not applied, so
the mismatch is reported as an error rather than silently ignored.",
    failing_example: "\
//// celerrate.toml
[project
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[project]
php = \"8.2\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`, not a rule: it is
neither disableable nor remappable, and it affects the exit code so CI
never reports success after failing to read its configuration.",
};

pub(crate) const CEL0044: ExplainPage = ExplainPage {
    why: "\
A key `celerrate.toml` uses is not part of the configuration schema.
An unknown key is an error rather than a warning because a typoed key
would otherwise silently configure nothing: the file would look
authoritative while doing nothing at all.",
    failing_example: "\
//// celerrate.toml
[project]
includes = [\"src\"]
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[project]
include = [\"src\"]
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. The v0.1 schema
knows `[project]` (`php`, `include`, `exclude`), `[rules.<name>]`
(`enabled`), `[severity]`, and the reserved `[plugins]` table.",
};

pub(crate) const CEL0045: ExplainPage = ExplainPage {
    why: "\
A known configuration key carries a value of the wrong type or shape:
a `php` value that is not a version point, an `include` entry that is
absolute or empty, a non-boolean `enabled`, or a severity that is not
`\"error\"` or `\"warning\"`. The malformed value is skipped and
reported; the well-formed rest of the file still applies.",
    failing_example: "\
//// celerrate.toml
[project]
php = \"^8.1\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[project]
php = \"8.1\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. In `celerrate.toml`
the `php` key is a version point (`\"8.2\"`), not a Composer-style
constraint: the constraint belongs to `composer.json`. The key is
validated now; a later version applies it to collapse the detected
range to one version.",
};

pub(crate) const CEL0046: ExplainPage = ExplainPage {
    why: "\
A `[rules.<name>]` table names a rule that does not exist. A typoed
rule name must not silently enable or disable nothing: the analysis
would run with a different rule set than the file claims.",
    failing_example: "\
//// celerrate.toml
[rules.nul-dereference]
enabled = false
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[rules.null-dereference]
enabled = false
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. `[rules.<name>]`
validates the name now; a later version applies it to activate the
rule, while per-identifier severity lives under `[severity]`. Rule
names are the stable kebab-case names published in the rule-name table
of the identifier reference:
https://github.com/celerrate/celerrate/blob/main/docs/diagnostics.md",
};

pub(crate) const CEL0047: ExplainPage = ExplainPage {
    why: "\
A `[rules.<name>]` table sets a key other than `enabled`. No shipped
rule has configurable options yet, so any other key would be silently
dead configuration; when parameterized rules arrive, their options
become sibling keys of `enabled` in this same table.",
    failing_example: "\
//// celerrate.toml
[rules.null-dereference]
max = 3
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[rules.null-dereference]
enabled = false
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. `enabled` is the
only recognized rule key in v0.1.",
};

pub(crate) const CEL0048: ExplainPage = ExplainPage {
    why: "\
A `[severity]` entry names a diagnostic identifier the registry does
not know. A typoed identifier must not silently remap nothing: the
severity the file claims and the severity the run uses would diverge
invisibly.",
    failing_example: "\
//// celerrate.toml
[severity]
\"CEL9999\" = \"warning\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[severity]
\"CEL0034\" = \"warning\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. Valid identifiers
are the `CEL####` codes of `celerrate explain`; only identifiers a
rule may emit can be remapped.",
};

pub(crate) const CEL0049: ExplainPage = ExplainPage {
    why: "\
A `[severity]` entry names a resilience diagnostic: a parse error, a
decode failure, or a project notice. Those are neither disableable nor
remappable by design, because they report the tool's own degraded
sight, not a property of the code under analysis.",
    failing_example: "\
//// celerrate.toml
[severity]
\"CEL0026\" = \"error\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    fixed_example: "\
//// celerrate.toml
[severity]
\"CEL0034\" = \"warning\"
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function example(): void {}
",
    configuration: "\
A configuration diagnostic from `celerrate_config`. Remappable
identifiers are exactly the ones the core rules may emit; everything
else in the registry is resilience.",
};
