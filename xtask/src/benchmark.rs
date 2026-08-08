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

/// The gate floor for the cold ratio. This is a regression guard, not
/// the published ambition. It is set at half the reference run's
/// measured median ratio, so shared-runner variance does not fail a
/// healthy build while a real regression, anything that halves the
/// advantage, still does.
///
/// The published ambition is a different figure and lives elsewhere:
/// "at least ~9x faster than PHPStan on a cold full analysis"
/// (`.claude/superpowers/specs/2026-07-09-celerrate-design.md`, section
/// 7, amended down from ~20x on 2026-08-07). Raising this floor to that
/// figure would fail every build until the ambition is reached, which
/// is the opposite of what a regression gate is for. The two move
/// independently: the ambition tracks what the project aims at, this
/// floor tracks what the last reference measurement established.
///
/// Published reference measurement (2026-08-06, commit 4bc0156, the
/// protocol machine). Wall clock: the pooled median over twenty-four
/// timed runs across three full runs, nine timed PHPStan runs and
/// fifteen timed Celerrate runs: PHPStan 39.058s, Celerrate 4.874s,
/// ratio 8.01x. CPU consumed: hyperfine reports one CPU total per
/// invocation, not per timed run, so this column is the median of the
/// three full runs' own CPU totals instead: PHPStan 242.5s, Celerrate
/// 22.0s, ratio 11.0x. The check pipeline is now parallel: the measured
/// effective core usage was Celerrate 4.51 of 10, PHPStan 6.21 of 10,
/// which is the honest account of the remaining gap between the two
/// ratios, since Celerrate's own CPU cost rose to buy the wall-clock
/// win. The three individual wall-clock ratios ranged 7.52x to 8.04x
/// (8.0135 / 2 = 4.00675, floored to 4.0).
///
/// The cold-run performance diagnostic of 2026-08-07
/// (`.claude/superpowers/plans/2026-08-07-cold-run-performance-diagnostic-measurements.md`)
/// has since measured why that effective core usage is what it is: the
/// analysis fan-out loses its cores to work expansion rather than to
/// idleness, costing 5.38 core-seconds at one thread against 14.08 at
/// ten for identical work. The same campaign measured PHPStan's own
/// cold median moving 17.5 % between two sessions on one machine in one
/// day, which is the order of variance this floor's margin exists to
/// absorb.
const COLD_RATIO_FLOOR: f64 = 4.0;

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

    // Untimed, and strictly after both timed measurements above: see
    // `analyzed_file_counts`'s doc comment for why the ordering
    // matters. This is a correctness check on the harness itself, not
    // a performance judgment, so it fails the run unconditionally,
    // gate or no gate.
    let (reported, counted) = analyzed_file_counts(&working, &binary)?;
    println!("celerrate reported {reported} files; independent filesystem count {counted} files");
    if let Some(failure) = diverging_file_counts(reported, counted, FILE_COUNT_TOLERANCE) {
        return Err(failure.into());
    }

    if gate && let Some(failure) = under_ratio_floor(ratio, COLD_RATIO_FLOOR) {
        return Err(failure.into());
    }
    Ok(())
}

/// One hyperfine invocation in the working tree. `--ignore-failure`
/// because both tools exit 1 when they report findings — a completed
/// analysis, not a failed one. `--warmup 1` discards one untimed run
/// before the timed ones: without it, the first timed run alone pays
/// for the cold filesystem page cache, which inflated the measured
/// spread past the stability criterion (issue #118). `--prepare` still
/// runs before the warmup too, so the warmup is not a loophole in
/// coldness: Celerrate's own cache and PHPStan's result cache are wiped
/// before every run, warmup included. Only the page cache is left
/// warm, which is filesystem state, not analysis work.
fn measure(working: &Path, command: &str, prepare: &str, runs: u32, export: &Path) -> Result<f64> {
    let status = Command::new("hyperfine")
        .current_dir(working)
        .args([
            "--ignore-failure",
            "--warmup",
            "1",
            "--runs",
            &runs.to_string(),
        ])
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

/// The maximum acceptable difference between Celerrate's own reported
/// file count (from `--verbose`) and an independent filesystem count
/// of the corpus's first-party `.php` files. Both counts derive from
/// the same input - the working tree with `celerrate.toml` pinning
/// `[project] include = ["."]`, vendor excluded - and nothing in that
/// configuration filters any first-party file selectively, so the two
/// are expected to agree exactly. Any divergence at all means the
/// equal-file-set invariant (section 5 of the comparison-corpus
/// design) has silently broken - a changed `include` semantic, an
/// ignored `celerrate.toml`, a future corpus that hides its files
/// differently - not that a handful of edge-case files disagree for a
/// legitimate reason.
const FILE_COUNT_TOLERANCE: usize = 0;

/// Runs `celerrate check . --verbose` once, untimed, strictly after
/// every timed run: never before, or the extra run would change what
/// hyperfine observes and invalidate the published medians. Returns
/// Celerrate's own reported file count (parsed from its `--verbose`
/// stderr) beside an independent filesystem count of the corpus's
/// first-party `.php` files, so a future divergence between the two -
/// the false green the design warns against, where the harness would
/// silently fall back to comparing an unequal file set while the gate
/// still passes - is visible rather than silent.
fn analyzed_file_counts(working: &Path, binary: &Path) -> Result<(usize, usize)> {
    let output = Command::new(binary)
        .arg("check")
        .arg(".")
        .arg("--verbose")
        .current_dir(working)
        .output()?;
    // Exit 1 means diagnostics were reported, which is a completed
    // analysis, matching every other invocation in this harness.
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "the file-count check did not complete (exit {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
        )
        .into());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reported = parse_reported_file_count(&stderr)?;
    let counted = count_first_party_php_files(working)?;
    Ok((reported, counted))
}

