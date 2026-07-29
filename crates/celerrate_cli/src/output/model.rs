//! The machine-report model: one serializable projection of the final
//! stream (post-suppression, post-baseline, post-configuration, sorted),
//! built once and consumed by every machine writer. Anchors and secondary
//! labels resolve here, at the same presentation edge the human renderer
//! uses. Pure presentation: nothing enters the queries or the cache.

use std::collections::BTreeMap;

use celerrate_diagnostics::{Anchor, Confidence, Diagnostic, LabelTarget, Severity};
use celerrate_rules::render::resolve::DatabaseResolver;
use celerrate_rules::render::{ResolvedLabel, SourceAccess, SymbolResolver, degraded_note};
use celerrate_source::{FileId, LineIndex, TextRange, TextSize};
use serde::Serialize;

use crate::Outcome;
use crate::analysis::AnalysisOutcome;
use crate::baseline::BaselineOutcome;
use crate::render::SessionSources;
use crate::session::{InternalError, Session};

/// The JSON contract version. Adding a field is non-breaking; removing
/// one or changing its meaning increments this constant and forks the
/// committed schema file.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct MachineReport {
    pub schema_version: u32,
    pub summary: Summary,
    pub notices: Vec<Notice>,
    pub internal_errors: Vec<ReportedInternalError>,
    pub diagnostics: Vec<ReportedDiagnostic>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub errors: usize,
    pub warnings: usize,
    pub notices: usize,
    pub baselined_hidden: usize,
    pub internal_errors: usize,
    pub exit_code: u8,
}

/// An exit-neutral notice: a project notice or a baseline notice, in the
/// order the human report prints them.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub id: String,
    pub message: String,
}

