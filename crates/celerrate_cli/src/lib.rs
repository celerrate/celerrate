//! The composition root: the concrete database, the startup sequence, the
//! parallel analysis loop, panic isolation, rendering, exit codes.
//!
//! The binary is thin on purpose. Everything runs through
//! [`run`], which takes its arguments and its output stream as values, so
//! the end-to-end tests drive the whole product in process: no spawning,
//! no timing flakiness, and the rendering pinned exactly.

pub mod analysis;
pub mod arguments;
pub mod baseline;
pub mod cache;
pub mod configuration;
pub mod database;
mod explain;
pub mod fix;
pub mod ground_truth;
mod migrate;
pub mod mixed_rate;
pub mod output;
pub mod plugins;
pub mod render;
pub mod session;
pub mod suggest;
pub mod verbose;
pub mod watch;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser as _;

use crate::analysis::{AnalysisOutcome, Cancelled, Panicked};
use crate::arguments::{Arguments, Command};
use crate::session::{InternalError, Session};

pub use celerrate_rules::render::ColorMode;

/// The color decision, pure so it is testable: styled only on a
/// terminal with `NO_COLOR` unset or empty (the no-color.org
/// convention). Read once in `main`, outside queries.
pub fn color_mode(stdout_is_terminal: bool, no_color: Option<&std::ffi::OsStr>) -> ColorMode {
    let disabled = no_color.is_some_and(|value| !value.is_empty());
    if stdout_is_terminal && !disabled {
        ColorMode::Styled
    } else {
        ColorMode::Plain
    }
}

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

    /// The numeric exit code, the single source `exit_code` wraps. The JSON
    /// summary embeds this same number, so payload and process cannot drift.
    pub fn code(self) -> u8 {
        match self {
            Self::Clean => 0,
            Self::DiagnosticsReported => 1,
            Self::InternalError | Self::UsageError => 2,
        }
    }

    pub fn exit_code(self) -> ExitCode {
        ExitCode::from(self.code())
    }
}

