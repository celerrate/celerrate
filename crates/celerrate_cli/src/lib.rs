//! The composition root: the concrete database, the startup sequence, the
//! parallel analysis loop, panic isolation, rendering, exit codes.
//!
//! The binary is thin on purpose. Everything runs through
//! [`run`], which takes its arguments and its output stream as values, so
//! the end-to-end tests drive the whole product in process: no spawning,
//! no timing flakiness, and the rendering pinned exactly.

pub mod analysis;
pub mod arguments;
pub mod database;
pub mod session;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser as _;

use crate::arguments::{Arguments, Command};

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
        Command::Check { path: _, watch: _ } => {
            // Task 6 wires startup, Task 7 the analysis, Task 8 the
            // rendering, Task 10 the watch loop.
            Outcome::Clean
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{Outcome, run};

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
