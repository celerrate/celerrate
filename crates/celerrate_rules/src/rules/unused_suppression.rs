//! CEL0042: a native directive that suppressed nothing. Exempt (not
//! evaluable) when any identifier belongs to an inactive rule - the
//! nursery-demotion storm guard - or is unknown (CEL0041 already
//! reports that mistake).

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::DirectiveOrigin;

use crate::context::ReportingContext;
use crate::finding::FindingSink;
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::ReportingRule;

pub const UNUSED_SUPPRESSION: DiagnosticId = DiagnosticId::new("CEL0042");

pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "unused-suppression".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![RuleIdentifier {
            id: UNUSED_SUPPRESSION,
            severity: Severity::Warning,
        }],
        tier: Tier::Default,
    }
}

pub struct UnusedSuppression;

impl ReportingRule for UnusedSuppression {
    fn check(&self, context: &ReportingContext<'_>, sink: &mut FindingSink<'_>) {
        for (index, outcome) in context.outcomes().iter().enumerate() {
            if outcome.directive.origin != DirectiveOrigin::Native || outcome.matched {
                continue;
            }
            let Ok(subject) = u32::try_from(index) else {
                continue;
            };
            let evaluable = outcome
                .directive
                .identifiers
                .iter()
                .all(|written| context.is_known(written) && !context.is_inactive(written));
            if !evaluable {
                continue;
            }
            sink.report_directive(
                UNUSED_SUPPRESSION,
                subject,
                outcome.directive.anchor,
                "this @celerrate-ignore directive suppressed nothing".to_owned(),
            );
        }
    }
}
