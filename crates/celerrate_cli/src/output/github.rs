//! `--output=github`: GitHub Actions workflow commands, which GitHub
//! renders as native pull-request annotations with no further setup.
//! One line per notice, one per diagnostic, one per internal error, and
//! the human summary wording last, so the summary genuinely closes the
//! output in every case.

use std::io::{self, Write};

use super::model::{MachineReport, ReportedAnchor, ReportedSeverity};
use crate::render::count;

pub fn write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()> {
    for notice in &report.notices {
        writeln!(
            output,
            "::notice::{}",
            escape_data(&format!("{}: {}", notice.id, notice.message)),
        )?;
    }
    for diagnostic in &report.diagnostics {
        let command = match diagnostic.severity {
            ReportedSeverity::Error => "error",
            ReportedSeverity::Warning => "warning",
        };
        let text = escape_data(&format!("{}: {}", diagnostic.id, diagnostic.message));
        match &diagnostic.anchor {
            ReportedAnchor::Span(location) => writeln!(
                output,
                "::{command} file={},line={},col={},endLine={},endColumn={}::{text}",
                escape_property(&location.path),
                location.start_line,
                location.start_column,
                location.end_line,
                location.end_column,
            )?,
            ReportedAnchor::Project => writeln!(output, "::{command}::{text}")?,
        }
    }
    // Internal errors print after the diagnostics and before the summary,
    // exactly like the human channel's own internal-error report, so the
    // summary genuinely closes the output whether or not the run degraded.
    // Each one carries no file property: an internal error is a problem
    // the tool itself hit, not a finding anchored to a location in the
    // analyzed code.
    for error in &report.internal_errors {
        writeln!(output, "::error::{}", escape_data(&error.message))?;
    }
    writeln!(
        output,
        "{}, {}",
        count(report.summary.notices, "notice", "notices"),
        count(
            report.summary.errors + report.summary.warnings,
            "diagnostic",
            "diagnostics",
        ),
    )?;
    if report.summary.baselined_hidden > 0 {
        writeln!(
            output,
            "{} hidden",
            count(
                report.summary.baselined_hidden,
                "baselined diagnostic",
                "baselined diagnostics",
            ),
        )?;
    }
    Ok(())
}

