use crate::identifier::DiagnosticId;

/// One identifier's long-form explanation, embedded in the binary and
/// served by `celerrate explain`. Every registry entry carries one: the
/// store is total, enforced both by `RegisteredDiagnostic.explain` being
/// mandatory (no `Option`) and by the content gate in `registry.rs`'s
/// tests that every section is non-empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplainPage {
    /// Why the reported pattern is a problem.
    pub why: &'static str,
    /// A minimal example that fires the identifier, executed by the
    /// harness in `celerrate_cli/tests/explain_pages.rs`, unless the
    /// identifier is on the declared environment-condition exemption
    /// list below.
    pub failing_example: &'static str,
    /// The same example, corrected; must not fire the identifier.
    pub fixed_example: &'static str,
    /// Configuration notes and the owning rule.
    pub configuration: &'static str,
}

/// One identifier whose executable example is waived. The page itself
/// stays mandatory; only the harness execution is skipped, and the
/// reason is part of the declaration so a waiver is reviewable, never
/// an accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExampleExemption {
    pub id: DiagnosticId,
    pub reason: &'static str,
}

/// The declared exemption list, in identifier order. Grown only
/// alongside the reason that justifies each entry; this declaration
/// is the exemption list's final and complete source.
pub const EXECUTABLE_EXAMPLE_EXEMPTIONS: &[ExampleExemption] = &[
    ExampleExemption {
        id: DiagnosticId::new("CEL0001"),
        reason: "fires only on a file whose decoded size exceeds 4 GiB, \
                 which cannot be committed as a fixture and depends on \
                 the execution environment",
    },
    ExampleExemption {
        id: DiagnosticId::new("CEL0013"),
        reason: "the parser's no-progress guard is a defensive backstop \
                 that no grammar-admitted source reaches; the day a \
                 reproduction exists it becomes this page's example",
    },
    ExampleExemption {
        id: DiagnosticId::new("CEL0022"),
        reason: "the shipped stub blob carries no symbol whose removal falls \
                 inside the supported 8.1 to 8.5 window; the framework-path \
                 fixture in celerrate_rules covers recall (same waiver as the \
                 seeded-defect suite)",
    },
    ExampleExemption {
        id: DiagnosticId::new("CEL0039"),
        reason: "fires on a permission-based IO error, which cannot be \
                 committed as a fixture and does not reproduce under root \
                 or on Windows CI",
    },
    ExampleExemption {
        id: DiagnosticId::new("CEL0040"),
        reason: "fires on a permission-based IO error, which cannot be \
                 committed as a fixture and does not reproduce under root \
                 or on Windows CI",
    },
];

#[cfg(test)]
mod tests {
    use crate::{DiagnosticId, ExplainPage, find_page};

    #[test]
    fn a_written_page_is_found_and_an_unknown_identifier_has_none() {
        assert!(find_page(DiagnosticId::new("CEL0018")).is_some());
        assert!(find_page(DiagnosticId::new("CEL9999")).is_none());
    }

    #[test]
    fn a_page_carries_its_four_sections() {
        static PAGE: ExplainPage = ExplainPage {
            why: "calling an unknown method fails at runtime",
            failing_example: "<?php (new \\DateTime())->fromat('Y');",
            fixed_example: "<?php (new \\DateTime())->format('Y');",
            configuration: "reported by the unknown-members rule",
        };
        assert!(!PAGE.why.is_empty());
        assert!(!PAGE.failing_example.is_empty());
        assert!(!PAGE.fixed_example.is_empty());
        assert!(!PAGE.configuration.is_empty());
    }

    #[test]
    fn every_exemption_names_a_registered_identifier_and_a_reason() {
        use crate::{EXECUTABLE_EXAMPLE_EXEMPTIONS, find_identifier};
        for exemption in EXECUTABLE_EXAMPLE_EXEMPTIONS {
            assert!(
                find_identifier(exemption.id.as_str()).is_some(),
                "{} is exempt but not registered",
                exemption.id.as_str(),
            );
            assert!(
                !exemption.reason.trim().is_empty(),
                "{} is exempt without a reason",
                exemption.id.as_str(),
            );
        }
    }

    #[test]
    fn exemptions_are_sorted_and_unique() {
        use crate::EXECUTABLE_EXAMPLE_EXEMPTIONS;
        let identifiers: Vec<&str> = EXECUTABLE_EXAMPLE_EXEMPTIONS
            .iter()
            .map(|exemption| exemption.id.as_str())
            .collect();
        let mut sorted = identifiers.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            identifiers, sorted,
            "keep the exemption list sorted and unique"
        );
    }
}
