//! The benchmark harness behind `benchmarks/PROTOCOL.md`: prepare the
//! corpus, prime the cache, apply the scripted edit, and let hyperfine
//! measure the built binary end to end — process startup and cache
//! loading included, because that is how the flagship number is
//! defined. hyperfine rather than criterion, per the protocol: the
//! number is a full CLI run, and criterion measures in-process.

use std::path::Path;
use std::process::Command;

use crate::Result;

/// The file the one-edit scenario touches, relative to the corpus
/// root, and the scripted edit itself: appended, so every span above
/// it stays put — one file changes, everything else is unchanged.
const EDIT_TARGET: &str = "src/Controller/BlogController.php";
const EDIT_TEXT: &str = "\\n// celerrate benchmark edit\\n";

/// The CI guard rail's generous ceilings, in seconds. Shared runners
/// are too noisy to measure on, so these catch structural regressions
/// (the cache silently ceasing to work) and claim nothing more. The
/// local target for warm one-edit is sub-second; the ceiling is not
/// the target.
const COLD_CEILING_SECONDS: f64 = 30.0;
const WARM_NO_CHANGE_CEILING_SECONDS: f64 = 3.0;
const WARM_ONE_EDIT_CEILING_SECONDS: f64 = 3.0;

/// One protocol scenario: its name, how many timed runs, what runs
/// before each timed run, and the guard-rail ceiling.
struct Scenario {
    name: &'static str,
    runs: u32,
    prepare: Option<String>,
    ceiling_seconds: f64,
}

/// Runs the three protocol scenarios and prints their medians. With
/// `check_ceilings`, any median over its ceiling fails the run.
pub fn run(check_ceilings: bool) -> Result<()> {
    ensure_hyperfine()?;
    let corpus = crate::corpus::prepare()?;
    let binary = crate::release_binary()?;
    let root = crate::workspace_root()?;

    let bench_directory = root.join("target/bench");
    let working = bench_directory.join("corpus");
    if working.exists() {
        std::fs::remove_dir_all(&working)?;
    }
    copy_directory(&corpus, &working)?;

    // The pristine bytes of the edit target, kept outside the working
    // tree so the walk never sees them.
    let edit_target = working.join(EDIT_TARGET);
    let original = bench_directory.join("edit-target-original.bak");
    std::fs::copy(&edit_target, &original)?;

    let quoted_binary = quoted(&binary);
    let scenarios = [
        Scenario {
            name: "cold full",
            runs: 5,
            prepare: Some("rm -rf .celerrate".to_owned()),
            ceiling_seconds: COLD_CEILING_SECONDS,
        },
        Scenario {
            name: "warm no-change",
            runs: 10,
            prepare: None,
            ceiling_seconds: WARM_NO_CHANGE_CEILING_SECONDS,
        },
        Scenario {
            name: "warm one-edit",
            runs: 10,
            prepare: Some(format!(
                "cp {} {} && ({quoted_binary} check . > /dev/null || true) && printf '{}' >> {}",
                quoted(&original),
                quoted(&edit_target),
                EDIT_TEXT,
                quoted(&edit_target),
            )),
            ceiling_seconds: WARM_ONE_EDIT_CEILING_SECONDS,
        },
    ];

    // The no-change scenario needs a cache to not change against.
    prime(&binary, &working)?;

    let mut failures = Vec::new();
    println!("{:<16} {:>10}", "scenario", "median");
    for scenario in &scenarios {
        let export = bench_directory.join(format!("{}.json", scenario.name.replace(' ', "-"),));
        let median = run_scenario(&quoted_binary, &working, scenario, &export)?;
        println!("{:<16} {:>9.3}s", scenario.name, median);
        if check_ceilings {
            failures.extend(over_ceiling(
                scenario.name,
                median,
                scenario.ceiling_seconds,
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("; ").into());
    }
    Ok(())
}

/// One hyperfine invocation, in the working copy. `--ignore-failure`
/// because the binary exits 1 when it reports diagnostics, which is a
/// completed analysis, not a failed one.
fn run_scenario(
    quoted_binary: &str,
    working: &Path,
    scenario: &Scenario,
    export: &Path,
) -> Result<f64> {
    let mut command = Command::new("hyperfine");
    command
        .current_dir(working)
        .args(["--ignore-failure", "--runs", &scenario.runs.to_string()])
        .arg("--export-json")
        .arg(export);
    if let Some(prepare) = &scenario.prepare {
        command.arg("--prepare").arg(prepare);
    }
    command.arg(format!("{quoted_binary} check ."));
    let status = command.status()?;
    if !status.success() {
        return Err(format!("hyperfine failed for the {} scenario", scenario.name).into());
    }
    median_seconds(&std::fs::read_to_string(export)?)
}

/// Extracts the median, in seconds, from hyperfine's `--export-json`
/// output.
pub fn median_seconds(json: &str) -> Result<f64> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("unreadable hyperfine JSON: {error}"))?;
    value
        .get("results")
        .and_then(|results| results.get(0))
        .and_then(|result| result.get("median"))
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| "hyperfine JSON carries no median".into())
}

