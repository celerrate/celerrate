use crate::identifier::DiagnosticId;

/// One identifier's long-form explanation, embedded in the binary and
/// served by `celerrate explain`. Every section is mandatory content-wise
/// at sub-project closure (the composition-root test enforces presence);
/// in this part the store exists and no page is written yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplainPage {
    /// Why the reported pattern is a problem.
    pub why: &'static str,
    /// A minimal example that fires the identifier (executable by the
    /// part 8 harness, unless the identifier is on the declared
    /// environment-condition exemption list; spec section 10).
    pub failing_example: &'static str,
    /// The same example, corrected; must not fire the identifier.
    pub fixed_example: &'static str,
    /// Configuration notes and the owning rule.
    pub configuration: &'static str,
}

/// One identifier whose executable example is waived. The page itself
/// stays mandatory; only the harness execution is skipped, and the
/// reason is part of the declaration so a waiver is reviewable, never
/// an accident (spec section 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExampleExemption {
    pub id: DiagnosticId,
    pub reason: &'static str,
}

/// The declared exemption list, in identifier order. Grown only by
/// the page tasks that justify each entry; the closing spec amendment
/// records the final contents.
pub const EXECUTABLE_EXAMPLE_EXEMPTIONS: &[ExampleExemption] = &[];

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
