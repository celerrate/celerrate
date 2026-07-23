//! The preview text format, and only it.
//!
//! Temporary by design. The umbrella design fixes this shape for the
//! preview and nothing more: no color, no terminal styling, no
//! `celerrate.toml`, no JSON, SARIF or GitHub output, no baseline. A
//! diagnostic's rich anatomy (annotated spans, notes, suggestions) and
//! the richer model that can carry a spanless finding honestly both
//! belong to sub-project 4, which owns diagnostic anatomy.

use std::io::{self, Write};

use celerrate_diagnostics::{Anchor, Diagnostic};
use celerrate_source::{FileId, TextSize};

use crate::analysis::AnalysisOutcome;
use crate::session::{InternalError, Session};

/// Where a bug report goes. Pre-filled: a user who hits an internal error
/// should not have to compose anything.
const ISSUE_INVITATION: &str = "https://github.com/celerrate/celerrate/issues/new?labels=internal-error&title=internal+error+while+checking";

/// The complete check screen: the report, then the internal errors.
/// The watch cycle uses this whole; the single-pass path calls the two
/// halves itself so the fix trailer can sit between them.
pub fn render_check(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
) -> io::Result<()> {
    render_report(output, session, outcome)?;
    render_internal_errors(output, session)
}

/// Notices, then diagnostics with their note and help sub-lines, then
/// the summary. No internal errors: the single-pass path prints those
/// last, after the fix trailer, through `render_internal_errors`.
///
/// Notices come first and in their own shape because a project-level
/// finding has no span: `MISSING_COMPOSER_MANIFEST` describes a file
/// that by definition does not exist, and anchoring it to
/// `composer.json:1:1` would be a fiction.
///
/// A notice announces itself as a notice, never as a warning. It is
/// counted as a notice in the summary and it never touches the exit code,
/// so the other word would contradict the same screen twice: a warning
/// diagnostic exits 1, and every notice announces a fallback already
/// taken.
pub fn render_report(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
) -> io::Result<()> {
    let notices = session.notices();
    if !notices.is_empty() {
        for notice in notices {
            writeln!(
                output,
                "notice {}: {}",
                notice.identifier().as_str(),
                notice.message(),
            )?;
        }
        writeln!(output)?;
    }

    if !outcome.diagnostics.is_empty() {
        for diagnostic in &outcome.diagnostics {
            writeln!(output, "{}", render_diagnostic(session, diagnostic))?;
            for note in &diagnostic.notes {
                writeln!(output, "  note: {note}")?;
            }
            for suggestion in &diagnostic.suggestions {
                writeln!(output, "  help: {}", suggestion.message)?;
            }
        }
        writeln!(output)?;
    }

    writeln!(
        output,
        "{}, {}",
        count(notices.len(), "notice", "notices"),
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
    )
}

/// One watch cycle: the screen cleared, the complete current state
/// reprinted, then the cost. The picture is always complete, never a
/// stale log of past edits.
pub fn render_cycle(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
    reanalyzed: usize,
    elapsed: std::time::Duration,
) -> io::Result<()> {
    // The two ANSI codes a plain format is allowed: clear, and home. No
    // color, no styling, no terminal crate.
    write!(output, "\x1b[2J\x1b[H")?;
    render_check(output, session, outcome)?;
    writeln!(output)?;
    writeln!(
        output,
        "{}  |  {}  |  {}ms",
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
        count(reanalyzed, "file re-analyzed", "files re-analyzed"),
        elapsed.as_millis(),
    )?;
    writeln!(output, "watching for changes...")?;
    output.flush()
}

