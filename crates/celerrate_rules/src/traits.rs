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
/// symbol index (surface arrives with part 4's migrated families).
pub trait SemanticRule: Send + Sync {
    fn check(&self, context: &SemanticContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the typed-body phase, executed once per body under the
/// per-body tracked tier.
pub trait TypedBodyRule: Send + Sync {
    fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the reporting phase: directives and their match outcomes.
/// Core-only in this sub-project (design section 4): declared so the
/// registry model and the ownership gate see the phase; its execution
/// point and context surface arrive in part 5, and the facade does not
/// re-export it.
pub trait ReportingRule: Send + Sync {
    fn check(&self, context: &ReportingContext<'_>, sink: &mut FindingSink<'_>);
}
