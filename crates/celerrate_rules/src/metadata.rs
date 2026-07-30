use celerrate_diagnostics::{DiagnosticId, Severity};

/// The rule groups. Only `correctness` exists until the style group
/// arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleGroup {
    Correctness,
}

/// Whether a rule joins the default-enabled set. Demotion under the
/// anti-false-positive policy is a one-line change of this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Default,
    Nursery,
}

/// One identifier a rule may emit, with its default severity
/// (families already mix `Error` and `Warning`, so severity is
/// per identifier, not per rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleIdentifier {
    pub id: DiagnosticId,
    pub severity: Severity,
}

/// A rule's declarative unit: a coherent family, not a single
/// identifier. Owned data, not `&'static` — plugin-registered rules
/// will travel their metadata as registration data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMetadata {
    /// Stable kebab-case name, e.g. `syntax-version-gating`.
    pub name: String,
    pub group: RuleGroup,
    /// The closed list of identifiers the rule may emit.
    pub identifiers: Vec<RuleIdentifier>,
    pub tier: Tier,
}

impl RuleMetadata {
    /// The default severity of `id`, `None` when the rule never
    /// declared it (the sink drops such an emission).
    pub fn severity_of(&self, id: DiagnosticId) -> Option<Severity> {
        self.identifiers
            .iter()
            .find(|identifier| identifier.id == id)
            .map(|identifier| identifier.severity)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_diagnostics::{DiagnosticId, Severity};

    use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};

    pub(crate) fn test_metadata() -> RuleMetadata {
        RuleMetadata {
            name: "test-rule".to_owned(),
            group: RuleGroup::Correctness,
            identifiers: vec![RuleIdentifier {
                id: DiagnosticId::new("CEL9998"),
                severity: Severity::Error,
            }],
            tier: Tier::Default,
        }
    }

    #[test]
    fn severity_is_looked_up_per_identifier() {
        let metadata = test_metadata();
        assert_eq!(
            metadata.severity_of(DiagnosticId::new("CEL9998")),
            Some(Severity::Error),
        );
        assert_eq!(metadata.severity_of(DiagnosticId::new("CEL9999")), None);
    }
}
