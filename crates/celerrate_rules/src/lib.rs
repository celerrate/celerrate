//! The rule framework: rules are coherent families with declared
//! identifiers and metadata, registered into the fifth extension-point
//! registry, executed by per-phase queries against sealed contexts,
//! and reporting through a sink whose severities come from metadata.

mod context;
mod finding;
mod metadata;
mod phases;
mod registry;
#[cfg(feature = "render")]
pub mod render;
pub mod rules;
mod traits;

// The diagnostics vocabulary this crate's API uses. Nominal re-exports
// only, so a facade consumer can take everything from one crate.
pub use celerrate_diagnostics::{DiagnosticId, ExplainPage, Severity};

pub use context::{DirectiveOutcome, ReportingContext, SyntaxContext, testing_syntax_context};
pub use finding::{FindingAnchor, FindingSink};
pub use metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
pub use phases::{
    reporting_phase_diagnostics, semantic_phase_diagnostics, syntax_phase_diagnostics,
    typed_body_phase_diagnostics,
};
pub use registry::{
    CORE_IDENTITY_NAME, RuleConflict, RuleImplementation, RuleRegistration, RuleRegistry,
    validate_rules,
};
pub use rules::ALLOCATED_IDENTIFIERS;
pub use rules::core_rules;
pub use traits::{ReportingRule, SemanticRule, SyntaxRule, TypedBodyRule};