/// The workflow-command data escaping GitHub documents: percent, then
/// carriage return, then newline. Percent must be escaped first, or the
/// later replacements would be double-encoded.
fn escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Property values additionally escape the separators.
fn escape_property(value: &str) -> String {
    escape_data(value).replace(',', "%2C").replace(':', "%3A")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use crate::output::model::{ReportedDiagnostic, ReportedInternalError, SpanLocation, Summary};

    #[test]
    fn data_escaping_covers_percent_and_line_breaks() {
        assert_eq!(escape_data("50% done\r\nnext"), "50%25 done%0D%0Anext");
    }

    /// A `Warning`-severity diagnostic must become a `::warning` command,
    /// not `::error`: no CLI fixture happens to produce a warning through
    /// this writer, so a swapped match arm (`Warning` silently falling
    /// through to `"error"`) would otherwise ship unnoticed. Also pins
    /// that the position properties still appear on a warning exactly as
    /// they do on an error.
    #[test]
    fn a_warning_severity_diagnostic_emits_a_warning_command_with_position_properties() {
        let location = SpanLocation {
            path: "src/Legacy.php".to_owned(),
            start_line: 10,
            start_column: 5,
            end_line: 10,
            end_column: 12,
            byte_start: 90,
            byte_end: 97,
        };
        let report = MachineReport {
            schema_version: 1,
            summary: Summary {
                warnings: 1,
                ..empty_summary()
            },
            notices: Vec::new(),
            internal_errors: Vec::new(),
            diagnostics: vec![ReportedDiagnostic {
                id: "CEL0200".to_owned(),
                severity: ReportedSeverity::Warning,
                rule: None,
                anchor: ReportedAnchor::Span(location),
                message: "calling a deprecated function".to_owned(),
                labels: Vec::new(),
                notes: Vec::new(),
                suggestions: Vec::new(),
            }],
        };
        let mut output = Vec::new();
        write(&mut output, &report).unwrap();
        let text = String::from_utf8(output).unwrap();
        let line = text
            .lines()
            .next()
            .expect("the diagnostic becomes the first line");
        assert!(
            line.starts_with("::warning"),
            "a warning severity must not emit ::error: {line}",
        );
        assert!(!line.starts_with("::error"), "{line}");
        assert_eq!(
            line,
            "::warning file=src/Legacy.php,line=10,col=5,endLine=10,endColumn=12\
             ::CEL0200: calling a deprecated function",
        );
    }

    #[test]
    fn property_escaping_also_covers_separators() {
        assert_eq!(escape_property("a,b:c"), "a%2Cb%3Ac");
    }

    fn empty_summary() -> Summary {
        Summary {
            errors: 0,
            warnings: 0,
            notices: 0,
            baselined_hidden: 0,
            internal_errors: 0,
            exit_code: 0,
        }
    }

    /// A diagnostic anchored at project scope (a configuration or
    /// project-level finding) has no file to name, and no live rule
    /// reaches this branch through a fixture: covered directly here, the
    /// way `sarif.rs` covers branches no CLI fixture reaches.
    #[test]
    fn a_project_anchored_diagnostic_emits_no_file_properties() {
        let report = MachineReport {
            schema_version: 1,
            summary: Summary {
                errors: 1,
                ..empty_summary()
            },
            notices: Vec::new(),
            internal_errors: Vec::new(),
            diagnostics: vec![ReportedDiagnostic {
                id: "CEL0100".to_owned(),
                severity: ReportedSeverity::Error,
                rule: None,
                anchor: ReportedAnchor::Project,
                message: "the project could not be fully analyzed".to_owned(),
                labels: Vec::new(),
                notes: Vec::new(),
                suggestions: Vec::new(),
            }],
        };
        let mut output = Vec::new();
        write(&mut output, &report).unwrap();
        let text = String::from_utf8(output).unwrap();
        let line = text
            .lines()
            .find(|line| line.starts_with("::error"))
            .expect("the diagnostic becomes an error annotation");
        assert_eq!(
            line,
            "::error::CEL0100: the project could not be fully analyzed",
        );
    }

    /// Internal errors become one `::error::` command each, carrying no
    /// file property, placed after the diagnostics and before the
    /// summary, so the summary genuinely closes the output in every
    /// case, even when the run degraded.
    #[test]
    fn internal_errors_become_error_annotations_after_diagnostics_before_the_summary() {
        let report = MachineReport {
            schema_version: 1,
            summary: Summary {
                errors: 1,
                internal_errors: 1,
                ..empty_summary()
            },
            notices: Vec::new(),
            internal_errors: vec![ReportedInternalError {
                kind: "file-unreadable".to_owned(),
                message: "src/Locked.php could not be read: permission denied".to_owned(),
                bug: false,
            }],
            diagnostics: vec![ReportedDiagnostic {
                id: "CEL0019".to_owned(),
                severity: ReportedSeverity::Error,
                rule: None,
                anchor: ReportedAnchor::Project,
                message: "unknown function `strlenn`".to_owned(),
                labels: Vec::new(),
                notes: Vec::new(),
                suggestions: Vec::new(),
            }],
        };
        let mut output = Vec::new();
        write(&mut output, &report).unwrap();
        let text = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let diagnostic_index = lines
            .iter()
            .position(|line| line.contains("CEL0019"))
            .expect("the diagnostic prints");
        let error_index = lines
            .iter()
            .position(|line| line.contains("could not be read"))
            .expect("the internal error prints");
        assert!(
            diagnostic_index < error_index,
            "the internal error must come after the diagnostics: {text}",
        );
        assert_eq!(
            lines[error_index],
            "::error::src/Locked.php could not be read: permission denied",
        );
        assert!(
            !lines[error_index].contains("file="),
            "an internal error carries no file property: {}",
            lines[error_index],
        );
        let last_index = lines.len() - 1;
        assert!(
            error_index < last_index,
            "the internal error must come before the summary: {text}",
        );
        let last = lines[last_index];
        assert!(last.contains("diagnostic"), "{last}");
        assert!(!last.starts_with("::"), "{last}");
    }
}
