use celerrate_source::{FileId, TextRange};

use crate::identifier::DiagnosticId;
use crate::severity::Severity;

/// One reported finding: a stable identifier, a severity, the primary
/// span it points at, and the rendered message. The minimal shared shape
/// every producer projects into; ordering is total and deterministic so
/// diagnostic lists can be sorted and compared byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: DiagnosticId,
    pub severity: Severity,
    pub file: FileId,
    pub range: TextRange,
    /// The rendered one-sentence message, parameterized by the producer
    /// (the written name, the required version). The rich anatomy —
    /// annotated spans, notes, suggestions — is sub-project 4.
    pub message: String,
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.file,
            self.range.start(),
            self.range.end(),
            self.id,
            self.severity,
            &self.message,
        )
            .cmp(&(
                other.file,
                other.range.start(),
                other.range.end(),
                other.id,
                other.severity,
                &other.message,
            ))
    }
}

impl PartialOrd for Diagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, TextRange, TextSize};

    use crate::{Diagnostic, DiagnosticId, Severity};

    fn diagnostic(file: u32, start: u32, end: u32, id: &'static str) -> Diagnostic {
        Diagnostic {
            id: DiagnosticId::new(id),
            severity: Severity::Error,
            file: FileId::new(file),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            message: String::new(),
        }
    }

    #[test]
    fn identifier_round_trips() {
        assert_eq!(DiagnosticId::new("CEL0001").as_str(), "CEL0001");
    }

    #[test]
    fn severity_orders_warning_below_error() {
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn diagnostics_order_by_file_then_range_then_identifier() {
        let mut diagnostics = [
            diagnostic(1, 0, 1, "CEL0002"),
            diagnostic(0, 5, 9, "CEL0002"),
            diagnostic(0, 0, 4, "CEL0003"),
            diagnostic(0, 0, 4, "CEL0002"),
        ];
        diagnostics.sort();
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.file.as_u32(), diagnostic.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (0, "CEL0002"),
                (0, "CEL0003"),
                (0, "CEL0002"),
                (1, "CEL0002")
            ],
        );
    }

    #[test]
    fn equal_diagnostics_compare_equal() {
        assert_eq!(
            diagnostic(0, 0, 1, "CEL0002"),
            diagnostic(0, 0, 1, "CEL0002")
        );
    }

    #[test]
    fn the_message_is_the_final_ordering_tie_break() {
        let first = Diagnostic {
            id: DiagnosticId::new("CEL9999"),
            severity: Severity::Error,
            file: FileId::new(0),
            range: TextRange::empty(0.into()),
            message: "alpha".to_owned(),
        };
        let second = Diagnostic {
            message: "beta".to_owned(),
            ..first.clone()
        };
        assert!(first < second);
    }
}
