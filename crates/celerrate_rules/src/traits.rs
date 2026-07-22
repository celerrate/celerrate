use celerrate_semantics::SemanticContext;
use celerrate_types::TypedBodyContext;

use crate::context::{ReportingContext, SyntaxContext};
use crate::finding::FindingSink;

/// A rule of the syntax phase: syntax outcomes and the PHP version
/// range, no name resolution, no types.
pub trait SyntaxRule: Send + Sync {
    fn check(&self, context: &SyntaxContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the semantic phase: reference resolution outcomes and the
/// symbol index. Part 4's migrated families arrived as their own
/// typed-body phase below (`TypedBodyRule`), not as an extension of
/// this surface: three typed rule families are registered in
/// `core_rules()` today (`unknown-members`, `null-dereference`,
/// `argument-checks`).
pub trait SemanticRule: Send + Sync {
    fn check(&self, context: &SemanticContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the typed-body phase, executed once per body under the
/// per-body tracked tier.
pub trait TypedBodyRule: Send + Sync {
    fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the reporting phase: directives and their match outcomes.
/// Core-only in this sub-project (design section 4): the registry model
/// and the ownership gate see the phase, and the facade does not
/// re-export it. Executed by `phases::reporting_phase_diagnostics`, a
/// plain function fed by the orchestration layer and never a salsa
/// query (decision 9 of the part-5 plan): its input is composed above,
/// from stored per-directive records on a warm cache hit and from the
/// resolution query on a miss.
pub trait ReportingRule: Send + Sync {
    fn check(&self, context: &ReportingContext<'_>, sink: &mut FindingSink<'_>);
}
