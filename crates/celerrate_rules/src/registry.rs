use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use celerrate_diagnostics::DiagnosticId;
use celerrate_semantics::PluginIdentity;

use crate::metadata::RuleMetadata;
use crate::traits::{ReportingRule, SemanticRule, SyntaxRule, TypedBodyRule};

/// The reserved registration identity of core rules. Core
/// registrations never enter the admitted plugin set, so they never
/// key the plugin-set digest — binary identity already keys the cache
/// for core behavior (design section 2). The composition root refuses
/// a plugin descriptor carrying this name.
pub const CORE_IDENTITY_NAME: &str = "celerrate-core";

/// A rule's phase-typed implementation.
#[derive(Clone)]
pub enum RuleImplementation {
    Syntax(Arc<dyn SyntaxRule>),
    Semantic(Arc<dyn SemanticRule>),
    TypedBody(Arc<dyn TypedBodyRule>),
    Reporting(Arc<dyn ReportingRule>),
}

impl RuleImplementation {
    fn phase_name(&self) -> &'static str {
        match self {
            Self::Syntax(_) => "syntax",
            Self::Semantic(_) => "semantic",
            Self::TypedBody(_) => "typed-body",
            Self::Reporting(_) => "reporting",
        }
    }
}

impl std::fmt::Debug for RuleImplementation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuleImplementation")
            .field("phase", &self.phase_name())
            .finish_non_exhaustive()
    }
}

/// One registered rule: who registered it, whether it is in the active
/// set, its declarative unit, and its implementation. `active` is
/// computed at the composition root (`Default`-tier rules are active,
/// `Nursery` rules are not); sub-project 5's configuration adjusts
/// that computation and nothing else.
#[derive(Clone)]
pub struct RuleRegistration {
    pub identity: PluginIdentity,
    pub active: bool,
    pub metadata: RuleMetadata,
    pub implementation: RuleImplementation,
}

impl std::fmt::Debug for RuleRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuleRegistration")
            .field("identity", &self.identity)
            .field("active", &self.active)
            .field("name", &self.metadata.name)
            .finish_non_exhaustive()
    }
}

/// The fifth extension-point registry, on the template of the existing
/// four: set once at the composition root with HIGH durability;
/// consumers read `try_get` — unset is the empty path.
#[salsa::input(singleton)]
pub struct RuleRegistry {
    #[returns(ref)]
    pub registrations: Vec<RuleRegistration>,
}

/// Why a rule set does not validate. A core-versus-core conflict is a
/// bug and fails the composition-root test in CI, never a runtime
/// degradation (design section 4); plugin conflicts reuse the
/// whole-plugin exclusion model when plugin rules become registrable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleConflict {
    DuplicateName {
        name: String,
    },
    DuplicateIdentifier {
        id: DiagnosticId,
        first: String,
        second: String,
    },
    EmptyIdentifierList {
        name: String,
    },
}

/// Checks the registry invariants: unique rule names, every identifier
/// claimed by exactly one rule, no rule with an empty claim list.
pub fn validate_rules(registrations: &[RuleRegistration]) -> Result<(), RuleConflict> {
    let mut names: BTreeSet<&str> = BTreeSet::new();
    let mut identifiers: BTreeMap<DiagnosticId, &str> = BTreeMap::new();
    for registration in registrations {
        let rule_name = registration.metadata.name.as_str();
        if !names.insert(rule_name) {
            return Err(RuleConflict::DuplicateName {
                name: rule_name.to_owned(),
            });
        }
        if registration.metadata.identifiers.is_empty() {
            return Err(RuleConflict::EmptyIdentifierList {
                name: rule_name.to_owned(),
            });
        }
        for identifier in &registration.metadata.identifiers {
            if let Some(first) = identifiers.insert(identifier.id, rule_name) {
                return Err(RuleConflict::DuplicateIdentifier {
                    id: identifier.id,
                    first: first.to_owned(),
                    second: rule_name.to_owned(),
                });
            }
        }
    }
    Ok(())
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

    use celerrate_diagnostics::{DiagnosticId, Severity};
    use celerrate_semantics::PluginIdentity;

    use super::{RuleConflict, RuleImplementation, RuleRegistration, validate_rules};
    use crate::context::SyntaxContext;
    use crate::finding::FindingSink;
    use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
    use crate::traits::SyntaxRule;

    struct NullSyntaxRule;

    impl SyntaxRule for NullSyntaxRule {
        fn check(&self, _context: &SyntaxContext<'_>, _sink: &mut FindingSink<'_>) {}
    }

    fn test_registration(name: &str, id: &'static str) -> RuleRegistration {
        RuleRegistration {
            identity: PluginIdentity {
                name: "test-plugin".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            active: true,
            metadata: RuleMetadata {
                name: name.to_owned(),
                group: RuleGroup::Correctness,
                identifiers: vec![RuleIdentifier {
                    id: DiagnosticId::new(id),
                    severity: Severity::Error,
                }],
                tier: Tier::Default,
            },
            implementation: RuleImplementation::Syntax(std::sync::Arc::new(NullSyntaxRule)),
        }
    }

    #[test]
    fn a_duplicate_identifier_claim_is_a_conflict_naming_both_rules() {
        let first = test_registration("first-rule", "CEL9998");
        let second = test_registration("second-rule", "CEL9998");
        assert_eq!(
            validate_rules(&[first, second]),
            Err(RuleConflict::DuplicateIdentifier {
                id: DiagnosticId::new("CEL9998"),
                first: "first-rule".to_owned(),
                second: "second-rule".to_owned(),
            }),
        );
    }

    #[test]
    fn a_duplicate_rule_name_is_a_conflict() {
        let first = test_registration("same-name", "CEL9997");
        let second = test_registration("same-name", "CEL9998");
        assert_eq!(
            validate_rules(&[first, second]),
            Err(RuleConflict::DuplicateName {
                name: "same-name".to_owned(),
            }),
        );
    }

    #[test]
    fn an_empty_identifier_list_is_a_conflict() {
        let mut registration = test_registration("no-identifiers", "CEL9998");
        registration.metadata.identifiers.clear();
        assert_eq!(
            validate_rules(std::slice::from_ref(&registration)),
            Err(RuleConflict::EmptyIdentifierList {
                name: "no-identifiers".to_owned(),
            }),
        );
    }

    #[test]
    fn a_conflict_free_set_validates() {
        let first = test_registration("first-rule", "CEL9997");
        let second = test_registration("second-rule", "CEL9998");
        assert_eq!(validate_rules(&[first, second]), Ok(()));
    }

    #[test]
    fn debug_prints_identity_and_name_never_the_implementation() {
        let registration = test_registration("printable", "CEL9998");
        let printed = format!("{registration:?}");
        assert!(printed.contains("printable"));
        assert!(!printed.contains("implementation"));
    }
}
