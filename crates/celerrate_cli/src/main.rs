//! The binary: arguments in, exit code out. Every decision lives in the
//! library, where the tests can reach it. The zero-panic lints apply here
//! with no exception, which is why this returns `ExitCode` and never
//! unwraps: `stub-compiler` is the idiom.

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut output = std::io::stdout().lock();
    let outcome = celerrate_cli::run(std::env::args_os().collect(), &mut output);
    let _ = output.flush();
    outcome.exit_code()
}
