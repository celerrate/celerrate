//! Semantic validation: the names the file uses, checked against the
//! sets the composition root provides. The sets are parameters because
//! they live above this crate in the DAG (the rule registry, the
//! diagnostic registry): the crate owns the diagnostics, the caller
//! owns the knowledge.

use std::collections::BTreeSet;

use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::FileId;

use crate::identifiers::{RESILIENCE_SEVERITY_REMAP, UNKNOWN_RULE, UNKNOWN_SEVERITY_IDENTIFIER};
use crate::model::Configuration;

/// What the composition root knows and this crate cannot: the
/// registered rule names, the identifiers rules may emit (the
/// remappable set), and every registered identifier.
pub struct KnownSets<'sets> {
    pub rule_names: BTreeSet<&'sets str>,
    pub remappable_identifiers: BTreeSet<&'sets str>,
    pub registered_identifiers: BTreeSet<&'sets str>,
}

/// Checks every name `configuration` uses. A typo must never silently
/// configure nothing: each unknown name is a span-anchored diagnostic.
pub fn validate(
    file: FileId,
    configuration: &Configuration,
    known: &KnownSets<'_>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for rule in &configuration.rules {
        if !known.rule_names.contains(rule.name.value.as_str()) {
            diagnostics.push(Diagnostic::spanned(
                UNKNOWN_RULE,
                Severity::Error,
                file,
                rule.name.range,
                format!("unknown rule `{}`", rule.name.value),
            ));
        }
    }
    for entry in &configuration.severity {
        let identifier = entry.identifier.value.as_str();
        if !known.registered_identifiers.contains(identifier) {
            diagnostics.push(Diagnostic::spanned(
                UNKNOWN_SEVERITY_IDENTIFIER,
                Severity::Error,
                file,
                entry.identifier.range,
                format!("unknown diagnostic identifier `{identifier}`"),
            ));
        } else if !known.remappable_identifiers.contains(identifier) {
            diagnostics.push(Diagnostic::spanned(
                RESILIENCE_SEVERITY_REMAP,
                Severity::Error,
                file,
                entry.identifier.range,
                format!(
                    "`{identifier}` is a resilience diagnostic; its severity cannot be remapped"
                ),
            ));
        }
    }
    // Deliberately unsorted: every caller concatenates this with the
    // structural diagnostics and sorts the whole, so sorting here would
    // be work whose result is immediately discarded.
    diagnostics
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use std::collections::BTreeSet;

    use celerrate_source::FileId;

    use crate::identifiers::{
        RESILIENCE_SEVERITY_REMAP, UNKNOWN_RULE, UNKNOWN_SEVERITY_IDENTIFIER,
    };
    use crate::parse::parse;
    use crate::validate::{KnownSets, validate};

    fn known() -> KnownSets<'static> {
        KnownSets {
            rule_names: BTreeSet::from(["null-dereference", "unknown-members"]),
            remappable_identifiers: BTreeSet::from(["CEL0034", "CEL0030"]),
            registered_identifiers: BTreeSet::from(["CEL0034", "CEL0030", "CEL0026"]),
        }
    }

    fn diagnostics_for(text: &str) -> Vec<celerrate_diagnostics::Diagnostic> {
        let file = FileId::new(0);
        let (configuration, structural) = parse(file, text);
        assert!(structural.is_empty(), "fixture must be structurally clean");
        validate(file, &configuration, &known())
    }

    #[test]
    fn a_known_rule_and_a_remappable_identifier_are_silent() {
        let text =
            "[rules.null-dereference]\nenabled = false\n\n[severity]\n\"CEL0034\" = \"warning\"\n";
        assert!(diagnostics_for(text).is_empty());
    }

    #[test]
    fn an_unknown_rule_reports_cel0046() {
        let diagnostics = diagnostics_for("[rules.nul-dereference]\nenabled = false\n");
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, UNKNOWN_RULE);
        assert_eq!(diagnostic.message, "unknown rule `nul-dereference`");
        assert!(diagnostic.span().is_some());
    }

    #[test]
    fn an_unregistered_severity_identifier_reports_cel0048() {
        let diagnostics = diagnostics_for("[severity]\n\"CEL9999\" = \"warning\"\n");
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, UNKNOWN_SEVERITY_IDENTIFIER);
        assert_eq!(
            diagnostic.message,
            "unknown diagnostic identifier `CEL9999`"
        );
    }

    #[test]
    fn a_resilience_identifier_remap_reports_cel0049() {
        let diagnostics = diagnostics_for("[severity]\n\"CEL0026\" = \"error\"\n");
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, RESILIENCE_SEVERITY_REMAP);
        assert_eq!(
            diagnostic.message,
            "`CEL0026` is a resilience diagnostic; its severity cannot be remapped",
        );
    }
}
