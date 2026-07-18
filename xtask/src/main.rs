use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let outcome = match (arguments.next().as_deref(), arguments.next().as_deref()) {
        (Some("bench"), None) => xtask::bench::run(false),
        (Some("bench"), Some("--ceilings")) => xtask::bench::run(true),
        (Some("memory"), None) => xtask::memory::run(false),
        (Some("memory"), Some("--ceiling")) => xtask::memory::run(true),
        (Some("codegen"), None) => xtask::codegen::run(),
        (Some("dependency-shape"), None) => xtask::dependency_shape::run(),
        (Some("fetch-stubs"), None) => xtask::stubs::fetch(),
        (Some("compile-stubs"), None) => xtask::stubs::compile(false),
        (Some("compile-stubs"), Some("--check")) => xtask::stubs::compile(true),
        (Some("fetch-corpus"), None) => xtask::corpus::prepare().map(|_| ()),
        (Some("corpus"), None) => xtask::corpus::check_snapshot(false),
        (Some("corpus"), Some("--bless")) => xtask::corpus::check_snapshot(true),
        (Some("ground-truth"), None) => xtask::ground_truth::run(false),
        (Some("ground-truth"), Some("--bless")) => xtask::ground_truth::run(true),
        (Some("mixed-rate"), None) => xtask::mixed_rate::check(false),
        (Some("mixed-rate"), Some("--bless")) => xtask::mixed_rate::check(true),
        (Some("fetch-phpdoc-parser"), None) => xtask::phpdoc_corpus::fetch().map(|_| ()),
        (Some("phpdoc-cases"), None) => xtask::phpdoc_corpus::extract(false),
        (Some("phpdoc-cases"), Some("--check")) => xtask::phpdoc_corpus::extract(true),
        (Some("release-notes"), Some(version)) => xtask::release::run(version),
        _ => {
            eprintln!(
                "usage: cargo xtask <codegen | dependency-shape | fetch-stubs | compile-stubs [--check] | fetch-corpus | corpus [--bless] | ground-truth [--bless] | mixed-rate [--bless] | fetch-phpdoc-parser | phpdoc-cases [--check] | bench [--ceilings] | memory [--ceiling] | release-notes <version>>"
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
