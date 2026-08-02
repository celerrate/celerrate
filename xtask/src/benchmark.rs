//! The PHPStan comparison harness: measure PHPStan and Celerrate cold
//! on the same working tree in the same run, and gate the ratio, not
//! wall-clock — shared runners are too noisy for absolute thresholds,
//! but a ratio taken on one machine in one run survives them. The
//! subject is the pinned comparison corpus
//! (`xtask/comparison-corpus.pin`), not the analysis corpus: a
//! publishable ratio needs first-party code large enough that
//! rule-checking dominates both wall clocks (issue #118). The
//! sub-second incremental claim is held by `cargo xtask bench` on the
//! reference machine, not here.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// The pinned PHPStan rule level: level 5 is the closest match to the
/// enabled Celerrate families (unknown symbols and members, argument
/// checks); the residual asymmetry is disclosed in the protocol.
const PHPSTAN_RULE_LEVEL: u8 = 5;
/// Passed to PHPStan on the command line so the run does not depend on
/// the machine's `php.ini`. `2G` is ample now that the vendor tree is
/// excluded from the analyzed set: the measured runs stay far below it.
const PHPSTAN_MEMORY_LIMIT: &str = "2G";
const PHPSTAN_COLD_RUNS: u32 = 3;
const CELERRATE_COLD_RUNS: u32 = 5;

/// The gate floor for the cold ratio. Set from the reference run on
/// the comparison corpus: half the measured median ratio, so
/// shared-runner variance does not fail a healthy build while a real
/// regression — anything that halves the advantage — still does. The
/// value below is provisional until the reference run lands.
const COLD_RATIO_FLOOR: f64 = 20.0;

/// Runs the comparison and prints the medians and the ratio. With
/// `gate`, a ratio under the floor fails the run.
pub fn run(gate: bool) -> Result<()> {
    crate::bench::ensure_hyperfine()?;
    let root = crate::workspace_root()?;
    let corpus = crate::corpus::prepare_comparison()?;
    let binary = crate::release_binary()?;
    let phpstan = install_phpstan(&root)?;
    let version = installed_phpstan_version(&phpstan)?;

    let benchmark_directory = root.join("target/benchmark");
    let working = benchmark_directory.join("corpus");
    if working.exists() {
        std::fs::remove_dir_all(&working)?;
    }
    crate::bench::copy_directory(&corpus, &working)?;
    std::fs::write(working.join("celerrate.toml"), celerrate_configuration())?;

    // The PHPStan temporary directory (its result cache) and the
    // generated configuration live outside the working tree. Celerrate's
    // own configuration, by contrast, is written inside it deliberately:
    // `celerrate.toml` is project configuration, and Celerrate reads it
    // from the tree it analyzes.
    let temporary = benchmark_directory.join("phpstan-tmp");
    let configuration_path = benchmark_directory.join("phpstan.neon");
    std::fs::write(
        &configuration_path,
        phpstan_configuration(&working, &temporary),
    )?;

    println!(
        "phpstan {version}, rule level {PHPSTAN_RULE_LEVEL}, result cache off, \
         memory limit {PHPSTAN_MEMORY_LIMIT}"
    );

    let phpstan_command = format!(
        "'php' '{}' analyse --configuration '{}' --no-progress --memory-limit {PHPSTAN_MEMORY_LIMIT}",
        phpstan.display(),
        configuration_path.display(),
    );
    let phpstan_median = measure(
        &working,
        &phpstan_command,
        &format!("rm -rf '{}'", temporary.display()),
        PHPSTAN_COLD_RUNS,
        &benchmark_directory.join("phpstan-cold.json"),
    )?;

    let celerrate_command = format!("'{}' check .", binary.display());
    let celerrate_median = measure(
        &working,
        &celerrate_command,
        "rm -rf .celerrate",
        CELERRATE_COLD_RUNS,
        &benchmark_directory.join("celerrate-cold.json"),
    )?;

    let ratio = cold_ratio(phpstan_median, celerrate_median)?;
    println!("{:<16} {:>10}", "scenario", "median");
    println!("{:<16} {:>9.3}s", "phpstan cold", phpstan_median);
    println!("{:<16} {:>9.3}s", "celerrate cold", celerrate_median);
    println!("cold ratio: {ratio:.1}x");

    if gate && let Some(failure) = under_ratio_floor(ratio, COLD_RATIO_FLOOR) {
        return Err(failure.into());
    }
    Ok(())
}

/// One hyperfine invocation in the working tree. `--ignore-failure`
/// because both tools exit 1 when they report findings — a completed
/// analysis, not a failed one.
fn measure(working: &Path, command: &str, prepare: &str, runs: u32, export: &Path) -> Result<f64> {
    let status = Command::new("hyperfine")
        .current_dir(working)
        .args(["--ignore-failure", "--runs", &runs.to_string()])
        .arg("--export-json")
        .arg(export)
        .arg("--prepare")
        .arg(prepare)
        .arg(command)
        .status()?;
    if !status.success() {
        return Err(format!("hyperfine failed for: {command}").into());
    }
    crate::bench::median_seconds(&std::fs::read_to_string(export)?)
}

/// Installs the pinned PHPStan from the committed lock file and returns
/// the path of its executable.
fn install_phpstan(root: &Path) -> Result<PathBuf> {
    let package_directory = root.join("benchmarks/phpstan");
    let status = Command::new("composer")
        .current_dir(&package_directory)
        .args(["install", "--no-interaction", "--no-progress"])
        .status()?;
    if !status.success() {
        return Err("composer install failed for benchmarks/phpstan".into());
    }
    Ok(package_directory.join("vendor/bin/phpstan"))
}

