//! The check screen: the report, and the internal errors under it.
//!
//! The report body itself is the rules crate's rustc-style renderer
//! (design section 9). This module owns only what the CLI owns: the
//! notice block, the block separators, the summary line, the
//! `celerrate explain` trailer, and the internal-error report. The
//! session is the renderer's source of text and display paths, and the
//! database its render-time symbol resolver; both are read here, at
//! presentation time, outside every query.

use std::io::{self, Write};

use celerrate_diagnostics::DiagnosticId;
use celerrate_rules::render::{
    ColorMode, FaultInjection, RenderFailure, SourceAccess, explain_pointers,
    render_report as render_blocks, resolve::DatabaseResolver,
};
use celerrate_source::FileId;

use crate::analysis::AnalysisOutcome;
use crate::session::{InternalError, Session};

/// Where a bug report goes. Pre-filled: a user who hits an internal error
/// should not have to compose anything.
const ISSUE_INVITATION: &str = "https://github.com/celerrate/celerrate/issues/new?labels=internal-error&title=internal+error+while+checking";

/// The complete check screen: the report, then the internal errors.
/// The watch cycle uses this whole; the single-pass path calls the two
/// halves itself so the fix trailer can sit between them.
///
/// The body is buffered so the immutable borrow the renderer needs ends
/// before the failures are absorbed: a rich-rendering failure becomes an
/// internal error, and the internal errors print under the report it
/// fell back inside.
pub fn render_check(
    output: &mut dyn Write,
    session: &mut Session,
    outcome: &AnalysisOutcome,
    color: ColorMode,
) -> io::Result<()> {
    let failures = {
        let mut body: Vec<u8> = Vec::new();
        let failures = render_report(&mut body, session, outcome, color)?;
        output.write_all(&body)?;
        failures
    };
    session.absorb_render_failures(failures);
    render_internal_errors(output, session)
}

/// The CLI's view of its sources, for the renderer: display paths from
/// the VFS, text from the decode query. Both borrow the session.
struct SessionSources<'a> {
    session: &'a Session,
}

impl SourceAccess for SessionSources<'_> {
    fn display_path(&self, file: FileId) -> Option<String> {
        Some(display_path(self.session, file))
    }

    fn text(&self, file: FileId) -> Option<&str> {
        let source = self.session.sources.get(&file)?;
        celerrate_db::source_text(&self.session.database, *source)
            .as_ref()
            .ok()
            .map(|text| text.text())
    }
}

/// Notices, then one rustc-style block per diagnostic, then the
/// summary, then the `celerrate explain` trailer. No internal errors:
/// the single-pass path prints those last, after the fix trailer,
/// through `render_internal_errors`.
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
///
/// Answers the diagnostics whose rich rendering failed, so the caller
/// can absorb them: the report itself stays intact, because each one
/// fell back to the minimal one-line format.
pub fn render_report(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
    color: ColorMode,
) -> io::Result<Vec<RenderFailure>> {
    render_report_with(output, session, outcome, color, &FaultInjection::None)
}

/// The seam the fault-injection tests use; production always passes
/// [`FaultInjection::None`] through [`render_report`].
pub(crate) fn render_report_with(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
    color: ColorMode,
    fault: &FaultInjection,
) -> io::Result<Vec<RenderFailure>> {
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

    let sources = SessionSources { session };
    let resolver = DatabaseResolver::new(&session.database, session.files);
    let report = render_blocks(&outcome.diagnostics, &sources, &resolver, color, fault);
    for block in &report.blocks {
        writeln!(output, "{block}")?;
        writeln!(output)?;
    }

    writeln!(
        output,
        "{}, {}",
        count(notices.len(), "notice", "notices"),
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
    )?;

    let mut identifiers: Vec<DiagnosticId> =
        notices.iter().map(|notice| notice.identifier()).collect();
    identifiers.extend(outcome.diagnostics.iter().map(|diagnostic| diagnostic.id));
    let pointers = explain_pointers(identifiers);
    if !pointers.is_empty() {
        writeln!(output)?;
        write!(output, "{pointers}")?;
    }

    Ok(report.failures)
}

/// How many leading blocks fit a line budget, and how many diagnostics
/// that hides. Each block costs its lines plus one separator line. At
/// least one block always shows: a frame that hides every diagnostic
/// while reporting a nonzero count would read as broken.
fn capped_blocks(blocks: &[String], budget: usize) -> (usize, usize) {
    let mut used = 0usize;
    let mut shown = 0usize;
    for block in blocks {
        let cost = block.lines().count() + 1;
        if shown > 0 && used + cost > budget {
            break;
        }
        used += cost;
        shown += 1;
    }
    (shown, blocks.len().saturating_sub(shown))
}

