//! CEL0041: a typo in a CEL code must not silently suppress nothing.
//! Native directives only; a known but inactive identifier is not
//! unknown (design section 8).

use std::collections::BTreeSet;

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::DirectiveOrigin;

use crate::context::ReportingContext;
use crate::finding::FindingSink;
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::ReportingRule;

pub const UNKNOWN_SUPPRESSION_IDENTIFIER: DiagnosticId = DiagnosticId::new("CEL0041");

pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "unknown-suppression-identifier".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![RuleIdentifier {
            id: UNKNOWN_SUPPRESSION_IDENTIFIER,
            severity: Severity::Warning,
        }],
        tier: Tier::Default,
    }
}

pub struct UnknownSuppressionIdentifier;

impl ReportingRule for UnknownSuppressionIdentifier {
    fn check(&self, context: &ReportingContext<'_>, sink: &mut FindingSink<'_>) {
        for (index, outcome) in context.outcomes().iter().enumerate() {
            if outcome.directive.origin != DirectiveOrigin::Native {
                continue;
            }
            let Ok(subject) = u32::try_from(index) else {
                continue;
            };
            // One finding per distinct written form, in order of first
            // appearance: `@celerrate-ignore CEL9999, CEL9999` is one
            // mistake written twice, and printing the identical
            // diagnostic twice tells the reader nothing the first one
            // did not.
            let mut reported = BTreeSet::new();
            for written in &outcome.directive.identifiers {
                if !context.is_known(written) && reported.insert(written.as_str()) {
                    sink.report_directive(
                        UNKNOWN_SUPPRESSION_IDENTIFIER,
                        subject,
                        outcome.directive.anchor,
                        format!(
                            "unknown diagnostic identifier `{written}` in a @celerrate-ignore directive"
                        ),
                    );
                }
            }
        }
    }
}