fn installed_phpstan_version(phpstan: &Path) -> Result<String> {
    let output = Command::new("php").arg(phpstan).arg("--version").output()?;
    phpstan_version(&String::from_utf8_lossy(&output.stdout))
}

/// Extracts the version from `phpstan --version` output
/// ("PHPStan - PHP Static Analysis Tool 2.1.22").
pub fn phpstan_version(output: &str) -> Result<String> {
    output
        .split_whitespace()
        .last()
        .filter(|token| {
            !token.is_empty()
                && token
                    .chars()
                    .all(|character| character.is_ascii_digit() || character == '.')
        })
        .map(str::to_owned)
        .ok_or_else(|| format!("unreadable PHPStan version output: {output:?}").into())
}

/// The generated PHPStan configuration: pinned level, the corpus
/// working tree with its vendor directory excluded, and a result cache
/// directory outside the analyzed tree, wiped before every timed run so
/// every run is cold.
///
/// The exclusion is what makes the comparison a comparison. Celerrate
/// parses and indexes the whole tree so names resolve, but it reports
/// only on the project's own files: an installed dependency's finding
/// is not the user's to fix. Pointing PHPStan at the tree root without
/// the exclusion would rule-check the entire installed vendor tree
/// nobody asks it to check, against only the first-party files
/// Celerrate reports on.
///
/// Excluding the directory does not hide it from PHPStan's reflection:
/// the vendor autoloader still resolves every symbol the project's
/// files reference, which the reported findings confirm.
pub fn phpstan_configuration(analyzed_directory: &Path, temporary_directory: &Path) -> String {
    format!(
        "parameters:\n    level: {PHPSTAN_RULE_LEVEL}\n    paths:\n        - \"{}\"\n    \
         excludePaths:\n        - \"{}\"\n    tmpDir: \"{}\"\n",
        analyzed_directory.display(),
        analyzed_directory.join("vendor").display(),
        temporary_directory.display(),
    )
}

/// The configuration written into the corpus working tree so Celerrate
/// analyzes exactly what PHPStan is given. Discovery walks Composer's
/// autoload roots, and a real application routinely loads part of its own
/// code through a runtime autoloader Composer never declares: on the
/// pinned corpus that hid 1010 of 6932 first-party files from Celerrate
/// while PHPStan saw them, which is not a comparison. Pinning the include
/// set to the whole tree restores equal reported work; vendor stays
/// indexed for reflection, as it is for PHPStan.
fn celerrate_configuration() -> String {
    "[project]\ninclude = [\".\"]\n".to_string()
}

/// The published ratio: PHPStan cold median over Celerrate cold median.
pub fn cold_ratio(phpstan_median: f64, celerrate_median: f64) -> Result<f64> {
    if celerrate_median <= 0.0 {
        return Err("the Celerrate cold median is not positive".into());
    }
    Ok(phpstan_median / celerrate_median)
}

/// The gate comparison, named on failure.
pub fn under_ratio_floor(ratio: f64, floor: f64) -> Option<String> {
    (ratio < floor).then(|| format!("the cold ratio ({ratio:.1}x) is under its {floor:.0}x floor"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{cold_ratio, phpstan_configuration, phpstan_version, under_ratio_floor};

    #[test]
    fn the_phpstan_version_is_the_trailing_dotted_number() {
        let output = "PHPStan - PHP Static Analysis Tool 2.1.22\n";
        assert_eq!(phpstan_version(output).unwrap(), "2.1.22");
    }

    #[test]
    fn version_output_without_a_number_is_an_error_not_a_panic() {
        assert!(phpstan_version("").is_err());
        assert!(phpstan_version("PHPStan crashed").is_err());
    }

    #[test]
    fn the_cold_ratio_divides_the_medians() {
        assert_eq!(cold_ratio(30.0, 1.5).unwrap(), 20.0);
    }

    #[test]
    fn a_non_positive_celerrate_median_is_an_error() {
        assert!(cold_ratio(30.0, 0.0).is_err());
        assert!(cold_ratio(30.0, -1.0).is_err());
    }

    #[test]
    fn a_ratio_under_the_floor_is_named() {
        let failure = under_ratio_floor(19.9, 20.0).unwrap();
        assert!(failure.contains("19.9"));
        assert!(failure.contains("20"));
        assert!(under_ratio_floor(20.0, 20.0).is_none());
    }

    #[test]
    fn the_generated_configuration_pins_level_paths_and_temporary_directory() {
        let analyzed_directory = std::path::Path::new("/work/corpus");
        let configuration = phpstan_configuration(
            analyzed_directory,
            std::path::Path::new("/work/phpstan-tmp"),
        );
        // The two directories the caller passes are rendered whole, so
        // they appear verbatim. The vendor directory is not: the
        // function joins it, and a join carries the platform's own
        // separator. Build the expected fragment by joining too, so the
        // assertion describes what the function produces wherever the
        // test runs rather than baking in a POSIX separator.
        let vendor_directory = analyzed_directory.join("vendor");
        assert!(configuration.contains("level: 5"));
        assert!(configuration.contains("- \"/work/corpus\""));
        assert!(configuration.contains("excludePaths:"));
        assert!(configuration.contains(&format!("- \"{}\"", vendor_directory.display())));
        assert!(configuration.contains("tmpDir: \"/work/phpstan-tmp\""));
    }

    #[test]
    fn the_celerrate_configuration_includes_the_whole_tree() {
        let text = super::celerrate_configuration();
        assert!(text.contains("[project]"));
        assert!(text.contains(r#"include = ["."]"#));
    }
}
