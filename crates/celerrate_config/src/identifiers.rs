//! The diagnostic identifiers this crate emits. Allocated CEL0043 to
//! CEL0049 in the canonical registry (`celerrate_diagnostics`); the
//! registry entries land with the explain pages, after the CLI wiring
//! makes their examples executable.

use celerrate_diagnostics::DiagnosticId;

/// `celerrate.toml` is not valid TOML (or not valid UTF-8, or
/// unreadable): the file exists but cannot be read as a configuration.
pub const INVALID_CONFIGURATION: DiagnosticId = DiagnosticId::new("CEL0043");
/// A key the schema does not know, anywhere in the file.
pub const UNKNOWN_CONFIGURATION_KEY: DiagnosticId = DiagnosticId::new("CEL0044");
/// A known key with a value of the wrong type or shape.
pub const INVALID_CONFIGURATION_VALUE: DiagnosticId = DiagnosticId::new("CEL0045");
/// A `[rules.<name>]` table naming a rule the registry does not know.
pub const UNKNOWN_RULE: DiagnosticId = DiagnosticId::new("CEL0046");
/// A `[rules.<name>]` key other than `enabled`: no rule has options yet.
pub const UNSUPPORTED_RULE_OPTION: DiagnosticId = DiagnosticId::new("CEL0047");
/// A `[severity]` key naming an identifier the registry does not know.
pub const UNKNOWN_SEVERITY_IDENTIFIER: DiagnosticId = DiagnosticId::new("CEL0048");
/// A `[severity]` key naming a resilience identifier: those are neither
/// disableable nor remappable by design.
pub const RESILIENCE_SEVERITY_REMAP: DiagnosticId = DiagnosticId::new("CEL0049");
