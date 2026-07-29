//! Machine output formats: pure serializations of the final stream, at
//! the edge, after suppression and the baseline. One pipeline, four
//! serializations.

pub mod github;
pub mod json;
pub mod model;
pub mod sarif;

use std::io::{self, Write};

use crate::arguments::OutputFormat;

/// The non-human formats. Converting up front keeps every writer match
/// exhaustive: adding a format extends this enum and the compiler points
/// at every dispatch site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineFormat {
    Json,
    Sarif,
    Github,
}

impl MachineFormat {
    pub fn of(format: OutputFormat) -> Option<Self> {
        match format {
            OutputFormat::Human => None,
            OutputFormat::Json => Some(Self::Json),
            OutputFormat::Sarif => Some(Self::Sarif),
            OutputFormat::Github => Some(Self::Github),
        }
    }
}

pub fn write(
    format: MachineFormat,
    output: &mut dyn Write,
    report: &model::MachineReport,
) -> io::Result<()> {
    match format {
        MachineFormat::Json => json::write(output, report),
        MachineFormat::Sarif => sarif::write(output, report),
        MachineFormat::Github => github::write(output, report),
    }
}
