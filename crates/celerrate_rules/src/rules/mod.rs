//! The core rules. Everything here is registered at the composition
//! root under the reserved core identity.

pub mod syntax_version_gating;

use std::sync::Arc;

use crate::metadata::RuleMetadata;
use crate::registry::RuleImplementation;

/// The core rule set, in registration order.
pub fn core_rules() -> Vec<(RuleMetadata, RuleImplementation)> {
    vec![(
        syntax_version_gating::metadata(),
        RuleImplementation::Syntax(Arc::new(syntax_version_gating::SyntaxVersionGating)),
    )]
}
