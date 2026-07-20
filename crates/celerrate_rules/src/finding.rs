use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::{AstId, ExpressionId};
use celerrate_source::TextRange;

use crate::metadata::RuleMetadata;

/// Where a finding lands. Range-late phases anchor by identity and
/// reconcile at the phase query's tail; a phase that honestly has a
/// same-file range (the syntax phase) uses it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingAnchor {
    /// A concrete range in the checked file.
    Range(TextRange),
    /// A declaration, resolved through the `AstIdMap` at the tail.
    Declaration(AstId),
    /// An expression in a body arena, resolved through the body
    /// source map at the tail (the `TypedVerdict` pattern generalized).
    Expression {
        body: AstId,
        expression: ExpressionId,
    },
}

/// One accepted finding, severity already resolved from metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub identifier: DiagnosticId,
    pub severity: Severity,
    pub anchor: FindingAnchor,
    pub message: String,
}

/// The sink one rule reports into. Severity comes from the rule's own
/// metadata — a rule cannot choose a severity its declaration did not
/// fix. An identifier outside the declared list is dropped, never a
/// panic (pinned by test); the registry's declaration checks make a
/// core rule doing so a bug caught in CI.
pub struct FindingSink<'rule> {
    metadata: &'rule RuleMetadata,
    findings: Vec<Finding>,
}

impl<'rule> FindingSink<'rule> {
    /// Crate-internal construction: only the phase-query runner (a
    /// later task) drives a rule and owns a sink's lifetime. Unread
    /// from production code until that task wires it up, hence the
    /// `dead_code` allow (the same situation as
    /// `celerrate_types::checks::CheckContext`).
    #[allow(dead_code)]
    pub(crate) fn new(metadata: &'rule RuleMetadata) -> Self {
        Self {
            metadata,
            findings: Vec::new(),
        }
    }

    pub fn report(&mut self, identifier: DiagnosticId, anchor: FindingAnchor, message: String) {
        let Some(severity) = self.metadata.severity_of(identifier) else {
            return;
        };
        self.findings.push(Finding {
            identifier,
            severity,
            anchor,
            message,
        });
    }

    /// Crate-internal drain: only the phase-query runner (a later
    /// task) collects a rule's findings once it finishes reporting.
    /// Unread from production code until that task wires it up, hence
    /// the `dead_code` allow.
    #[allow(dead_code)]
    pub(crate) fn into_findings(self) -> Vec<Finding> {
        self.findings
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

    use celerrate_diagnostics::{DiagnosticId, Severity};
    use celerrate_source::{TextRange, TextSize};

    use crate::finding::{FindingAnchor, FindingSink};
    use crate::metadata::tests::test_metadata;

    #[test]
    fn a_declared_identifier_is_accepted_with_its_metadata_severity() {
        let metadata = test_metadata();
        let mut sink = FindingSink::new(&metadata);
        sink.report(
            DiagnosticId::new("CEL9998"),
            FindingAnchor::Range(TextRange::new(TextSize::from(0), TextSize::from(1))),
            "finding".to_owned(),
        );
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn an_undeclared_identifier_is_dropped_never_a_panic() {
        let metadata = test_metadata();
        let mut sink = FindingSink::new(&metadata);
        sink.report(
            DiagnosticId::new("CEL9999"),
            FindingAnchor::Range(TextRange::new(TextSize::from(0), TextSize::from(1))),
            "undeclared".to_owned(),
        );
        assert!(sink.into_findings().is_empty());
    }
}
