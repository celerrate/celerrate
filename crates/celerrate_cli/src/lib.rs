//! The composition root: the concrete database, the startup sequence, the
//! parallel analysis loop, panic isolation, rendering, exit codes.
//!
//! The binary is thin on purpose. Everything runs through
//! [`run`], which takes its arguments and its output stream as values, so
//! the end-to-end tests drive the whole product in process: no spawning,
//! no timing flakiness, and the rendering pinned exactly.

pub mod analysis;
pub mod arguments;
pub mod cache;
pub mod database;
pub mod ground_truth;
pub mod plugins;
pub mod render;
pub mod session;
pub mod watch;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser as _;

use crate::analysis::{AnalysisOutcome, Cancelled, Panicked};
use crate::arguments::{Arguments, Command};
use crate::session::{InternalError, Session};

/// How the run ended, and therefore what the shell is told.
///
/// The umbrella design fixes the codes: 0 clean, 1 any diagnostic
/// reported (warning or error alike), 2 internal error. A usage error
/// also exits 2: the run did not complete. Notices never affect the exit
/// code, because each one announces a fallback already taken, and
/// zero-configuration must never block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    DiagnosticsReported,
    InternalError,
    UsageError,
}

impl Outcome {
    /// Two dominates one: an internal error is never masked by the
    /// diagnostics the run did manage to produce.
    pub fn of(diagnostics: usize, internal_errors: usize) -> Self {
        if internal_errors > 0 {
            Self::InternalError
        } else if diagnostics > 0 {
            Self::DiagnosticsReported
        } else {
            Self::Clean
        }
    }

    pub fn exit_code(self) -> ExitCode {
        match self {
            Self::Clean => ExitCode::SUCCESS,
            Self::DiagnosticsReported => ExitCode::from(1),
            Self::InternalError | Self::UsageError => ExitCode::from(2),
        }
    }
}

/// The whole product, as a function.
pub fn run(arguments: Vec<OsString>, output: &mut dyn Write) -> Outcome {
    let arguments = match Arguments::try_parse_from(arguments) {
        Ok(arguments) => arguments,
        Err(error) => {
            // clap renders `--help` and `--version` as "errors" too; both
            // are a successful run that produced no analysis.
            let _ = write!(output, "{error}");
            return if error.use_stderr() {
                Outcome::UsageError
            } else {
                Outcome::Clean
            };
        }
    };
    match arguments.command {
        Command::Check { path, watch } => {
            if let Some(message) = unusable_root(&path) {
                let _ = writeln!(output, "{message}");
                return Outcome::UsageError;
            }
            let root = match absolute_root(&path) {
                Ok(root) => root,
                Err(message) => {
                    let _ = writeln!(output, "{message}");
                    return Outcome::UsageError;
                }
            };
            let mut session = Session::start(&root);
            report_excluded_plugins(&session);
            if watch {
                return watch::watch(&mut session, output);
            }
            let inputs = session.inputs();
            let outcome = single_pass(&mut session, || analysis::analyze(&inputs));
            session.absorb_outcome(&outcome);
            if render::render_check(output, &session, &outcome).is_err() {
                return Outcome::InternalError;
            }
            cache::persist(&mut session, &outcome);
            session.statistics.report();
            Outcome::of(outcome.diagnostics.len(), session.internal_errors.len())
        }
        Command::GroundTruth { path } => {
            if let Some(message) = unusable_root(&path) {
                let _ = writeln!(output, "{message}");
                return Outcome::UsageError;
            }
            let root = match absolute_root(&path) {
                Ok(root) => root,
                Err(message) => {
                    let _ = writeln!(output, "{message}");
                    return Outcome::UsageError;
                }
            };
            let session = Session::start(&root);
            report_excluded_plugins(&session);
            if ground_truth::run(&session, output).is_err() {
                return Outcome::InternalError;
            }
            // Divergences are data, not failure: the channel's own exit
            // code is always clean whenever the analysis ran at all.
            Outcome::Clean
        }
    }
}

/// The command line's root, made absolute before anything downstream
/// sees it.
///
/// Discovery documents that its root must be absolute, because
/// `normalize_path` joins a relative path onto its base: a relative
/// root self-joined (`project` became `project/project`, `.` the empty
/// path), the walk found no such directory, and the run exited 0 under
/// an untrue notice - a green build over a project nothing looked at,
/// from the exact command the README's quick start names. Absolutizing
/// is the command line's question, like `unusable_root` below: it is
/// the one place the user's spelling meets the process's current
/// directory, kept out of every query so analysis stays a pure
/// function of its inputs.
fn absolute_root(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    match std::env::current_dir() {
        Ok(current_directory) => Ok(celerrate_vfs::normalize_path(path, &current_directory)),
        Err(reason) => Err(format!(
            "error: {} cannot be resolved: the current directory is unavailable: {reason}",
            path.display(),
        )),
    }
}

