use celerrate_source::{FileId, TextRange};

use crate::identifier::DiagnosticId;
use crate::label::Label;
use crate::severity::Severity;
use crate::suggestion::Suggestion;

/// Where a diagnostic points.
///
/// Almost every finding has a primary span. A project-level finding (a
/// missing Composer manifest, a version fallback) has none, and anchoring
/// it to a fictional `composer.json:1:1` is forbidden by design; the
/// anchor carries that honestly instead of forcing a fake range.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Anchor {
    /// The whole project: a finding with no honest span. Exit-code
    /// neutral by the notice contract.
    Project,
    /// A primary span in one file.
    Span { file: FileId, range: TextRange },
}

impl Anchor {
    /// The deterministic ordering key: project findings first, then
    /// span findings in `(file, start, end)` order, exactly the key the
    /// pre-anatomy model sorted by.
    fn key(&self) -> (u8, u32, u32, u32) {
        match self {
            Self::Project => (0, 0, 0, 0),
            Self::Span { file, range } => {
                (1, file.as_u32(), range.start().into(), range.end().into())
            }
        }
    }
}

/// One reported finding: a stable identifier, a severity, an anchor, the
/// rendered message, and the rich anatomy (labeled spans, notes,
/// structured suggestions). Ordering is total and deterministic so
/// diagnostic lists can be sorted and compared byte for byte; equality is
/// cheap and deterministic because salsa early cutoff depends on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: DiagnosticId,
    pub severity: Severity,
    pub anchor: Anchor,
    /// The rendered one-sentence message, parameterized by the producer
    /// (the written name, the required version).
    pub message: String,
    /// Secondary annotated spans, local or symbolic.
    pub labels: Vec<Label>,
    /// The engine's reasoning, one line each.
    pub notes: Vec<String>,
    /// Structured suggestions with their confidence and same-file edits.
    pub suggestions: Vec<Suggestion>,
}

impl Diagnostic {
    /// The common case: a finding with a primary span and no anatomy
    /// yet. Every pre-anatomy producer constructs through this.
    pub fn spanned(
        id: DiagnosticId,
        severity: Severity,
        file: FileId,
        range: TextRange,
        message: String,
    ) -> Self {
        Self {
            id,
            severity,
            anchor: Anchor::Span { file, range },
            message,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// A project-level finding with no honest span.
    pub fn project(id: DiagnosticId, severity: Severity, message: String) -> Self {
        Self {
            id,
            severity,
            anchor: Anchor::Project,
            message,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// The primary span, if the finding has one. `None` for
    /// project-anchored findings, which no span-keyed machinery
    /// (suppression, per-file persistence) ever touches.
    pub fn span(&self) -> Option<(FileId, TextRange)> {
        match self.anchor {
            Anchor::Project => None,
            Anchor::Span { file, range } => Some((file, range)),
        }
    }
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.anchor.key(),
            self.id,
            self.severity,
            &self.message,
            &self.labels,
            &self.notes,
            &self.suggestions,
        )
            .cmp(&(
                other.anchor.key(),
                other.id,
                other.severity,
                &other.message,
                &other.labels,
                &other.notes,
                &other.suggestions,
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

    use crate::{Anchor, Diagnostic, DiagnosticId, Severity};

    fn diagnostic(file: u32, start: u32, end: u32, id: &'static str) -> Diagnostic {
        Diagnostic::spanned(
            DiagnosticId::new(id),
            Severity::Error,
            FileId::new(file),
            TextRange::new(TextSize::from(start), TextSize::from(end)),
            String::new(),
        )
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
                .map(|diagnostic| (
                    diagnostic.span().map(|(file, _)| file.as_u32()),
                    diagnostic.id.as_str(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (Some(0), "CEL0002"),
                (Some(0), "CEL0003"),
                (Some(0), "CEL0002"),
                (Some(1), "CEL0002")
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
    fn a_project_finding_orders_before_every_span_finding() {
        let mut diagnostics = [
            diagnostic(0, 0, 1, "CEL0002"),
            Diagnostic::project(
                DiagnosticId::new("CEL0025"),
                Severity::Warning,
                "no composer.json found".to_owned(),
            ),
        ];
        diagnostics.sort();
        assert!(matches!(
            diagnostics.first().map(|diagnostic| &diagnostic.anchor),
            Some(Anchor::Project)
        ));
        assert!(diagnostics.first().unwrap().span().is_none());
    }

    #[test]
    fn the_message_is_the_ordering_tie_break_before_the_anatomy() {
        let first = Diagnostic {
            message: "alpha".to_owned(),
            ..diagnostic(0, 0, 1, "CEL9999")
        };
        let second = Diagnostic {
            message: "beta".to_owned(),
            ..first.clone()
        };
        assert!(first < second);
    }

    #[test]
    fn the_anatomy_is_the_final_ordering_tie_break() {
        let bare = diagnostic(0, 0, 1, "CEL9999");
        let annotated = Diagnostic {
            notes: vec!["inferred `string|null` because this path returns `null`".to_owned()],
            ..bare.clone()
        };
        assert!(bare < annotated);
        assert_ne!(bare, annotated);
    }
}
