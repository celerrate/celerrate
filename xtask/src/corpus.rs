//! The pinned regression and benchmark corpus: symfony/demo at a
//! committed SHA, fetched shallowly, its vendor tree installed from its
//! own lock file. The corpus is both the anti-false-positive regression
//! surface and the benchmark subject; bumping it is a deliberate pin
//! change with a human-reviewed snapshot diff, never a floating HEAD.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;
use crate::pin::Pin;

/// Reads and parses the committed corpus pin.
pub fn pin() -> Result<Pin> {
    crate::pin::read(&crate::workspace_root()?.join("xtask/corpus.pin"))
}

/// Where the corpus lives: under `target/`, so it is already gitignored
/// and swept by `cargo clean`.
pub fn snapshot_directory() -> Result<PathBuf> {
    Ok(crate::workspace_root()?
        .join("target/corpus")
        .join(pin()?.commit))
}

/// Fetches the corpus and installs its vendor tree; returns the corpus
/// root, ready to be analyzed.
pub fn prepare() -> Result<PathBuf> {
    let directory = snapshot_directory()?;
    crate::pin::fetch_snapshot(&pin()?, &directory)?;
    install_vendor(&directory)?;
    Ok(directory)
}

/// Runs `composer install` from the corpus's committed lock file, once:
/// a present vendor directory is trusted, because the lock file pins
/// the tree exactly. `--no-scripts` and `--no-plugins` keep the install
/// hermetic (no code from the corpus runs), and `--ignore-platform-reqs`
/// decouples it from the local PHP extension set: Celerrate never
/// executes the corpus, it only reads it.
fn install_vendor(directory: &Path) -> Result<()> {
    if directory.join("vendor").is_dir() {
        return Ok(());
    }
    let status = Command::new("composer")
        .current_dir(directory)
        .args([
            "install",
            "--no-interaction",
            "--no-progress",
            "--no-scripts",
            "--no-plugins",
            "--ignore-platform-reqs",
        ])
        .status()
        .map_err(|error| format!("cannot run composer (is it installed?): {error}"))?;
    if !status.success() {
        return Err("composer install failed".into());
    }
    Ok(())
}

/// The identifiers of the unknown-symbol family. symfony/demo is
/// correct code: any of these in its report is a false positive, which
/// the umbrella design classifies as a priority bug, not an opinion.
const UNKNOWN_SYMBOL_IDENTIFIERS: [&str; 3] = ["CEL0018", "CEL0019", "CEL0020"];

/// The identifiers of the unknown-member family (`CEL0030` method,
/// `CEL0031` property, `CEL0032` class constant, `CEL0033` enum case).
/// These identifiers sit at zero occurrences on the corpus, so
/// they join the hard-refusal list: like an unknown symbol, an
/// unknown member on this correct code is a priority bug, refused even
/// under `--bless`, never a snapshot entry. The nullability and
/// argument families (`CEL0034`-`CEL0038`) are gated by snapshot
/// equality alone — they carry legitimate stances whose regressions a
/// diverging snapshot already catches.
const TYPED_MEMBER_IDENTIFIERS: [&str; 4] = ["CEL0030", "CEL0031", "CEL0032", "CEL0033"];

/// The committed expected report.
pub fn snapshot_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("xtask/corpus-snapshot.txt"))
}

/// Every report line carrying an unknown-symbol diagnostic. The
/// identifier is the second field of the diagnostic line format
/// (`path:line:column identifier message`); a plain substring match is
/// enough because the identifiers never appear in message text.
pub fn unknown_symbol_violations(report: &str) -> Vec<String> {
    lines_with_any(report, &UNKNOWN_SYMBOL_IDENTIFIERS)
}

/// Every report line carrying an unknown-member diagnostic, by the same
/// line-scanning shape as [`unknown_symbol_violations`].
pub fn typed_member_violations(report: &str) -> Vec<String> {
    lines_with_any(report, &TYPED_MEMBER_IDENTIFIERS)
}

/// Report lines containing any of `identifiers`. A plain substring
/// match is enough because the identifiers never appear in message text.
fn lines_with_any(report: &str, identifiers: &[&str]) -> Vec<String> {
    report
        .lines()
        .filter(|line| {
            identifiers
                .iter()
                .any(|identifier| line.contains(identifier))
        })
        .map(str::to_owned)
        .collect()
}

