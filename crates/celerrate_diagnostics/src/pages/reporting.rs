//! Pages for the reporting rules: findings about suppression
//! directives themselves (CEL0041 to CEL0042).

use crate::explain::ExplainPage;

pub(crate) const CEL0041: ExplainPage = ExplainPage {
    why: "\
A suppression directive naming an identifier the tool does not know
suppresses nothing: a typo in a CEL code would otherwise silently
leave the directive inert while looking intentional. A known but
currently inactive identifier is not unknown.",
    failing_example: "\
<?php
namespace App;

// @celerrate-ignore CEL9999
function f(): void {}
",
    fixed_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(User $u): void {
    // @celerrate-ignore CEL0030 (renamed upstream, fix scheduled)
    $u->svae();
}
",
    configuration: "\
Reported by the `unknown-suppression-identifier` rule (correctness
group, default tier) as a warning, on native `@celerrate-ignore`
directives only; foreign directives (PHPStan, Psalm) legitimately
name identifiers Celerrate does not emit.",
};

pub(crate) const CEL0042: ExplainPage = ExplainPage {
    why: "\
A `@celerrate-ignore` directive that matched no finding at the line it
covers is not harmless: the finding it once suppressed is gone, but
the directive stays behind and now hides the next real finding that
lands at the same site. A suppression that outlives its finding
silently drifts from a deliberate waiver into a blind spot.",
    failing_example: "\
<?php
namespace App;

// @celerrate-ignore CEL0030
function f(): void {}
",
    fixed_example: "\
<?php
namespace App;

class User { public function save(): void {} }

function f(User $u): void {
    // @celerrate-ignore CEL0030 (renamed upstream, fix scheduled)
    $u->svae();
}
",
    configuration: "\
Reported by the `unused-suppression` rule (correctness group, default
tier) as a warning, on native `@celerrate-ignore` directives only. A
directive is exempt (not evaluable) when any identifier it names
belongs to an inactive rule (a demoted rule must not turn every
directive that names it into a finding), or is unknown, since CEL0041
already reports that mistake.",
};