/// `path:line:column identifier message`, one-based, relative to the
/// project root.
fn render_diagnostic(session: &Session, diagnostic: &Diagnostic) -> String {
    match diagnostic.anchor {
        Anchor::Span { file, range } => {
            let (line, column) = position(session, file, range.start());
            format!(
                "{}:{line}:{column} {} {}",
                display_path(session, file),
                diagnostic.id.as_str(),
                diagnostic.message,
            )
        }
        Anchor::Project => format!("notice {}: {}", diagnostic.id.as_str(), diagnostic.message),
    }
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
        .map(|path| relative_path(session, path))
        .unwrap_or_else(|| format!("<file {}>", file.as_u32()))
}

/// Every path in the report is relative to the project root.
///
/// It is reached two ways, and both are trimmed here so the report reads
/// consistently. A diagnostic names a `FileId`, which resolves back to a
/// path through the `Vfs`. An `InternalError` carries a `PathBuf` outright:
/// `FileUnreadable` and `PathUnwatchable` are about a path, not about a
/// file that reported. (An unreadable file does still get a `FileId`:
/// `Session::load` interns it through the `Vfs` on the failing arm too, and
/// enters it into the analyzed set with empty bytes. It is the internal
/// error that carries the path, not the absence of an identity.)
///
/// The project root is the one path the trimming cannot shorten: a
/// manifest that declares no autoload makes the root its own walk root, so
/// a refused watch can name it, and trimming it against itself would leave
/// nothing to print. It names itself in full instead.
fn relative_path(session: &Session, path: &std::path::Path) -> String {
    let relative = path.strip_prefix(&session.discovery.root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        return path.display().to_string();
    }
    relative.display().to_string()
}

fn count(total: usize, singular: &str, plural: &str) -> String {
    if total == 1 {
        format!("{total} {singular}")
    } else {
        format!("{total} {plural}")
    }
}

/// The fix trailer: what was applied, what was skipped and why.
/// Prints only under a fix flag. `applied 0 fixes to 0 files` is the
/// honest line the design requires: at closure of this sub-project
/// every shipped fix is `NeedsReview`, so `--fix` alone applies
/// nothing, visibly.
pub fn render_fix_summary(
    output: &mut dyn Write,
    session: &Session,
    planned: &crate::fix::PlannedFixes,
    applied: &crate::fix::AppliedFixes,
) -> io::Result<()> {
    writeln!(
        output,
        "applied {} to {}",
        count(planned.accepted, "fix", "fixes"),
        count(applied.files_written, "file", "files"),
    )?;
    for skipped in &planned.skipped {
        let reason = match skipped.reason {
            crate::fix::SkipReason::Overlap => "overlaps an already-applied fix",
            crate::fix::SkipReason::ForeignFile => "edits another file",
        };
        writeln!(
            output,
            "skipped fix in {}: {} ({reason})",
            display_path(session, skipped.file),
            skipped.message,
        )?;
    }
    writeln!(output)
}

