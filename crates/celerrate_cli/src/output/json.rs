//! `--output=json`: the versioned JSON report. Pretty-printed for stable
//! diffs, one document, one trailing newline, nothing else on stdout.

use std::io::{self, Write};

use super::model::MachineReport;

pub fn write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report).map_err(io::Error::from)?;
    writeln!(output)
}
