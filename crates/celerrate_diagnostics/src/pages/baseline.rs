//! Explain pages for the baseline notices, owned by `celerrate_cli` where
//! the baseline mechanics live.

use crate::explain::ExplainPage;

pub(crate) const CEL0050: ExplainPage = ExplainPage {
    why: "\
A baseline entry records a known finding so it stops failing the run. When \
no current finding matches an entry any longer, whether because the code \
was fixed, the enclosing method was renamed, or an engine upgrade reworded \
the message, the entry is obsolete. Celerrate reports it and never prunes \
silently: re-record with `celerrate check --baseline` to refresh the file.",
    failing_example: "\
//// celerrate-baseline.toml
version = 1

[[entry]]
path = \"src/Example.php\"
identifier = \"CEL0018\"
symbol = \"App\\\\Example\"
message = \"a finding that no longer exists\"
count = 1
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    fixed_example: "\
//// celerrate-baseline.toml
version = 1
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    configuration: "\
This notice is exit-neutral and can be neither disabled nor remapped. \
Re-record with `celerrate check --baseline`, or delete \
`celerrate-baseline.toml` to drop the baseline entirely.",
};

pub(crate) const CEL0051: ExplainPage = ExplainPage {
    why: "\
`celerrate-baseline.toml` exists but could not be fully read: invalid TOML, \
a missing or unsupported version, or a malformed entry. Unreadable entries \
are ignored and their findings are reported: noisy but honest, never \
silent. Valid entries in the same file still apply.",
    failing_example: "\
//// celerrate-baseline.toml
version = 1
[[entry]
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    fixed_example: "\
//// celerrate-baseline.toml
version = 1
//// composer.json
{\"require\": {\"php\": \"^8.1\"}, \"autoload\": {\"psr-4\": {\"App\\\\\": \"src/\"}}}
//// src/Example.php
<?php

namespace App;

class Example
{
}
",
    configuration: "\
This notice is exit-neutral and can be neither disabled nor remapped. Fix \
or re-record the file with `celerrate check --baseline`.",
};
