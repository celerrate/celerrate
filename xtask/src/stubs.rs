//! The pinned phpstorm-stubs snapshot: fetch it at the pinned commit
//! and drive the stub compiler. Network happens only here — never in
//! a build script, never in a query. The pin is bumped deliberately,
//! like the corpus SHAs.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// The parsed `xtask/phpstorm-stubs.pin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubsPin {
    pub repository: String,
    pub commit: String,
}

/// Reads and parses the committed pin file.
pub fn pin() -> Result<StubsPin> {
    let path = crate::workspace_root()?.join("xtask/phpstorm-stubs.pin");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse_pin(&text)
}

/// Parses the pin file: `key = value` lines, `#` comments, both
/// `repository` and a full-length hexadecimal `commit` required.
pub fn parse_pin(text: &str) -> Result<StubsPin> {
    let mut repository = None;
    let mut commit = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("malformed pin line: {line}").into());
        };
        match key.trim() {
            "repository" => repository = Some(value.trim().to_owned()),
            "commit" => commit = Some(value.trim().to_owned()),
            unknown => return Err(format!("unknown pin key: {unknown}").into()),
        }
    }
    let repository = repository.ok_or("pin file misses the repository key")?;
    let commit = commit.ok_or("pin file misses the commit key")?;
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the pinned commit must be a full 40-character SHA".into());
    }
    Ok(StubsPin { repository, commit })
}

/// Where the pinned snapshot lives: under `target/`, so it is already
/// gitignored and swept by `cargo clean`.
pub fn snapshot_directory() -> Result<PathBuf> {
    Ok(crate::workspace_root()?
        .join("target/phpstorm-stubs")
        .join(pin()?.commit))
}

/// Fetches the pinned snapshot if it is not already present. The
/// checkout lands in a staging directory first and is renamed only
/// when complete, so an interrupted fetch never masquerades as a
/// snapshot.
pub fn fetch() -> Result<()> {
    let pin = pin()?;
    let directory = snapshot_directory()?;
    if directory.exists() {
        println!("snapshot already present at {}", directory.display());
        return Ok(());
    }
    let staging = directory.with_file_name(format!("{}.staging", pin.commit));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    run_git(&staging, &["init", "--quiet"])?;
    run_git(
        &staging,
        &[
            "fetch",
            "--quiet",
            "--depth",
            "1",
            &pin.repository,
            &pin.commit,
        ],
    )?;
    run_git(&staging, &["checkout", "--quiet", "--detach", "FETCH_HEAD"])?;
    std::fs::rename(&staging, &directory)?;
    println!("fetched {} at {}", pin.repository, pin.commit);
    Ok(())
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

fn run_git(directory: &Path, arguments: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .status()?;
    if !status.success() {
        return Err(format!("git {} failed", arguments.join(" ")).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::parse_pin;

    const VALID_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_valid_pin_parses() {
        let pin = parse_pin(&format!(
            "# a comment\n\
             repository = https://github.com/JetBrains/phpstorm-stubs\n\
             \n\
             commit = {VALID_SHA}\n",
        ))
        .unwrap();
        assert_eq!(
            pin.repository,
            "https://github.com/JetBrains/phpstorm-stubs"
        );
        assert_eq!(pin.commit, VALID_SHA);
    }

    #[test]
    fn a_missing_key_is_rejected() {
        assert!(parse_pin("repository = https://example.com/repo").is_err());
        assert!(parse_pin(&format!("commit = {VALID_SHA}")).is_err());
    }

    #[test]
    fn a_short_or_non_hexadecimal_commit_is_rejected() {
        assert!(parse_pin("repository = r\ncommit = abc123").is_err());
        assert!(
            parse_pin("repository = r\ncommit = zzzz456789abcdef0123456789abcdef01234567").is_err()
        );
    }

    #[test]
    fn unknown_keys_are_rejected_to_catch_typos() {
        assert!(parse_pin(&format!("repo = r\ncommit = {VALID_SHA}")).is_err());
    }
}