/// Extracts the file count from `--verbose`'s run-summary line, as
/// rendered by `render_run_summary` in
/// `crates/celerrate_cli/src/verbose.rs`:
/// `verbose: <N> project file(s) reported; verdicts ...`. A widened
/// foreign-directive line also starts with `verbose: `, so the count
/// is not just the first `verbose: ` line but the one whose remainder
/// contains `" reported;"`.
///
/// `--verbose` is explicitly documented there as not a stable surface,
/// so a wording change must fail loudly here, naming the flag, rather
/// than silently reporting nothing: a check that quietly disables
/// itself is worse than none.
pub fn parse_reported_file_count(stderr: &str) -> Result<usize> {
    for line in stderr.lines() {
        let Some(rest) = line.strip_prefix("verbose: ") else {
            continue;
        };
        if !rest.contains(" reported;") {
            continue;
        }
        let Some(token) = rest.split_whitespace().next() else {
            continue;
        };
        return token
            .parse::<usize>()
            .map_err(|error| format!("unreadable project-file count in {line:?}: {error}").into());
    }
    Err(format!(
        "no project-files-reported line in `celerrate check --verbose` stderr; \
         --verbose is not a stable surface (see crates/celerrate_cli/src/verbose.rs) \
         and its wording may have changed:\n{stderr}"
    )
    .into())
}

/// Counts every `.php` file outside the corpus root's own `vendor/`,
/// `.git/`, and `.celerrate/`, walked from `root`: the independent
/// ground truth [`parse_reported_file_count`]'s figure is checked
/// against.
///
/// Only `root`'s own top-level `vendor` directory is excluded, by
/// path, not by name at every depth: a real corpus routinely contains
/// first-party directories that merely happen to share the name (test
/// fixtures, per-asset directories under a theme), and excluding every
/// occurrence undercounted the pinned comparison corpus by 45 files -
/// the exact kind of silent divergence this check exists to catch, if
/// it had shipped uncaught.
///
/// `DirEntry::file_type` does not follow symlinks, so a symlinked
/// directory is never descended into - the same discipline
/// `crate::bench::copy_directory` applies, and for the same reason.
/// The extension match is case-insensitive, mirroring
/// `has_php_extension` in `crates/celerrate_vfs/src/walk.rs`, which
/// decides what Celerrate's own discovery counts as a PHP file.
pub fn count_first_party_php_files(root: &Path) -> Result<usize> {
    let vendor = root.join("vendor");
    let mut count = 0usize;
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            if path == vendor || name == ".git" || name == ".celerrate" {
                continue;
            }
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("php"))
            {
                count += 1;
            }
        }
    }
    Ok(count)
}