/// The internal-error report: what went wrong, which file, and how to
/// tell us. A panic does not kill the run, so this prints at the end,
/// after every file that did report.
///
/// `FileUnreadable`, `DirectoryUnreadable` and `PathUnwatchable` are named
/// like every other internal error, but they are not Celerrate's bug: a
/// permission-denied file, a directory nobody may list, a dangling
/// symlink, an autoload directory the project declares before creating it,
/// an operating system that will not extend its watch budget, are all the
/// environment's condition, and inviting a bug report for one would be a
/// lie. The "please report it" trailer therefore only prints when at least
/// one internal error is a genuine Celerrate bug. When every internal
/// error is the environment's, the report ends after listing them.
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
                relative_path(session, path),
            )?,
            InternalError::DirectoryUnreadable { path, reason } => writeln!(
                output,
                "internal error: the directory {} could not be read: {reason}; nothing under it was analyzed",
                relative_path(session, path),
            )?,
            InternalError::PathUnwatchable { path, reason } => writeln!(
                output,
                "internal error: {} could not be watched: {reason}",
                relative_path(session, path),
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
            InternalError::FixUnappliable { file, reason } => {
                has_celerrate_bug = true;
                writeln!(
                    output,
                    "internal error: the fix for {} could not be applied: {reason}",
                    display_path(session, *file),
                )?;
            }
            InternalError::FixWriteFailed { path, reason } => writeln!(
                output,
                "internal error: {} could not be written: {reason}; the fix was not applied",
                relative_path(session, path),
            )?,
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

    /// A directory the project declares before creating it, and an
    /// operating system that will not extend its watch budget, are both the
    /// environment's condition, not Celerrate's bug. The report names the
    /// path and the refusal, so the user is never left with `watching for
    /// changes...` printed over a watch that is partly dead, and it invites
    /// no bug report for something that is not one.
    #[test]
    fn an_unwatchable_path_alone_is_reported_without_a_bug_invitation() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        session
            .internal_errors
            .push(InternalError::PathUnwatchable {
                path: root.path().join("tests"),
                reason: "No path was found.".to_owned(),
            });

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &AnalysisOutcome::default()).unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(
            text.contains("internal error: tests could not be watched"),
            "{text}"
        );
        assert!(text.contains("No path was found."));
        assert!(
            !text.contains("Please report it"),
            "an environment condition is not a Celerrate bug: {text}",
        );
        assert!(!text.contains("github.com"), "{text}");
    }

    /// A manifest that declares no autoload makes the project root its own
    /// walk root, and trimming that path against itself would leave nothing
    /// to print: a refusal must never be reported about the empty path.
    #[test]
    fn an_unwatchable_project_root_names_itself_in_full() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        session
            .internal_errors
            .push(InternalError::PathUnwatchable {
                path: root.path().to_path_buf(),
                reason: "OS file watch limit reached.".to_owned(),
            });

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &AnalysisOutcome::default()).unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(
            text.contains(&format!(
                "internal error: {} could not be watched",
                root.path().display(),
            )),
            "{text}",
        );
    }

    #[test]
    fn a_cycle_reprints_the_complete_state_and_the_timing() {
        // The picture is always complete, never a stale log of past edits,
        // and the timing line is where the differentiator stops being a
        // claim: the user sees that number on every save.
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let session = Session::start(root.path());

        let mut output = Vec::new();
        render::render_cycle(
            &mut output,
            &session,
            &AnalysisOutcome::default(),
            1,
            std::time::Duration::from_millis(4),
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(
            text.starts_with("\x1b[2J\x1b[H"),
            "the screen is cleared first"
        );
        assert!(text.contains("0 diagnostics  |  1 file re-analyzed  |  4ms"));
        assert!(text.trim_end().ends_with("watching for changes..."));
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

    /// The trailer's synthetic case: the natural pass cannot yet
    /// produce an overlap (every shipped suggestion is single-edit
    /// `NeedsReview`), so the skip is driven directly through the
    /// planner's own types.
    #[test]
    fn the_fix_trailer_names_the_skipped_fix_its_file_and_its_reason() {
        use std::collections::BTreeMap;

        use celerrate_source::{TextEdit, TextRange, TextSize};

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        let file = *session.sources.keys().next().unwrap();
        let mut edits_by_file = BTreeMap::new();
        edits_by_file.insert(
            file,
            vec![TextEdit {
                file,
                range: TextRange::new(TextSize::from(6), TextSize::from(10)),
                replacement: "x".to_owned(),
            }],
        );
        let planned = crate::fix::PlannedFixes {
            accepted: 1,
            edits_by_file,
            skipped: vec![crate::fix::SkippedFix {
                file,
                message: "did you mean `save`?".to_owned(),
                reason: crate::fix::SkipReason::Overlap,
            }],
        };
        let applied = crate::fix::apply_to_disk(&mut session, &planned);
        let mut output = Vec::new();
        render::render_fix_summary(&mut output, &session, &planned, &applied).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("applied 1 fix to 1 file"), "{text}");
        assert!(
            text.contains(
                "skipped fix in a.php: did you mean `save`? (overlaps an already-applied fix)"
            ),
            "{text}",
        );
    }
}
