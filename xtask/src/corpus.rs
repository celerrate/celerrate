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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    #[test]
    fn the_committed_corpus_pin_parses_and_names_the_corpus() {
        let pin = super::pin().unwrap();
        assert!(
            pin.repository.contains("symfony/demo"),
            "the corpus is symfony/demo, per the design: {}",
            pin.repository,
        );
    }
}
