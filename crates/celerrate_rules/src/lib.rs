//! The rule framework: rules are coherent families with declared
//! identifiers and metadata, registered into the fifth extension-point
//! registry, executed by per-phase queries against sealed contexts,
//! and reporting through a sink whose severities come from metadata.

mod context;
mod finding;
mod metadata;
mod traits;

pub use context::{ReportingContext, SyntaxContext, testing_syntax_context};
pub use finding::{FindingAnchor, FindingSink};
pub use metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
pub use traits::{ReportingRule, SemanticRule, SyntaxRule, TypedBodyRule};
