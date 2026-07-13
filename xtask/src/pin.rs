//! The pin mechanism shared by every vendored snapshot: a committed
//! `key = value` file naming a repository and a commit, fetched
//! shallowly into `target/`, bumped deliberately, never floating.

use std::path::Path;
use std::process::Command;

use crate::Result;

/// A parsed pin file: one repository, one full-length commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub repository: String,
    pub commit: String,
}

/// Reads and parses a committed pin file.
pub fn read(path: &Path) -> Result<Pin> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse(&text)
}

/// Parses a pin file: `key = value` lines, `#` comments, both
/// `repository` and a full-length hexadecimal `commit` required.
pub fn parse(text: &str) -> Result<Pin> {
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
    Ok(Pin { repository, commit })
}

/// Fetches the pinned snapshot into `directory` if it is not already
/// present. The checkout lands in a staging directory first and is
/// renamed only when complete, so an interrupted fetch never
/// masquerades as a snapshot.
pub fn fetch_snapshot(pin: &Pin, directory: &Path) -> Result<()> {
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
    std::fs::rename(&staging, directory)?;
    println!("fetched {} at {}", pin.repository, pin.commit);
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

    use super::parse;

    const VALID_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_valid_pin_parses() {
        let pin = parse(&format!(
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
        assert!(parse("repository = https://example.com/repo").is_err());
        assert!(parse(&format!("commit = {VALID_SHA}")).is_err());
    }

    #[test]
    fn a_short_or_non_hexadecimal_commit_is_rejected() {
        assert!(parse("repository = r\ncommit = abc123").is_err());
        assert!(
            parse("repository = r\ncommit = zzzz456789abcdef0123456789abcdef01234567").is_err()
        );
    }

    #[test]
    fn unknown_keys_are_rejected_to_catch_typos() {
        assert!(parse(&format!("repo = r\ncommit = {VALID_SHA}")).is_err());
    }
}