/// How many rows [`render_internal_errors`] will actually write for
/// `session`'s already-accumulated internal errors plus `new_failures`
/// more (the render failures this cycle produced, about to be absorbed
/// into that same list right before it runs). Derived from that
/// function's own shape, not a guess: a blank separator line, exactly
/// one line per error — every `InternalError` variant's match arm is a
/// single `writeln!`, so the count is uniform across variants even
/// though their messages are not — and the three-line "please report
/// it" trailer, written exactly when at least one error is a genuine
/// Celerrate bug rather than the environment's condition. A render
/// failure always becomes [`InternalError::DiagnosticRenderFailed`],
/// which is always a bug, so `new_failures` alone can settle that
/// question for the errors not yet absorbed.
fn internal_error_rows(session: &Session, new_failures: usize) -> usize {
    let total = session.internal_errors.len() + new_failures;
    if total == 0 {
        return 0;
    }
    let has_celerrate_bug = new_failures > 0
        || session.internal_errors.iter().any(|error| {
            matches!(
                error,
                InternalError::StubBlobUndecodable(_)
                    | InternalError::FilePanicked { .. }
                    | InternalError::AnalysisPanicked
                    | InternalError::FixUnappliable { .. }
                    | InternalError::DiagnosticRenderFailed { .. }
            )
        });
    let trailer = if has_celerrate_bug { 3 } else { 0 };
    1 + total + trailer
}

