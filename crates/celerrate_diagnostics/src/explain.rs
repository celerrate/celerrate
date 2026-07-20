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

#[cfg(test)]
mod tests {
    use crate::{DiagnosticId, ExplainPage, find_page};

    #[test]
    fn no_page_is_registered_yet() {
        assert!(find_page(DiagnosticId::new("CEL0018")).is_none());
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
}
