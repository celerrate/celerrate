//! The mixed-rate harness: the hidden CLI channel run cold over the
//! pinned corpus, byte-compared
//! against a committed baseline. Unlike `ground_truth.rs`'s
//! classified-merge gate, this is the corpus-snapshot pattern
//! (`corpus.rs`'s `check_snapshot`): the report is plain counters with
//! no classification column, so `--bless` simply rewrites the file
//! whole.

use std::process::Command;

use crate::Result;

/// The committed baseline's path.
fn baseline_path() -> Result<std::path::PathBuf> {
    Ok(crate::workspace_root()?.join("xtask/mixed-rate-baseline.txt"))
}

/// Runs the built binary's hidden `mixed-rate` channel over the
/// pinned corpus, cold (any cache directory left by an earlier run —
/// including one restored by CI's corpus cache — is removed first, so
/// the report never depends on mutable state, mirroring
/// `corpus::check_snapshot`'s precedent exactly), and either gates the
/// produced report against the committed baseline byte-for-byte or
/// (`bless`) rewrites it. Exit code 1 is tolerated like `corpus`'s own
/// `check` binary invocation: the instrument never reports
/// diagnostics, but tolerating both 0 and 1 keeps this harness
/// agnostic to `Outcome`'s exact mapping.
pub fn check(bless: bool) -> Result<()> {
    let corpus = crate::corpus::prepare()?;
    let cache_directory = corpus.join(".celerrate");
    if cache_directory.exists() {
        std::fs::remove_dir_all(&cache_directory)?;
    }
    let binary = crate::release_binary()?;
    let output = Command::new(&binary)
        .arg("mixed-rate")
        .arg(&corpus)
        .output()?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "celerrate mixed-rate did not complete (exit {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
        )
        .into());
    }
    let actual = String::from_utf8(output.stdout)
        .map_err(|error| format!("the mixed-rate report is not valid UTF-8: {error}"))?;

    let path = baseline_path()?;
    if bless {
        std::fs::write(&path, &actual)?;
        println!("blessed {}", path.display());
        return Ok(());
    }

    let expected = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read {}: {error}; run `cargo xtask mixed-rate --bless` and review the result",
            path.display(),
        )
    })?;
    if actual != expected {
        let actual_path = crate::workspace_root()?.join("target/corpus/actual-mixed-rate.txt");
        std::fs::write(&actual_path, &actual)?;
        // Exit code 1 from `git diff` means "differences", which is the point.
        let _ = Command::new("git")
            .args(["--no-pager", "diff", "--no-index"])
            .arg(&path)
            .arg(&actual_path)
            .status();
        return Err(
            "the mixed-rate report diverged from the committed baseline; review the diff above \
             and, if the change is intended, run `cargo xtask mixed-rate --bless`"
                .into(),
        );
    }
    println!("the mixed-rate report matches the committed baseline");
    Ok(())
}