/// Why the given root cannot be analyzed, when it cannot be.
///
/// A root that is not an existing directory is a usage error: the run did
/// not complete, so it exits 2, like every other usage error. It used to
/// be a silent success. Zero-configuration discovery accepted any path,
/// found no manifest under it, announced that it was analyzing the current
/// directory (which it had not been given), walked nothing, and exited 0.
/// A typo'd path that passes is the one thing a CI-facing checker must
/// never do, and the notice was untrue on top of it.
///
/// A root that cannot be *read* is the same failure wearing a disguise.
/// `is_dir` stats the path through its parent, so it succeeds on a
/// directory whose contents no one may list. The walk then yields nothing,
/// the run reports nothing, and it exits 0 again: a green build over a
/// project nothing looked at. Listing the directory is the cheapest
/// question that separates "empty" from "unreadable", and the two must not
/// answer alike.
///
/// This is checked here, before `Session::start`, because discovery's own
/// contract is that it never fails: a path it cannot use is not its
/// question to answer, it is the command line's.
fn unusable_root(path: &std::path::Path) -> Option<String> {
    if !path.is_dir() {
        return Some(if path.exists() {
            format!(
                "error: {} is not a directory; celerrate check takes a project root",
                path.display(),
            )
        } else {
            format!("error: {} does not exist", path.display())
        });
    }
    match std::fs::read_dir(path) {
        Ok(_) => None,
        Err(reason) => Some(format!(
            "error: {} cannot be read: {reason}",
            path.display(),
        )),
    }
}

/// The composition root's registration ran inside `Session::start`,
/// before any query and before this function's caller sees the
/// session back. Any plugin it excluded is announced here, once, on
/// stderr, before either the single pass or `--watch` renders its
/// first picture: a degraded run must say so before it says anything
/// else.
fn report_excluded_plugins(session: &Session) {
    for excluded in &session.plugins.excluded {
        eprintln!(
            "warning: plugin {} excluded: {}; the run is degraded",
            excluded.name, excluded.reason,
        );
    }
}

/// One analysis pass, with the loop itself guarded.
///
/// A panic outside any file's guard is an internal error the run reports
/// and survives, exactly as it already is under `--watch`, where the
/// worker's join catches it. The single pass runs on the main thread, so
/// without this it escaped `run` and `main` and the user got a raw Rust
/// panic and exit 101 instead of the internal-error report and exit 2.
///
/// The pass is a parameter rather than a call, so a test can inject the
/// panic at the one place it can come from: there is no way to make the
/// real fan-out panic on demand, and a guard nothing ever proves catches
/// anything is a guard that quietly stops catching.
fn single_pass(
    session: &mut Session,
    pass: impl FnOnce() -> Result<AnalysisOutcome, Cancelled>,
) -> AnalysisOutcome {
    match analysis::isolated(pass) {
        // Nothing mutates the inputs in a single pass, so `Cancelled` is
        // unreachable here; treating it as an empty run is still honest,
        // and it costs nothing.
        Ok(result) => result.unwrap_or_default(),
        Err(Panicked) => {
            session
                .internal_errors
                .push(InternalError::AnalysisPanicked);
            AnalysisOutcome::default()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::{InternalError, Outcome, Session, render, run, single_pass};

    /// The single-pass path had no way to produce
    /// `InternalError::AnalysisPanicked`: `analyze` re-raises a panic that
    /// is not one file's, and `run` called it straight on the main thread,
    /// so the user got a raw Rust panic and exit 101 rather than the
    /// internal-error report and exit 2. Under `--watch` the same panic was
    /// caught by the worker's join and reported properly, which is the seam
    /// the per-task reviews could not see.
    ///
    /// This drives exactly what `run` drives, with the panic injected at the
    /// only place it can come from.
    #[test]
    fn a_panic_in_the_analysis_loop_is_reported_rather_than_killing_the_process() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());

        let outcome = single_pass(&mut session, || panic!("a bug in the analysis loop"));

        assert!(outcome.diagnostics.is_empty());
        assert_eq!(
            session.internal_errors,
            vec![InternalError::AnalysisPanicked],
            "the panic is recorded, not propagated",
        );

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &outcome).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains("internal error: the analysis loop panicked"),
            "{text}",
        );
        assert!(text.contains("Please report it:"), "{text}");
        assert_eq!(
            Outcome::of(outcome.diagnostics.len(), session.internal_errors.len()),
            Outcome::InternalError,
            "and it exits 2, not 101",
        );
    }

    #[test]
    fn the_exit_codes_are_the_ones_the_design_fixes() {
        assert_eq!(Outcome::of(0, 0), Outcome::Clean);
        assert_eq!(Outcome::of(3, 0), Outcome::DiagnosticsReported);
        assert_eq!(Outcome::of(0, 1), Outcome::InternalError);
        assert_eq!(
            Outcome::of(3, 1),
            Outcome::InternalError,
            "two dominates one",
        );
    }

    #[test]
    fn a_bad_flag_prints_its_own_message_and_exits_two() {
        let mut output = Vec::new();
        let outcome = run(
            vec!["celerrate".into(), "check".into(), "--nope".into()],
            &mut output,
        );
        assert_eq!(outcome, Outcome::UsageError);
        assert!(String::from_utf8(output).unwrap().contains("--nope"));
    }

    #[test]
    fn help_is_not_a_failure() {
        let mut output = Vec::new();
        let outcome = run(vec!["celerrate".into(), "--help".into()], &mut output);
        assert_eq!(outcome, Outcome::Clean);
        assert!(String::from_utf8(output).unwrap().contains("check"));
    }
}
