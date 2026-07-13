use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let outcome = match (arguments.next().as_deref(), arguments.next().as_deref()) {
        (Some("codegen"), None) => xtask::codegen::run(),
        (Some("fetch-stubs"), None) => xtask::stubs::fetch(),
        (Some("compile-stubs"), None) => xtask::stubs::compile(false),
        (Some("compile-stubs"), Some("--check")) => xtask::stubs::compile(true),
        (Some("fetch-corpus"), None) => xtask::corpus::prepare().map(|_| ()),
        (Some("corpus"), None) => xtask::corpus::check_snapshot(false),
        (Some("corpus"), Some("--bless")) => xtask::corpus::check_snapshot(true),
        _ => {
            eprintln!(
                "usage: cargo xtask <codegen | fetch-stubs | compile-stubs [--check] | fetch-corpus | corpus [--bless] | bench [--ceilings]>"
            );
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
