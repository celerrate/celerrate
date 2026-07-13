//! The pinned phpstorm-stubs snapshot: fetch it at the pinned commit
//! and drive the stub compiler. Network happens only here — never in
//! a build script, never in a query. The pin is bumped deliberately,
//! like the corpus SHA.

use std::path::PathBuf;
use std::process::Command;

use crate::Result;
use crate::pin::Pin;

/// Reads and parses the committed pin file.
pub fn pin() -> Result<Pin> {
    crate::pin::read(&crate::workspace_root()?.join("xtask/phpstorm-stubs.pin"))
}

/// Where the pinned snapshot lives: under `target/`, so it is already
/// gitignored and swept by `cargo clean`.
pub fn snapshot_directory() -> Result<PathBuf> {
    Ok(crate::workspace_root()?
        .join("target/phpstorm-stubs")
        .join(pin()?.commit))
}

/// Fetches the pinned snapshot if it is not already present.
pub fn fetch() -> Result<()> {
    crate::pin::fetch_snapshot(&pin()?, &snapshot_directory()?)
}

/// Fetches if needed, then runs the stub compiler over the snapshot.
/// `check` compares against the committed blob instead of writing it.
pub fn compile(check: bool) -> Result<()> {
    fetch()?;
    let root = crate::workspace_root()?;
    let snapshot = snapshot_directory()?;
    let blob = root.join("crates/celerrate_stubs/src/stubs.bin");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut command = Command::new(cargo);
    command
        .current_dir(&root)
        .args([
            "run",
            "--release",
            "--package",
            "celerrate_stubs",
            "--features",
            "compiler",
            "--bin",
            "stub-compiler",
            "--",
        ])
        .arg(&snapshot)
        .arg(&blob);
    if check {
        command.arg("--check");
    }
    let status = command.status()?;
    if !status.success() {
        return Err("stub compilation failed".into());
    }
    Ok(())
}