/// One watch cycle: the screen cleared, the complete current state
/// reprinted, then the cost. The picture is always complete, never a
/// stale log of past edits.
///
/// `height` is the terminal's row count for this cycle, read by the
/// caller outside every query so the analysis stays a pure function of
/// its inputs. When it is `Some`, the diagnostic blocks are capped so
/// the frame never scrolls itself away, and the capped list ends with an
/// "and N more diagnostics" line; the summary line below still counts
/// every diagnostic, shown or not. `None` (the one-shot report's case,
/// and a terminal size that could not be read) never caps.
///
/// The frame is assembled off-screen, capped, then written after the
/// clear so a slow render never shows a half frame. Unlike
/// [`render_check`], it does not print the `celerrate explain` trailer:
/// the frame is transient and the cap needs its rows for diagnostics
/// instead.
///
/// The budget the blocks are capped against also reserves rows for
/// [`render_internal_errors`], which this function calls below and
/// which can itself emit an unbounded number of lines: see
/// [`internal_error_rows`] for exactly how many are reserved and why.
/// Without that reservation a cycle carrying internal errors could
/// still overrun the terminal height it was supposedly capped against.
pub fn render_cycle(
    output: &mut dyn Write,
    session: &mut Session,
    outcome: &AnalysisOutcome,
    reanalyzed: usize,
    elapsed: std::time::Duration,
    color: ColorMode,
    height: Option<usize>,
) -> io::Result<()> {
    let sources = SessionSources { session: &*session };
    let resolver = DatabaseResolver::new(&session.database, session.files);
    let report = render_blocks(
        &outcome.diagnostics,
        &sources,
        &resolver,
        color,
        &FaultInjection::None,
    );

    let notices = session.notices();
    // Overhead: the notice lines plus their blank separator, the summary
    // line, one blank line, the status line, the watching line, one spare
    // row for the cursor, and the rows `render_internal_errors` will
    // actually write below for the internal errors this cycle carries
    // (the session's already-accumulated ones plus the render failures
    // this cycle produced, which is exactly what gets absorbed into the
    // same list right before that function runs).
    let overhead = if notices.is_empty() {
        0
    } else {
        notices.len() + 1
    } + 6
        + internal_error_rows(session, report.failures.len());
    let (shown, hidden) = match height {
        Some(rows) => capped_blocks(&report.blocks, rows.saturating_sub(overhead)),
        None => (report.blocks.len(), 0),
    };

    // The two ANSI codes a plain format is allowed: clear, and home.
    // Everything else the frame is styled with comes from the renderer's
    // own `ColorMode`.
    write!(output, "\x1b[2J\x1b[H")?;
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
    for block in report.blocks.iter().take(shown) {
        writeln!(output, "{block}")?;
        writeln!(output)?;
    }
    if hidden > 0 {
        writeln!(
            output,
            "and {}",
            count(hidden, "more diagnostic", "more diagnostics"),
        )?;
        writeln!(output)?;
    }
    writeln!(
        output,
        "{}, {}",
        count(notices.len(), "notice", "notices"),
        count(outcome.diagnostics.len(), "diagnostic", "diagnostics"),
    )?;

    session.absorb_render_failures(report.failures);
    render_internal_errors(output, session)?;

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
            InternalError::DiagnosticRenderFailed {
                identifier,
                location,
            } => {
                has_celerrate_bug = true;
                writeln!(
                    output,
                    "internal error: rendering {identifier} at {location} failed; \
                     the diagnostic was shown in the minimal format",
                )?;
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

    use celerrate_rules::render::{ColorMode, FaultInjection};

    use crate::analysis::AnalysisOutcome;
    use crate::render::render_report_with;
    use crate::session::{InternalError, Session};
    use crate::{Outcome, render};

    /// A real project with exactly one CEL0018, analyzed exactly as
    /// `run` analyzes it, so the fault seam is driven against a
    /// diagnostic the product itself produced. The temporary directory
    /// is answered too: the session reads it for as long as it lives.
    fn fixture_with_one_unknown_class() -> (tempfile::TempDir, Session, AnalysisOutcome) {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("src").join("Kernel.php"),
            "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n",
        )
        .unwrap();
        let session = Session::start(root.path());
        let inputs = session.inputs();
        let outcome = crate::analysis::analyze(&inputs).unwrap();
        (root, session, outcome)
    }

    /// The fallback, end to end: the faulted diagnostic still reports,
    /// in the minimal one-line format, and the failure it fell back
    /// from is named as the Celerrate bug it is.
    #[test]
    fn a_render_failure_falls_back_and_reports_an_internal_error() {
        let (_root, mut session, outcome) = fixture_with_one_unknown_class();
        let mut body: Vec<u8> = Vec::new();
        let failures = render_report_with(
            &mut body,
            &session,
            &outcome,
            ColorMode::Plain,
            &FaultInjection::ForIdentifier(
                celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
            ),
        )
        .unwrap();
        session.absorb_render_failures(failures);
        render::render_internal_errors(&mut body, &session).unwrap();
        let text = String::from_utf8(body).unwrap();

        assert!(
            text.contains(" CEL0018 "),
            "the fallback line renders: {text}"
        );
        assert!(
            !text.contains("error[CEL0018]"),
            "no rich block for the faulted one: {text}",
        );
        assert!(
            text.contains("internal error: rendering CEL0018 at "),
            "the failure is reported: {text}",
        );
        assert!(
            text.contains("This is a bug in Celerrate"),
            "the invitation follows: {text}",
        );
        assert_eq!(
            Outcome::of(outcome.diagnostics.len(), session.internal_errors.len()),
            Outcome::InternalError,
            "a render fallback exits 2, like every other internal error",
        );
        insta::assert_snapshot!("fault_fallback", text);
    }

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
        render::render_check(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            ColorMode::Plain,
        )
        .unwrap();
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
        render::render_check(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            ColorMode::Plain,
        )
        .unwrap();
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
        render::render_check(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            ColorMode::Plain,
        )
        .unwrap();
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
        render::render_check(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            ColorMode::Plain,
        )
        .unwrap();
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
        render::render_check(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            ColorMode::Plain,
        )
        .unwrap();
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
        let mut session = Session::start(root.path());

        let mut output = Vec::new();
        render::render_cycle(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            1,
            std::time::Duration::from_millis(4),
            ColorMode::Plain,
            None,
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

    /// A project with several independent unknown-class diagnostics, so
    /// the watch-mode height cap has more than one block to work with.
    /// Every file stands alone (nothing extends another file's class),
    /// so the diagnostic count does not depend on analysis order.
    fn fixture_with_many_diagnostics() -> (tempfile::TempDir, Session, AnalysisOutcome) {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        for index in 0..5 {
            std::fs::write(
                root.path().join("src").join(format!("Kernel{index}.php")),
                format!(
                    "<?php\nnamespace App;\n\nclass Kernel{index} extends Missing{index}\n{{\n}}\n"
                ),
            )
            .unwrap();
        }
        let session = Session::start(root.path());
        let inputs = session.inputs();
        let outcome = crate::analysis::analyze(&inputs).unwrap();
        (root, session, outcome)
    }

    #[test]
    fn capped_blocks_stops_before_the_budget_and_counts_the_hidden() {
        let blocks: Vec<String> = (0..5)
            .map(|index| format!("block {index}\nline\nline\nline"))
            .collect();
        // Each block is 4 lines + 1 separator = 5; a budget of 12 fits 2.
        assert_eq!(render::capped_blocks(&blocks, 12), (2, 3));
        // A huge budget fits everything.
        assert_eq!(render::capped_blocks(&blocks, 1000), (5, 0));
        // A tiny budget still shows the first block: a frame that hides
        // every diagnostic while reporting a nonzero count reads broken.
        assert_eq!(render::capped_blocks(&blocks, 1), (1, 4));
    }

    #[test]
    fn a_capped_cycle_ends_with_the_more_diagnostics_line() {
        let (_root, mut session, outcome) = fixture_with_many_diagnostics();
        let mut body: Vec<u8> = Vec::new();
        render::render_cycle(
            &mut body,
            &mut session,
            &outcome,
            outcome.diagnostics.len(),
            std::time::Duration::from_millis(4),
            ColorMode::Plain,
            Some(20),
        )
        .unwrap();
        let text = String::from_utf8(body).unwrap();
        assert!(
            text.contains("more diagnostic"),
            "the cap announces what it hid: {text}",
        );
        assert!(
            text.contains("watching for changes..."),
            "the status trailer survives the cap: {text}",
        );
    }

    #[test]
    fn an_uncapped_cycle_renders_everything() {
        let (_root, mut session, outcome) = fixture_with_many_diagnostics();
        let mut body: Vec<u8> = Vec::new();
        render::render_cycle(
            &mut body,
            &mut session,
            &outcome,
            0,
            std::time::Duration::from_millis(4),
            ColorMode::Plain,
            None,
        )
        .unwrap();
        let text = String::from_utf8(body).unwrap();
        assert!(
            !text.contains("more diagnostic"),
            "no cap without a height: {text}"
        );
    }

    /// Parses the "and N more diagnostic(s)" line a capped cycle ends
    /// with, or `0` when nothing was hidden.
    fn hidden_diagnostic_count(text: &str) -> usize {
        text.lines()
            .find_map(|line| line.strip_prefix("and ")?.split_whitespace().next())
            .and_then(|token| token.parse().ok())
            .unwrap_or(0)
    }

    /// Review finding 1 (task 10): `render_internal_errors` can emit an
    /// unbounded number of rows -- one per internal error, plus its own
    /// fixed surrounding lines -- and the pre-fix overhead never budgeted
    /// for them, so a cycle carrying internal errors could still overrun
    /// the terminal height it was supposedly capped against. Reserving
    /// those rows shrinks the block budget by exactly as much: the same
    /// height, chosen to fit exactly three of the five blocks with
    /// nothing to spare, must cap to fewer blocks once the session also
    /// carries an internal error worth the three-line bug-report trailer,
    /// and the whole frame must still respect the height.
    #[test]
    fn internal_errors_reserve_rows_and_shrink_the_capped_block_count() {
        let (_root, mut session, outcome) = fixture_with_many_diagnostics();

        // Measure a real block's cost exactly the way `capped_blocks`
        // does, and the overhead exactly the way `render_cycle` does
        // before it reserves anything for internal errors, so the chosen
        // height is derived from the real content, not guessed. Built
        // from the same trio `render_cycle` itself builds the report
        // from, since there is not yet a shared helper for it.
        let sources = render::SessionSources { session: &session };
        let resolver = render::DatabaseResolver::new(&session.database, session.files);
        let report = render::render_blocks(
            &outcome.diagnostics,
            &sources,
            &resolver,
            ColorMode::Plain,
            &FaultInjection::None,
        );
        let block_cost = report.blocks[0].lines().count() + 1;
        let notices = session.notices();
        let base_overhead = if notices.is_empty() {
            0
        } else {
            notices.len() + 1
        } + 6;
        let height = base_overhead + 3 * block_cost;

        let mut without_errors: Vec<u8> = Vec::new();
        render::render_cycle(
            &mut without_errors,
            &mut session,
            &outcome,
            outcome.diagnostics.len(),
            std::time::Duration::from_millis(4),
            ColorMode::Plain,
            Some(height),
        )
        .unwrap();
        let text_without = String::from_utf8(without_errors).unwrap();
        let hidden_without = hidden_diagnostic_count(&text_without);
        assert_eq!(
            hidden_without, 2,
            "the height was chosen to fit exactly three of the five blocks: {text_without}",
        );

        // A genuine Celerrate bug: `render_internal_errors` prints it,
        // then the three-line "please report it" trailer -- five rows
        // the pre-fix overhead never budgeted for.
        let file = *session.sources.keys().next().unwrap();
        session
            .internal_errors
            .push(InternalError::FilePanicked { file });

        let mut with_errors: Vec<u8> = Vec::new();
        render::render_cycle(
            &mut with_errors,
            &mut session,
            &outcome,
            outcome.diagnostics.len(),
            std::time::Duration::from_millis(4),
            ColorMode::Plain,
            Some(height),
        )
        .unwrap();
        let text_with = String::from_utf8(with_errors).unwrap();
        let hidden_with = hidden_diagnostic_count(&text_with);

        assert!(
            hidden_with > hidden_without,
            "reserving the internal-error rows must shrink how many blocks fit: \
             {hidden_without} hidden before, {hidden_with} after: {text_with}",
        );
        assert!(
            text_with.contains("panicked"),
            "the internal error itself still prints in the capped frame: {text_with}",
        );
        assert!(
            text_with.lines().count() <= height,
            "the frame must still respect the height budget: {} lines against a height of \
             {height}: {text_with}",
            text_with.lines().count(),
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
        render::render_check(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            ColorMode::Plain,
        )
        .unwrap();
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