/// The file-count comparison, named on failure, like
/// [`under_ratio_floor`].
pub fn diverging_file_counts(reported: usize, counted: usize, tolerance: usize) -> Option<String> {
    (reported.abs_diff(counted) > tolerance).then(|| {
        format!(
            "celerrate reported {reported} files but the independent filesystem count is \
             {counted}: the equal-file-set invariant may have silently broken"
        )
    })
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

    #[test]
    fn the_reported_line_yields_its_file_count_in_the_plural_form() {
        let stderr = "verbose: 6932 project files reported; verdicts 0 served / \
                       0 discarded / 6932 absent from the cache\n";
        assert_eq!(super::parse_reported_file_count(stderr).unwrap(), 6932);
    }

    #[test]
    fn the_reported_line_yields_its_file_count_in_the_singular_form() {
        let stderr = "verbose: 1 project file reported; verdicts 0 served / \
                       0 discarded / 1 absent from the cache\n";
        assert_eq!(super::parse_reported_file_count(stderr).unwrap(), 1);
    }

    #[test]
    fn a_widened_directive_line_does_not_confuse_the_summary_line() {
        // A widened-directive line also starts with "verbose: " but
        // names a file and a line, not a count; the parser must not
        // stop at the first "verbose: " line, and must not mistake a
        // non-numeric token for the count.
        let stderr = "verbose: src/a.php:3: unmapped identifier `x`: the directive \
                       widens to scope-wide suppression\n\
                       verbose: 42 project files reported; verdicts 0 served / \
                       0 discarded / 42 absent from the cache\n";
        assert_eq!(super::parse_reported_file_count(stderr).unwrap(), 42);
    }

    #[test]
    fn stderr_without_a_reported_line_is_an_error_not_a_panic() {
        // `--verbose` is explicitly not a stable surface; a wording
        // change must fail loudly, naming the flag, rather than
        // silently reporting nothing.
        let error = super::parse_reported_file_count("nothing here\n").unwrap_err();
        assert!(error.to_string().contains("--verbose"));
    }

    #[test]
    fn first_party_php_files_are_counted_outside_vendor_and_the_cache() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/A.php"), "<?php").unwrap();
        std::fs::write(root.path().join("src/README.md"), "# hi").unwrap();
        std::fs::create_dir_all(root.path().join("vendor/pkg")).unwrap();
        std::fs::write(root.path().join("vendor/pkg/B.php"), "<?php").unwrap();
        std::fs::create_dir_all(root.path().join(".celerrate/cache")).unwrap();
        std::fs::write(root.path().join(".celerrate/cache/C.php"), "<?php").unwrap();
        assert_eq!(super::count_first_party_php_files(root.path()).unwrap(), 1);
    }

    #[test]
    fn a_nested_directory_that_merely_shares_the_vendor_name_is_still_counted() {
        // PrestaShop's own first-party tree contains directories named
        // `vendor` that are not the Composer vendor tree at all:
        // `tests/Resources/modules/<x>/vendor` fixtures and per-asset
        // `vendor` directories under its theme folders. Only the
        // corpus root's own top-level `vendor/` is the installed
        // dependency tree that PHPStan's `excludePaths` and Celerrate's
        // discovery both exclude; excluding every directory that
        // merely shares the name undercounts the real first-party set
        // (45 files on the pinned comparison corpus).
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("vendor/pkg")).unwrap();
        std::fs::write(root.path().join("vendor/pkg/B.php"), "<?php").unwrap();
        std::fs::create_dir_all(root.path().join("tests/fixtures/module/vendor")).unwrap();
        std::fs::write(
            root.path().join("tests/fixtures/module/vendor/Fixture.php"),
            "<?php",
        )
        .unwrap();
        assert_eq!(super::count_first_party_php_files(root.path()).unwrap(), 1);
    }

    #[test]
    fn the_php_extension_match_is_case_insensitive_like_celerrates_own_walk() {
        // Mirrors `has_php_extension` in `crates/celerrate_vfs/src/walk.rs`,
        // which Celerrate's own discovery uses to decide what counts
        // as a PHP file; the independent count must apply the same
        // rule or a case-mismatched extension would itself look like a
        // divergence.
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("Legacy.PHP"), "<?php").unwrap();
        assert_eq!(super::count_first_party_php_files(root.path()).unwrap(), 1);
    }

    #[test]
    fn matching_counts_do_not_diverge() {
        assert!(super::diverging_file_counts(6932, 6932, 0).is_none());
    }

    #[test]
    fn counts_beyond_the_tolerance_are_named() {
        let failure = super::diverging_file_counts(5922, 6932, 0).unwrap();
        assert!(failure.contains("5922"));
        assert!(failure.contains("6932"));
    }

    #[test]
    fn counts_within_the_tolerance_do_not_diverge() {
        assert!(super::diverging_file_counts(6930, 6932, 2).is_none());
        assert!(super::diverging_file_counts(6929, 6932, 2).is_some());
    }
}
