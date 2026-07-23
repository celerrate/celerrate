//! The binary: arguments in, exit code out. Every decision lives in the
//! library, where the tests can reach it. The zero-panic lints apply here
//! with no exception, which is why this returns `ExitCode` and never
//! unwraps: `stub-compiler` is the idiom.

use std::io::{IsTerminal as _, Write as _};
use std::process::ExitCode;

fn main() -> ExitCode {
    let color = celerrate_cli::color_mode(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").as_deref(),
    );
    let mut output = std::io::stdout().lock();
    let outcome = celerrate_cli::run(std::env::args_os().collect(), &mut output, color);
    let _ = output.flush();
    outcome.exit_code()
}
