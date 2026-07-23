//! Pages for the project notices: `celerrate_project`'s
//! zero-configuration fallbacks (CEL0025 to CEL0029, CEL0039 to
//! CEL0040). Every notice is spanless, exit-code-neutral, and reports
//! a fallback already taken; each `why` names that fallback, because
//! it is the finding's real content.

use crate::explain::ExplainPage;

pub(crate) const CEL0025: ExplainPage = ExplainPage {
    why: "\
No `composer.json` was found at the project root, so autoload
mappings and the PHP version constraint are unknown. Analysis
continues over the whole root with the default supported PHP range,
which is broader than what the project actually targets: version
gating loses precision until a manifest exists.",
    failing_example: "\
//// src/Example.php
<?php
namespace App;

function f(): void {}
",
    fixed_example: "\
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php
namespace App;

function f(): void {}
",
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};

pub(crate) const CEL0026: ExplainPage = ExplainPage {
    why: "\
`composer.json` exists but does not parse as a JSON object (an array,
a scalar, or malformed JSON): its content cannot be read as a
manifest at all. Analysis falls back to the same defaults as a
missing manifest, over the whole project root with the default
supported PHP range, so autoload precision and version gating are
both lost until the manifest is a well-formed object.",
    failing_example: r#"//// composer.json
[]
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    fixed_example: r#"//// composer.json
{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};

pub(crate) const CEL0027: ExplainPage = ExplainPage {
    why: "\
`composer.json` exists but declares no PHP version at all, neither a
`config.platform.php` point release nor a `require.php` constraint.
Analysis falls back to assuming the latest supported stable PHP
version, which is broader than the project's real floor: a symbol
introduced between the project's true minimum and the assumed latest
passes version gating silently, when the project's actual runtime
would have rejected it.",
    failing_example: r#"//// composer.json
{"autoload": {"psr-4": {"App\\": "src/"}}}
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    fixed_example: r#"//// composer.json
{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};

pub(crate) const CEL0028: ExplainPage = ExplainPage {
    why: "\
`composer.json` declares a PHP version constraint that cannot be
parsed, or that admits no version in the supported range. The
constraint is unusable, so analysis falls back to assuming the latest
supported stable PHP version instead, which is broader than whatever
the project's author actually intended by the malformed constraint.",
    failing_example: r#"//// composer.json
{"require": {"php": "not-a-constraint"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    fixed_example: r#"//// composer.json
{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};

pub(crate) const CEL0029: ExplainPage = ExplainPage {
    why: "\
`vendor/composer/installed.json` exists but does not parse as a JSON
object or array of installed packages: the vendor package list cannot
be read at all. Vendor autoload is skipped entirely, so classes,
functions, and constants declared by dependencies are treated as
unknown rather than resolved through the (unreadable) package list.",
    failing_example: r#"//// composer.json
{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// vendor/composer/installed.json
not json
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    fixed_example: r#"//// composer.json
{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}
//// vendor/composer/installed.json
[]
//// src/Example.php
<?php
namespace App;

function f(): void {}
"#,
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};

pub(crate) const CEL0039: ExplainPage = ExplainPage {
    why: "\
`composer.json` exists but could not be read: an IO error other than
not-found, for example the file's permissions deny read access to the
user running Celerrate. This is distinct from a missing manifest,
which is an ordinary zero-configuration case; here a manifest exists
but the tool was refused. Analysis falls back to the same defaults as
a missing manifest, over the whole project root with the default
supported PHP range, but the notice names the underlying error rather
than reporting absence.",
    failing_example: "\
This page's example is authored, not executed by the explain-page
harness: it fires on a permission-based IO error, which cannot be
committed as a fixture and does not reproduce under root or on
Windows CI. Picture a `composer.json` whose file mode denies read
access to the user running Celerrate (`chmod 000 composer.json` on a
POSIX system, while a different user owns the file): the manifest is
present, but every read attempt fails with a permission-denied error
instead of finding no file at all.",
    fixed_example: "\
Restoring read access to the manifest (`chmod 644 composer.json`, or
running Celerrate as a user that can read the file) lets discovery
read it normally: the notice stops firing, and the manifest's own
content, valid or not, decides which of CEL0025 to CEL0028 applies
instead, if any.",
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};

pub(crate) const CEL0040: ExplainPage = ExplainPage {
    why: "\
`vendor/composer/installed.json` exists but could not be read: an IO
error other than not-found, for example the file's permissions deny
read access to the user running Celerrate. This is distinct from a
missing file, which drops vendor autoload silently and legitimately;
here a package list exists but the tool was refused. Vendor autoload
is skipped, exactly as for a missing file, but the notice names the
underlying error rather than dropping the cause silently.",
    failing_example: "\
This page's example is authored, not executed by the explain-page
harness: it fires on a permission-based IO error, which cannot be
committed as a fixture and does not reproduce under root or on
Windows CI. Picture a `vendor/composer/installed.json` whose file
mode denies read access to the user running Celerrate
(`chmod 000 vendor/composer/installed.json` on a POSIX system, while
a different user owns the file): the package list is present, but
every read attempt fails with a permission-denied error instead of
finding no file at all.",
    fixed_example: "\
Restoring read access to the file (`chmod 644
vendor/composer/installed.json`, or running Celerrate as a user that
can read it) lets discovery read it normally: the notice stops
firing, and the file's own content, valid or not, decides whether
CEL0029 applies instead.",
    configuration: "\
A project notice from `celerrate_project`, not a rule: it is neither
disableable nor configurable, and it never affects the exit code.",
};