/// Runs the release binary over the corpus and holds the report to its
/// two contracts: no unknown-symbol diagnostic anywhere (refused even
/// under `--bless`), and byte-for-byte agreement with the committed
/// snapshot. Exit code 1 from the binary means diagnostics were
/// reported, which is a completed analysis; anything above 1 is not.
pub fn check_snapshot(bless: bool) -> Result<()> {
    let corpus = prepare()?;
    // The snapshot check always runs cold: a cache left by an earlier
    // run (or restored by CI's corpus cache) must not be captured into
    // the pin-keyed cache entry, and the job is only honest about the
    // committed report if it never depends on mutable state.
    let cache_directory = corpus.join(".celerrate");
    if cache_directory.exists() {
        std::fs::remove_dir_all(&cache_directory)?;
    }
    let binary = crate::release_binary()?;
    let output = Command::new(&binary).arg("check").arg(&corpus).output()?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "celerrate check did not complete (exit {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
        )
        .into());
    }
    let actual = String::from_utf8(output.stdout)
        .map_err(|error| format!("the report is not valid UTF-8: {error}"))?;

    let violations = unknown_symbol_violations(&actual);
    if !violations.is_empty() {
        return Err(format!(
            "the corpus report contains {} unknown-symbol diagnostic(s); each is a \
             false positive on correct code and a priority bug:\n{}",
            violations.len(),
            violations.join("\n"),
        )
        .into());
    }

    let member_violations = typed_member_violations(&actual);
    if !member_violations.is_empty() {
        return Err(format!(
            "the corpus report contains {} unknown-member diagnostic(s); each is a \
             false positive on correct code and a priority bug:\n{}",
            member_violations.len(),
            member_violations.join("\n"),
        )
        .into());
    }

    let path = snapshot_path()?;
    if bless {
        std::fs::write(&path, &actual)?;
        println!("blessed {}", path.display());
        return Ok(());
    }

    let expected = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read {}: {error}; run `cargo xtask corpus --bless` and review the result",
            path.display(),
        )
    })?;
    if actual != expected {
        let actual_path = crate::workspace_root()?.join("target/corpus/actual-snapshot.txt");
        std::fs::write(&actual_path, &actual)?;
        // Exit code 1 from `git diff` means "differences", which is the point.
        let _ = Command::new("git")
            .args(["--no-pager", "diff", "--no-index"])
            .arg(&path)
            .arg(&actual_path)
            .status();
        return Err(
            "the corpus report diverged from the committed snapshot; review the diff above \
             and, if the change is intended, run `cargo xtask corpus --bless`"
                .into(),
        );
    }
    println!("the corpus report matches the committed snapshot");
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::{typed_member_violations, unknown_symbol_violations};

    #[test]
    fn the_committed_corpus_pin_parses_and_names_the_corpus() {
        let pin = super::pin().unwrap();
        assert!(
            pin.repository.contains("symfony/demo"),
            "the corpus is symfony/demo: {}",
            pin.repository,
        );
    }

    #[test]
    fn the_unknown_symbol_family_is_caught_line_by_line() {
        let report = "src/A.php:3:1 CEL0018 unknown class \\App\\Missing\n\
                      src/B.php:9:5 CEL0024 match expressions require PHP 8.0\n\
                      src/C.php:1:1 CEL0019 unknown function \\missing()\n\
                      src/D.php:2:2 CEL0020 unknown constant \\MISSING\n\
                      0 notices, 4 diagnostics\n";
        let violations = unknown_symbol_violations(report);
        assert_eq!(violations.len(), 3);
        assert!(violations.iter().all(|line| !line.contains("CEL0024")));
    }

    #[test]
    fn a_clean_report_has_no_violations() {
        let report = "src/B.php:9:5 CEL0024 match expressions require PHP 8.0\n\
                      0 notices, 1 diagnostic\n";
        assert!(unknown_symbol_violations(report).is_empty());
        assert!(typed_member_violations(report).is_empty());
    }

    #[test]
    fn the_unknown_member_family_is_caught_line_by_line() {
        let report = "src/A.php:3:1 CEL0030 unknown method save on \\App\\User\n\
                      src/B.php:9:5 CEL0034 accessing save on a possibly null \\App\\User|null\n\
                      src/C.php:1:1 CEL0031 unknown property name on \\App\\User\n\
                      src/D.php:2:2 CEL0032 unknown class constant LIMIT on \\App\\Config\n\
                      src/E.php:4:4 CEL0033 unknown enum case Draft on \\App\\Status\n\
                      0 notices, 5 diagnostics\n";
        let violations = typed_member_violations(report);
        // The four unknown-member identifiers are caught; the
        // nullability line (CEL0034) is gated by snapshot equality only.
        assert_eq!(violations.len(), 4);
        assert!(violations.iter().all(|line| !line.contains("CEL0034")));
    }
}
