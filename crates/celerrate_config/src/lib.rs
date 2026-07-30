//! `celerrate.toml` parsing and validation: the pure configuration
//! model the command-line product builds on.
//!
//! Two functions, one boundary: [`parse`] turns file text into a
//! [`Configuration`] plus structural diagnostics, and [`validate`]
//! checks the names the file uses against [`KnownSets`] the caller
//! provides, because the sets live above this crate in the DAG (the
//! rule registry) and the composition root is the only place that
//! sees both.

mod identifiers;
mod model;
mod parse;
mod validate;

pub use identifiers::{
    ALLOCATED_IDENTIFIERS, INVALID_CONFIGURATION, INVALID_CONFIGURATION_VALUE,
    RESILIENCE_SEVERITY_REMAP, UNKNOWN_CONFIGURATION_KEY, UNKNOWN_RULE,
    UNKNOWN_SEVERITY_IDENTIFIER, UNSUPPORTED_RULE_OPTION,
};
pub use model::{Configuration, RuleEntry, SeverityEntry, Spanned};
pub use parse::parse;
pub use validate::{KnownSets, validate};
