//! The unknown-member family, migrated (design section 5): the body
//! walks and the ternary membership judgments live below in
//! `celerrate_types::checks`; this rule consumes the per-body outcome
//! records and constructs the diagnostics.

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_types::{TypedBodyContext, TypedVerdictKind};

use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::TypedBodyRule;

/// Unknown method on the receiver's resolved type.
pub const UNKNOWN_METHOD: DiagnosticId = DiagnosticId::new("CEL0030");
/// Unknown property on the receiver's resolved type.
pub const UNKNOWN_PROPERTY: DiagnosticId = DiagnosticId::new("CEL0031");
/// Unknown class constant on the receiver's resolved type.
pub const UNKNOWN_CLASS_CONSTANT: DiagnosticId = DiagnosticId::new("CEL0032");
/// Unknown case on the receiver's resolved enum.
pub const UNKNOWN_ENUM_CASE: DiagnosticId = DiagnosticId::new("CEL0033");

/// The rule that reports member accesses whose member is provably
/// missing on the receiver's resolved surface (the walk's ternary
/// judgment; `PossiblyExists` was silence below and never reaches an
/// outcome record).
pub struct UnknownMembers;

/// The family's declarative unit.
pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "unknown-members".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![
            RuleIdentifier {
                id: UNKNOWN_METHOD,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: UNKNOWN_PROPERTY,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: UNKNOWN_CLASS_CONSTANT,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: UNKNOWN_ENUM_CASE,
                severity: Severity::Error,
            },
        ],
        tier: Tier::Default,
    }
}

impl TypedBodyRule for UnknownMembers {
    fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>) {
        for verdict in context.verdicts() {
            // Exhaustive by design, never a `_` arm: a verdict kind
            // added to the walk below must fail to compile here rather
            // than be computed and then silently dropped by every rule.
            let (id, message) = match &verdict.kind {
                TypedVerdictKind::UnknownMethod { member, receiver } => (
                    UNKNOWN_METHOD,
                    format!("unknown method `{member}` on `{receiver}`"),
                ),
                TypedVerdictKind::UnknownProperty { member, receiver } => (
                    UNKNOWN_PROPERTY,
                    format!("unknown property `${member}` on `{receiver}`"),
                ),
                TypedVerdictKind::UnknownClassConstant { member, receiver } => (
                    UNKNOWN_CLASS_CONSTANT,
                    format!("unknown class constant `{member}` on `{receiver}`"),
                ),
                TypedVerdictKind::UnknownEnumCase { member, receiver } => (
                    UNKNOWN_ENUM_CASE,
                    format!("unknown enum case `{member}` on `{receiver}`"),
                ),
                // `null-dereference` renders the first; `argument-checks`
                // the other four.
                TypedVerdictKind::NullDereference { .. }
                | TypedVerdictKind::ArgumentType { .. }
                | TypedVerdictKind::TooFewArguments { .. }
                | TypedVerdictKind::TooManyArguments { .. }
                | TypedVerdictKind::UnknownNamedArgument { .. } => continue,
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

    use super::{UNKNOWN_CLASS_CONSTANT, UNKNOWN_ENUM_CASE, UNKNOWN_METHOD, UNKNOWN_PROPERTY};
    use crate::rules::test_support::{
        default_range, registered_fixture, typed_body_diagnostics, typed_body_diagnostics_of,
    };

    #[test]
    fn an_unknown_method_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
class User { public function save(): void {} }
function f(User $u): void { $u->svae(); }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_METHOD);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].message, "unknown method `svae` on `User`");
    }

    #[test]
    fn an_unknown_property_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
class User { public string $name = ''; }
function f(User $u): void { $x = $u->nmae; }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_PROPERTY);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].message, "unknown property `$nmae` on `User`");
    }

    #[test]
    fn an_unknown_class_constant_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
class Config { public const LIMIT = 10; }
function f(): int { return Config::LIMTI; }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_CLASS_CONSTANT);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "unknown class constant `LIMTI` on `Config`"
        );
    }

    #[test]
    fn an_unknown_enum_case_is_reported_with_the_legacy_message() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
enum Status { case Active; }
function f(): Status { return Status::Draft; }
"#]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_ENUM_CASE);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "unknown enum case `Draft` on `Status`"
        );
    }

    #[test]
    fn a_file_without_defects_produces_no_typed_diagnostics() {
        let diagnostics = typed_body_diagnostics(&[r#"<?php
class User { public function save(): void {} }
function f(User $u): void { $u->save(); }
"#]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    /// The migration's equivalence proof, alive only while both paths
    /// exist (task 9 deletes the legacy one): on a fixture firing all
    /// three typed families, the phase output equals
    /// `celerrate_types::typed_diagnostics` byte for byte.
    #[test]
    fn the_phase_reproduces_the_legacy_typed_diagnostics_byte_for_byte() {
        let source = r#"<?php
class User { public function save(): void {} }
function f(User $u, ?User $n): void {
    $u->svae();
    $x = $u->nmae;
    $n->save();
}
function pair(int $a, int $b): void {}
function g(): void { pair(1); pair(1, 2, 3); pair(a: 1, c: 2); }
"#;
        let fixture = registered_fixture(&[source], vec![], default_range());
        let phase = typed_body_diagnostics_of(&fixture);
        let legacy = celerrate_types::typed_diagnostics(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            fixture.file,
        );
        assert!(!phase.is_empty(), "the fixture must fire");
        assert_eq!(&phase, legacy);
    }
}
