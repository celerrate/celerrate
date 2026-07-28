//! The baseline: known findings hidden from the report and the exit code.
//!
//! Entirely CLI-layer machinery. Nothing here enters a salsa query or the
//! persistent cache: the baseline is presentation, applied after analysis
//! and suppression, before rendering and the exit code.

pub mod entry;
pub mod file;

/// The fixed file name at the project root. No configuration key moves it.
pub const BASELINE_FILE_NAME: &str = "celerrate-baseline.toml";
