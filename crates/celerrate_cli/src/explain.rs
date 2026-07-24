//! The `celerrate explain` subcommand: prints the embedded page for
//! one diagnostic identifier. Pure formatting over the registry — no
//! analysis session, no color, no environment reads.

use std::io::{self, Write};

use celerrate_diagnostics::RegisteredDiagnostic;

/// Renders `entry`'s page. Every registry entry carries a page (the
/// field is mandatory), so lookup failures cannot reach this function.
pub(crate) fn render_page(entry: &RegisteredDiagnostic, output: &mut dyn Write) -> io::Result<()> {
    let page = entry.explain;
    writeln!(output, "{}: {}", entry.id.as_str(), entry.family)?;
    writeln!(output)?;
    writeln!(output, "{}", page.why.trim_end())?;
    writeln!(output)?;
    writeln!(output, "failing example:")?;
    writeln!(output)?;
    write_indented(page.failing_example, output)?;
    writeln!(output)?;
    writeln!(output, "fixed example:")?;
    writeln!(output)?;
    write_indented(page.fixed_example, output)?;
    writeln!(output)?;
    writeln!(output, "{}", page.configuration.trim_end())?;
    Ok(())
}

fn write_indented(text: &str, output: &mut dyn Write) -> io::Result<()> {
    for line in text.trim_end().lines() {
        if line.is_empty() {
            writeln!(output)?;
        } else {
            writeln!(output, "    {line}")?;
        }
    }
    Ok(())
}
