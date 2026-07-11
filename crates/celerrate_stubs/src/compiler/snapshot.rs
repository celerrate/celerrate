//! The snapshot walk: every stub file of the pinned phpstorm-stubs
//! checkout, in an order that is deterministic across platforms.

use std::path::{Path, PathBuf};

/// Directories that carry no stubs: the repository's own tooling.
const SKIPPED_DIRECTORIES: [&str; 4] = [".git", ".github", ".idea", "tests"];

/// Every `*.php` file under `snapshot` (extension matched ASCII-case-
/// insensitively), recursively, skipping tooling directories, sorted
/// by path components so the walk order never depends on the platform.
pub fn stub_files(snapshot: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk(snapshot, &mut files)?;
    files.sort_by(|left, right| {
        left.components()
            .map(|component| component.as_os_str().to_owned())
            .cmp(
                right
                    .components()
                    .map(|component| component.as_os_str().to_owned()),
            )
    });
    Ok(files)
}

fn walk(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let skipped = path
                .file_name()
                .is_some_and(|name| SKIPPED_DIRECTORIES.iter().any(|skip| name == *skip));
            if !skipped {
                walk(&path, files)?;
            }
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("php"))
        {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::fs;
    use std::path::Path;

    use super::stub_files;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn the_walk_finds_php_files_recursively_in_sorted_order() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("standard/basic.php"), "<?php");
        write(&root.path().join("Core/Core.php"), "<?php");
        write(&root.path().join("Core/deep/nested.PHP"), "<?php");
        write(&root.path().join("README.md"), "not php");
        let files = stub_files(root.path()).unwrap();
        let relative: Vec<String> = files
            .iter()
            .map(|path| {
                path.strip_prefix(root.path())
                    .unwrap()
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        assert_eq!(
            relative,
            vec![
                "Core/Core.php",
                "Core/deep/nested.PHP",
                "standard/basic.php"
            ],
        );
    }

    #[test]
    fn tool_and_test_directories_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("standard/basic.php"), "<?php");
        write(&root.path().join(".git/objects/fake.php"), "<?php");
        write(&root.path().join(".github/workflow.php"), "<?php");
        write(&root.path().join(".idea/config.php"), "<?php");
        write(&root.path().join("tests/StubsTest.php"), "<?php");
        let files = stub_files(root.path()).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn a_missing_root_is_an_error_not_a_panic() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("absent");
        assert!(stub_files(&missing).is_err());
    }
}
