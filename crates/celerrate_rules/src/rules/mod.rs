//! The core rules. Everything here is registered at the composition
//! root under the reserved core identity.

pub mod argument_checks;
pub mod null_dereference;
pub mod symbol_version_gating;
pub mod syntax_version_gating;
#[cfg(test)]
pub(crate) mod test_support;
pub mod unknown_members;
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
    symbol_version_gating::SYMBOL_NOT_AVAILABLE,
    symbol_version_gating::SYMBOL_REMOVED,
    symbol_version_gating::SYMBOL_DEPRECATED,
    syntax_version_gating::SYNTAX_NOT_AVAILABLE,
    unknown_members::UNKNOWN_METHOD,
    unknown_members::UNKNOWN_PROPERTY,
    unknown_members::UNKNOWN_CLASS_CONSTANT,
    unknown_members::UNKNOWN_ENUM_CASE,
    null_dereference::NULL_DEREFERENCE,
    argument_checks::ARGUMENT_TYPE,
    argument_checks::TOO_FEW_ARGUMENTS,
    argument_checks::TOO_MANY_ARGUMENTS,
    argument_checks::UNKNOWN_NAMED_ARGUMENT,
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
        (
            symbol_version_gating::metadata(),
            RuleImplementation::Semantic(Arc::new(symbol_version_gating::SymbolVersionGating)),
        ),
        (
            unknown_members::metadata(),
            RuleImplementation::TypedBody(Arc::new(unknown_members::UnknownMembers)),
        ),
        (
            null_dereference::metadata(),
            RuleImplementation::TypedBody(Arc::new(null_dereference::NullDereference)),
        ),
        (
            argument_checks::metadata(),
            RuleImplementation::TypedBody(Arc::new(argument_checks::ArgumentChecks)),
        ),
    ]
}
