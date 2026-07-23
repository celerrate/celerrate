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