/// An internal error the run survived: the tool degraded instead of
/// crashing. `kind` names the condition so tooling can route it, `message`
/// is the same sentence the human channel prints, and `bug` separates a
/// defect in Celerrate from a condition of the environment.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReportedInternalError {
    pub kind: String,
    pub message: String,
    pub bug: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReportedDiagnostic {
    pub id: String,
    pub severity: ReportedSeverity,
    /// The owning rule's kebab-case name; absent for identifiers no rule
    /// owns (syntax, project, configuration resilience).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub anchor: ReportedAnchor,
    pub message: String,
    pub labels: Vec<ResolvedReportLabel>,
    pub notes: Vec<String>,
    pub suggestions: Vec<ReportedSuggestion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportedSeverity {
    Warning,
    Error,
}

impl From<Severity> for ReportedSeverity {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Warning => Self::Warning,
            Severity::Error => Self::Error,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ReportedAnchor {
    Project,
    Span(SpanLocation),
}

/// A concrete location: project-relative path with forward slashes,
/// 1-based lines, 1-based columns counted in Unicode code points, plus
/// the exact byte offsets for tools that edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpanLocation {
    pub path: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub byte_start: u32,
    pub byte_end: u32,
}

/// A secondary label that resolved to a concrete location, local or
/// symbolic alike. Degraded symbolic labels become notes instead, with
/// the same wording as the human renderer.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedReportLabel {
    pub location: SpanLocation,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReportedSuggestion {
    pub message: String,
    pub confidence: ReportedConfidence,
    pub edits: Vec<ReportedEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportedConfidence {
    Safe,
    NeedsReview,
}

impl From<Confidence> for ReportedConfidence {
    fn from(confidence: Confidence) -> Self {
        match confidence {
            Confidence::Safe => Self::Safe,
            Confidence::NeedsReview => Self::NeedsReview,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReportedEdit {
    pub location: SpanLocation,
    pub replacement: String,
}

/// Build the report from the final stream. `presented` is the exact
/// vector the human renderer receives; `verdict` is the run's one
/// `Outcome`, computed by the caller from the same inputs as the human
/// arm, never recomputed here.
pub fn build(
    session: &Session,
    presented: &AnalysisOutcome,
    baseline: &BaselineOutcome,
    verdict: Outcome,
) -> MachineReport {
    let sources = SessionSources { session };
    let resolver = DatabaseResolver::new(&session.database, session.files);
    let rules = rule_name_index();
    let diagnostics: Vec<ReportedDiagnostic> = presented
        .diagnostics
        .iter()
        .map(|diagnostic| reported(diagnostic, &sources, &resolver, &rules))
        .collect();
    let errors = presented
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count();
    let warnings = presented
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    let mut notices: Vec<Notice> = session
        .notices()
        .iter()
        .map(|notice| Notice {
            id: notice.identifier().as_str().to_owned(),
            message: notice.message(),
        })
        .collect();
    notices.extend(baseline.notices.iter().map(|notice| Notice {
        id: notice.identifier().as_str().to_owned(),
        message: notice.message(),
    }));
    let internal_errors: Vec<ReportedInternalError> = session
        .internal_errors
        .iter()
        .map(|error| ReportedInternalError {
            kind: internal_error_kind(error).to_owned(),
            message: crate::render::internal_error_message(session, error),
            bug: crate::render::is_celerrate_bug(error),
        })
        .collect();
    MachineReport {
        schema_version: SCHEMA_VERSION,
        summary: Summary {
            errors,
            warnings,
            notices: notices.len(),
            baselined_hidden: baseline.hidden,
            internal_errors: session.internal_errors.len(),
            exit_code: verdict.code(),
        },
        notices,
        internal_errors,
        diagnostics,
    }
}

/// The variant name in kebab-case, matched explicitly rather than derived
/// from the type name: a rename of the Rust variant must not silently
/// change the wire value tooling routes on.
fn internal_error_kind(error: &InternalError) -> &'static str {
    match error {
        InternalError::StubBlobUndecodable(_) => "stub-blob-undecodable",
        InternalError::FileUnreadable { .. } => "file-unreadable",
        InternalError::DirectoryUnreadable { .. } => "directory-unreadable",
        InternalError::PathUnwatchable { .. } => "path-unwatchable",
        InternalError::FilePanicked { .. } => "file-panicked",
        InternalError::AnalysisPanicked => "analysis-panicked",
        InternalError::FixUnappliable { .. } => "fix-unappliable",
        InternalError::FixWriteFailed { .. } => "fix-write-failed",
        InternalError::DiagnosticRenderFailed { .. } => "diagnostic-render-failed",
    }
}

fn reported(
    diagnostic: &Diagnostic,
    sources: &SessionSources<'_>,
    resolver: &DatabaseResolver<'_>,
    rules: &BTreeMap<&'static str, String>,
) -> ReportedDiagnostic {
    let anchored_file = match diagnostic.anchor {
        Anchor::Span { file, .. } => Some(file),
        Anchor::Project => None,
    };
    let anchor = match diagnostic.anchor {
        Anchor::Project => ReportedAnchor::Project,
        Anchor::Span { file, range } => ReportedAnchor::Span(location(sources, file, range)),
    };
    let mut labels = Vec::new();
    let mut notes = diagnostic.notes.clone();
    for label in &diagnostic.labels {
        match &label.target {
            LabelTarget::Local { range } => match anchored_file {
                Some(file) => labels.push(ResolvedReportLabel {
                    location: location(sources, file, *range),
                    message: label.message.clone(),
                }),
                // A local label on a project anchor has no file to
                // resolve against; the message survives as a note.
                None => notes.push(label.message.clone()),
            },
            LabelTarget::Symbolic { symbol } => match resolver.resolve(symbol) {
                ResolvedLabel::Concrete { file, range } => {
                    labels.push(ResolvedReportLabel {
                        location: location(sources, file, range),
                        message: label.message.clone(),
                    });
                }
                ResolvedLabel::Degraded => {
                    notes.push(degraded_note(symbol, &label.message));
                }
            },
        }
    }
    let suggestions = diagnostic
        .suggestions
        .iter()
        .map(|suggestion| ReportedSuggestion {
            message: suggestion.message.clone(),
            confidence: suggestion.confidence.into(),
            edits: suggestion
                .edits
                .iter()
                .map(|edit| ReportedEdit {
                    location: location(sources, edit.file, edit.range),
                    replacement: edit.replacement.clone(),
                })
                .collect(),
        })
        .collect();
    ReportedDiagnostic {
        id: diagnostic.id.as_str().to_owned(),
        severity: diagnostic.severity.into(),
        rule: rules.get(diagnostic.id.as_str()).cloned(),
        anchor,
        message: diagnostic.message.clone(),
        labels,
        notes,
        suggestions,
    }
}

/// Total conversion: an unreadable file degrades to line 1, column 1,
/// mirroring `render_minimal`, and the byte offsets stay exact.
fn location(sources: &SessionSources<'_>, file: FileId, range: TextRange) -> SpanLocation {
    let path = sources
        .display_path(file)
        .unwrap_or_else(|| String::from("<unknown>"))
        .replace('\\', "/");
    let (start_line, start_column, end_line, end_column) = match sources.text(file) {
        Some(text) => {
            let index = LineIndex::new(text);
            let (start_line, start_column) = position(&index, text, range.start());
            let (end_line, end_column) = position(&index, text, range.end());
            (start_line, start_column, end_line, end_column)
        }
        None => (1, 1, 1, 1),
    };
    SpanLocation {
        path,
        start_line,
        start_column,
        end_line,
        end_column,
        byte_start: range.start().into(),
        byte_end: range.end().into(),
    }
}

/// 1-based line, 1-based column in Unicode code points. The line index
/// speaks zero-based byte columns; the conversion happens here, once.
fn position(index: &LineIndex, text: &str, offset: TextSize) -> (u32, u32) {
    let line_column = index.line_column(offset);
    let offset = usize::from(offset);
    let line_start = offset.saturating_sub(line_column.column as usize);
    let code_points = text
        .get(line_start..offset)
        .map(|prefix| prefix.chars().count() as u32)
        .unwrap_or(line_column.column);
    (line_column.line + 1, code_points + 1)
}

/// Identifier to owning rule name, derived from the core rule metadata
/// the same way `configuration::severity_remap` walks it. Identifiers no
/// rule owns (syntax, project, configuration) are simply absent.
fn rule_name_index() -> BTreeMap<&'static str, String> {
    let mut index = BTreeMap::new();
    for (metadata, _) in celerrate_rules::core_rules() {
        for identifier in &metadata.identifiers {
            index.insert(identifier.id.as_str(), metadata.name.clone());
        }
    }
    index
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use celerrate_source::{LineIndex, TextSize};

    use super::position;

    /// A plain ASCII prefix: the byte column and the code-point column
    /// coincide, so this pins the ordinary case before the multi-byte
    /// ones below exercise the conversion.
    #[test]
    fn an_ascii_offset_reports_the_same_column_in_bytes_and_code_points() {
        let text = "hello world";
        let index = LineIndex::new(text);
        assert_eq!(position(&index, text, TextSize::from(6)), (1, 7));
    }

    /// "é" is a 2-byte UTF-8 character. The byte offset right after it is
    /// 3, which `LineIndex::line_column` reports as a 0-based byte column
    /// of 3 (a naive `+ 1` would report column 4), but only two code
    /// points (`h`, `é`) actually precede it: the reported column must be
    /// 3, not 4.
    #[test]
    fn a_multi_byte_character_before_the_offset_is_counted_once_not_by_its_byte_length() {
        let text = "héllo world";
        let index = LineIndex::new(text);
        assert_eq!(
            position(&index, text, TextSize::from(3)),
            (1, 3),
            "two code points precede the offset, not three bytes' worth",
        );
    }

    /// Byte 2 sits inside "é" (a 2-byte character spanning bytes 1 and
    /// 2): not a char boundary, so `text.get(line_start..offset)` returns
    /// `None` and the conversion must fall back to the 0-based byte
    /// column rather than panic. No real diagnostic anchors mid-character,
    /// but the fallback exists precisely so a hostile or miscomputed
    /// offset degrades instead of crashing the report.
    #[test]
    fn an_offset_landing_inside_a_multi_byte_character_falls_back_to_the_byte_column() {
        let text = "héllo world";
        let index = LineIndex::new(text);
        assert_eq!(
            position(&index, text, TextSize::from(2)),
            (1, 3),
            "falls back to the 0-based byte column (2), plus one",
        );
    }
}
