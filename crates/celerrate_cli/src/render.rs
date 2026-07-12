//! The preview text format, and only it.
//!
//! Temporary by design. The umbrella design fixes this shape for the
//! preview and nothing more: no color, no terminal styling, no
//! `celerrate.toml`, no JSON, SARIF or GitHub output, no baseline. A
//! diagnostic's rich anatomy (annotated spans, notes, suggestions) and
//! the richer model that can carry a spanless finding honestly both
//! belong to sub-project 4, which owns diagnostic anatomy.

use std::io::{self, Write};

use celerrate_diagnostics::Diagnostic;
use celerrate_source::{FileId, TextSize};

use crate::analysis::AnalysisOutcome;
use crate::session::{InternalError, Session};

/// Where a bug report goes. Pre-filled: a user who hits an internal error
/// should not have to compose anything.
const ISSUE_INVITATION: &str = "https://github.com/celerrate/celerrate/issues/new?labels=internal-error&title=internal+error+while+checking";

/// Notices, then diagnostics, then the summary. Notices come first and in
/// their own shape because a project-level finding has no span:
/// `MISSING_COMPOSER_MANIFEST` describes a file that by definition does
/// not exist, and anchoring it to `composer.json:1:1` would be a fiction.
pub fn render_check(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
) -> io::Result<()> {
    let notices = session.notices();
    if !notices.is_empty() {
        for notice in notices {
            writeln!(
                output,
                "warning {}: {}",
                notice.identifier().as_str(),
                notice.message(),
            )?;
        }
        writeln!(output)?;
    }

    if !outcome.diagnostics.is_empty() {
        for diagnostic in &outcome.diagnostics {
            writeln!(output, "{}", render_diagnostic(session, diagnostic))?;
        }
        writeln!(output)?;
    }

    writeln!(
        output,
        "{}, {}",
        count(notices.len(), "notice", "notices"),
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
    )?;

    render_internal_errors(output, session)
}

/// `path:line:column identifier message`, one-based, relative to the
/// project root.
fn render_diagnostic(session: &Session, diagnostic: &Diagnostic) -> String {
    let (line, column) = position(session, diagnostic.file, diagnostic.range.start());
    format!(
        "{}:{line}:{column} {} {}",
        display_path(session, diagnostic.file),
        diagnostic.id.as_str(),
        diagnostic.message,
    )
}

/// The line index is zero-based, and its column is a byte offset within
/// the line. Editors are one-based, so the renderer converts here, once.
fn position(session: &Session, file: FileId, offset: TextSize) -> (u32, u32) {
    let Some(&source) = session.sources.get(&file) else {
        return (1, 1);
    };
    let zero_based = celerrate_db::line_index(&session.database, source).line_column(offset);
    (zero_based.line + 1, zero_based.column + 1)
}