/// The guard-rail comparison, one scenario at a time.
pub fn over_ceiling(name: &str, median: f64, ceiling: f64) -> Option<String> {
    (median > ceiling)
        .then(|| format!("the {name} median ({median:.3}s) is over its {ceiling:.1}s ceiling"))
}

/// One analysis over the working copy, to write the cache the warm
/// scenarios start from. Exit 1 means diagnostics were reported, which
/// is a completed analysis.
fn prime(binary: &Path, working: &Path) -> Result<()> {
    let status = Command::new(binary)
        .arg("check")
        .arg(".")
        .current_dir(working)
        .stdout(std::process::Stdio::null())
        .status()?;
    if !matches!(status.code(), Some(0 | 1)) {
        return Err(format!(
            "the priming run did not complete (exit {:?})",
            status.code()
        )
        .into());
    }
    Ok(())
}

/// Copies the corpus into a disposable working tree, skipping `.git`
/// (never analyzed) and `.celerrate` (each scenario controls the
/// cache). Symlinks (composer's `vendor/bin`) are copied by content,
/// which is fine: the tree is read, never executed.
fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" || name == ".celerrate" {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// hyperfine runs its command through a shell; the binary path is
/// quoted so a space in the workspace path cannot split it.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display())
}

/// A named check with an installation pointer beats a bare "No such
/// file or directory" from the spawn.
fn ensure_hyperfine() -> Result<()> {
    let found = Command::new("hyperfine")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok();
    if !found {
        return Err(
            "hyperfine is required: https://github.com/sharkdp/hyperfine#installation".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{copy_directory, median_seconds, over_ceiling};

    #[test]
    fn the_median_is_read_from_hyperfine_json() {
        let json = r#"{"results": [{"command": "celerrate check .",
            "mean": 0.9, "stddev": 0.1, "median": 0.85,
            "min": 0.7, "max": 1.1, "times": [0.7, 0.85, 1.1]}]}"#;
        assert_eq!(median_seconds(json).unwrap(), 0.85);
    }

    #[test]
    fn json_without_a_median_is_an_error_not_a_panic() {
        assert!(median_seconds("{}").is_err());
        assert!(median_seconds("not json at all").is_err());
        assert!(median_seconds(r#"{"results": []}"#).is_err());
    }

    #[test]
    fn a_median_over_its_ceiling_is_named() {
        assert!(over_ceiling("warm one-edit", 3.2, 3.0).is_some());
        assert!(over_ceiling("warm one-edit", 2.9, 3.0).is_none());
    }

    #[test]
    fn the_working_copy_skips_the_git_directory_and_the_cache() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join(".git")).unwrap();
        std::fs::create_dir_all(source.path().join(".celerrate/cache")).unwrap();
        std::fs::create_dir_all(source.path().join("src")).unwrap();
        std::fs::write(source.path().join(".git/HEAD"), "ref").unwrap();
        std::fs::write(source.path().join("src/A.php"), "<?php").unwrap();
        std::fs::write(source.path().join("composer.json"), "{}").unwrap();

        let destination = tempfile::tempdir().unwrap();
        let copy = destination.path().join("corpus");
        copy_directory(source.path(), &copy).unwrap();

        assert!(copy.join("src/A.php").is_file());
        assert!(copy.join("composer.json").is_file());
        assert!(!copy.join(".git").exists());
        assert!(!copy.join(".celerrate").exists());
    }
}
