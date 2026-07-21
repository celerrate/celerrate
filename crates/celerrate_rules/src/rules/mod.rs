//! The core rules. Everything here is registered at the composition
//! root under the reserved core identity.

pub mod syntax_version_gating;
#[cfg(test)]
pub(crate) mod test_support;
pub mod unknown_symbols;

use std::sync::Arc;

use celerrate_diagnostics::DiagnosticId;

use crate::metadata::RuleMetadata;
use crate::registry::RuleImplementation;

/// Every identifier this crate allocates, for the registry check at
/// the composition root. Grows with each migrated family; identifier
/// order.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[
    unknown_symbols::UNKNOWN_CLASS,
    unknown_symbols::UNKNOWN_FUNCTION,
    unknown_symbols::UNKNOWN_CONSTANT,
    syntax_version_gating::SYNTAX_NOT_AVAILABLE,
];

/// The core rule set, in registration order.
pub fn core_rules() -> Vec<(RuleMetadata, RuleImplementation)> {
    vec![
        (
            syntax_version_gating::metadata(),
            RuleImplementation::Syntax(Arc::new(syntax_version_gating::SyntaxVersionGating)),
        ),
        (
            unknown_symbols::metadata(),
            RuleImplementation::Semantic(Arc::new(unknown_symbols::UnknownSymbols)),
        ),
    ]
}
