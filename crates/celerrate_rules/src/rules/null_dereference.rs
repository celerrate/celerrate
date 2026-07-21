//! The nullability family, migrated (design section 5): the guard
//! analysis and the null-containment predicate live below in
//! `celerrate_types::checks::nullability`; this rule consumes the
//! per-body outcome records and constructs the diagnostics.

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_types::{TypedBodyContext, TypedVerdictKind};

use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::TypedBodyRule;

/// Dereference of a possibly-null value.
pub const NULL_DEREFERENCE: DiagnosticId = DiagnosticId::new("CEL0034");

/// The rule that reports dereferences whose receiver's type
/// explicitly contains `null` (the walk's predicate; guarded reads
/// and null-safe chains were silence below and never reach a record).
pub struct NullDereference;

/// The family's declarative unit.
pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "null-dereference".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![RuleIdentifier {
            id: NULL_DEREFERENCE,
            severity: Severity::Error,
        }],
        tier: Tier::Default,
    }
}

impl TypedBodyRule for NullDereference {
    fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>) {
        for verdict in context.verdicts() {
            // Exhaustive by design, never a `_` arm: a verdict kind
            // added to the walk below must fail to compile here rather
            // than be computed and then silently dropped by every rule.
            let (member, receiver) = match &verdict.kind {
                TypedVerdictKind::NullDereference { member, receiver } => (member, receiver),
                // `unknown-members` renders the first four;
                // `argument-checks` the last four.
                TypedVerdictKind::UnknownMethod { .. }
                | TypedVerdictKind::UnknownProperty { .. }
                | TypedVerdictKind::UnknownClassConstant { .. }
                | TypedVerdictKind::UnknownEnumCase { .. }
                | TypedVerdictKind::ArgumentType { .. }
                | TypedVerdictKind::TooFewArguments { .. }
                | TypedVerdictKind::TooManyArguments { .. }
                | TypedVerdictKind::UnknownNamedArgument { .. } => continue,
            };
            sink.report(
                NULL_DEREFERENCE,
                FindingAnchor::Expression {
                    body: verdict.body,
                    expression: verdict.expression,
                },
                format!("accessing `{member}` on a possibly null `{receiver}`"),
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

    use super::NULL_DEREFERENCE;
    use crate::rules::test_support::typed_body_diagnostics;

    #[test]
    fn a_possibly_null_dereference_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
class User { public function save(): void {} }
function f(?User $u): void { $u->save(); }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, NULL_DEREFERENCE);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "accessing `save` on a possibly null `User|null`"
        );
    }
}
