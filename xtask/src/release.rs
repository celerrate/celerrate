//! `cargo xtask release-notes <version>`: extract one version's entry
//! from `CHANGELOG.md`. The release workflow publishes this text as
//! the GitHub Release notes, so the changelog stays the single source
//! of what a release says about itself.

use crate::Result;

/// Prints the changelog entry for `version` to standard output.
pub fn run(version: &str) -> Result<()> {
    let root = crate::workspace_root()?;
    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md"))?;
    println!("{}", notes(&changelog, version)?);
    Ok(())
}

/// The entry for `version`: everything between its `## [version]`
/// heading and the next `## ` heading, with the trailing
/// link-reference block dropped (it follows the last entry), trimmed.
/// A missing or empty entry is an error: a release must not be
/// created with blank notes.
pub fn notes(changelog: &str, version: &str) -> Result<String> {
    let heading = format!("## [{version}]");
    let mut entry_lines: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in changelog.lines() {
        if inside {
            if line.starts_with("## ") {
                break;
            }
            entry_lines.push(line);
        } else if line.starts_with(&heading) {
            inside = true;
        }
    }
    if !inside {
        return Err(format!("CHANGELOG.md has no `{heading}` entry").into());
    }
    while let Some(last) = entry_lines.last() {
        if last.is_empty() || is_link_reference(last) {
            entry_lines.pop();
        } else {
            break;
        }
    }
    let entry = entry_lines.join("\n").trim().to_owned();
    if entry.is_empty() {
        return Err(format!("the `{heading}` entry of CHANGELOG.md is empty").into());
    }
    Ok(entry)
}

/// A Keep a Changelog link-reference line: `[0.0.1]: https://...`.
fn is_link_reference(line: &str) -> bool {
    line.starts_with('[') && line.contains("]: ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::notes;

    const CHANGELOG: &str = "\
# Changelog

Introductory prose the extractor must never return.

## [Unreleased]

### Added

- Something not yet released.

## [0.0.2] - 2026-08-01

### Added

- The second release.

## [0.0.1] - 2026-07-13

The first public preview.

### Added

- `celerrate check`.

[Unreleased]: https://example.invalid/compare/v0.0.2...HEAD
[0.0.2]: https://example.invalid/compare/v0.0.1...v0.0.2
[0.0.1]: https://example.invalid/releases/tag/v0.0.1
";

    #[test]
    fn extracts_one_entry_without_its_heading_or_its_neighbors() {
        let entry = notes(CHANGELOG, "0.0.2").unwrap();
        assert_eq!(entry, "### Added\n\n- The second release.");
    }

    #[test]
    fn the_last_entry_stops_before_the_link_reference_block() {
        let entry = notes(CHANGELOG, "0.0.1").unwrap();
        assert_eq!(
            entry,
            "The first public preview.\n\n### Added\n\n- `celerrate check`.",
        );
    }

    #[test]
    fn a_version_with_no_entry_is_an_error() {
        let error = notes(CHANGELOG, "9.9.9").unwrap_err();
        assert!(error.to_string().contains("9.9.9"));
    }

    #[test]
    fn an_empty_entry_is_an_error() {
        let changelog = "# Changelog\n\n## [Unreleased]\n\n## [0.0.1] - 2026-07-13\n";
        assert!(notes(changelog, "Unreleased").is_err());
    }
}
