//! The argument-check family, migrated (design section 5): arity
//! matching and assignability judgments live below in
//! `celerrate_types::checks::arguments`; this rule consumes the
//! per-body outcome records and constructs the diagnostics.

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_types::{ArgumentLabel, TypedBodyContext, TypedVerdictKind};

use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::TypedBodyRule;

/// An argument fails assignability against its parameter.
pub const ARGUMENT_TYPE: DiagnosticId = DiagnosticId::new("CEL0035");
/// A required parameter is bound by no argument.
pub const TOO_FEW_ARGUMENTS: DiagnosticId = DiagnosticId::new("CEL0036");
/// More positional arguments than the signature accepts.
pub const TOO_MANY_ARGUMENTS: DiagnosticId = DiagnosticId::new("CEL0037");
/// A named argument matching no declared parameter.
pub const UNKNOWN_NAMED_ARGUMENT: DiagnosticId = DiagnosticId::new("CEL0038");

/// The rule that reports calls whose arguments fail the declared
/// signature: a failed assignability, a missing or excess argument,
/// or an unknown argument name.
pub struct ArgumentChecks;

/// The family's declarative unit.
pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "argument-checks".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![
            RuleIdentifier {
                id: ARGUMENT_TYPE,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: TOO_FEW_ARGUMENTS,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: TOO_MANY_ARGUMENTS,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: UNKNOWN_NAMED_ARGUMENT,
                severity: Severity::Error,
            },
        ],
        tier: Tier::Default,
    }
}

impl TypedBodyRule for ArgumentChecks {
    fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>) {
        for verdict in context.verdicts() {
            // Exhaustive by design, never a `_` arm: a verdict kind
            // added to the walk below must fail to compile here rather
            // than be computed and then silently dropped by every rule.
            let (id, message) = match &verdict.kind {
                TypedVerdictKind::ArgumentType {
                    label,
                    callee,
                    expected,
                    given,
                } => (
                    ARGUMENT_TYPE,
                    match label {
                        ArgumentLabel::Positional(position) => format!(
                            "argument {position} of `{callee}` expects `{expected}`, `{given}` given"
                        ),
                        ArgumentLabel::Named(name) => format!(
                            "argument `${name}` of `{callee}` expects `{expected}`, `{given}` given"
                        ),
                    },
                ),
                TypedVerdictKind::TooFewArguments {
                    callee,
                    given,
                    required,
                } => (
                    TOO_FEW_ARGUMENTS,
                    format!("too few arguments to `{callee}`: {given} given, {required} required"),
                ),
                TypedVerdictKind::TooManyArguments {
                    callee,
                    given,
                    accepted,
                } => (
                    TOO_MANY_ARGUMENTS,
                    format!(
                        "too many arguments to `{callee}`: {given} given, at most {accepted} accepted"
                    ),
                ),
                TypedVerdictKind::UnknownNamedArgument { callee, name } => (
                    UNKNOWN_NAMED_ARGUMENT,
                    format!("unknown named argument `${name}` on `{callee}`"),
                ),
                // `unknown-members` renders the first four; the last is
                // `null-dereference`'s.
                TypedVerdictKind::UnknownMethod { .. }
                | TypedVerdictKind::UnknownProperty { .. }
                | TypedVerdictKind::UnknownClassConstant { .. }
                | TypedVerdictKind::UnknownEnumCase { .. }
                | TypedVerdictKind::NullDereference { .. } => continue,
            };
            sink.report(
                id,
                FindingAnchor::Expression {
                    body: verdict.body,
                    expression: verdict.expression,
                },
                message,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_diagnostics::Severity;

    use super::{ARGUMENT_TYPE, TOO_FEW_ARGUMENTS, TOO_MANY_ARGUMENTS, UNKNOWN_NAMED_ARGUMENT};
    use crate::rules::test_support::typed_body_diagnostics;

    #[test]
    fn a_positional_argument_type_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
declare(strict_types=1);
class Plain {}
function takes(int $n): void {}
function f(Plain $p): void { takes($p); }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, ARGUMENT_TYPE);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "argument 1 of `takes` expects `int`, `Plain` given"
        );
    }

    /// The named spelling of the same identifier: `$s` addresses the
    /// parameter by name, so the message names it rather than a
    /// position.
    #[test]
    fn a_named_argument_type_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
declare(strict_types=1);
function takes(int $n, string $s = ''): void {}
function f(): void { takes(1, s: 42); }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, ARGUMENT_TYPE);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "argument `$s` of `takes` expects `string`, `42` given"
        );
    }

    #[test]
    fn a_missing_argument_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
function pair(int $a, int $b): void {}
function f(): void { pair(1); }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, TOO_FEW_ARGUMENTS);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "too few arguments to `pair`: 1 given, 2 required"
        );
    }

    #[test]
    fn an_excess_argument_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
function single(int $a): void {}
function f(): void { single(1, 2); }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, TOO_MANY_ARGUMENTS);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "too many arguments to `single`: 2 given, at most 1 accepted"
        );
    }

    /// `single(b: 1)` fires two of the family's identifiers at one
    /// call, and the test pins both: the name matches no parameter
    /// (CEL0038), which also leaves the required `$a` bound by nothing
    /// (CEL0036). Both anchor to the same call expression, so the
    /// diagnostic total order puts the lower identifier first.
    #[test]
    fn an_unknown_named_argument_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
function single(int $a): void {}
function f(): void { single(b: 1); }
"#]);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert_eq!(diagnostics[0].id, TOO_FEW_ARGUMENTS);
        assert_eq!(diagnostics[1].id, UNKNOWN_NAMED_ARGUMENT);
        assert_eq!(diagnostics[1].severity, Severity::Error);
        assert_eq!(
            diagnostics[1].message,
            "unknown named argument `$b` on `single`"
        );
    }
}