/// The whole product, as a function.
pub fn run(arguments: Vec<OsString>, output: &mut dyn Write, color: ColorMode) -> Outcome {
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
        Command::Check {
            path,
            watch,
            fix,
            fix_suggestions,
            baseline,
            ignore_baseline,
            output: format,
        } => {
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
            if let Some(_machine) = crate::output::MachineFormat::of(format) {
                let incompatible = [
                    (watch, "--watch"),
                    (fix, "--fix"),
                    (fix_suggestions, "--fix-suggestions"),
                    (baseline, "--baseline"),
                ]
                .into_iter()
                .find_map(|(set, flag)| set.then_some(flag));
                if let Some(flag) = incompatible {
                    let _ = writeln!(
                        output,
                        "error: --output={} cannot be combined with {flag}",
                        format.as_argument(),
                    );
                    return Outcome::UsageError;
                }
            }
            let mode = baseline::Mode::of(baseline, ignore_baseline);
            let mut session = Session::start(&root);
            report_excluded_plugins(&session);
            if watch {
                return watch::watch(&mut session, output, color, mode);
            }
            let inputs = session.inputs();
            let outcome = single_pass(&mut session, || analysis::analyze(&inputs));
            session.absorb_outcome(&outcome);
            // Presentation only: the persisted verdicts read `outcome`,
            // never the enriched copy.
            let mut presented = analysis::AnalysisOutcome {
                diagnostics: suggest::enrich(&session, &outcome.diagnostics),
                panicked: outcome.panicked.clone(),
            };
            let baseline_outcome = match mode {
                baseline::Mode::Record => {
                    match baseline::record(&session, &presented.diagnostics) {
                        Ok(recorded) => {
                            let before = presented.diagnostics.len();
                            presented
                                .diagnostics
                                .retain(|diagnostic| diagnostic.span().is_none());
                            baseline::BaselineOutcome {
                                hidden: before - presented.diagnostics.len(),
                                recorded,
                                notices: Vec::new(),
                            }
                        }
                        Err(error) => {
                            let _ = writeln!(
                                output,
                                "error: could not write {}: {error}",
                                baseline::BASELINE_FILE_NAME
                            );
                            return Outcome::InternalError;
                        }
                    }
                }
                baseline::Mode::Apply => baseline::apply(&session, &mut presented.diagnostics),
                baseline::Mode::Ignore => baseline::BaselineOutcome::default(),
            };
            // Configuration diagnostics are presentation and exit-code
            // input, never cache input: `outcome` stays untouched, so
            // the persisted verdicts cannot absorb them.
            let configuration_diagnostics =
                configuration::merge_diagnostics(&session, &mut presented);
            if let Some(machine) = crate::output::MachineFormat::of(format) {
                // Persist first: a persist-time internal error must be counted in
                // the verdict the payload embeds. Nothing after serialization can
                // change the outcome (no rich rendering, no fix on this path).
                cache::persist(&mut session, &outcome);
                let verdict = Outcome::of(
                    outcome
                        .diagnostics
                        .len()
                        .saturating_sub(baseline_outcome.hidden)
                        + configuration_diagnostics,
                    session.internal_errors.len(),
                );
                let report =
                    crate::output::model::build(&session, &presented, &baseline_outcome, verdict);
                if crate::output::write(machine, output, &report).is_err() {
                    return Outcome::InternalError;
                }
                session.statistics.report();
                return verdict;
            }
            let failures =
                match render::render_report(output, &session, &presented, color, &baseline_outcome)
                {
                    Ok(failures) => failures,
                    Err(_) => return Outcome::InternalError,
                };
            session.absorb_render_failures(failures);
            cache::persist(&mut session, &outcome);
            if let Some(threshold) = fix::fix_threshold(fix, fix_suggestions) {
                let planned = fix::plan(&presented.diagnostics, threshold);
                let applied = fix::apply_to_disk(&mut session, &planned);
                if render::render_fix_summary(output, &session, &planned, &applied).is_err() {
                    return Outcome::InternalError;
                }
            }
            if render::render_internal_errors(output, &session).is_err() {
                return Outcome::InternalError;
            }
            session.statistics.report();
            Outcome::of(
                outcome
                    .diagnostics
                    .len()
                    .saturating_sub(baseline_outcome.hidden)
                    + configuration_diagnostics,
                session.internal_errors.len(),
            )
        }
        Command::Migrate {
            path,
            from_phpstan,
            force,
        } => {
            if !from_phpstan {
                let _ = writeln!(output, "error: migrate needs a source; pass --from-phpstan");
                return Outcome::UsageError;
            }
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
            migrate::execute(&root, force, output)
        }
        Command::Explain { identifier } => {
            let normalized = identifier.to_ascii_uppercase();
            match celerrate_diagnostics::REGISTRY
                .iter()
                .find(|entry| entry.id.as_str() == normalized)
            {
                Some(entry) => {
                    if explain::render_page(entry, output).is_err() {
                        return Outcome::InternalError;
                    }
                    Outcome::Clean
                }
                None => {
                    let _ = writeln!(
                        output,
                        "error: unknown diagnostic identifier `{identifier}`",
                    );
                    let _ = writeln!(
                        output,
                        "identifiers look like CEL0030; a report names the ones it uses",
                    );
                    Outcome::UsageError
                }
            }
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
        Command::MixedRate { path } => {
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
            if mixed_rate::run(&session, output).is_err() {
                return Outcome::InternalError;
            }
            // The instrument prints counters, never diagnostics: the
            // run's own exit code is always clean whenever the
            // analysis ran at all.
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

    use super::{ColorMode, InternalError, Outcome, Session, color_mode, render, run, single_pass};

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
        render::render_check(&mut output, &mut session, &outcome, ColorMode::Plain).unwrap();
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
            ColorMode::Plain,
        );
        assert_eq!(outcome, Outcome::UsageError);
        assert!(String::from_utf8(output).unwrap().contains("--nope"));
    }

    #[test]
    fn help_is_not_a_failure() {
        let mut output = Vec::new();
        let outcome = run(
            vec!["celerrate".into(), "--help".into()],
            &mut output,
            ColorMode::Plain,
        );
        assert_eq!(outcome, Outcome::Clean);
        assert!(String::from_utf8(output).unwrap().contains("check"));
    }

    #[test]
    fn color_is_styled_only_on_a_terminal_without_no_color() {
        use std::ffi::OsStr;
        assert_eq!(color_mode(true, None), ColorMode::Styled);
        assert_eq!(color_mode(false, None), ColorMode::Plain);
        assert_eq!(color_mode(true, Some(OsStr::new("1"))), ColorMode::Plain);
        // The NO_COLOR convention: an empty value does not disable color.
        assert_eq!(color_mode(true, Some(OsStr::new(""))), ColorMode::Styled);
    }
}