fn display_path(session: &Session, file: FileId) -> String {
    session
        .vfs
        .path(file)
        .map(|path| {
            path.strip_prefix(&session.discovery.root)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .unwrap_or_else(|| format!("<file {}>", file.as_u32()))
}

fn count(total: usize, singular: &str, plural: &str) -> String {
    if total == 1 {
        format!("{total} {singular}")
    } else {
        format!("{total} {plural}")
    }
}

/// The internal-error report: what went wrong, which file, and how to
/// tell us. A panic does not kill the run, so this prints at the end,
/// after every file that did report.
///
/// `FileUnreadable` is named like every other internal error, but it is
/// not Celerrate's bug: a permission-denied file or a dangling symlink is
/// the environment's fault, and inviting a bug report for it would be a
/// lie. The "please report it" trailer therefore only prints when at
/// least one internal error is a genuine Celerrate bug (any variant other
/// than `FileUnreadable`). When every internal error is an unreadable
/// file, the report ends after listing them.
pub fn render_internal_errors(output: &mut dyn Write, session: &Session) -> io::Result<()> {
    if session.internal_errors.is_empty() {
        return Ok(());
    }
    writeln!(output)?;
    let mut has_celerrate_bug = false;
    for error in &session.internal_errors {
        match error {
            InternalError::StubBlobUndecodable(reason) => {
                has_celerrate_bug = true;
                writeln!(
                    output,
                    "internal error: the embedded stub index could not be decoded: {reason}",
                )?;
            }
            InternalError::FileUnreadable { path, reason } => writeln!(
                output,
                "internal error: {} could not be read: {reason}",
                path.display(),
            )?,
            InternalError::FilePanicked { file } => {
                has_celerrate_bug = true;
                writeln!(
                    output,
                    "internal error: analyzing {} panicked",
                    display_path(session, *file),
                )?;
            }
            InternalError::AnalysisPanicked => {
                has_celerrate_bug = true;
                writeln!(output, "internal error: the analysis loop panicked")?;
            }
        }
    }
    if !has_celerrate_bug {
        return Ok(());
    }
    writeln!(output)?;
    writeln!(
        output,
        "This is a bug in Celerrate, and it should never happen. Please report it:",
    )?;
    writeln!(output, "  {ISSUE_INVITATION}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use crate::analysis::AnalysisOutcome;
    use crate::session::{InternalError, Session};
    use crate::{Outcome, render};

    #[test]
    fn a_panicked_file_is_named_and_invites_a_report() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("Broken.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        let file = *session.sources.keys().next().unwrap();
        session
            .internal_errors
            .push(InternalError::FilePanicked { file });

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &AnalysisOutcome::default()).unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(text.contains("internal error: analyzing Broken.php panicked"));
        assert!(text.contains("Please report it:"));
        assert!(text.contains("github.com/celerrate/celerrate/issues/new"));
        assert_eq!(
            Outcome::of(0, session.internal_errors.len()),
            Outcome::InternalError,
            "a panic exits 2, even when nothing else was reported",
        );
    }

    #[test]
    fn an_undecodable_stub_blob_is_reported_not_panicked() {
        // The blob that ships is valid, so this drives the report
        // directly. What matters is that the failure has a rendering at
        // all: `embedded_stub_index` hands the composition root a value
        // to report, and the run falls back to an empty index.
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        session
            .internal_errors
            .push(InternalError::StubBlobUndecodable(
                celerrate_stubs::StubBlobError::BadMagic,
            ));

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &AnalysisOutcome::default()).unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(text.contains("internal error: the embedded stub index could not be decoded"));
        assert!(text.contains("Please report it:"));
    }

    /// A permission-denied file (or a dangling symlink, or a deletion
    /// race) is the environment's fault, not Celerrate's. The report
    /// still names the file and the reason, but it must not invite a bug
    /// report for something that is not a bug in the tool.
    #[test]
    fn an_unreadable_file_alone_is_reported_without_a_bug_invitation() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        session.internal_errors.push(InternalError::FileUnreadable {
            path: root.path().join("Locked.php"),
            reason: "Permission denied (os error 13)".to_owned(),
        });

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &AnalysisOutcome::default()).unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(text.contains("Locked.php"));
        assert!(text.contains("Permission denied"));
        assert!(
            !text.contains("Please report it"),
            "an environment failure is not a Celerrate bug: {text}",
        );
        assert!(
            !text.contains("github.com"),
            "no issue link when there is nothing to report: {text}",
        );
    }

    /// When an unreadable file sits alongside a genuine Celerrate bug,
    /// both are named, and the bug-report invitation still appears,
    /// because at least one internal error really is Celerrate's fault.
    #[test]
    fn an_unreadable_file_alongside_a_real_bug_still_invites_a_report() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("Broken.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        let file = *session.sources.keys().next().unwrap();
        session.internal_errors.push(InternalError::FileUnreadable {
            path: root.path().join("Locked.php"),
            reason: "Permission denied (os error 13)".to_owned(),
        });
        session
            .internal_errors
            .push(InternalError::FilePanicked { file });

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &AnalysisOutcome::default()).unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(text.contains("Locked.php"));
        assert!(text.contains("Broken.php"));
        assert!(text.contains("Please report it:"));
    }
}
