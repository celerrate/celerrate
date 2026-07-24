//! The diagnostic identifiers this crate emits. Allocated CEL0043 to
//! CEL0049 in the canonical registry (`celerrate_diagnostics`), each
//! with the explain page that documents it.

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

/// Every identifier this crate allocates, for the registry check at the
/// composition root.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[
    INVALID_CONFIGURATION,
    UNKNOWN_CONFIGURATION_KEY,
    INVALID_CONFIGURATION_VALUE,
    UNKNOWN_RULE,
    UNSUPPORTED_RULE_OPTION,
    UNKNOWN_SEVERITY_IDENTIFIER,
    RESILIENCE_SEVERITY_REMAP,
];

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use std::collections::BTreeSet;

    use celerrate_diagnostics::DiagnosticId;
    use celerrate_source::FileId;

    use super::ALLOCATED_IDENTIFIERS;
    use crate::parse::parse;
    use crate::validate::{KnownSets, validate};

    /// One `celerrate.toml` per allocated identifier, in allocation
    /// order, each written so the crate emits that identifier and
    /// nothing else. The list below is therefore checked against what
    /// the crate actually produces rather than against itself.
    const EMITTING_FIXTURES: &[&str] = &[
        // CEL0043
        "[project",
        // CEL0044
        "[project]\nincludes = [\"src\"]\n",
        // CEL0045
        "[project]\nphp = \"^8.1\"\n",
        // CEL0046
        "[rules.nul-dereference]\nenabled = false\n",
        // CEL0047
        "[rules.null-dereference]\nmax = 3\n",
        // CEL0048
        "[severity]\n\"CEL9999\" = \"warning\"\n",
        // CEL0049
        "[severity]\n\"CEL0026\" = \"error\"\n",
    ];

    fn known() -> KnownSets<'static> {
        KnownSets {
            rule_names: BTreeSet::from(["null-dereference"]),
            remappable_identifiers: BTreeSet::from(["CEL0034"]),
            registered_identifiers: BTreeSet::from(["CEL0034", "CEL0026"]),
        }
    }

    #[test]
    fn the_allocation_list_is_exactly_what_the_crate_emits() {
        let known = known();
        let used: Vec<DiagnosticId> = EMITTING_FIXTURES
            .iter()
            .map(|text| {
                let file = FileId::new(0);
                let (configuration, mut diagnostics) = parse(file, text);
                diagnostics.extend(validate(file, &configuration, &known));
                assert_eq!(
                    diagnostics.len(),
                    1,
                    "the fixture must emit exactly one identifier: {text:?} emitted {diagnostics:?}",
                );
                diagnostics[0].id
            })
            .collect();
        assert_eq!(used, ALLOCATED_IDENTIFIERS.to_vec());
    }
}
