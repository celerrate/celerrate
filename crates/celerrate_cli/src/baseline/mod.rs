//! The baseline: known findings hidden from the report and the exit code.
//!
//! Entirely CLI-layer machinery. Nothing here enters a salsa query or the
//! persistent cache: the baseline is presentation, applied after analysis
//! and suppression, before rendering and the exit code.

pub mod entry;
pub mod file;
pub mod symbol;

/// The fixed file name at the project root. No configuration key moves it.
pub const BASELINE_FILE_NAME: &str = "celerrate-baseline.toml";

/// How this run treats the baseline file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A present file applies automatically (the default).
    Apply,
    /// `--baseline`: record or rewrite the file from the current findings.
    Record,
    /// `--ignore-baseline`: strict run, the file is not consulted.
    Ignore,
}

impl Mode {
    pub fn of(record: bool, ignore: bool) -> Self {
        // clap guarantees record and ignore are mutually exclusive.
        if record {
            Self::Record
        } else if ignore {
            Self::Ignore
        } else {
            Self::Apply
        }
    }
}
