use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let argument_references: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let outcome = match argument_references.as_slice() {
        ["bench"] => xtask::bench::run(false),
        ["bench", "--ceilings"] => xtask::bench::run(true),
        ["benchmark"] => xtask::benchmark::run(false),
        ["benchmark", "--gate"] => xtask::benchmark::run(true),
        ["memory"] => xtask::memory::run(false),
        ["memory", "--ceiling"] => xtask::memory::run(true),
        ["codegen"] => xtask::codegen::run(),
        ["dependency-shape"] => xtask::dependency_shape::run(),
        ["dist"] => xtask::dist::run(None),
        ["dist", "--target", triple] => xtask::dist::run(Some(triple)),
        ["emission-scan"] => xtask::emission_scan::run(),
        ["fetch-stubs"] => xtask::stubs::fetch(),
        ["compile-stubs"] => xtask::stubs::compile(false),
        ["compile-stubs", "--check"] => xtask::stubs::compile(true),
        ["fetch-corpus"] => xtask::corpus::prepare().map(|_| ()),
        ["corpus"] => xtask::corpus::check_snapshot(false),
        ["corpus", "--bless"] => xtask::corpus::check_snapshot(true),
        ["ground-truth"] => xtask::ground_truth::run(false),
        ["ground-truth", "--bless"] => xtask::ground_truth::run(true),
        ["mixed-rate"] => xtask::mixed_rate::check(false),
        ["mixed-rate", "--bless"] => xtask::mixed_rate::check(true),
        ["fetch-phpdoc-parser"] => xtask::phpdoc_corpus::fetch().map(|_| ()),
        ["phpdoc-cases"] => xtask::phpdoc_corpus::extract(false),
        ["phpdoc-cases", "--check"] => xtask::phpdoc_corpus::extract(true),
        ["release-notes", version] => xtask::release::run(version),
        _ => {
            eprintln!(
                "usage: cargo xtask <codegen | dependency-shape | dist [--target <triple>] | emission-scan | fetch-stubs | compile-stubs [--check] | fetch-corpus | corpus [--bless] | ground-truth [--bless] | mixed-rate [--bless] | fetch-phpdoc-parser | phpdoc-cases [--check] | bench [--ceilings] | benchmark [--gate] | memory [--ceiling] | release-notes <version>>"
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
